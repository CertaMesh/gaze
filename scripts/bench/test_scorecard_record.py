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

    def with_lineage(self, events=None, **provenance):
        response = copy.deepcopy(self.response)
        item = response["final_protection_trace"][0]
        start, end = item["raw_start"], item["raw_end"]
        item["provenance"].update({
            "settlement": "resolve",
            "contributions": [
                {"original": 0, "recognizer_id": "synthetic:email", "role": "winner",
                 "raw_start": start, "raw_end": end,
                 "tier": None, "defeat_kind": None, "defeated_by": None},
                {"original": 1, "recognizer_id": "synthetic:name", "role": "defeated",
                 "raw_start": start, "raw_end": start + 5,
                 "tier": "class_priority", "defeat_kind": "pair",
                 "defeated_by": "synthetic:email"},
            ],
        })
        item["provenance"].update(provenance)
        base = {key: None for key in record.EVENT_FIELDS}
        response["candidate_events"] = events if events is not None else [
            {**base, "original": 0, "recognizer_id": "synthetic:email", "class": "email",
             "raw_start": start, "raw_end": end, "outcome": "selected",
             "selection_start": start, "selection_end": end, "settlement": "resolve",
             "role": "winner"},
            {**base, "original": 1, "recognizer_id": "synthetic:name", "class": "name",
             "raw_start": start, "raw_end": start + 5, "outcome": "defeated",
             "selection_start": start, "selection_end": end, "settlement": "resolve",
             "tier": "class_priority", "defeat_kind": "pair", "winner": 0},
            {**base, "recognizer_id": "synthetic:card", "class": "custom:credit_card",
             "raw_start": 44, "raw_end": 50, "outcome": "vetoed",
             "veto_reason": "luhn_failed"},
        ]
        return response

    def test_lineage_writes_schema_v2_and_scores_identically(self):
        with tempfile.TemporaryDirectory() as temporary:
            plain = Path(temporary) / "plain.jsonl.gz"
            self.make_record(plain)
            self.response = self.with_lineage()
            typed = Path(temporary) / "typed.jsonl.gz"
            card = self.make_record(typed)
            header, rows = record._read(typed)
            self.assertEqual(header["schema_version"], 2)
            self.assertEqual(record._read(plain)[0]["schema_version"], 1)
            response = rows[0]["response"]
            self.assertEqual(
                response["final_protection_trace"][0]["provenance"]["contributions"][1]["role"],
                "defeated",
            )
            self.assertEqual(len(response["candidate_events"]), 3)
            # Lineage is metadata: every contract scores the two records alike.
            self.assertEqual(record.rescore(typed, score.SCORED_LABEL_CONTRACT_V1), card)
            for contract in (
                score.SCORED_LABEL_CONTRACT_V1,
                score.load_scored_label_contract(ROOT / "docs/reference/benchmarks/scored-labels-v3.json"),
            ):
                self.assertEqual(
                    record.rescore(typed, contract)["runs"][0]["metrics"],
                    record.rescore(plain, contract)["runs"][0]["metrics"],
                )
            # A v2 header over a trace without lineage, or the reverse, is refused.
            for source, version in ((plain, 2), (typed, 1)):
                with gzip.open(source, "rt", encoding="utf-8") as stream:
                    lines = [json.loads(line) for line in stream]
                lines[0]["schema_version"] = version
                forged = Path(temporary) / f"forged-{version}.jsonl.gz"
                with gzip.open(forged, "wt", encoding="utf-8") as stream:
                    stream.writelines(json.dumps(line) + "\n" for line in lines)
                with self.assertRaisesRegex(record.RecordError, "disagrees with its trace lineage"):
                    record._read(forged)

    def test_safety_net_and_residual_items_are_exempt_from_the_selection_join(self):
        response = self.with_lineage()
        base = {key: None for key in record.EVENT_FIELDS}
        response["candidate_events"].append(
            {**base, "original": 2, "recognizer_id": "synthetic:part", "class": "name",
             "raw_start": 0, "raw_end": 5, "outcome": "unlinked"})
        plain = {"tier": None, "defeat_kind": None, "defeated_by": None}
        response["final_protection_trace"] = [
            {"raw_start": 0, "raw_end": 5, "class": "name", "action": "tokenize",
             "provenance": {"stage": "primary_pipeline", "decision": "policy",
                            "source_ids": ["synthetic:part"], "settlement": "residual",
                            "contributions": [{"original": 2, "recognizer_id": "synthetic:part",
                                               "role": "winner", "raw_start": 0, "raw_end": 5,
                                               **plain}]}},
            *response["final_protection_trace"],
            {"raw_start": 44, "raw_end": 50, "class": "name", "action": "redact",
             "provenance": {"stage": "safety_net", "decision": "redact",
                            "source_ids": ["nym"], "settlement": "safety_net",
                            "contributions": [{"original": None, "recognizer_id": "nym",
                                               "role": "winner", "raw_start": 44, "raw_end": 50,
                                               **plain}]}},
        ]
        record.validate_response_lineage(response, 50, "exempt")
        # A residual parent still needs its candidate event.
        broken = copy.deepcopy(response)
        broken["final_protection_trace"][0]["provenance"]["contributions"][0]["original"] = 9
        with self.assertRaisesRegex(record.RecordError, "without a candidate event"):
            record.validate_response_lineage(broken, 50, "exempt")

    def test_malformed_lineage_is_refused(self):
        typed = self.with_lineage()
        winner, defeated = typed["final_protection_trace"][0]["provenance"]["contributions"]
        selected, lost, vetoed = typed["candidate_events"]
        trace_cases = {
            "unknown settlement": {"settlement": "guess"},
            "disagrees with stage": {"settlement": "safety_net"},
            "without contributions": {"contributions": []},
            "unknown contribution role": {"contributions": [{**winner, "role": "helper"}]},
            "fields differ": {"contributions": [{**winner, "score": 1}]},
            "without its own span": {"contributions": [{**winner, "raw_end": None}]},
            "co_member contribution without its own span": {"contributions": [
                winner, {**winner, "role": "co_member", "raw_start": None, "raw_end": None}]},
            "own span in the document": {"contributions": [{**winner, "raw_end": 10_000}]},
            "does not overlap its trace item": {"contributions": [
                {**winner, "raw_start": 44, "raw_end": 50}]},
            "detected no span": {"contributions": [
                winner, {**winner, "original": None, "recognizer_id": "ner",
                         "role": "derived_dependency"}]},
            "unknown conflict tier": {"contributions": [winner, {**defeated, "tier": "InventedTier"}]},
            "unknown defeat kind": {"contributions": [winner, {**defeated, "defeat_kind": "vibes"}]},
            "names no winner": {"contributions": [winner, {**defeated, "defeated_by": None}]},
            "exactly the defeated role": {"contributions": [{**winner, "tier": "score"}]},
            "without a detecting contribution": {"contributions": [defeated]},
        }
        event_cases = {
            "event fields differ": [{**selected, "value": "x"}],
            "unknown candidate outcome": [{**selected, "outcome": "maybe"}],
            "candidate span outside": [{**selected, "raw_end": 10_000}],
            "exactly a vetoed candidate has no pool index": [{**vetoed, "original": 2}],
            "pool index malformed": [{**selected, "original": -1}],
            "placed twice in one selection": [selected, {**lost, "original": 0, "winner": 0}, vetoed],
            "selected twice": [selected, {**selected, "selection_start": 44, "selection_end": 50}],
            "unlinked candidate has a placement": [selected, {
                **{key: None for key in record.EVENT_FIELDS}, "original": 0,
                "recognizer_id": "synthetic:email", "class": "email",
                "raw_start": 0, "raw_end": 5, "outcome": "unlinked"}],
            "one per candidate": [{**selected, "original": 3}],
            "role does not belong": [{**lost, "original": 0, "role": "winner"}],
            "tier does not belong": [{**selected, "tier": "score"}],
            "has a detecting role": [{**selected, "role": "defeated"}],
            "unknown selection settlement": [{**selected, "settlement": "residual"}],
            "unknown conflict tier": [selected, {**lost, "tier": "InventedTier"}],
            "unknown defeat kind": [selected, {**lost, "defeat_kind": "vibes"}],
            "not a placed candidate": [selected, {**lost, "winner": 1}],
            "unknown veto reason": [{**vetoed, "veto_reason": "felt_wrong"}],
        }
        dependency = {**winner, "original": None, "recognizer_id": "ner", "role": "derived_dependency",
                      "raw_start": None, "raw_end": None}
        join_cases = {
            # An empty events list cannot back a traced selection.
            "contribution without a candidate event": ({}, []),
            "contribution disagrees with its candidate's ID or span": (
                {"contributions": [{**winner, "recognizer_id": "synthetic:other"}, defeated]}, None),
            "not placed in its trace item's selection": (
                {}, [{**selected, "selection_end": selected["selection_end"] - 1}, lost, vetoed]),
            "settlement disagrees": ({}, [{**selected, "settlement": "recovery"}, lost, vetoed]),
            "role disagrees with its candidate event": (
                {"contributions": [{**winner, "role": "same_span_merge"}, defeated]}, None),
            "defeated contribution disagrees": (
                {"contributions": [winner, {**defeated, "tier": "score"}]}, None),
            "defeated contribution disagrees with its candidate event": (
                {"contributions": [winner, {**defeated, "defeated_by": "synthetic:name"}]}, None),
            "event missing from its trace item": ({"contributions": [winner]}, None),
            "has no pool candidate": ({"contributions": [winner, defeated, {**dependency, "original": 1}]}, None),
        }
        cases = [(message, self.with_lineage(**change)) for message, change in trace_cases.items()]
        cases += [(message, self.with_lineage(events=events, **change))
                  for message, (change, events) in join_cases.items()]
        cases += [(message, self.with_lineage(events=events)) for message, events in event_cases.items()]
        for message, response in cases:
            with self.subTest(message):
                writer = record.RecordWriter(
                    [self.document], self.measurements, corpus_sha256="0" * 64
                )
                with self.assertRaisesRegex(record.RecordError, message):
                    writer.add("C", "policy-file", self.document, response, self.measurements)
        half = copy.deepcopy(self.response)
        half["final_protection_trace"][0]["provenance"]["settlement"] = "resolve"
        writer = record.RecordWriter([self.document], self.measurements, corpus_sha256="0" * 64)
        with self.assertRaisesRegex(record.RecordError, "half a lineage"):
            writer.add("C", "policy-file", self.document, half, self.measurements)
        no_events = self.with_lineage()
        del no_events["candidate_events"]
        writer = record.RecordWriter([self.document], self.measurements, corpus_sha256="0" * 64)
        with self.assertRaisesRegex(record.RecordError, "some trace items carry lineage"):
            writer.add("C", "policy-file", self.document, no_events, self.measurements)

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
            # Every credited class's benign counterweight must survive replay.
            score.Document("layer-d9", "voucher 123456789", "en", "US", "synthetic", (),
                           cell="D|ref_number_9|prose|benign"),
            score.Document("layer-d10", "voucher 1234567890", "en", "US", "synthetic", (),
                           cell="D|ref_number_10|prose|benign"),
            score.Document("layer-d11", "voucher 12345678901", "en", "US", "synthetic", (),
                           cell="D|ref_number_11|prose|benign"),
            score.Document("layer-d16", "voucher 1234 5678 9012 3457", "en", "US", "synthetic", (),
                           cell="D|ref_number_16|prose|benign"),
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
                              "documents": 6, "documents_by_layer": {"A": 1, "D": 4, "R": 1}},
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
            for document in layer_docs:
                response = copy.deepcopy(self.response)
                response["fixture_id"] = document.uid
                response["final_protection_trace"] = []
                response["manifest_integrity"]["spans"] = 0
                layer_responses[document.uid] = response
            for layer in ("A", "D", "R"):
                documents = [document for document in layer_docs if document.cell.startswith(f"{layer}|")]
                measurements = None if layer == "D" else layer_measurements
                run = score.run_config(
                    Path("."), Path("."), "policy-file", documents, Path("."),
                    None, None, None, 0.3, Path("."),
                    validator_measurements=measurements,
                    replay_responses={document.uid: layer_responses[document.uid]
                                      for document in documents},
                )
                card["layers"][layer] = {
                    "population": score.population_summary(documents),
                    "runs": [run],
                }
                if measurements is not None:
                    card["layers"][layer]["validator_gold_census"] = (
                        score.validator_gold_census(documents, measurements)
                    )
                    writer.layer_measurements[layer] = measurements
                for document in documents:
                    writer.add(layer, "policy-file", document, layer_responses[document.uid], measurements)
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
            for document in layer_docs:
                layer = document.cell.split("|")[0]
                layer_writer.add(
                    layer, "policy-file", document, layer_responses[document.uid],
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
