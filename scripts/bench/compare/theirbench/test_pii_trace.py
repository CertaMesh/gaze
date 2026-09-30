"""Model-free tests for the PII-TRACE public-subset loader and scorer."""

from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import loaders  # noqa: E402
import pii_trace_repro as scorer  # noqa: E402
import render_theirbench as render  # noqa: E402
import theirbench  # noqa: E402

pq = pytest.importorskip("pyarrow.parquet")
pa = pytest.importorskip("pyarrow")

ROWS = [{
    "id": "c1",
    # 555-0100 to 555-0199 is the NANPA block reserved for fictional use.
    "turns": [{"turn": 0, "user": "Mail é@x.io or call 555-0123.", "assistant": "Noted, é@x.io."},
              {"turn": 1, "user": "Nothing here.", "assistant": "OK."}],
    "spans": [{"label": "private_email", "turn": 0, "source": "user", "start": 5, "end": 11, "text": "é@x.io"},
              {"label": "private_phone", "turn": 0, "source": "user", "start": 20, "end": 28, "text": "555-0123"}],
}]


@pytest.fixture
def parquet(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    path = tmp_path / "train.parquet"
    pq.write_table(pa.Table.from_pylist(ROWS), path)
    monkeypatch.setitem(loaders.PII_TRACE, "sha256", loaders.sha256(path))
    monkeypatch.setitem(loaders.PII_TRACE, "messages", 4)
    monkeypatch.setitem(loaders.PII_TRACE, "gold_spans", 2)
    return path


def test_loader_makes_one_document_per_message_with_byte_offsets(parquet: Path) -> None:
    splits, identity = loaders.load_pii_trace(parquet)
    documents = splits["test"]
    assert [d.uid for d in documents] == ["pii-trace/c1/0/user", "pii-trace/c1/0/assistant",
                                          "pii-trace/c1/1/user", "pii-trace/c1/1/assistant"]
    email = documents[0].spans[0]
    assert documents[0].text.encode()[email.start:email.end] == "é@x.io".encode()  # é is two bytes
    assert not documents[1].spans and identity["documents"] == 4


def test_loader_refuses_a_changed_file_or_a_wrong_count(parquet: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setitem(loaders.PII_TRACE, "gold_spans", 3)
    with pytest.raises(ValueError, match="count differs"):
        loaders.load_pii_trace(parquet)
    monkeypatch.setitem(loaders.PII_TRACE, "gold_spans", 2)  # counts right again: only the digest is wrong
    monkeypatch.setitem(loaders.PII_TRACE, "sha256", "0" * 64)
    with pytest.raises(ValueError, match="SHA-256"):
        loaders.load_pii_trace(parquet)


def test_loader_refuses_a_gold_span_that_does_not_match_its_text(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    bad = [{**ROWS[0], "spans": [{**ROWS[0]["spans"][0], "text": "wrong!"}]}]
    path = tmp_path / "bad.parquet"
    pq.write_table(pa.Table.from_pylist(bad), path)
    monkeypatch.setitem(loaders.PII_TRACE, "sha256", loaders.sha256(path))
    with pytest.raises(ValueError, match="does not match its text"):
        loaders.load_pii_trace(path)


COMPOSED = {"EMAIL": ["private_email"], "PHONE": ["private_phone"], "NAME": ["private_person"], "DEAD": []}


def scored(parquet: Path, predictions: list[list[list[object]]]) -> dict:
    return scorer.score_messages(loaders.pii_trace_messages(parquet), predictions, COMPOSED)


def test_perfect_prediction_scores_one_everywhere(parquet: Path) -> None:
    result = scored(parquet, [[[5, 11, "EMAIL"], [20, 28, "PHONE"]], [], [], []])
    assert result["char_level_label_agnostic"]["f1"] == 1.0
    assert result["exact_typed_micro"]["f1"] == 1.0 and result["exact_untyped_micro"]["f1"] == 1.0
    assert result["exact_typed_f1_by_label"]["private_email"] == 1.0
    assert result["exact_typed_macro_f1"] == round(2 / 9, 4)  # two of nine labels have gold here


def test_wrong_type_is_an_untyped_hit_and_a_typed_miss(parquet: Path) -> None:
    result = scored(parquet, [[[5, 11, "PHONE"], [20, 28, "PHONE"]], [], [], []])
    assert result["exact_untyped_micro"]["tp"] == 2 and result["exact_typed_micro"]["tp"] == 1
    assert result["exact_typed_f1_by_label"]["private_phone"] < 1.0  # the wrong-type span is its false positive


def test_assistant_detections_are_false_positives_and_partial_overlap_is_char_credit_only(parquet: Path) -> None:
    result = scored(parquet, [[[5, 9, "EMAIL"]], [[7, 13, "EMAIL"]], [], []])
    char = result["char_level_label_agnostic"]
    assert char["tp_chars"] == 4 and char["predicted_chars"] == 10 and char["gold_chars"] == 14
    assert result["exact_typed_micro"]["tp"] == 0 and result["exact_typed_micro"]["predicted"] == 2


def test_unmapped_and_uncovering_labels(parquet: Path) -> None:
    with pytest.raises(ValueError, match="unmapped labels"):
        scored(parquet, [[[5, 11, "NOPE"]], [], [], []])
    result = scored(parquet, [[[5, 11, "DEAD"]], [], [], []])
    assert result["exact_typed_micro"]["tp"] == 0 and result["exact_untyped_micro"]["tp"] == 1


def test_scorer_refuses_short_predictions_and_out_of_range_spans(parquet: Path) -> None:
    with pytest.raises(ValueError, match="cover every message"):
        scored(parquet, [[]])
    with pytest.raises(ValueError, match="outside the message"):
        scored(parquet, [[[5, 999, "EMAIL"]], [], [], []])


def test_gold_label_map_composes_for_every_tool_and_names_only_known_labels() -> None:
    maps = loaders.load_label_maps()
    assert set(maps["gold"]["pii-trace"]) == set(scorer.LABELS)
    import compare
    import pii_tracer

    tools = {**compare.load_mapping(), pii_tracer.TOOL: pii_tracer.load_label_map()}
    for family, table in tools.items():
        composed = loaders.compose_mapping(family, table, "pii-trace")
        assert set(composed) == set(table)
        assert all(set(natives) <= set(scorer.LABELS) for natives in composed.values())
    # PII-Tracer emits the dataset's own labels, so on PII-TRACE each covers exactly itself.
    own = loaders.compose_mapping("pii-tracer", pii_tracer.load_label_map(), "pii-trace")
    assert own["private_person"] == ("private_person",) and own["other_pii"] == ("other_pii",)
    assert own["custom:secret"] == ("secret",)
    # Every other tool still goes through the canonical labels, so other_pii is unreachable for it.
    assert all("other_pii" not in natives for family, table in tools.items() if family != "pii-tracer"
               for natives in loaders.compose_mapping(family, table, "pii-trace").values())
    assert "pii-trace" in theirbench.BENCHMARKS


def test_pii_trace_common_intersection_is_unchanged_by_the_new_gold_labels() -> None:
    import compare
    import pii_tracer

    composed = {f: loaders.compose_mapping(f, t, "pii-trace") for f, t in compare.load_mapping().items()}
    before = theirbench.common_intersection(composed)
    composed["pii-tracer"] = loaders.compose_mapping("pii-tracer", pii_tracer.load_label_map(), "pii-trace")
    assert theirbench.common_intersection(composed) == before


def test_render_states_no_vendor_number_and_names_the_subset() -> None:
    assert "pii-trace" in render.TITLES and render.OWN_METRIC["pii-trace"][0] == "char_f1"
    assert any("PII-TRACE full set" in name for name in render.NOT_RUN)
    entry = {"reproduction": {"published": None, "vendor_system": "pii-tracer",
                              "vendor_result": {"char_f1": 0.9, "exact_typed_micro_f1": 0.8}}}
    assert entry["reproduction"]["published"] is None


REAL = Path(os.environ.get("GAZE_PII_TRACE_PARQUET", "/nonexistent/train.parquet"))


@pytest.mark.skipif(not REAL.exists(), reason="set GAZE_PII_TRACE_PARQUET to the pinned parquet (bench host only)")
def test_real_subset_matches_its_pin() -> None:
    splits, identity = loaders.load_pii_trace(REAL)
    assert len(splits["test"]) == 4500 and sum(len(d.spans) for d in splits["test"]) == 2653
    assert identity["sha256"] == loaders.PII_TRACE["sha256"]


def test_native_identity_map_must_match_the_tools_labels() -> None:
    maps = loaders.load_label_maps()
    broken = {**maps, "native_gold": {"pii-trace": {"pii-tracer": {"private_person": ["nope"]}}}}
    import pii_tracer

    with pytest.raises(ValueError, match="native_gold"):
        loaders.compose_mapping("pii-tracer", pii_tracer.load_label_map(), "pii-trace", broken)


def tagged_trace() -> tuple[dict, dict, dict, dict]:
    """A committed PII-TRACE entry and a tagged Gaze report + own-scorer result that fit it."""
    from test_add_tool import DATASET, PREDICTIONS, own_pii_trace
    from test_theirbench import release_provenance, row, synthetic

    data = synthetic()
    entry = data["benchmarks"].pop("presidio-research")
    data["benchmarks"]["pii-trace"] = entry
    entry.update(identity={"messages": 3, "sha256": DATASET}, splits={"test": {"documents": 3}},
                 label_maps_sha256="a" * 64, mapping_sha256="b" * 64, typed_hold=["gaze", "opf"])
    entry["rescored_with"]["comparison_sha256"] = {"compare.py": "c" * 64}
    del entry["rows"]["gaze-v0.15.1"]
    report = {
        "schema_version": 1, "benchmark": "pii-trace", "preflight": None, "harness_dirty": False,
        "harness_revision": "d" * 40, "hardware": "hw", "generated_at": "2026-09-30T00:00:00+00:00",
        "identity": entry["identity"], "splits": entry["splits"],
        "common_intersection_labels": entry["common_intersection_labels"],
        "label_maps_sha256": entry["label_maps_sha256"], "mapping_sha256": entry["mapping_sha256"],
        "typed_hold": entry["typed_hold"], "comparison_sha256": {"compare.py": "c" * 64},
        "rows": {"gaze-v0.15.1": {"test": row(12)}},
        "provenance": {"gaze-v0.15.1": {"release": {**release_provenance(), "prediction_sha256": PREDICTIONS}}},
    }
    report["provenance"]["gaze-v0.15.1"]["release"]["reproduces"]["prediction_sha256"] = PREDICTIONS
    return data, entry, report, own_pii_trace("gaze-v0.15.1")


def test_a_tagged_gaze_row_joins_pii_trace_through_the_validated_path() -> None:
    from test_theirbench import RESOLVE

    data, entry, report, own = tagged_trace()
    assert render.add_tagged(data, report, own, RESOLVE) == "gaze-v0.15.1"
    assert entry["own_metric"]["gaze-v0.15.1"] == own["scored"]
    assert entry["provenance"]["gaze-v0.15.1"]["own_scorer_input"]["messages"] == 3


@pytest.mark.parametrize("edit,match", [
    (lambda r, o: o["input"].update(dataset_sha256="0" * 64), "own scorer used dataset"),
    (lambda r, o: o["input"].update(prediction_sha256="0" * 64), "own scorer read predictions"),
    (lambda r, o: o["input"].update(messages=2), "scored 2 messages"),
    (lambda r, o: o["overall"].update(char_f1=1.0), "not what the receipt's counts give"),
    (lambda r, o: o["scored"]["exact_typed_micro"].update(f1=1.0), "not what the receipt's counts give"),
    (lambda r, o: o["input"].pop("counts"), "no input receipt with counts"),
    (lambda r, o: r.update(identity={"messages": 3, "sha256": "0" * 64}), "identity differs"),
], ids=["dataset", "predictions", "messages", "overall", "typed", "no-counts", "identity"])
def test_a_tagged_pii_trace_row_is_refused_on_any_mismatch(edit, match: str) -> None:
    from test_theirbench import RESOLVE

    data, _entry, report, own = tagged_trace()
    edit(report, own)
    with pytest.raises(ValueError, match=match):
        render.add_tagged(data, report, own, RESOLVE)


def test_scorer_receipt_counts_rebuild_the_published_scores(parquet: Path) -> None:
    counts = scorer.count_messages(loaders.pii_trace_messages(parquet),
                                   [[[5, 11, "EMAIL"], [20, 28, "NAME"]], [[7, 13, "EMAIL"]], [], []], COMPOSED)
    overall, scored = scorer.published(scorer.scores_from_counts(counts))
    own = {"overall": overall, "scored": scored, "input": {"messages": 4, "counts": counts}}
    scorer.check_result(own, 4, "fixture")  # the producer's own output always verifies
    assert counts["char"] == {"tp": 14, "predicted": 20, "gold": 14}
    assert counts["exact_typed"]["tp"] == 1 and counts["exact_untyped"]["tp"] == 2
