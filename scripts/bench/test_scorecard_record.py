"""Contract replay must depend on observations and retain no document values."""

import gzip
import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaze_bench_score as score
import agentic_layers as agentic
import scorecard_record as record


ROOT = Path(__file__).resolve().parents[2]


class RecordReplayTests(unittest.TestCase):
    def setUp(self):
        email = "alice@example.invalid"
        text = f"{email} {email} secret"
        self.document = score.Document(
            "synthetic-1", text, "en", "US", "synthetic",
            (score.Span(len(text) - 6, len(text), "PASSWORD"),
             score.Span(0, len(email), "EMAIL")),
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
            rows = [json.loads(line) for line in body.splitlines()]
            self.assertNotIn("timing", rows[1]["response"])
            second = Path(temporary) / "second.jsonl.gz"
            self.make_record(second)
            with gzip.open(second, "rt", encoding="utf-8") as stream:
                second_rows = [json.loads(line) for line in stream]
            self.assertEqual(rows[1:], second_rows[1:])

    def test_mutated_prediction_changes_score(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "observations.jsonl.gz"
            self.make_record(path)
            before = record.rescore(path, score.SCORED_LABEL_CONTRACT_V1)
            with gzip.open(path, "rt", encoding="utf-8") as stream:
                rows = [json.loads(line) for line in stream]
            rows[1]["response"]["final_protection_trace"][0]["raw_start"] = 0
            rows[1]["response"]["final_protection_trace"][0]["raw_end"] = 21
            rows[1]["gold_gap_evidence"][0][:5] = [0, 21, "email", 0, 21]
            with gzip.open(path, "wt", encoding="utf-8") as stream:
                for row in rows:
                    stream.write(json.dumps(row) + "\n")
            after = record.rescore(path, score.SCORED_LABEL_CONTRACT_V1)
            self.assertNotEqual(
                before["runs"][0]["metrics"]["utf8_bytes"],
                after["runs"][0]["metrics"]["utf8_bytes"],
            )

    def test_historical_template_requires_exact_correctness(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, template, pinned = (
                root / "capture.gz", root / "committed.json", root / "pinned.gz"
            )
            card = self.make_record(source)
            template.write_text(json.dumps(card), encoding="utf-8")
            record.pin_template(source, template, pinned)
            self.assertEqual(
                record.rescore(pinned, score.SCORED_LABEL_CONTRACT_V1), card
            )
            card["runs"][0]["metrics"]["utf8_bytes"]["leaked"] += 1
            template.write_text(json.dumps(card), encoding="utf-8")
            with self.assertRaisesRegex(record.RecordError, "correctness"):
                record.pin_template(source, template, pinned)

    def test_rescored_layers_reach_gate_and_missing_identity_refuses(self):
        layer_contract = agentic.load_contract(ROOT)
        layer_docs = [
            score.Document("layer-a", "alice@example.invalid", "en", "US", "synthetic",
                           (score.Span(0, 21, "EMAIL"),), cell="A|email|prose_cue|valid"),
            score.Document("layer-d", "benign sku", "en", "US", "synthetic", (),
                           cell="D|sku|prose|benign"),
            score.Document("layer-r", "alice@example.invalid", "en", "US", "synthetic",
                           (score.Span(0, 21, "EMAIL"),), cell="R|email|repeat|valid"),
        ]
        layer_measurements = {
            "validator_kinds_by_class": {},
            "documents": {
                document.uid: {
                    "gold_validation": [
                        {"start": span.start, "end": span.end, "label": span.label,
                         "applicable": False, "validator_passed": None}
                        for span in document.spans
                    ],
                    "predictions": None,
                }
                for document in layer_docs
            },
        }
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "observations.jsonl.gz"
            card = self.make_record(path)
            card["parameters"]["policy_sha256"] = "1" * 64
            card["layers"] = {
                "generator": {"corpus_sha256": "2" * 64},
                "scored_label_contract": score.scored_label_contract_report(
                    layer_contract, layer_docs
                ),
                "gold_validity": {"C": agentic.gold_validity_digest(
                    [self.document], self.measurements
                )},
            }
            writer = record.RecordWriter(
                [self.document], self.measurements, corpus_sha256="0" * 64,
                extra_documents=layer_docs, layer_contract=layer_contract,
            )
            writer.add("C", "policy-file", self.document, self.response, self.measurements)
            layer_responses = {}
            for layer, document in zip(("A", "D", "R"), layer_docs, strict=True):
                response = copy.deepcopy(self.response)
                response["fixture_id"] = document.uid
                response["final_protection_trace"] = []
                response["manifest_integrity"]["spans"] = 0
                layer_responses[layer] = response
                measurements = None if layer == "D" else layer_measurements
                run = score.run_config(
                    Path("."), Path("."), "policy-file", [document], Path("."),
                    None, None, None, 0.3, Path("."),
                    validator_measurements=measurements,
                    replay_responses={document.uid: response},
                )
                card["layers"][layer] = {
                    "population": score.population_summary([document]),
                    "runs": [run],
                }
                if measurements is not None:
                    card["layers"][layer]["validator_gold_census"] = (
                        score.validator_gold_census([document], measurements)
                    )
                    writer.layer_measurements[layer] = measurements
                writer.add(layer, "policy-file", document, response, measurements)
            writer.write(path, card, add_reference=False)
            replayed = record.rescore(path, score.SCORED_LABEL_CONTRACT_V1)
            self.assertEqual(agentic.gate(card, replayed)["verdict"], "fail")
            template = copy.deepcopy(card)
            del template["layers"]
            template_path, pinned_path = (
                Path(temporary) / "template.json", Path(temporary) / "c-only.gz"
            )
            template_path.write_text(json.dumps(template), encoding="utf-8")
            record.pin_template(path, template_path, pinned_path, strip_layers=True)
            self.assertEqual(
                record.rescore(pinned_path, score.SCORED_LABEL_CONTRACT_V1), template
            )
            layer_only = {
                "schema_version": 4,
                "parameters": card["parameters"],
                "runs": [],
                "layers": copy.deepcopy(card["layers"]),
            }
            del layer_only["layers"]["gold_validity"]
            layer_writer = record.RecordWriter(
                [], layer_measurements, corpus_sha256="2" * 64,
                extra_documents=layer_docs, layer_contract=layer_contract,
            )
            for layer, document in zip(("A", "D", "R"), layer_docs, strict=True):
                layer_writer.add(
                    layer, "policy-file", document, layer_responses[layer],
                    None if layer == "D" else layer_measurements,
                )
            layer_path = Path(temporary) / "layer-only.gz"
            layer_writer.write(layer_path, layer_only, add_reference=False)
            self.assertEqual(
                record.rescore(layer_path, score.SCORED_LABEL_CONTRACT_V1),
                layer_only,
            )
            broken = copy.deepcopy(replayed)
            del broken["layers"]["gold_validity"]
            with self.assertRaisesRegex(agentic.LayerError, "gold-validity"):
                agentic.gate(card, broken)
            implicit_v1 = copy.deepcopy(replayed)
            del implicit_v1["scoring"]["scored_label_contract"]
            self.assertIn(agentic.gate(card, implicit_v1)["verdict"], {"pass", "fail"})
            broken_identity = copy.deepcopy(replayed)
            del broken_identity["scoring"]["scored_label_contract"]["id"]
            with self.assertRaisesRegex(agentic.LayerError, "kiji_contract"):
                agentic.gate(card, broken_identity)


if __name__ == "__main__":
    unittest.main()
