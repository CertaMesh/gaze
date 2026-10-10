from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import render_pii_tracer as page  # noqa: E402


def contract(leaked: int) -> dict:
    metrics = {"leaked_bytes": leaked, "false_positive_bytes": 9, "document_leak_rate": 0.25,
               "char_level": {"f2": 0.8}}
    cells = {layer: {"leaked_bytes": leaked, "false_positive_bytes": 9,
                     "metrics": {"product_coverage": {"full": metrics}}} for layer in page.LAYERS}
    return {version: cells for version in page.CONTRACTS}


def reports() -> tuple[dict, dict]:
    shared = {"corpus": {"agentic": {"generator_version": 4},
                         "layers": {layer: {"documents": 10} for layer in page.LAYERS}},
              "contracts": {version: "x" for version in page.CONTRACTS},
              "common_intersection_labels": ["EMAIL"]}
    committed = {**shared, "tools": {"opf": {"contracts": contract(50)}, "gliner": {"contracts": contract(20)}}}
    runtime = {"device": "cpu", "dtype": "bfloat16", "torch": "1", "transformers": "5",
               "worker_sha256": "w" * 64, "requirements_sha256": "r" * 64}
    report = {**copy.deepcopy(shared), "harness_dirty": False, "harness_revision": "0123456789ab",
              "hardware": "hw", "contention": {"foreign_cpu_samples": [{"foreign_cpu_percent": 40.0}]},
              "tools": {page.TOOL: {"contracts": contract(5), "provenance": {
                  "model_repo": "perplexity-ai/PII-Tracer", "revision": "d25c16f2", "runtime": runtime}}}}
    return report, committed


def test_rows_are_sorted_by_leak_and_include_pii_tracer() -> None:
    body = page.render(*reports())
    order = [line.split(" | ")[0][2:] for line in body.splitlines() if line.startswith("| ") and " | 0.8" in line]
    assert order == ["pii-tracer", "gliner", "opf"]
    assert "Foreign CPU stayed at or under 100%" in body and "generator v4" in body


def test_latency_is_withheld_when_foreign_cpu_exceeded_the_rule() -> None:
    report, committed = reports()
    report["contention"]["foreign_cpu_samples"].append({"foreign_cpu_percent": 250.0})
    assert "Latency is withheld" in page.render(report, committed)


@pytest.mark.parametrize("key", ["corpus", "contracts", "common_intersection_labels"])
def test_refuses_a_different_corpus_contract_or_label_set(key: str) -> None:
    report, committed = reports()
    report[key] = {"other": 1} if key != "common_intersection_labels" else ["IBAN"]
    with pytest.raises(ValueError, match="not comparable"):
        page.render(report, committed)


def test_refuses_a_dirty_or_multi_tool_report() -> None:
    report, committed = reports()
    report["harness_dirty"] = True
    with pytest.raises(ValueError, match="clean single-tool"):
        page.render(report, committed)
    report, committed = reports()
    report["tools"]["opf"] = {}
    with pytest.raises(ValueError, match="clean single-tool"):
        page.render(report, committed)


@pytest.mark.skipif(not page.REPORT.exists(), reason="report not committed yet")
def test_committed_page_matches_its_data() -> None:
    assert page.PAGE.read_text(encoding="utf-8") == page.render(
        json.loads(page.REPORT.read_text(encoding="utf-8")), json.loads(page.COMMITTED.read_text(encoding="utf-8")))
