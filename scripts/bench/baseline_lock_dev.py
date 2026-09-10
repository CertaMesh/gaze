#!/usr/bin/env python3
"""One frozen DEV256, four explicit arms. Count non-regression is not byte containment."""
import argparse
import copy
import json
import os
from pathlib import Path
import sys

import baseline_lock_stage as stage
import redact_repaired_dev as previous
import run_no_opf_benchmark as runner
import gaze_bench_score as score
from bench_subprocess import BenchSubprocess

LOCK = "pass2-ner-redact-baseline-lock-candidate"
ARMS = (*previous.ARMS, LOCK)
AUDITED = (LOCK, ARMS[2])
LOCK_POLICY = "baseline-lock-candidate-v1"
COUNT_KEYS = {"admitted", "baseline_overlap", "supplemental_overlap"}
LIFECYCLES = {"request_begin", "request_success", "request_refusal", "request_error"}
write_new = stage.write_new
digest = stage.digest


def audit_path(out, phase, arm, variant="candidate"):
    assert arm in ARMS and phase in {"smoke", "dev"} and variant in {"candidate", "reference"}
    return out / f"{phase}-{variant}-{arm}.jsonl"


def lock_audit(path, ids):
    """Validate the concrete count-only schema; keep unknown distinct from complete-empty."""
    rows = [{"request": i + 1, "document_id": uid, "terminal": "unknown", "records": []}
            for i, uid in enumerate(ids)]
    result = {"policy": LOCK_POLICY, "diagnostic_only": True,
              "coordinates": None, "schema_valid": True,
              "file_present": path.exists(), "rows": rows}
    if not path.exists():
        return result
    current = None
    next_ordinal = 1
    batch = False
    try:
        assert path.stat().st_size <= 8 * 1024 * 1024
        with path.open("rb") as handle:
            while line := handle.readline(1025):
                assert len(line) <= 1024 and line.endswith(b"\n")
                record = json.loads(line)
                assert isinstance(record, dict) and record.get("policy") == LOCK_POLICY
                status = record.get("status")
                assert status in LIFECYCLES | {"batch_complete"}
                keys = {"policy", "request", "status"}
                assert set(record) == (keys | COUNT_KEYS if status == "batch_complete" else keys)
                ordinal = record["request"]
                assert type(ordinal) is int and 1 <= ordinal <= len(ids)
                if status == "request_begin":
                    assert current is None and ordinal == next_ordinal
                    current = rows[ordinal - 1]
                    batch = False
                else:
                    assert current is not None and current["request"] == ordinal
                if status == "batch_complete":
                    assert not batch
                    assert all(type(record[key]) is int and record[key] >= 0 for key in COUNT_KEYS)
                    assert sum(record[key] for key in COUNT_KEYS) <= 4096
                    batch = True
                if status == "request_success":
                    assert batch
                current["records"].append(record)
                if status in LIFECYCLES - {"request_begin"}:
                    current["terminal"] = status
                    current = None
                    next_ordinal += 1
    except (OSError, ValueError, TypeError, AssertionError, KeyError):
        # Never copy a malformed line or arbitrary keys into count-only artifacts.
        result["schema_valid"] = False
    return result


def joined_evidence(out, phase, arm, ids, variant="candidate"):
    path = audit_path(out, phase, arm, variant)
    if arm == LOCK:
        return lock_audit(path, ids)
    if path.exists():
        try:
            return previous.joined_audit(path, ids)
        except (OSError, ValueError, TypeError, AssertionError, KeyError):
            pass
    return {"diagnostic_only": True, "coordinates": "detector_input_utf8", "file_present": path.exists(),
            "schema_valid": not path.exists(),
            "rows": [{"request": i + 1, "document_id": uid, "terminal": "unknown", "records": []}
                     for i, uid in enumerate(ids)]}


def count_non_regression(candidate, baseline):
    # Let the unchanged comparator validate source, planned rows, gold and count schemas first.
    comparison = score.compare_output_proofs(candidate, baseline)
    base = {row["document_id"]: row for row in baseline["rows"]}
    rows = []
    for after in candidate["rows"]:
        before = base[after["document_id"]]
        common = before["outcome"] == after["outcome"] == "completed_reversible"
        rows.append({"document_id": after["document_id"], "common_reversible": common,
                     "passed": (after["surviving_bytes"] <= before["surviving_bytes"]
                                and after["full_span_escapes"] <= before["full_span_escapes"])
                     if common else None,
                     "surviving_bytes_delta": after["surviving_bytes"] - before["surviving_bytes"] if common else None,
                     "full_span_escapes_delta": after["full_span_escapes"] - before["full_span_escapes"] if common else None})
    measured = [row for row in rows if row["common_reversible"]]
    return {"kind": "per-row-actual-output-count-non-regression", "raw_byte_containment": "unavailable",
            "passed": bool(measured) and all(row["passed"] for row in measured),
            "unavailable_rows": len(rows) - len(measured), "rows": rows,
            "quality_gates_passed": comparison["passed"]}


def audit_output_binding(audit, proof):
    rows = proof["rows"]
    assert [row["document_id"] for row in audit["rows"]] == [row["document_id"] for row in rows]
    bad = []
    for evidence, output in zip(audit["rows"], rows):
        # A flushed success audit can precede a failed stdout write. It cannot promote an output row.
        if output["outcome"] == "completed_reversible" and evidence["terminal"] != "request_success":
            bad.append(output["document_id"])
    return {"passed": audit["schema_valid"] and audit["file_present"] and not bad
            and any(row["outcome"] == "completed_reversible" for row in rows),
            "successful_output_without_terminal_audit": bad,
            "unknown_terminal_rows": sum(row["terminal"] == "unknown" for row in audit["rows"])}


def smoke_document():
    return score.Document("synthetic-fullwidth-smoke", "😀 plain\n\tＳｃｈｍｉｄｔ  end",
                          "en", "US", "synthetic", (score.Span(12, 33, "LASTNAME"),))


def exchange(repo, binary, arm, document, environment):
    with BenchSubprocess([str(binary), "--config", arm], cwd=repo, env=environment) as process:
        response = score.validate_response(document, process.exchange({
            "fixture_id": document.uid, "locale_chain": document.locale_chain, "text": document.text}))
    return response


def semantic_response(response):
    value = copy.deepcopy(response)
    value.pop("timing", None)
    return value


def validate_smoke(value, freeze):
    assert value["freeze_sha256"] == freeze
    assert set(value["arms"]) == set(ARMS)
    assert set(value["exact_raw_interval"]) == set(ARMS[1:])
    assert set(value["reference_equivalence"]) == set(previous.ARMS)
    assert all(item is True for item in value["reference_equivalence"].values())
    for arm in ARMS:
        rows = value["arms"][arm]
        assert len(rows) == 1 and rows[0]["document_id"] == smoke_document().uid
        score.output_proof_sidecar(arm, score.output_source_contract([smoke_document()]), rows)
        row = rows[0]
        assert row["outcome"] == "completed_reversible"
        assert row["exact_restore_count"] == 1 and row["redact_count"] == 0
        if arm != ARMS[0]:
            assert row["surviving_bytes"] == 0
            assert value["exact_raw_interval"][arm] is True
    assert value["lock_audit_binding"]["passed"] is True
    assert value["lock_audit_binding"]["unknown_terminal_rows"] == 0


def run_smoke(repo, out, pins, freeze):
    document = smoke_document()
    results, parity, intervals = {}, {}, {}
    for arm in ARMS:
        env = arm_environment(pins, out, "smoke", arm)
        actual = exchange(repo, repo / stage.PRODUCER, arm, document, env)
        results[arm] = [score.observe_output(document, actual)]
        if arm != ARMS[0]:
            intervals[arm] = any(item["raw_start"] == 12 and item["raw_end"] == 33
                                 and item["action"] == "tokenize"
                                 and (arm == LOCK or any(source.startswith("redact-patched-coreml-v1:")
                                      for source in item["provenance"]["source_ids"]))
                                 for item in actual.get("final_protection_trace", []))
        if arm in previous.ARMS:
            reference = exchange(repo, repo / stage.REFERENCE, arm, document,
                                 arm_environment(pins, out, "smoke", arm, "reference"))
            parity[arm] = semantic_response(actual) == semantic_response(reference)
        # Only validated counts/booleans leave memory, never response text or token fields.
    evidence = lock_audit(audit_path(out, "smoke", LOCK), [document.uid])
    result = {"freeze_sha256": freeze, "arms": results, "reference_equivalence": parity,
              "exact_raw_interval": intervals,
              "lock_audit_binding": audit_output_binding(evidence, {"rows": results[LOCK]})}
    write_new(out / "smoke.json", result)
    write_new(out / "smoke-joined-lock.json", evidence)
    validate_smoke(result, freeze)


def arm_environment(pins, out, phase, arm, variant="candidate"):
    environment = dict(os.environ)
    environment.pop("GAZE_NER_LOCALE", None)
    environment.update(pins["model_environment"])
    environment["GAZE_REDACT_ADMISSION_AUDIT_FILE"] = str(audit_path(out, phase, arm, variant))
    return environment


def measure_four_arms(repo, out, docs, pins, davlan, kiji):
    ids = [doc.uid for doc in docs]
    proof_dir = out / "output-proof-v1"
    sidecars = proof_dir / "repetition-1"
    sidecars.mkdir(parents=True, exist_ok=False)
    for arm in ARMS:
        write_new(sidecars / (arm + ".json"), score.output_proof_sidecar(
            arm, pins["source_contract"], [score.planned_output_row(doc) for doc in docs]))
    try:
        measurements = score.collect_validator_measurements(repo / stage.VALIDATOR, docs, ids)
        runs, repetitions = [], []
        for arm in ARMS:
            run, repetition = runner.execute_measurements(
                repo_root=repo, binary=repo / stage.PRODUCER, documents=docs,
                davlan_model=davlan, kiji_model=kiji, threshold=0.3,
                diagnostics_dir=out / "logs", warmup_count=0, measured_repetitions=1,
                validator_measurements=measurements, source_environment=arm_environment(pins, out, "dev", arm),
                configs=(arm,), output_proof_dir=proof_dir)
            runs.extend(run)
            repetitions.extend(repetition)
        write_new(out / "runs.json", {"runs": runs, "repetitions": repetitions})
    finally:
        for arm in AUDITED:
            write_new(out / ("joined-" + arm + ".json"), joined_evidence(out, "dev", arm, ids))
    return {arm: json.loads((sidecars / (arm + ".json")).read_text()) for arm in ARMS}


def common_four_arm_summary(proofs):
    indexed = {arm: score._validated_output_rows(proofs[arm]) for arm in ARMS}
    contract = proofs[ARMS[0]]["source_contract"]
    assert all(proof["source_contract"] == contract for proof in proofs.values())
    common = {uid for uid in indexed[ARMS[0]]
              if all(indexed[arm][uid]["outcome"] == "completed_reversible" for arm in ARMS)}
    return {"population": score.identified_document_population(common),
            "gold_bytes": sum(indexed[ARMS[0]][uid]["gold_bytes"] for uid in common),
            "arms": {arm: {
                "counts": {field: sum(rows[uid][field] for uid in common) for field in sorted(score.VERIFIED_COUNTS)},
                "negative_false_positive_bytes": sum(rows[uid]["false_positive_bytes"] for uid in common
                                                     if rows[uid]["negative"]),
                "exact_restore_count": sum(rows[uid]["exact_restore_count"] for uid in common),
                "redact_count": sum(rows[uid]["redact_count"] for uid in common),
                "planned_outcomes": {outcome: sum(row["outcome"] == outcome for row in rows.values())
                                     for outcome in sorted(score.ROW_OUTCOMES)},
                "unavailable_gold_bytes": sum(row["gold_bytes"] for row in rows.values()
                                              if row["outcome"] != "completed_reversible"),
            } for arm, rows in indexed.items()}}


def run(args):
    if not __debug__:
        raise RuntimeError("assertion guards required")
    assert stage.supervisor.time.time() < stage.deadline_value(args.deadline_utc) - stage.supervisor.CLEANUP_RESERVE
    repo = Path(__file__).resolve().parents[2]
    out = repo / stage.PAIRED
    out.mkdir(parents=True, exist_ok=True)
    state = stage.source_state(repo)
    assert state["clean"]
    receipts = {name: stage.validate_receipt(repo, name, state, args.deadline_utc)
                for name in stage.PREREQUISITES}
    for key, value in stage.environment().items():
        assert os.environ.get(key) == value
    bridge = Path(os.environ["GAZE_REDACT_BRIDGE"])
    assert digest(bridge) == previous.BRIDGE_SHA
    davlan = Path.home() / ".local/share/gaze/models/davlan-mbert-ner-hrl"
    kiji = Path.home() / ".local/share/gaze/models/kiji-distilbert"
    models = runner.validate_required_models(repo, davlan, kiji)
    private = previous.verify_private_model()
    frozen_ids = repo / stage.ROOT / "frozen-dev.json"
    dataset = Path("/Users/krishankoenig/Workspace/EmpireTwo/gaze/target/bench-data/dataiku-en-de/test.parquet")
    docs = previous.selected_documents(repo, frozen_ids, dataset)
    ids = [doc.uid for doc in docs]
    pins = {"source": state, "arms": list(ARMS), "repetitions": 1, "warmups": 0,
            "ordered_ids": ids, "ordered_ids_sha256": previous.IDS_SHA,
            "frozen_file_sha256": digest(frozen_ids), "source_contract": score.output_source_contract(docs),
            "build_outputs": {str(path): digest(repo / path) for path in
                              (stage.PRODUCER, stage.REFERENCE, stage.VALIDATOR)},
            "bridge_sha256": digest(bridge), "private_model_manifest_sha256": private, "models": models,
            "threshold": 0.3, "redact_threshold": 0.6, "org_enabled": True,
            "deadline_utc": args.deadline_utc, "receipt_sha256": receipts,
            "supervisor_sha256": stage.SUPERVISOR_SHA, "stage_sha256": digest(stage.__file__),
            "model_environment": {"GAZE_NER_MODEL_DIR": str(davlan), "GAZE_NER_THRESHOLD": "0.3",
                                  "GAZE_KIJI_DISTILBERT_MODEL_DIR": str(kiji),
                                  "GAZE_REDACT_BRIDGE": str(bridge),
                                  "GAZE_REDACT_MODEL_DIR": os.environ["GAZE_REDACT_MODEL_DIR"]},
            "bridge_environment": {"DAL_APP_ID": "gaze-local-redact-primary", "DAL_COREML_COMPUTE_UNITS": "all"}}
    if args.phase == "freeze":
        write_new(out / "freeze.json", pins)
        return 0
    assert json.loads((out / "freeze.json").read_text()) == pins
    freeze_receipt = stage.validate_receipt(repo, "freeze", state, args.deadline_utc)
    freeze = digest(out / "freeze.json")
    if args.phase == "smoke":
        run_smoke(repo, out, pins, freeze)
        return 0
    validate_smoke(json.loads((out / "smoke.json").read_text()), freeze)
    smoke_receipt = stage.validate_receipt(repo, "smoke", state, args.deadline_utc)
    write_new(out / "dev-started.json", {"source": state, "arms": list(ARMS), "planned": len(ids),
                                       "freeze_sha256": freeze, "freeze_receipt_sha256": freeze_receipt,
                                       "smoke_sha256": digest(out / "smoke.json"),
                                       "smoke_receipt_sha256": smoke_receipt})
    proofs = measure_four_arms(repo, out, docs, pins, davlan, kiji)
    comparisons = []
    for arm in ARMS[1:]:
        result = score.compare_output_proofs(proofs[arm], proofs[ARMS[0]])
        write_new(out / ("comparison-" + arm + ".json"), result)
        comparisons.append(result)
    write_new(out / "common-four-arm.json", common_four_arm_summary(proofs))
    # A second reference comparison uses the same four-arm outputs, never another inference.
    write_new(out / "comparison-lock-vs-semantic.json", score.compare_output_proofs(proofs[LOCK], proofs[ARMS[2]]))
    invariant = count_non_regression(proofs[LOCK], proofs[ARMS[0]])
    write_new(out / "count-non-regression.json", invariant)
    binding = audit_output_binding(json.loads((out / ("joined-" + LOCK + ".json")).read_text()), proofs[LOCK])
    write_new(out / "audit-output-binding.json", binding)
    # This is a report disposition only; there is no promotion or follow-up command.
    return 0 if all(item["passed"] for item in comparisons) and invariant["passed"] and binding["passed"] else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("freeze", "smoke", "dev"))
    parser.add_argument("--deadline-utc", required=True)
    try:
        sys.exit(run(parser.parse_args()))
    except Exception as error:
        print(json.dumps({"error_type": type(error).__name__}), file=sys.stderr)
        sys.exit(2)
