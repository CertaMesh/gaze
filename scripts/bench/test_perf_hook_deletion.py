"""Replay real CLI positives and erase every inventoried output independently."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

import perf_attribution as perf
from perf_hook_deletion import FIXTURE, INPUTS, MODEL_INPUT, SWEEP_INPUTS, UNOBSERVABLE, sweep
from perf_lifecycle_contract import HOOK_AUDIT


class HookDeletionTests(unittest.TestCase):
    def setUp(self):
        self.fixture = json.loads(FIXTURE.read_text())

    def admit(self, case, rows):
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / "policy.toml"
            policy.write_text(case["policy"])
            return perf.validate_lifecycle(rows, len(rows) - 1, daemon=True, policy=policy, required=True)

    def test_every_inventoried_hook_is_exercised_and_classified(self):
        proof = json.loads(Path(__file__).with_name("perf-lifecycle-synthetic-proof.json").read_text())
        self.assertEqual(self.fixture["binary_sha256"], proof["timer_binary_sha256"])
        expected = {perf.sha(text.encode()) for text in (*INPUTS, MODEL_INPUT, *SWEEP_INPUTS)}
        self.assertLessEqual(expected, {case["input_sha256"] for case in self.fixture["cases"]})
        self.assertTrue(all(case["successful_response"] for case in self.fixture["cases"]))
        result = sweep(self.fixture)
        hooks = {key for keys, _, _ in HOOK_AUDIT for key in keys}
        self.assertEqual({r["hook"] for r in result["deletions"]}, hooks)
        self.assertEqual({r["hook"] for r in result["deletions"] if r["status"] != "not-fired"}, hooks)
        unobservable = {r["hook"] for r in result["deletions"] if r["status"] == "undetectable"}
        self.assertLessEqual(unobservable, set(UNOBSERVABLE))
        for row in result["deletions"]:
            if row["hook"] in ("sweep_ns", "registry_ns", "resolve1_ns") and row["record"] == 1:
                self.assertEqual(row["status"], "refused", row)

    def test_reviewer_missing_sweep_on_seven_real_successful_inputs(self):
        for case in self.fixture["cases"][:7]:
            with self.subTest(case=case["id"]):
                self.admit(case, case["records"])
                rows = copy.deepcopy(case["records"])
                self.assertGreater(rows[1]["metrics"]["manifest_ns"]["invocations"], 0)
                rows[1]["metrics"]["sweep_ns"].update(ns=0, invocations=0, candidates=0)
                with self.assertRaisesRegex(ValueError, "manifest-sweep"):
                    self.admit(case, rows)

    def test_partial_failure_before_ledger_and_empty_input_remain_admitted(self):
        # A failed registry, first resolution or sweep emits its prefix on drop.
        # Later work must not be required merely because an earlier hook ran.
        case = self.fixture["cases"][0]
        self.admit(case, case["records"])
        self.assertEqual(case["records"][1]["counters"]["input_bytes"], 0)
        self.assertEqual(sum(r["candidates"] for r in case["records"][1]["recognizers"].values()), 0)
        for stop in ("registry_ns", "resolve1_ns", "sweep_ns"):
            rows = copy.deepcopy(case["records"])
            following = ("resolve1_ns", "sweep_ns", "manifest_ns")
            for key in following[(0 if stop == "registry_ns" else 1 if stop == "resolve1_ns" else 2):]:
                rows[1]["metrics"][key].update(ns=0, invocations=0, candidates=0)
            self.admit(case, rows)

    def test_real_empty_input_candidate_error_remains_admitted(self):
        for case in self.fixture['partial_errors']:
            row = case['records'][1]
            self.assertFalse(case['successful_response'])
            self.assertEqual(row['counters']['input_bytes'], 0)
            self.assertGreater(sum(e['candidates'] for e in row['recognizers'].values()), 0)
            self.assertEqual(row['metrics']['manifest_ns']['invocations'], 0)
            self.admit(case, case['records'])

    def test_nonempty_input_condition_covers_both_source_paths(self):
        for case in self.fixture['cases']:
            counters = case['records'][1]['counters']
            if counters['ner_windows'] or counters['sweep_candidates']:
                rows = copy.deepcopy(case['records'])
                rows[1]['counters']['input_bytes'] = 0
                with self.assertRaisesRegex(ValueError, 'nonempty-input-work'):
                    self.admit(case, rows)


if __name__ == "__main__":
    unittest.main()
