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

import render_theirbench as render  # noqa: E402

CELL = {"leaked_bytes": 5, "false_positive_bytes": 2, "true_positive_bytes": 95, "document_leak_rate": 0.1,
        "typed_entities": {"f1": 0.4, "f2": 0.5}}
ROW = {"product_coverage": CELL, "common_intersection": CELL, "latency": {"p50_ms": 1.0}}
ENTRY = {"identity": {"documents": 3}, "splits": {"test": {"documents": 3, "ids_sha256": "aa"}},
         "common_intersection_labels": ["EMAIL"], "mapping_sha256": "m", "typed_hold": ["opf"],
         "rows": {"opf": ROW}, "provenance": {"opf": {}}, "own_metric": {"opf": {"f2": 0.1}}}


def report(**changes: object) -> dict:
    base = {"benchmark": "presidio-research", "identity": ENTRY["identity"], "splits": ENTRY["splits"],
            "common_intersection_labels": ["EMAIL"], "mapping_sha256": "m", "typed_hold": ["opf", "pii-tracer"],
            "rows": {"pii-tracer": {"test": ROW}}, "provenance": {"pii-tracer": {"cpu": {}}},
            "harness_revision": "0123456789ab", "harness_dirty": False, "gaze_crates_tree": "t",
            "comparison_revision": "b1446215", "label_maps_sha256": "l",
            "rescored_with": {"harness_dirty": False, "harness_revision": "abcdef012345"}}
    return {**base, **changes}


def write(tmp_path: Path, report_body: dict, own: dict | None = None) -> tuple[Path, Path]:
    rp, op = tmp_path / "report.json", tmp_path / "own.json"
    rp.write_text(json.dumps(report_body), encoding="utf-8")
    op.write_text(json.dumps(own or {"system": "pii-tracer", "overall": {"f2": 0.6}}), encoding="utf-8")
    return rp, op


def data() -> dict:
    return {"benchmarks": {"presidio-research": copy.deepcopy(ENTRY)}}


def test_adds_the_row_and_leaves_existing_rows_alone(tmp_path: Path) -> None:
    before, after = data(), data()
    render.add_tool(after, *write(tmp_path, report()), "pii-tracer")
    entry = after["benchmarks"]["presidio-research"]
    assert entry["rows"]["opf"] == before["benchmarks"]["presidio-research"]["rows"]["opf"]
    assert entry["rows"]["pii-tracer"] == ROW and entry["own_metric"]["pii-tracer"] == {"f2": 0.6}
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
