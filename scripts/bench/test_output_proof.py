"""Synthetic actual-output mutations and paired-population attacks, no models."""
import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gaze_bench_score as score
import run_no_opf_benchmark as runner
from bench_subprocess import ProducerFailure


def document(text="é synthetic", uid="synthetic-proof", spans=None):
    return score.Document(uid, text, "en", "US", "synthetic-proof-v1",
                          tuple(spans if spans is not None else [score.Span(0, 2, "NAME")]))


def response(doc, operations=()):
    """Operations are (raw_start, raw_end, replacement or None for deletion)."""
    raw = doc.text.encode()
    clean = bytearray()
    manifest, trace = [], []
    cursor = 0
    for start, end, replacement in operations:
        clean.extend(raw[cursor:start])
        if replacement is not None:
            cstart = len(clean)
            clean.extend(replacement.encode())
            manifest.append(dict(raw_start=start, raw_end=end, clean_start=cstart,
                                 clean_end=len(clean), **{"class": "name"}))
        trace.append(dict(raw_start=start, raw_end=end, **{
            "class": "name", "action": "tokenize" if replacement is not None else "redact",
            "provenance": {"stage": "primary_pipeline" if replacement is not None else "safety_net",
                           "decision": "policy" if replacement is not None else "fallback_redact",
                           "source_ids": ["rule.synthetic"]},
        }))
        cursor = end
    clean.extend(raw[cursor:])
    return {
        "fixture_id": doc.uid, "clean_text": clean.decode(), "manifest_spans": manifest,
        "pre_safety_text_len": None, "pre_safety_manifest_spans": None,
        "leak_suspects": [], "safety_net_mode": "off", "strict_would_reject": False,
        "initial_safety_net_stats": {key: 0 for key in score.SAFETY_NET_STATS_FIELDS},
        "post_policy_safety_net_stats": None,
        "restore": {"exact": not any(op[2] is None for op in operations), "decision": "success",
                    "unknown_token_count": 0, "manifest_bypass_count": 0,
                    "fresh_pii_detected_count": 0, "phase_execution_mask": 1},
        "manifest_integrity": {key: len(manifest) if key == "spans" else 0
                               for key in score.MANIFEST_INTEGRITY_FIELDS},
        "timing": {"clean_ms": 1.0, "restore_ms": 0.1, "post_policy_scan_ms": None},
        "final_protection_trace": trace,
    }


def sidecar(docs, responses, config="pass2-ner"):
    return score.output_proof_sidecar(config, score.output_source_contract(docs),
                                     [score.observe_output(doc, value) for doc, value in zip(docs, responses)])


class ReplayTests(unittest.TestCase):
    def test_legitimate_unicode_copy_token_deletion_and_multiple_tokens(self):
        doc = document("é xx 文 yy", spans=[score.Span(0, 2, "NAME"), score.Span(6, 9, "NAME")])
        for operations in [(), [(0, 2, "<Name_1>")], [(0, 2, None)],
                           [(0, 2, "<Name_1>"), (6, 9, "<Name_2>")],
                           [(0, 2, None), (6, 9, "<Name_2>")]]:
            with self.subTest(operations=operations):
                row = score.observe_output(doc, response(doc, operations))
                self.assertNotEqual(row["outcome"], "unmeasured")
                removed = sum(end - start for start, end, _ in operations)
                self.assertEqual(row["surviving_bytes"], 5 - removed)

    def test_detection_normalization_does_not_change_passthrough(self):
        doc = document("Ａ\u200d e\u0301 Å", spans=[])
        self.assertEqual(score.observe_output(doc, response(doc))["outcome"], "completed_reversible")
        value = response(doc)
        value["clean_text"] = "A é Å"
        self.assertEqual(score.observe_output(doc, value)["reason"], "output_replay")

    def test_forged_redact_trace_leaving_raw_never_scores_zero(self):
        doc = document()
        value = response(doc, [(0, 2, None)])
        value["clean_text"] = doc.text
        row = score.observe_output(doc, value)
        self.assertEqual(row["outcome"], "unmeasured")
        self.assertEqual(row["reason"], "output_replay")
        self.assertIsNone(row["surviving_bytes"])
        self.assertIsNone(row["exact_restore_count"])
        self.assertIsNone(row["redact_count"])

    def test_copy_mutations_and_wrong_token_offsets(self):
        doc = document("ab é cd 文 ef", spans=[score.Span(3, 5, "NAME"), score.Span(9, 12, "NAME")])
        original = response(doc, [(3, 5, "<Name_1>"), (9, 12, "<Name_1>")])
        mutations = []
        for text in [original["clean_text"] + "ab ", original["clean_text"].replace("ab ", "cd ", 1),
                     original["clean_text"].replace(" ef", "")]:
            value = copy.deepcopy(original)
            value["clean_text"] = text
            mutations.append(value)
        value = copy.deepcopy(original)
        for key in ("clean_start", "clean_end"):
            value["manifest_spans"][0][key], value["manifest_spans"][1][key] = (
                value["manifest_spans"][1][key], value["manifest_spans"][0][key])
        mutations.append(value)
        value = copy.deepcopy(original)
        value["manifest_spans"][1].update(clean_start=3, clean_end=11)
        mutations.append(value)
        for value in mutations:
            self.assertEqual(score.observe_output(doc, value)["outcome"], "unmeasured")

    def test_equal_token_elsewhere_cannot_validate_shifted_mapping(self):
        doc = document("<Name_1> é", spans=[score.Span(9, 11, "NAME")])
        value = response(doc, [(9, 11, "<Name_1>")])
        value["manifest_spans"][0].update(clean_start=0, clean_end=8)
        self.assertEqual(score.observe_output(doc, value)["reason"], "manifest_mapping")

    def test_actual_utf8_bounds_and_duplicate_trace(self):
        doc = document()
        original = response(doc, [(0, 2, "<Name_1>")])
        for field, bad in [("raw_start", 1), ("raw_end", 100), ("clean_end", 100), ("clean_start", 3)]:
            value = copy.deepcopy(original)
            value["manifest_spans"][0][field] = bad
            self.assertEqual(score.observe_output(doc, value)["outcome"], "unmeasured")
        value = copy.deepcopy(original)
        value["final_protection_trace"].append(copy.deepcopy(value["final_protection_trace"][0]))
        self.assertEqual(score.observe_output(doc, value)["outcome"], "unmeasured")
        doc = document("abc", spans=[score.Span(0, 1, "NAME")])
        value = response(doc, [(0, 1, "é")])
        value["manifest_spans"][0]["clean_end"] = 1
        self.assertEqual(score.observe_output(doc, value)["reason"], "manifest_mapping")

    def test_overlap_drop_uses_final_manifest_only(self):
        doc = document("é xx 文", spans=[score.Span(0, 2, "NAME"), score.Span(6, 9, "NAME")])
        value = response(doc, [(0, 9, None)])
        value["pre_safety_manifest_spans"] = [
            dict(raw_start=0, raw_end=2, clean_start=0, clean_end=8, **{"class": "name"})]
        value["pre_safety_text_len"] = 15
        row = score.observe_output(doc, value)
        self.assertEqual(row["outcome"], "completed_nonreversible")
        self.assertEqual(row["surviving_bytes"], 0)
        self.assertEqual(row["false_positive_bytes"], 4)
        value["manifest_spans"] = value["pre_safety_manifest_spans"]
        value["manifest_integrity"]["spans"] = 1
        self.assertEqual(score.observe_output(doc, value)["outcome"], "unmeasured")

    def test_integrity_and_restore_precedence(self):
        doc = document()
        for field in score.MANIFEST_INTEGRITY_FIELDS - {"spans"}:
            value = response(doc, [(0, 2, "<Name_1>")])
            value["manifest_integrity"][field] = 1
            row = score.observe_output(doc, value)
            self.assertEqual(row["reason"], "manifest_integrity")
            self.assertIsNone(row["exact_restore_count"])
            self.assertIsNone(row["redact_count"])
        for operations in [(), [(0, 2, None)]]:
            value = response(doc, operations)
            value["restore"]["decision"] = "unknown_token"
            self.assertEqual(score.observe_output(doc, value)["outcome"], "restore_failure")
        value = response(doc)
        value["restore"]["exact"] = False
        self.assertEqual(score.observe_output(doc, value)["outcome"], "restore_failure")

    def test_missing_or_mismatched_manifest_twins_fail_proof(self):
        doc = document()
        original = response(doc, [(0, 2, "<Name_1>")])
        for mutation in ("missing_manifest", "missing_trace", "class_mismatch", "raw_mismatch"):
            value = copy.deepcopy(original)
            if mutation == "missing_manifest":
                value["manifest_spans"] = []
                value["manifest_integrity"]["spans"] = 0
            elif mutation == "missing_trace":
                value["final_protection_trace"] = []
            elif mutation == "class_mismatch":
                value["manifest_spans"][0]["class"] = "email"
            else:
                value["manifest_spans"][0]["raw_end"] = 3
            row = score.observe_output(doc, value)
            self.assertEqual(row["outcome"], "unmeasured", mutation)
            self.assertIsNone(row["surviving_bytes"])
            self.assertIsNone(row["exact_restore_count"])
            self.assertIsNone(row["redact_count"])

    def test_refusal_has_no_restore_or_redact_observation(self):
        doc = document()
        value = {"fixture_id": doc.uid, "pipeline_error_stage": "clean",
                 "pipeline_error_code": "policy", "timing": {"total_ms": 1.0}}
        row = score.observe_output(doc, value)
        self.assertEqual(row["outcome"], "fail_closed")
        self.assertIsNone(row["exact_restore_count"])
        self.assertIsNone(row["redact_count"])

    def test_gold_union_partial_and_full_span_counts(self):
        doc = document("abcdef", spans=[score.Span(0, 4, "NAME"), score.Span(2, 6, "NAME")])
        row = score.observe_output(doc, response(doc, [(0, 2, "<Name_1>")]))
        self.assertEqual((row["gold_bytes"], row["surviving_bytes"], row["full_span_escapes"],
                          row["fully_surviving_spans"]), (6, 4, 2, 1))


class PairedTests(unittest.TestCase):
    def setUp(self):
        self.docs = [document(uid="synthetic-a"), document(uid="synthetic-b")]
        self.base = sidecar(self.docs, [response(doc) for doc in self.docs])
        self.cand = sidecar(self.docs, [response(doc, [(0, 2, "<Name_1>")]) for doc in self.docs])

    def test_reversible_byte_gain_passes_and_export_has_only_counts(self):
        result = score.compare_output_proofs(self.cand, self.base)
        self.assertTrue(result["passed"])
        self.assertEqual(result["common_completed"]["candidate_minus_baseline"]["surviving_bytes"], -4)
        serialized = json.dumps(self.cand)
        self.assertNotIn("<Name_1>", serialized)
        self.assertNotIn("clean_text", serialized)
        self.assertNotIn("manifest", serialized)

    def test_population_gaming_refusal_restore_error_and_unmeasured(self):
        for kind in ("fail_closed", "restore_failure", "unmeasured"):
            cand = copy.deepcopy(self.cand)
            if kind == "fail_closed":
                value = {"fixture_id": self.docs[1].uid, "pipeline_error_stage": "clean",
                         "pipeline_error_code": "policy", "timing": {"total_ms": 1.0}}
            elif kind == "restore_failure":
                value = response(self.docs[1])
                value["restore"]["decision"] = "unknown_token"
            else:
                value = {}
            cand["rows"][1] = score.observe_output(self.docs[1], value)
            result = score.compare_output_proofs(cand, self.base)
            self.assertFalse(result["passed"])
            self.assertIn("additional_refusal_or_error", result["failures"])
            self.assertEqual(result["baseline_completed"]["unavailable_gold_bytes"], 2)

    def test_equal_availability_counts_cannot_swap_failed_population(self):
        base, cand = copy.deepcopy(self.base), copy.deepcopy(self.cand)
        base["rows"][0] = score.planned_output_row(self.docs[0])
        cand["rows"][1] = score.planned_output_row(self.docs[1])
        self.assertIn("additional_refusal_or_error", score.compare_output_proofs(cand, base)["failures"])

    def test_nonreversible_gain_fails_restoration(self):
        cand = sidecar(self.docs, [response(doc, [(0, 2, None)]) for doc in self.docs])
        result = score.compare_output_proofs(cand, self.base)
        self.assertIn("incomplete_restoration", result["failures"])
        self.assertEqual(result["common_reversible"]["population"]["documents"], 0)

    def test_negative_regression_not_hidden_by_positive_fp_gain(self):
        docs = [document("ab", spans=[score.Span(0, 1, "NAME")]),
                document("xy", uid="synthetic-negative", spans=[])]
        base = sidecar(docs, [response(docs[0], [(1, 2, "<Name_1>")]), response(docs[1])])
        cand = sidecar(docs, [response(docs[0], [(0, 1, "<Name_1>")]),
                              response(docs[1], [(0, 1, "<Name_1>")])])
        self.assertIn("false_positive_or_negative_regression", score.compare_output_proofs(cand, base)["failures"])

    def test_fewer_bytes_cannot_hide_more_full_span_escapes(self):
        doc = document("abcdefghi", spans=[score.Span(0, 6, "NAME"),
                                           score.Span(6, 7, "NAME"), score.Span(7, 8, "NAME")])
        base = sidecar([doc], [response(doc, [(6, 8, "<Name_1>")])])
        cand = sidecar([doc], [response(doc, [(0, 6, "<Name_1>")])])
        result = score.compare_output_proofs(cand, base)
        self.assertLess(result["common_completed"]["candidate_minus_baseline"]["surviving_bytes"], 0)
        self.assertIn("more_full_span_escapes", result["failures"])

    def test_source_contract_duplicates_missing_and_unknown_outcome_rejected(self):
        mutations = []
        for rows in [self.cand["rows"][:1], self.cand["rows"] + self.cand["rows"][:1]]:
            value = copy.deepcopy(self.cand)
            value["rows"] = rows
            mutations.append(value)
        value = copy.deepcopy(self.cand)
        value["source_contract"]["source_gold_sha256"] = "0" * 64
        mutations.append(value)
        value = copy.deepcopy(self.cand)
        value["rows"][0]["outcome"] = "success"
        mutations.append(value)
        for value in mutations:
            with self.assertRaises(ValueError):
                score.compare_output_proofs(value, self.base)

    def test_source_contract_binds_text_gold_and_order_independent_population(self):
        original = score.output_source_contract(self.docs)
        self.assertEqual(original, score.output_source_contract(self.docs[::-1]))
        changed = [document("é changed", uid=self.docs[0].uid), self.docs[1]]
        self.assertNotEqual(original, score.output_source_contract(changed))


class RunnerTests(unittest.TestCase):
    def test_real_run_config_observer_keeps_legacy_counts_separate(self):
        docs = [document(uid="synthetic-a"), document(uid="synthetic-b")]
        values = [response(docs[0], [(0, 2, None)]), response(docs[1], [(0, 2, "<Name_1>")])]
        values[0]["clean_text"] = docs[0].text
        process = mock.Mock()
        process.__enter__ = mock.Mock(return_value=process)
        process.__exit__ = mock.Mock(return_value=False)
        process.message_deadline = 0
        process.exchange.side_effect = values
        with mock.patch.object(score, "BenchSubprocess", return_value=process):
            rows = []
            run = score.run_config(
                repo_root=Path("/synthetic"), binary=Path("/synthetic/producer"),
                config="pass2-ner", documents=docs, model_dir=Path("/synthetic"),
                kiji_model_dir=Path("/synthetic"), opf_command=None, opf_checkpoint=None,
                opf_daemon_socket=None, threshold=0.3, diagnostics_dir=Path("/synthetic"),
                output_rows=rows,
            )
        self.assertEqual(run["metrics"]["utf8_bytes"]["leaked"], 0)
        self.assertEqual(rows[0]["outcome"], "unmeasured")
        self.assertIsNone(rows[0]["surviving_bytes"])
        self.assertEqual(rows[1]["outcome"], "completed_reversible")

    def test_frozen_ids_preserve_order_and_reject_duplicates_or_missing(self):
        docs = [document(uid="synthetic-a"), document(uid="synthetic-b")]
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "ids.json"
            path.write_text(json.dumps([docs[1].uid, docs[0].uid]))
            selected, report = runner.frozen_id_selection(docs, path)
            self.assertEqual(selected, docs[::-1])
            self.assertEqual(report["evaluated_document_ids_digest"], score.document_ids_digest([doc.uid for doc in selected]))
            for ids in [[docs[0].uid] * 2, ["absent"], []]:
                path.write_text(json.dumps(ids))
                with self.assertRaises(runner.CandidateError):
                    runner.frozen_id_selection(docs, path)

    def test_backend_failure_exports_every_planned_row_for_every_arm(self):
        docs = [document(uid="synthetic-a"), document(uid="synthetic-b")]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with mock.patch.object(score, "run_config", side_effect=ProducerFailure("protocol")):
                with self.assertRaises(ProducerFailure):
                    runner.execute_measurements(
                        repo_root=root, binary=root / "unused", documents=docs,
                        davlan_model=root, kiji_model=root, threshold=0.3,
                        diagnostics_dir=root / "logs", warmup_count=0,
                        measured_repetitions=1, output_proof_dir=root / "proof")
            for config in score.DEFAULT_CONFIGS:
                saved = json.loads((root / "proof" / "repetition-1" / f"{config}.json").read_text())
                self.assertEqual(len(score._validated_output_rows(saved)), 2)
                self.assertTrue(all(row["outcome"] == "unmeasured" for row in saved["rows"]))

    def test_cancellation_invalidates_observed_rows_and_accounts_for_future_arms(self):
        docs = [document(uid="synthetic-a"), document(uid="synthetic-b")]

        def interrupted(*args, **kwargs):
            kwargs["output_rows"].append(score.observe_output(docs[0], response(docs[0])))
            raise KeyboardInterrupt()

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with mock.patch.object(score, "run_config", side_effect=interrupted):
                with self.assertRaises(KeyboardInterrupt):
                    runner.execute_measurements(
                        repo_root=root, binary=root / "unused", documents=docs,
                        davlan_model=root, kiji_model=root, threshold=0.3,
                        diagnostics_dir=root / "logs", warmup_count=0,
                        measured_repetitions=1, output_proof_dir=root / "proof")
            for config in score.DEFAULT_CONFIGS:
                saved = json.loads((root / "proof" / "repetition-1" / f"{config}.json").read_text())
                self.assertEqual(set(score._validated_output_rows(saved)), {doc.uid for doc in docs})
                for row in saved["rows"]:
                    self.assertEqual(row["outcome"], "unmeasured")
                    self.assertIsNone(row["surviving_bytes"])
                    self.assertIsNone(row["exact_restore_count"])
                    self.assertIsNone(row["redact_count"])

    def test_named_redact_build_feature_and_no_new_threshold(self):
        args = runner.parse_args(["quick", "--config", "pass2-ner", "--config", "pass2-ner-redact", "--output-proof"])
        self.assertEqual(args.config, ["pass2-ner", "pass2-ner-redact"])
        with mock.patch.object(runner.subprocess, "run") as run:
            runner.build_selected_binary(Path("/synthetic"), tuple(args.config))
        self.assertIn("redact-live", run.call_args.args[0])


if __name__ == "__main__":
    unittest.main()
