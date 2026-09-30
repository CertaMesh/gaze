"""add-tool: one separately measured row joins assembled rows only when it is comparable."""

from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import pii_trace_repro  # noqa: E402
import render_theirbench as render  # noqa: E402

CELL = {"leaked_bytes": 5, "false_positive_bytes": 2, "true_positive_bytes": 95, "document_leak_rate": 0.1,
        "typed_entities": {"f1": 0.4, "f2": 0.5}}
ROW = {"product_coverage": CELL, "common_intersection": CELL, "latency": {"p50_ms": 1.0}}
DATASET, PREDICTIONS = "d" * 64, "9" * 64
ENTRY = {"identity": {"documents": 3, "sha256": DATASET}, "splits": {"test": {"documents": 3, "ids_sha256": "aa"}},
         "common_intersection_labels": ["EMAIL"], "mapping_sha256": "m", "typed_hold": ["opf"],
         "rows": {"opf": ROW}, "provenance": {"opf": {}}, "own_metric": {"opf": {"f2": 0.1}}}


def report(**changes: object) -> dict:
    base = {"benchmark": "presidio-research", "identity": ENTRY["identity"], "splits": ENTRY["splits"],
            "common_intersection_labels": ["EMAIL"], "mapping_sha256": "m", "typed_hold": ["opf", "pii-tracer"],
            "rows": {"pii-tracer": {"test": ROW}}, "provenance": {"pii-tracer": {"cpu": {}, "prediction_sha256": PREDICTIONS}},
            "harness_revision": "0123456789ab", "harness_dirty": False, "gaze_crates_tree": "t",
            "comparison_revision": "b1446215", "label_maps_sha256": "l",
            "rescored_with": {"harness_dirty": False, "harness_revision": "abcdef012345"}}
    return {**base, **changes}


def write(tmp_path: Path, report_body: dict, own: dict | None = None) -> tuple[Path, Path]:
    rp, op = tmp_path / "report.json", tmp_path / "own.json"
    rp.write_text(json.dumps(report_body), encoding="utf-8")
    op.write_text(json.dumps(own or own_presidio()), encoding="utf-8")
    return rp, op


def own_presidio() -> dict:
    """60 of 80 predicted, 100 annotated: precision 0.75, recall 0.6, F2 0.625 (rounded to 3)."""
    return {"system": "pii-tracer", "scored": {"documents": 3, "precision": 0.75, "recall": 0.6, "f2": 0.625},
            "input": {"prediction_sha256": PREDICTIONS, "dataset_sha256": DATASET, "documents": 3, "beta": 2,
                      "digits": 3, "counts": {"true_positives": 60, "predicted": 80, "annotated": 100}}}


def data() -> dict:
    return {"benchmarks": {"presidio-research": copy.deepcopy(ENTRY)}, "not_run": {"old set": "licence"}}


def test_adds_the_row_and_leaves_existing_rows_alone(tmp_path: Path) -> None:
    before, after = data(), data()
    render.add_tool(after, *write(tmp_path, report()), "pii-tracer")
    entry = after["benchmarks"]["presidio-research"]
    assert entry["rows"]["opf"] == before["benchmarks"]["presidio-research"]["rows"]["opf"]
    assert entry["rows"]["pii-tracer"] == ROW and entry["own_metric"]["pii-tracer"] == own_presidio()["scored"]
    assert entry["provenance"]["pii-tracer"]["own_scorer_input"]["prediction_sha256"] == PREDICTIONS
    assert entry["typed_hold"] == ["opf", "pii-tracer"]
    assert entry["rows_measured_separately"]["pii-tracer"]["harness_revision"] == "0123456789ab"


@pytest.mark.parametrize("key,value", [("identity", {"documents": 4}),
                                       ("splits", {"test": {"documents": 3, "ids_sha256": "bb"}}),
                                       ("common_intersection_labels", ["EMAIL", "IBAN"]),
                                       ("mapping_sha256", "other")])
def test_refuses_a_measurement_on_other_documents_or_a_moved_intersection(tmp_path: Path, key: str, value: object) -> None:
    with pytest.raises(ValueError, match="not comparable"):
        render.add_tool(data(), *write(tmp_path, report(**{key: value})), "pii-tracer")


def test_refuses_dirty_unrescored_preflight_wrong_tool_and_smoke(tmp_path: Path) -> None:
    body = report()
    for broken in (report(harness_dirty=True), report(preflight=5),
                   {k: v for k, v in body.items() if k != "rescored_with"},
                   report(rescored_with={"harness_dirty": True, "harness_revision": "x"}),
                   report(rows={"pii-tracer": {"test": ROW}, "opf": {"test": ROW}})):
        with pytest.raises(ValueError):
            render.add_tool(data(), *write(tmp_path, broken), "pii-tracer")
    with pytest.raises(ValueError, match="own-scorer"):
        render.add_tool(data(), *write(tmp_path, body, {"system": "pii-tracer", "smoke_limit": 5, "overall": {}}), "pii-tracer")
    with pytest.raises(ValueError, match="own-scorer"):
        render.add_tool(data(), *write(tmp_path, body, {"system": "other", "overall": {}}), "pii-tracer")


def own_pii_trace(system: str = "pii-tracer", messages: int = 3) -> dict:
    """A PII-TRACE own-scorer result whose every score follows from its receipt's counts."""
    by_label = {label: {"tp": 0, "fp": 0, "fn": 0} for label in pii_trace_repro.LABELS}
    by_label["private_email"] = {"tp": 2, "fp": 1, "fn": 0}
    counts = {"messages": messages, "char": {"tp": 90, "predicted": 95, "gold": 100},
              "exact_typed": {"tp": 2, "predicted": 3, "gold": 2}, "exact_untyped": {"tp": 2, "predicted": 3, "gold": 2},
              "by_label": by_label}
    overall, scored = pii_trace_repro.published(pii_trace_repro.scores_from_counts(counts))
    return {"system": system, "overall": overall, "scored": scored, "versions": {"python": "3.12"},
            "input": {"prediction_sha256": PREDICTIONS, "dataset_sha256": DATASET, "messages": messages,
                      "counts": counts}}


def trace_report(**changes: object) -> dict:
    base = report(benchmark="pii-trace", chart_configs={}, comparison_sha256={}, gaze_crates_tree="t", hardware="hw",
                  label_maps_sha256="l")
    return {**base, **changes}


def test_add_benchmark_creates_an_entry_with_only_the_vendor_row(tmp_path: Path) -> None:
    body = data()
    render.add_benchmark(body, *write(tmp_path, trace_report(), own_pii_trace()), "pii-tracer")
    entry = body["benchmarks"]["pii-trace"]
    assert list(entry["rows"]) == ["pii-tracer"] and entry["chart_rows"] == ["pii-tracer"]
    assert entry["reproduction"]["published"] is None
    assert entry["reproduction"]["vendor_result"]["char_f1"] == round(2 * 90 / 195, 4)
    assert entry["own_metric"]["pii-tracer"] == own_pii_trace()["scored"]
    assert entry["provenance"]["pii-tracer"]["own_scorer_input"]["dataset_sha256"] == DATASET
    assert "presidio-research" in body["benchmarks"]  # the others are untouched
    assert body["not_run"]["old set"] == "licence"
    assert any("PII-TRACE full set" in name for name in body["not_run"])


def test_add_benchmark_refuses_an_existing_benchmark_and_bad_inputs(tmp_path: Path) -> None:
    body = data()
    with pytest.raises(ValueError, match="already assembled"):
        render.add_benchmark(body, *write(tmp_path, report(), own_pii_trace()), "pii-tracer")
    for broken in (trace_report(harness_dirty=True), trace_report(preflight=3),
                   trace_report(rows={"opf": {"test": ROW}})):
        with pytest.raises(ValueError):
            render.add_benchmark(data(), *write(tmp_path, broken, own_pii_trace()), "pii-tracer")
    with pytest.raises(ValueError, match="own-scorer"):
        render.add_benchmark(data(), *write(tmp_path, trace_report(), {**own_pii_trace(), "smoke_limit": 5}), "pii-tracer")


def refused_benchmark(tmp_path: Path, match: str, report_body: dict | None = None, own: dict | None = None) -> None:
    with pytest.raises(ValueError, match=match):
        render.add_benchmark(data(), *write(tmp_path, report_body or trace_report(), own or own_pii_trace()),
                             "pii-tracer")


def test_add_benchmark_refuses_an_own_result_from_other_data_or_other_predictions(tmp_path: Path) -> None:
    own = own_pii_trace()
    own["input"]["dataset_sha256"] = "0" * 64  # scored against another dataset
    refused_benchmark(tmp_path, "own scorer used dataset", own=own)
    own = own_pii_trace()
    own["input"]["prediction_sha256"] = "0" * 64  # scored another run's predictions
    refused_benchmark(tmp_path, "own scorer read predictions", own=own)
    own = own_pii_trace()
    del own["input"]
    refused_benchmark(tmp_path, "no input receipt", own=own)
    body = trace_report()
    del body["provenance"]["pii-tracer"]["prediction_sha256"]
    refused_benchmark(tmp_path, "no prediction digest", report_body=body)


def test_add_benchmark_refuses_a_wrong_message_count(tmp_path: Path) -> None:
    refused_benchmark(tmp_path, "scored 2 messages", own=own_pii_trace(messages=2))
    own = own_pii_trace()
    own["input"]["messages"] = 4
    refused_benchmark(tmp_path, "scored 4 messages", own=own)


@pytest.mark.parametrize("edit", [
    lambda o: o["overall"].update(char_f1=1.0),
    lambda o: o["scored"].update(char_f1=1.0),
    lambda o: o["scored"]["char_level_label_agnostic"].update(f1=1.0),
    lambda o: o["scored"]["exact_typed_micro"].update(f1=1.0),
    lambda o: o["scored"]["exact_typed_f1_by_label"].update(private_person=1.0),
    lambda o: o["input"]["counts"]["char"].update(tp=95),
], ids=["overall", "headline", "char", "typed", "per-label", "counts"])
def test_add_benchmark_refuses_an_edited_score(tmp_path: Path, edit) -> None:
    own = own_pii_trace()
    edit(own)
    refused_benchmark(tmp_path, "not what the receipt's counts give", own=own)


def test_add_benchmark_refuses_impossible_counts(tmp_path: Path) -> None:
    own = own_pii_trace()
    own["input"]["counts"]["char"]["tp"] = 101
    refused_benchmark(tmp_path, "char counts are inconsistent", own=own)


def test_add_tool_runs_the_same_receipt_check(tmp_path: Path) -> None:
    own = own_presidio()
    own["input"]["dataset_sha256"] = "0" * 64
    with pytest.raises(ValueError, match="own scorer used dataset"):
        render.add_tool(data(), *write(tmp_path, report(), own), "pii-tracer")
    own = own_presidio()
    own["scored"]["f2"] = 0.9
    with pytest.raises(ValueError, match="published f2"):
        render.add_tool(data(), *write(tmp_path, report(), own), "pii-tracer")
