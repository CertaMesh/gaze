"""A scoring contract must never silently trigger another detection capture."""

import contextlib
import gzip
import io
import json
import subprocess
import sys
import unittest
from pathlib import Path
from unittest import mock

import run_no_opf_benchmark as runner
import rescore_past_release as past
import known_record_arm
import known_record_cells
import gaze_bench_score as score
import scorecard_record as record


class RecordOnceTests(unittest.TestCase):
    def test_legacy_oracle_captures_fail_before_reading_inputs(self):
        args = [
            "--binary", "missing-fixture-binary", "--policy", "missing-fixture-policy",
            "--model-dir", "missing-fixture-model", "--contract", "v2",
            "--output", "missing-fixture-output",
        ]
        for module, prefix in ((known_record_arm, []), (known_record_cells, ["prove"])):
            with self.subTest(driver=module.__name__):
                result = subprocess.run(
                    [sys.executable, module.__file__, *prefix, *args],
                    capture_output=True, text=True, timeout=20,
                )
                self.assertEqual(result.returncode, 2)
                self.assertIn("requires --cross-check", result.stderr)
                parsed = module.parse_args([*prefix, *args, "--cross-check"])
                self.assertTrue(parsed.cross_check)

    def test_v2_record_replays_independent_v1_scorecard(self):
        fixtures = Path(__file__).parent / "fixtures"
        with gzip.open(fixtures / "record-once.jsonl.gz", "rt") as stream:
            captured = json.loads(next(stream))["scorecard"]
        self.assertEqual(captured["scoring"]["scored_label_contract"]["version"], 2)
        replay = record.rescore(
            fixtures / "record-once.jsonl.gz", score.SCORED_LABEL_CONTRACT_V1,
            max_workers=1,
        )
        expected = json.loads((fixtures / "record-once-v1.json").read_text())
        replay.pop("observation_record")
        self.assertEqual(replay, expected)

        # Build a fresh producer response from synthetic text, independently of
        # the value-free record and frozen card. Exercise the direct boundary,
        # including response validation; replay_responses is deliberately absent.
        email = "alice@example.invalid"
        text = f"{email} {email} secret"
        document = score.Document(
            "synthetic-1", text, "en", "US", "synthetic",
            (score.Span(len(text) - 6, len(text), "PASSWORD"),
             score.Span(0, len(email), "EMAIL")),
        )
        start, end = len(email) + 1, len(email) * 2 + 1
        response = {
            "fixture_id": document.uid,
            "clean_text": f"{email} <Email_1> secret",
            "manifest_spans": [{"raw_start": start, "raw_end": end,
                                "clean_start": start, "clean_end": start + 9, "class": "email"}],
            "pre_safety_text_len": None, "pre_safety_manifest_spans": None,
            "leak_suspects": [], "safety_net_mode": "disabled",
            "final_protection_trace": [{
                "raw_start": start, "raw_end": end, "class": "email", "action": "tokenize",
                "provenance": {"stage": "primary_pipeline", "decision": "policy",
                               "source_ids": ["synthetic:email"]},
            }],
            "restore": {"exact": True, "decision": "success", "unknown_token_count": 0,
                        "manifest_bypass_count": 0, "fresh_pii_detected_count": 0,
                        "phase_execution_mask": 0},
            "manifest_integrity": {"spans": 1, "invalid_clean_bounds": 0,
                                   "invalid_raw_bounds": 0, "overlapping_clean_spans": 0,
                                   "non_monotonic_raw_spans": 0,
                                   "token_restore_failures": 0, "raw_value_mismatches": 0},
            "initial_safety_net_stats": {"suspect_count": 0, "uncovered_count": 0,
                                         "partial_bleed_count": 0, "class_mismatch_count": 0,
                                         "locale_skipped_count": 0},
            "strict_would_reject": False, "post_policy_safety_net_stats": None,
            "timing": {"clean_ms": 1.0, "restore_ms": 1.0, "post_policy_scan_ms": None},
        }
        measurements = {
            "validator_kinds_by_class": {},
            "documents": {document.uid: {
                "gold_validation": [
                    {"start": span.start, "end": span.end, "label": span.label,
                     "applicable": False, "validator_passed": None} for span in document.spans
                ],
                "predictions": None,
            }},
        }
        process = mock.MagicMock(message_deadline=None)
        process.exchange.return_value = response
        with mock.patch.object(score, "BenchSubprocess") as producer, mock.patch.object(
            score.time, "perf_counter", side_effect=[0.0, 0.0, 0.0, 1.0],
        ):
            producer.return_value.__enter__.return_value = process
            direct_run = score.run_config(
                Path("."), Path("synthetic-producer"), "policy-file", [document], Path("."),
                None, None, None, 0.3, Path("."), validator_measurements=measurements,
            )
        process.exchange.assert_called_once_with({
            "fixture_id": document.uid, "locale_chain": document.locale_chain, "text": text,
        })
        direct = {
            "schema_version": 4, "generated_at": "synthetic",
            "gaze": {"revision": "synthetic", "dirty": False},
            "dataset": {"integrity": {"sha256": "0" * 64},
                        "validator_gold_census": score.validator_gold_census([document], measurements)},
            "scoring": {"scored_label_contract": score.scored_label_contract_report(
                score.SCORED_LABEL_CONTRACT_V1, [document])},
            "parameters": {"ner_threshold": 0.3, "configs": ["policy-file"]},
            "runs": [direct_run],
        }
        self.assertEqual(direct, replay)

    def test_contract_capture_requires_explicit_cross_check(self):
        for flag in ("--scored-labels", "--agentic-scored-labels"):
            with self.subTest(flag=flag):
                with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                    runner.parse_args(["full", flag, "contract.json"])
                self.assertEqual(error.exception.code, 2)

    def test_cross_check_retains_independent_capture_for_equivalence_checks(self):
        args = runner.parse_args([
            "full", "--cross-check", "--scored-labels", "contract.json",
        ])
        self.assertTrue(args.cross_check)

    def test_past_release_contract_capture_requires_cross_check(self):
        args = [
            "--release-root", ".", "--binary", "fixture-binary",
            "--binary-profile", "release", "--configs", "policy-file",
            "--output", "fixture.json", "--scored-labels", "contract.json",
        ]
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
            past.parse_args(args)
        self.assertEqual(error.exception.code, 2)
        self.assertTrue(past.parse_args([*args, "--cross-check"]).cross_check)


if __name__ == "__main__":
    unittest.main()
