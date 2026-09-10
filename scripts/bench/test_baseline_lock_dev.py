"""Synthetic source contract tests; no model/corpus access. Execute only after root grant."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import baseline_lock_dev as dev
import baseline_lock_stage as stage
import gaze_bench_score as score
import run_no_opf_benchmark as runner


def lifecycle(ordinal, status):
    return {"policy": dev.LOCK_POLICY, "request": ordinal, "status": status}


def complete(ordinal, **counts):
    return dict(lifecycle(ordinal, "batch_complete"),
                **{key: counts.get(key, 0) for key in dev.COUNT_KEYS})


class AuditTests(unittest.TestCase):
    def joined(self, records, tail=b""):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "audit.jsonl"
            path.write_bytes(b"".join(json.dumps(record).encode() + b"\n" for record in records) + tail)
            return dev.lock_audit(path, ["synthetic-a", "synthetic-b", "synthetic-c"])

    def test_refusal_unknown_complete_empty_and_final_flush_missing_are_distinct(self):
        value = self.joined([lifecycle(1, "request_begin"), lifecycle(1, "request_refusal"),
                             lifecycle(2, "request_begin"), complete(2), lifecycle(2, "request_success"),
                             lifecycle(3, "request_begin"), complete(3, admitted=2)])
        self.assertTrue(value["schema_valid"])
        a, b, c = value["rows"]
        self.assertEqual(a["terminal"], "request_refusal")
        self.assertTrue(all(not (dev.COUNT_KEYS & record.keys()) for record in a["records"]))
        self.assertEqual(b["terminal"], "request_success")
        self.assertEqual(b["document_id"], "synthetic-b")
        self.assertTrue(all(b["records"][1][key] == 0 for key in dev.COUNT_KEYS))
        self.assertEqual(c["terminal"], "unknown")
        self.assertEqual(c["records"][1]["admitted"], 2)
        self.assertIsNone(value["coordinates"])

    def test_malformed_counts_unknown_keys_policy_ordinals_and_duplicates_refuse(self):
        begin = lifecycle(1, "request_begin")
        cases = [
            [dict(begin, admitted=0)],
            [dict(begin, policy="semantic-policy")],
            [lifecycle(2, "request_begin")],
            [dict(begin, request=True)],
            [begin, lifecycle(1, "request_success")],
            [begin, complete(1, admitted=True)],
            [begin, complete(1, admitted=-1)],
            [begin, complete(1, admitted=4097)],
            [begin, complete(2)],
            [begin, complete(1), complete(1)],
            [begin, dict(complete(1), text="synthetic forbidden field")],
            [begin, complete(1), lifecycle(1, "request_success"), begin],
        ]
        for records in cases:
            with self.subTest(records=records):
                value = self.joined(records)
                self.assertFalse(value["schema_valid"])
                self.assertEqual(len(value["rows"]), 3)
                self.assertNotIn("synthetic forbidden field", json.dumps(value))

    def test_missing_file_and_partial_line_never_invent_zero(self):
        with tempfile.TemporaryDirectory() as directory:
            value = dev.lock_audit(Path(directory) / "absent", ["synthetic-a"])
        self.assertFalse(value["file_present"])
        self.assertEqual(value["rows"][0]["records"], [])
        value = self.joined([lifecycle(1, "request_begin")], b'{"status":')
        self.assertFalse(value["schema_valid"])
        self.assertEqual(value["rows"][0]["terminal"], "unknown")

    def test_literal_duplicate_json_keys_never_become_valid_evidence(self):
        # Literal bytes are essential: a Python dict would erase the duplicate before encoding.
        lines = [
            b'{"policy":"baseline-lock-candidate-v1","request":9,"request":1,"status":"request_begin"}\n',
            b'{"policy":"wrong","policy":"baseline-lock-candidate-v1","request":1,"status":"request_begin"}\n',
            b'{"policy":"baseline-lock-candidate-v1","request":1,"status":"request_error","status":"request_begin"}\n',
        ]
        for line in lines:
            value = self.joined([], line)
            self.assertFalse(value["schema_valid"])
            self.assertEqual(value["rows"][0]["records"], [])
        value = self.joined([lifecycle(1, "request_begin")],
            b'{"policy":"baseline-lock-candidate-v1","request":1,"status":"batch_complete",'
            b'"admitted":4097,"admitted":0,"baseline_overlap":0,"supplemental_overlap":0}\n')
        self.assertFalse(value["schema_valid"])
        self.assertEqual(value["rows"][0]["records"], [lifecycle(1, "request_begin")])
        self.assertEqual(value["rows"][0]["terminal"], "unknown")

    def test_sparse_audit_fails_even_when_missing_outputs_are_unmeasured(self):
        records = [lifecycle(1, "request_begin"), complete(1), lifecycle(1, "request_success")]
        outputs = {"rows": [{"document_id": uid, "outcome": outcome} for uid, outcome in
                            (("synthetic-a", "completed_reversible"),
                             ("synthetic-b", "unmeasured"), ("synthetic-c", "fail_closed"))]}
        sparse = self.joined(records)
        result = dev.audit_output_binding(sparse, outputs)
        self.assertFalse(result["passed"])
        self.assertEqual(result["unknown_terminal_rows"], 2)
        self.assertFalse(result["planned_terminal_coverage_complete"])
        self.assertEqual(sparse["rows"][1]["records"], [])
        complete_audit = self.joined(records + [lifecycle(2, "request_begin"), lifecycle(2, "request_error"),
                                               lifecycle(3, "request_begin"), lifecycle(3, "request_refusal")])
        self.assertTrue(dev.audit_output_binding(complete_audit, outputs)["passed"])
        self.assertEqual(outputs["rows"][1]["outcome"], "unmeasured")

    def test_complete_refusals_are_failure_evidence_not_protection(self):
        records = []
        for ordinal in (1, 2, 3):
            records.extend([lifecycle(ordinal, "request_begin"), lifecycle(ordinal, "request_refusal")])
        audit = self.joined(records)
        outputs = {"rows": [{"document_id": row["document_id"], "outcome": "fail_closed"} for row in audit["rows"]]}
        result = dev.audit_output_binding(audit, outputs)
        self.assertTrue(result["passed"])
        self.assertTrue(result["planned_terminal_coverage_complete"])
        self.assertTrue(result["row_terminals_consistent"])
        self.assertTrue(all(row["outcome"] == "fail_closed" for row in outputs["rows"]))
        outputs["rows"][0]["outcome"] = "completed_reversible"
        self.assertFalse(dev.audit_output_binding(audit, outputs)["passed"])
        audit["schema_valid"] = False
        outputs["rows"][0]["outcome"] = "fail_closed"
        self.assertFalse(dev.audit_output_binding(audit, outputs)["passed"])

    def test_success_audit_does_not_promote_failed_output(self):
        audit = self.joined([lifecycle(1, "request_begin"), complete(1), lifecycle(1, "request_success")])
        proof = {"rows": [{"document_id": uid, "outcome": "unmeasured"}
                           for uid in ("synthetic-a", "synthetic-b", "synthetic-c")]}
        self.assertFalse(dev.audit_output_binding(audit, proof)["passed"])
        self.assertTrue(all(row["outcome"] == "unmeasured" for row in proof["rows"]))
        proof["rows"][1]["outcome"] = "completed_reversible"
        self.assertFalse(dev.audit_output_binding(audit, proof)["passed"])

    def test_arm_phase_and_reference_paths_are_unique_and_exclusive(self):
        paths = {dev.audit_path(Path("/synthetic"), phase, arm, variant)
                 for phase in ("smoke", "dev") for arm in dev.ARMS
                 for variant in ("candidate", "reference")}
        self.assertEqual(len(paths), 16)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "proof.json"
            stage.write_new(path, {})
            with self.assertRaises(FileExistsError):
                stage.write_new(path, {})


def proof(config, leaks=(2, 2), fp=(0, 0), refused=()):
    docs = [score.Document(f"synthetic-{i}", "abcd", "en", "US", "synthetic",
                           (score.Span(0, 4, "NAME"),)) for i in range(2)]
    rows = []
    for i, doc in enumerate(docs):
        row = score.planned_output_row(doc)
        if i in refused:
            row.update(outcome="fail_closed", stage="clean", reason="recognizer_detect")
        else:
            row.update(outcome="completed_reversible", reason=None, exact_restore_count=1, redact_count=0,
                       surviving_bytes=leaks[i], verified_covered_bytes=4-leaks[i], false_positive_bytes=fp[i],
                       full_span_escapes=int(leaks[i] > 0), fully_surviving_spans=int(leaks[i] == 4))
        rows.append(row)
    return score.output_proof_sidecar(config, score.output_source_contract(docs), rows)


class CountTests(unittest.TestCase):
    def test_aggregate_improvement_cannot_hide_one_worse_row(self):
        before = proof(dev.ARMS[0], (3, 1))
        after = proof(dev.LOCK, (0, 2))
        self.assertTrue(score.compare_output_proofs(after, before)["passed"])
        result = dev.count_non_regression(after, before)
        self.assertFalse(result["passed"])
        self.assertEqual([row["passed"] for row in result["rows"]], [True, False])

    def test_refused_missing_rows_are_unknown_and_quality_gate_still_fails(self):
        before = proof(dev.ARMS[0])
        after = proof(dev.LOCK, (0, 0), refused=(1,))
        result = dev.count_non_regression(after, before)
        self.assertTrue(result["passed"])
        self.assertFalse(result["quality_gates_passed"])
        self.assertEqual(result["unavailable_rows"], 1)
        self.assertIsNone(result["rows"][1]["passed"])
        self.assertFalse(dev.count_non_regression(proof(dev.LOCK, refused=(0, 1)), before)["passed"])

    def test_more_fp_and_equal_counts_do_not_become_quality_or_containment_claim(self):
        before = proof(dev.ARMS[0])
        result = dev.count_non_regression(proof(dev.LOCK, (0, 0), fp=(1, 0)), before)
        self.assertTrue(result["passed"])
        self.assertFalse(result["quality_gates_passed"])
        equal = dev.count_non_regression(proof(dev.LOCK), before)
        self.assertTrue(equal["passed"])
        self.assertFalse(equal["quality_gates_passed"])
        self.assertEqual(equal["raw_byte_containment"], "unavailable")

    def test_four_arm_common_denominator_keeps_full_planned_unavailability(self):
        proofs = {arm: proof(arm, refused=(1,) if arm == dev.LOCK else ()) for arm in dev.ARMS}
        result = dev.common_four_arm_summary(proofs)
        self.assertEqual(result["gold_bytes"], 4)
        self.assertEqual(result["arms"][dev.LOCK]["unavailable_gold_bytes"], 4)
        self.assertEqual(result["arms"][dev.ARMS[0]]["unavailable_gold_bytes"], 0)
        self.assertEqual(result["arms"][dev.ARMS[0]]["planned_outcomes"]["completed_reversible"], 2)
        self.assertEqual(result["arms"][dev.ARMS[0]]["exact_restore_count"], 1)


class FailureRetentionTests(unittest.TestCase):
    def test_validator_failure_retains_all_planned_arms_and_unknown_audit(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            out = repo / "paired"
            out.mkdir()
            docs = [dev.smoke_document()]
            pins = {"source_contract": score.output_source_contract(docs), "model_environment": {}}
            with mock.patch.object(score, "collect_validator_measurements", side_effect=ValueError), \
                 mock.patch.object(runner, "execute_measurements") as execute:
                with self.assertRaises(ValueError):
                    dev.measure_four_arms(repo, out, docs, pins, repo, repo)
                execute.assert_not_called()
            for arm in dev.ARMS:
                sidecar = json.loads((out / "output-proof-v1/repetition-1" / (arm + ".json")).read_text())
                self.assertEqual(sidecar["rows"][0]["outcome"], "unmeasured")
                self.assertIsNone(sidecar["rows"][0]["surviving_bytes"])
            audit = json.loads((out / ("joined-" + dev.LOCK + ".json")).read_text())
            self.assertFalse(audit["file_present"])
            self.assertEqual(audit["rows"][0]["terminal"], "unknown")

    def test_four_arms_run_once_with_zero_warmups_and_distinct_audit_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            out = repo / "paired"
            out.mkdir()
            docs = [dev.smoke_document()]
            pins = {"source_contract": score.output_source_contract(docs), "model_environment": {}}
            with mock.patch.object(score, "collect_validator_measurements", return_value={}), \
                 mock.patch.object(runner, "execute_measurements", return_value=([], [])) as execute:
                dev.measure_four_arms(repo, out, docs, pins, repo, repo)
            calls = [call.kwargs for call in execute.call_args_list]
            self.assertEqual([call["configs"] for call in calls], [(arm,) for arm in dev.ARMS])
            self.assertTrue(all(call["warmup_count"] == 0 and call["measured_repetitions"] == 1 for call in calls))
            self.assertEqual(len({call["source_environment"]["GAZE_REDACT_ADMISSION_AUDIT_FILE"] for call in calls}), 4)


class SmokeTests(unittest.TestCase):
    def good(self):
        row = dict(score.planned_output_row(dev.smoke_document()), outcome="completed_reversible",
                   reason=None, surviving_bytes=0, exact_restore_count=1, redact_count=0,
                   verified_covered_bytes=21, false_positive_bytes=0,
                   full_span_escapes=0, fully_surviving_spans=0)
        return {"freeze_sha256": "synthetic-freeze", "arms": {arm: [copy.deepcopy(row)] for arm in dev.ARMS},
                "reference_equivalence": {arm: True for arm in dev.previous.ARMS},
                "exact_raw_interval": {arm: True for arm in dev.ARMS[1:]},
                "lock_audit_binding": {"passed": True, "unknown_terminal_rows": 0}}

    def test_missing_arm_duplicate_row_stale_freeze_wrong_row_failed_reference_and_flush_refuse(self):
        dev.validate_smoke(self.good(), "synthetic-freeze")
        cases = []
        value = self.good(); del value["arms"][dev.LOCK]; cases.append(value)
        value = self.good(); value["arms"][dev.LOCK] *= 2; cases.append(value)
        value = self.good(); value["freeze_sha256"] = "stale"; cases.append(value)
        value = self.good(); value["arms"][dev.LOCK][0]["document_id"] = "wrong"; cases.append(value)
        value = self.good(); value["reference_equivalence"][dev.ARMS[0]] = False; cases.append(value)
        value = self.good(); value["lock_audit_binding"]["unknown_terminal_rows"] = 1; cases.append(value)
        value = self.good(); value["exact_raw_interval"][dev.LOCK] = False; cases.append(value)
        for value in cases:
            with self.assertRaises((AssertionError, KeyError)):
                dev.validate_smoke(value, "synthetic-freeze")


class StageTests(unittest.TestCase):
    def test_hostile_inherited_targets_cannot_redirect_hashed_build_outputs(self):
        env = stage.build_environment({"CARGO_TARGET_DIR": "/synthetic/elsewhere",
                                       "CARGO_BUILD_TARGET": "synthetic-other-triple"})
        self.assertNotIn("CARGO_TARGET_DIR", env)
        self.assertNotIn("CARGO_BUILD_TARGET", env)
        commands = stage.commands("2099-01-01T00:00:00+00:00")
        expected = {"workspace-bootstrap": Path("target"), "producer-build": stage.PRODUCER,
                    "reference-build": stage.REFERENCE, "validator-build": stage.VALIDATOR}
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            for name, output in expected.items():
                command = commands[name]
                target = Path(command[command.index("--target-dir") + 1])
                self.assertNotIn("--target", command)
                if name == "workspace-bootstrap":
                    self.assertEqual(target, output)
                    continue
                suffix = Path("debug/validator-recall-probe") if name == "validator-build" else Path("debug/examples/clean_for_bench")
                self.assertEqual(target / suffix, output)
                (repo / output).parent.mkdir(parents=True, exist_ok=True)
                (repo / output).write_bytes(b"synthetic build output")
                self.assertEqual(stage.output_hashes(repo, name), {str(output): stage.digest(repo / output)})

    def test_explicit_deadline_all_builds_locked_offline_same_checkout_and_marker(self):
        deadline = "2099-01-01T00:00:00+00:00"  # Synthetic contract only, never a runtime default.
        commands = stage.commands(deadline)
        for name in ("workspace-bootstrap", "producer-build", "reference-build", "validator-build"):
            self.assertIn("--locked", commands[name])
            self.assertIn("--offline", commands[name])
        self.assertIn("safety-net-kiji,redact-live,benchmark-baseline-lock", commands["producer-build"])
        self.assertNotIn("--features", commands["validator-build"])
        self.assertEqual(commands["producer-build"][-2:], ["--target-dir", "target"])
        self.assertIsNone(stage.environment()["CARGO_TARGET_DIR"])
        self.assertIsNone(stage.environment()["CARGO_BUILD_TARGET"])
        self.assertNotIn("benchmark-baseline-lock", ",".join(commands["reference-build"]))
        self.assertEqual(commands["dev"][-1], deadline)
        with self.assertRaises(ValueError):
            stage.deadline_value("2099-01-01T00:00:00")

    def test_receipt_rejects_changed_source_command_output_deadline_and_cleanup(self):
        deadline = "2099-01-01T00:00:00+00:00"
        source = {"head": "synthetic-head", "clean": True, "tracked_source_sha256": "synthetic-source"}
        good = {"step": "producer-build", "status": "exited", "exit_code": 0,
                "source_before": source, "source_after": source,
                "command": stage.commands(deadline)["producer-build"], "effective_environment": {},
                "toolchain_sha256": {}, "supervisor_sha256": stage.SUPERVISOR_SHA,
                "stage_sha256": stage.SUPERVISOR_SHA, "deadline_utc": deadline,
                "cleanup_reserve_seconds": 5.0, "owned_remaining": 0, "output_sha256": {},
                "log_sha256": stage.SUPERVISOR_SHA}
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            path = repo / stage.ROOT / "producer-build.json"
            path.parent.mkdir(parents=True)
            with mock.patch.object(stage, "digest", return_value=stage.SUPERVISOR_SHA), \
                 mock.patch.object(stage, "environment", return_value={}), \
                 mock.patch.object(stage, "toolchain_hashes", return_value={}), \
                 mock.patch.object(stage, "output_hashes", return_value={}):
                path.write_text(json.dumps(good))
                stage.validate_receipt(repo, "producer-build", source, deadline)
                for field, replacement in (("source_after", {}), ("command", []),
                                           ("output_sha256", {"stale": "binary"}),
                                           ("deadline_utc", "2098-01-01T00:00:00+00:00"),
                                           ("owned_remaining", 1), ("log_sha256", "stale")):
                    path.write_text(json.dumps(dict(good, **{field: replacement})))
                    with self.assertRaises(AssertionError):
                        stage.validate_receipt(repo, "producer-build", source, deadline)


class FeatureTests(unittest.TestCase):
    def test_fourth_arm_alone_adds_marker_and_references_keep_exact_build_argv(self):
        for arm in (*dev.previous.ARMS[1:], dev.LOCK):
            with mock.patch.object(runner.subprocess, "run") as run:
                runner.build_selected_binary(Path("/synthetic/repo"), (arm,))
            features = "redact-live,benchmark-baseline-lock" if arm == dev.LOCK else "redact-live"
            self.assertEqual(run.call_args.args[0], [
                "cargo", "build", "--locked", "-q", "-p", "gaze-recognizers",
                "--example", "clean_for_bench", "--features", features])
        with mock.patch.object(runner.dataiku, "build_binary") as ordinary:
            runner.build_selected_binary(Path("/synthetic/repo"), (dev.ARMS[0],))
            ordinary.assert_called_once_with(Path("/synthetic/repo"), (dev.ARMS[0],))
        self.assertNotIn(dev.LOCK, score.DEFAULT_CONFIGS)


if __name__ == "__main__":
    unittest.main()
