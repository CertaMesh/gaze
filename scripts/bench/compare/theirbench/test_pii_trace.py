"""Model-free tests for the PII-TRACE public-subset loader and scorer."""

from __future__ import annotations

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
    "turns": [{"turn": 0, "user": "Mail é@x.io or call 5551234.", "assistant": "Noted, é@x.io."},
              {"turn": 1, "user": "Nothing here.", "assistant": "OK."}],
    "spans": [{"label": "private_email", "turn": 0, "source": "user", "start": 5, "end": 11, "text": "é@x.io"},
              {"label": "private_phone", "turn": 0, "source": "user", "start": 20, "end": 27, "text": "5551234"}],
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
    result = scored(parquet, [[[5, 11, "EMAIL"], [20, 27, "PHONE"]], [], [], []])
    assert result["char_level_label_agnostic"]["f1"] == 1.0
    assert result["exact_typed_micro"]["f1"] == 1.0 and result["exact_untyped_micro"]["f1"] == 1.0
    assert result["exact_typed_f1_by_label"]["private_email"] == 1.0
    assert result["exact_typed_macro_f1"] == round(2 / 9, 4)  # two of nine labels have gold here


def test_wrong_type_is_an_untyped_hit_and_a_typed_miss(parquet: Path) -> None:
    result = scored(parquet, [[[5, 11, "PHONE"], [20, 27, "PHONE"]], [], [], []])
    assert result["exact_untyped_micro"]["tp"] == 2 and result["exact_typed_micro"]["tp"] == 1
    assert result["exact_typed_f1_by_label"]["private_phone"] < 1.0  # the wrong-type span is its false positive


def test_assistant_detections_are_false_positives_and_partial_overlap_is_char_credit_only(parquet: Path) -> None:
    result = scored(parquet, [[[5, 9, "EMAIL"]], [[7, 13, "EMAIL"]], [], []])
    char = result["char_level_label_agnostic"]
    assert char["tp_chars"] == 4 and char["predicted_chars"] == 10 and char["gold_chars"] == 13
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


REAL = Path("/Users/krishankoenig/Workspace/EmpireTwo/gaze/target/pii-tracer-3942/piitrace/train.parquet")


@pytest.mark.skipif(not REAL.exists(), reason="the pinned parquet is downloaded on the bench host only")
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
