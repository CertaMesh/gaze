"""Contract replay must depend on observations and retain no document values."""

import gzip
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaze_bench_score as score
import scorecard_record as record


ROOT = Path(__file__).resolve().parents[2]


class RecordReplayTests(unittest.TestCase):
    def setUp(self):
        email = "alice@example.invalid"
        text = f"{email} {email} secret"
        self.document = score.Document(
            "synthetic-1", text, "en", "US", "synthetic",
            (score.Span(0, len(email), "EMAIL"),
             score.Span(len(text) - 6, len(text), "PASSWORD")),
        )
        self.prediction = score.Span(len(email) + 1, len(email) * 2 + 1, "email")
        self.response = {
            "fixture_id": self.document.uid,
            "final_protection_trace": [{
                "raw_start": self.prediction.start, "raw_end": self.prediction.end,
                "class": "email", "action": "tokenize",
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
                                         "partial_bleed_count": 0,
                                         "class_mismatch_count": 0,
                                         "locale_skipped_count": 0},
            "strict_would_reject": False,
            "post_policy_safety_net_stats": None,
            "timing": {"clean_ms": 1.0, "restore_ms": 1.0,
                       "post_policy_scan_ms": None},
        }
        self.measurements = {
            "validator_kinds_by_class": {},
            "documents": {self.document.uid: {
                "gold_validation": [
                    {"start": span.start, "end": span.end, "label": span.label,
                     "applicable": False, "validator_passed": None}
                    for span in self.document.spans
                ],
                "predictions": None,
            }},
        }

    def make_record(self, path):
        run = score.run_config(
            Path("."), Path("."), "policy-file", [self.document], Path("."),
            None, None, None, 0.3, Path("."),
            validator_measurements=self.measurements,
            replay_responses={self.document.uid: self.response},
        )
        card = {
            "schema_version": 4, "generated_at": "synthetic",
            "gaze": {"revision": "synthetic", "dirty": False},
            "dataset": {"integrity": {"sha256": "0" * 64},
                        "validator_gold_census": score.validator_gold_census(
                            [self.document], self.measurements)},
            "scoring": {"scored_label_contract": score.scored_label_contract_report(
                score.SCORED_LABEL_CONTRACT_V1, [self.document])},
            "parameters": {"ner_threshold": 0.3, "configs": ["policy-file"]},
            "runs": [run],
        }
        writer = record.RecordWriter(
            [self.document], self.measurements, corpus_sha256="0" * 64
        )
        writer.add("C", "policy-file", self.document, self.response, self.measurements)
        writer.write(path, card, add_reference=False)
        return card

    def test_v1_is_exact_v2_changes_gold_and_v3_credits_repeat(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "observations.jsonl.gz"
            original = self.make_record(path)
            self.assertEqual(record.rescore(path, score.SCORED_LABEL_CONTRACT_V1), original)
            v2 = record.rescore(
                path, score.load_scored_label_contract(
                    ROOT / "docs/reference/benchmarks/scored-labels-v2.json"
                )
            )
            self.assertEqual(v2["runs"][0]["metrics"]["utf8_bytes"]["pii"], 21)
            v3 = record.rescore(
                path, score.load_scored_label_contract(
                    ROOT / "docs/reference/benchmarks/scored-labels-v3.json"
                )
            )
            self.assertEqual(
                v3["runs"][0]["metrics"]["gold_gap"]["gold_gap_protected_bytes"], 21
            )
            self.assertNotIn("alice@example.invalid", path.read_bytes().decode("latin1"))
            with gzip.open(path, "rt", encoding="utf-8") as stream:
                body = stream.read()
            self.assertNotIn("alice@example.invalid", body)
            self.assertNotIn("secret", body)

    def test_mutated_prediction_changes_score(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "observations.jsonl.gz"
            self.make_record(path)
            before = record.rescore(path, score.SCORED_LABEL_CONTRACT_V1)
            with gzip.open(path, "rt", encoding="utf-8") as stream:
                rows = [json.loads(line) for line in stream]
            rows[1]["response"]["final_protection_trace"] = []
            with gzip.open(path, "wt", encoding="utf-8") as stream:
                for row in rows:
                    stream.write(json.dumps(row) + "\n")
            after = record.rescore(path, score.SCORED_LABEL_CONTRACT_V1)
            self.assertNotEqual(
                before["runs"][0]["metrics"]["utf8_bytes"],
                after["runs"][0]["metrics"]["utf8_bytes"],
            )


if __name__ == "__main__":
    unittest.main()
