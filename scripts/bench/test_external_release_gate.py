from copy import deepcopy
import hashlib
import json

import pytest

import external_release_gate as gate
from external_quality_receipt import COUNTERS, INPUTS, RECEIPTS, SETS


def quality(kind="sealed"):
    receipts = {name: "a" * (40 if name.endswith("revision") else 64) for name in RECEIPTS}
    receipts["inputs_sha256"] = {name: "b" * 64 for name in INPUTS[kind]}
    row = {
        "attempted_documents": 2, "completed_documents": 2, "failed_closed_documents": 0,
        "contract_documents": 2, "restore_exact_documents": 2, "restore_failures": 0,
        "restore_success_decisions": 2, "restore_decision_failures": 0,
        "manifest_valid_documents": 2, "manifest_invalid_documents": 0,
    }
    return {"schema_version": 1, "benchmark": kind, "receipts": receipts,
            "quality": {key: deepcopy(row) for key in SETS[kind]}}


def sealed_scores():
    return {"results": {key: {"documents": 2, "failed_closed": 0, "gold_bytes": 100,
                              "leaked_bytes": 20, "false_positive_bytes": 3}
                        for key in SETS["sealed"]}}


def rows(kind="sealed"):
    return {key: {**quality(kind)["quality"][key], "gold_bytes": 100,
                  "leaked_bytes": 20, "false_positive_bytes": 3} for key in SETS[kind]}


class NoTraversal(dict):
    def __iter__(self):
        raise AssertionError("native report traversal")

    def items(self):
        raise AssertionError("native report traversal")

    def values(self):
        raise AssertionError("native report traversal")


def test_published_schema_is_aggregate_only():
    before = sealed_scores()
    after = sealed_scores()
    after["results"]["gretel/en"]["leaked_bytes"] = 10
    for report in (before, after):
        report["documents"] = "PRIVATE_SENTINEL_DOCUMENT_ID_TEXT_SPANS_FAILURE"
        for key, row in list(report["results"].items()):
            row["failure_details"] = "PRIVATE_SENTINEL_DOCUMENT_ID_TEXT_SPANS_FAILURE"
            report["results"][key] = NoTraversal(row)
    report = gate.build_report(NoTraversal(before), quality(), NoTraversal(after), quality())
    assert set(report) == {"schema_version", "benchmark", "contract", "gate_script_sha256",
                           "base", "candidate", "gate"}
    for side in ("base", "candidate"):
        assert set(report[side]) == {"receipts", "sets"}
        assert set(report[side]["receipts"]) == RECEIPTS
        assert all(set(row) == COUNTERS | gate.BYTE_FIELDS for row in report[side]["sets"].values())
    assert "PRIVATE_SENTINEL" not in json.dumps(report)
    assert report["gate"]["verdict"] == "pass"


@pytest.mark.parametrize("kind", ["piibench-commercial", "pii-trace"])
def test_generic_report_uses_only_all_gold_product_aggregates(kind):
    native = NoTraversal({"documents": 2, "true_positive_bytes": 80,
                          "leaked_bytes": 20, "false_positive_bytes": 3,
                          "failure_details": "PRIVATE_SENTINEL"})
    scores = NoTraversal({"rows": {"gaze-full": {"test": {"product_coverage": native,
                                                         "common_intersection": "PRIVATE_SENTINEL"}}}})
    result = gate.aggregate_rows(scores, quality(kind))
    assert result["test"]["gold_bytes"] == 100
    assert "PRIVATE_SENTINEL" not in json.dumps(result)


def test_overlap_is_guarded_but_not_counted_twice():
    before, after = rows(), rows()
    after["openpii/all"]["leaked_bytes"] -= 10
    after["openpii/en-de-fr"]["leaked_bytes"] -= 8
    result = gate.decide("sealed", before, after)
    assert result["verdict"] == "pass"
    assert result["leaked_bytes_decrease"] == 10
    after["openpii/en-de-fr"]["leaked_bytes"] = 21
    assert gate.decide("sealed", before, after)["verdict"] == "fail"
    after = deepcopy(before)
    after["openpii/en-de-fr"]["false_positive_bytes"] = 0
    assert gate.decide("sealed", before, after)["verdict"] == "fail"


@pytest.mark.parametrize("regression", ["leak", "refusal", "restore", "decision", "manifest", "population"])
def test_any_set_regression_blocks_another_sets_gain(regression):
    before, after = rows(), rows()
    after["openpii/all"]["leaked_bytes"] = 0
    row = after["gretel/de"]
    if regression == "leak":
        row["leaked_bytes"] += 1
    elif regression == "refusal":
        row.update(completed_documents=1, failed_closed_documents=1, contract_documents=1,
                   restore_exact_documents=1, restore_success_decisions=1, manifest_valid_documents=1)
    elif regression == "population":
        row.update(attempted_documents=3, failed_closed_documents=1)
    else:
        passed, failed = {"restore": ("restore_exact_documents", "restore_failures"),
                          "decision": ("restore_success_decisions", "restore_decision_failures"),
                          "manifest": ("manifest_valid_documents", "manifest_invalid_documents")}[regression]
        row[passed] -= 1
        row[failed] += 1
    assert gate.decide("sealed", before, after)["verdict"] == "fail"


def test_gain_boundaries_and_reversibility_gain():
    before, after = rows(), rows()
    assert gate.decide("sealed", before, after)["verdict"] == "fail"
    after["gretel/en"]["leaked_bytes"] -= 10
    after["gretel/en"]["false_positive_bytes"] += 10
    assert gate.decide("sealed", before, after)["verdict"] == "fail"
    after["gretel/en"]["false_positive_bytes"] -= 1
    assert gate.decide("sealed", before, after)["verdict"] == "pass"
    before, after = rows(), rows()
    after["gretel/en"]["false_positive_bytes"] -= 1
    assert gate.decide("sealed", before, after)["verdict"] == "pass"
    before, after = rows(), rows()
    before["gretel/en"].update(restore_exact_documents=1, restore_failures=1)
    assert gate.decide("sealed", before, after)["reason"] == "reversibility_improved"


@pytest.mark.parametrize("field", ["driver_sha256", "wrapper_sha256", "harness_revision",
                                   "ner_manifest_sha256", "nym_manifest_sha256", "inputs_sha256"])
def test_measurement_identity_mismatch_is_rejected(field):
    first, second = quality(), quality()
    value = second["receipts"][field]
    second["receipts"][field] = ({key: "c" * 64 for key in value} if isinstance(value, dict)
                                  else "c" * len(value))
    with pytest.raises(ValueError):
        gate.build_report(sealed_scores(), first, sealed_scores(), second)


def test_extra_quality_fields_and_wrong_output_digest_are_rejected(tmp_path):
    bad = quality()
    bad["quality"]["gretel/en"]["document_id"] = "PRIVATE_SENTINEL"
    with pytest.raises(ValueError):
        gate.aggregate_rows(sealed_scores(), bad)
    payload = json.dumps(sealed_scores()).encode()
    scores, receipt = tmp_path / "scores.json", tmp_path / "quality.json"
    scores.write_bytes(payload)
    receipt.write_text(json.dumps(quality()))
    with pytest.raises(ValueError):
        gate.load_pair(scores, receipt)
    valid = quality()
    valid["receipts"]["driver_output_sha256"] = hashlib.sha256(payload).hexdigest()
    receipt.write_text(json.dumps(valid))
    assert gate.load_pair(scores, receipt)[0] == sealed_scores()


def test_cli_error_exposes_only_fixed_code(tmp_path, monkeypatch, capsys):
    scores, receipt, output = (tmp_path / name for name in ("scores.json", "quality.json", "gate.json"))
    scores.write_text(json.dumps({"private_failure_details": "PRIVATE_SENTINEL"}))
    value = quality()
    value["receipts"]["driver_output_sha256"] = hashlib.sha256(scores.read_bytes()).hexdigest()
    receipt.write_text(json.dumps(value))
    monkeypatch.setattr("sys.argv", ["external_release_gate.py", "--base-scores", str(scores),
                                    "--base-quality", str(receipt), "--candidate-scores", str(scores),
                                    "--candidate-quality", str(receipt), "--output", str(output)])
    assert gate.main() == 1
    captured = capsys.readouterr()
    assert captured.out == "EXTERNAL_AGGREGATE_GATE_FAILED\n"
    assert captured.err == ""
    assert not output.exists()
