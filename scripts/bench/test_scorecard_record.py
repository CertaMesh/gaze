"""Contract replay must depend on observations and retain no document values."""

import gzip
import io
import contextlib
import copy
import json
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaze_bench_score as score
import agentic_layers as agentic
import scorecard_record as record
import verify_record_scorecards as proof


ROOT = Path(__file__).resolve().parents[2]


class RecordReplayTests(unittest.TestCase):
    def test_committed_v0151_record_replays_all_contracts(self):
        bench = ROOT / "docs/reference/benchmarks"
        release = next(item for item in json.loads(
            (bench / "release-history.json").read_text(encoding="utf-8")
        )["releases"] if item["version"] == "v0.15.1")
        for pointer in (
            release["observation_record"],
            release["agentic_layers"]["observation_record"],
        ):
            path = bench / pointer["file"]
            self.assertEqual(score.sha256_file(path), pointer["sha256"])
            self.assertEqual(path.stat().st_size, pointer["bytes"])
        self.assertEqual(
            proof.verify(
                bench / release["observation_record"]["file"],
                bench / "scorecard-v0.15.1.json",
                bench / "scorecard-v0.15.1-scored-labels-v2.json",
                bench / "scored-labels-v2.json",
                bench / "scored-labels-v3.json",
            ),
            {"v1_exact_except_timing": True, "v2_exact_except_timing": True,
             "v3_valid": True},
        )
        full_record = bench / release["agentic_layers"]["observation_record"]["file"]
        for contract in (
            score.SCORED_LABEL_CONTRACT_V1,
            score.load_scored_label_contract(bench / "scored-labels-v2.json"),
        ):
            card = record.rescore(full_record, contract)
            self.assertEqual(agentic.gate(card, card, allow_legacy_policy_inputs=True)["verdict"], "fail")
            broken = copy.deepcopy(card)
            del broken["scoring"]["scored_label_contract"]["id"]
            with self.assertRaisesRegex(agentic.LayerError, "kiji_contract"):
                agentic.gate(card, broken)

    def test_committed_v3_rows_rebuild_from_their_records(self):
        """Every v3 headline number is a re-score of a committed record."""
        bench = ROOT / "docs/reference/benchmarks"
        history = json.loads((bench / "release-history.json").read_text(encoding="utf-8"))
        v3 = score.load_scored_label_contract(
            bench / "scored-labels-v3.json",
            display_path="docs/reference/benchmarks/scored-labels-v3.json",
        )
        records = {
            "v0.14.0": "observations-v0.14.0.jsonl.gz",
            "v0.15.0": "observations-v0.15.0.jsonl.gz",
            "v0.15.1": "observations-v0.15.1.jsonl.gz",
        }
        for release in history["releases"]:
            [result] = [
                item for item in release.get("contract_results", ())
                if item["scored_label_contract"]["version"] == 3
            ]
            path = bench / records[release["version"]]
            pointer = result.get("observation_record") or release["observation_record"]
            self.assertEqual(pointer["file"], path.name)
            self.assertEqual(score.sha256_file(path), pointer["sha256"])
            committed = json.loads((bench / result["scorecard"]).read_text(encoding="utf-8"))
            self.assertEqual(score.sha256_file(bench / result["scorecard"]), result["scorecard_sha256"])
            self.assertEqual(record.rescore(path, v3), committed, release["version"])

    def test_later_captures_reproduce_their_releases_committed_numbers(self):
        """v0.15.0 and v0.14.0 were captured on 2026-09-27 with that day's harness.

        Every run field matches the committed v1 and v2 scorecards except
        `validator_recall_by_label`, which the harness's probe computes, not the
        release, and which grew since those runs: v0.15.0 differs only in
        credit-card shape-only recall (99 -> 124 of 126); v0.14.0's original v1
        run also lacks the `production_recall_by_gold_validity` sub-blocks and
        differs in shape-only recall for cards (94 -> 124), IBAN, phone and tax
        numbers and in card validator-backed recall. Against v0.14.0's v2 run
        and its v1 calibration, captured with a later harness, the block
        matches. The document renders validator recall only from the current
        release's own scorecard, so no displayed number depends on it.
        """
        bench = ROOT / "docs/reference/benchmarks"
        contracts = (bench / "scored-labels-v2.json", bench / "scored-labels-v3.json")
        for version, v1, probe_differs in (
            ("v0.15.0", "scorecard-v0.15.0.json", {"v1": True, "v2": True}),
            ("v0.14.0", "scorecard-v0.14.0.json", {"v1": True, "v2": False}),
            ("v0.14.0", "scorecard-v0.14.0-rescore-calibration-v1.json", {"v1": False, "v2": False}),
        ):
            with self.subTest(version=version, v1=v1):
                result = proof.verify_capture(
                    bench / f"observations-{version}.jsonl.gz",
                    bench / v1,
                    bench / f"scorecard-{version}-scored-labels-v2.json",
                    *contracts,
                    ignore=["validator_recall_by_label"],
                )
                self.assertEqual(
                    result["ignored_run_fields_differ"], {"validator_recall_by_label": probe_differs}
                )

    def test_capture_mode_compares_runs_and_reports_skipped_fields(self):
        bench = ROOT / "docs/reference/benchmarks"
        args = (
            bench / "observations-v0.15.1.jsonl.gz",
            bench / "scorecard-v0.15.1.json",
            bench / "scorecard-v0.15.1-scored-labels-v2.json",
            bench / "scored-labels-v2.json",
            bench / "scored-labels-v3.json",
        )
        result = proof.verify_capture(*args, ignore=["validator_recall_by_label"])
        self.assertTrue(result["v1_runs_match"] and result["v2_runs_match"])
        # Only a harness-computed field may be skipped: skipping the release's
        # own results would make any record pass.
        for field in ("metrics", "pipeline_contract"):
            with self.assertRaisesRegex(record.RecordError, "harness-computed"):
                proof.verify_capture(*args, ignore=[field])
        argv = ["verify_record_scorecards.py", str(args[0]), "--v1", str(args[1]),
                "--v2", str(args[2]), "--v2-contract", str(args[3]),
                "--v3-contract", str(args[4]), "--capture", "--ignore-run-field", "metrics"]
        with mock.patch.object(sys, "argv", argv), contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as refused:
                proof.main()
        self.assertEqual(refused.exception.code, 2)
        self.assertEqual(
            result["ignored_run_fields_differ"],
            {"validator_recall_by_label": {"v1": False, "v2": False}},
        )
        with tempfile.TemporaryDirectory() as tmp:
            changed = json.loads(args[1].read_text(encoding="utf-8"))
            changed["runs"][0]["metrics"]["utf8_bytes"]["leaked"] += 1
            v1 = Path(tmp) / "v1.json"
            v1.write_text(json.dumps(changed), encoding="utf-8")
            with self.assertRaisesRegex(record.RecordError, "v1_runs_match': False"):
                proof.verify_capture(args[0], v1, *args[2:])
            # A skipped field still reports its difference instead of hiding it.
            changed = json.loads(args[1].read_text(encoding="utf-8"))
            changed["runs"][0]["validator_recall_by_label"] = {}
            v1.write_text(json.dumps(changed), encoding="utf-8")
            result = proof.verify_capture(args[0], v1, *args[2:], ignore=["validator_recall_by_label"])
            self.assertEqual(result["ignored_run_fields_differ"]["validator_recall_by_label"]["v1"], True)

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
            self.assertNotIn("validator", rows[1])
            self.assertEqual(rows[1]["document_id"], self.document.uid)
            self.assertEqual(rows[0]["documents"][0]["gold"], [[44, 50, "PASSWORD"], [0, 21, "EMAIL"]])
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
            rows[1]["response"]["gold_gap_evidence"][0][:5] = [0, 21, "email", 0, 21]
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
                "generator": {"corpus_sha256": "2" * 64, "generator_version": 3,
                              "documents": 3, "documents_by_layer": {"A": 1, "D": 1, "R": 1}},
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
            with gzip.open(path, "rt", encoding="utf-8") as stream:
                stored = [json.loads(line) for line in stream]

            def tampered(name):
                rows = copy.deepcopy(stored)
                header = rows[0]
                if name == "generator-sha":
                    header["scorecard"]["layers"]["generator"]["corpus_sha256"] = "9" * 64
                elif name == "generator-version":
                    header["scorecard"]["layers"]["generator"]["generator_version"] = 4
                elif name == "layer-gold":
                    descriptor = next(row for row in header["documents"] if row["layer"] == "A")
                    descriptor["gold"][0] = [0, 20, "EMAIL"]
                elif name == "missing-contract":
                    header.pop("layer_contract")
                elif name == "null-contract":
                    header["layer_contract"] = None
                output = Path(temporary) / f"{name}.gz"
                with gzip.open(output, "wt", encoding="utf-8") as stream:
                    for row in rows:
                        stream.write(json.dumps(row) + "\n")
                return output

            for name in ("generator-sha", "generator-version", "layer-gold"):
                with self.subTest(name=name), self.assertRaisesRegex(record.RecordError, "layer identity"):
                    record.rescore(tampered(name), score.SCORED_LABEL_CONTRACT_V1)
            for name in ("missing-contract", "null-contract"):
                broken = tampered(name)
                with self.subTest(name=name), self.assertRaisesRegex(record.RecordError, "missing layer contract"):
                    record.rescore(broken, score.SCORED_LABEL_CONTRACT_V1)
                with self.subTest(name=f"{name}-override"), self.assertRaisesRegex(record.RecordError, "missing layer contract"):
                    record.rescore(broken, score.SCORED_LABEL_CONTRACT_V1, layer_contract)
                self.assertEqual(record.main([str(broken), "--output", str(Path(temporary) / "unused.json")]), 2)
            replayed = record.rescore(path, score.SCORED_LABEL_CONTRACT_V1)
            self.assertEqual(agentic.gate(card, replayed, allow_legacy_policy_inputs=True)["verdict"], "fail")
            alternate = score.ScoredLabelContract(
                "synthetic-layer-exclusion", 2, "synthetic.json", "3" * 64,
                frozenset(), frozenset({"EMAIL"}),
            )
            rescored_layers = record.rescore(
                path, score.SCORED_LABEL_CONTRACT_V1, alternate
            )
            self.assertEqual(
                rescored_layers["layers"]["A"]["runs"][0]["metrics"]["utf8_bytes"]["leaked"], 0
            )
            stale = score.ScoredLabelContract(
                "synthetic-layer-stale", 2, "stale.json", "4" * 64,
                frozenset(), frozenset({"EMAIL", "ABSENT"}),
            )
            with self.assertRaisesRegex(agentic.LayerError, "never emits"):
                record.rescore(path, score.SCORED_LABEL_CONTRACT_V1, stale)
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
            self.assertIn(agentic.gate(card, implicit_v1, allow_legacy_policy_inputs=True)["verdict"], {"pass", "fail"})
            broken_identity = copy.deepcopy(replayed)
            del broken_identity["scoring"]["scored_label_contract"]["id"]
            with self.assertRaisesRegex(agentic.LayerError, "kiji_contract"):
                agentic.gate(card, broken_identity)


if __name__ == "__main__":
    unittest.main()
