"""A scoring contract must never silently trigger another detection capture."""

import contextlib
import io
import json
import unittest
from pathlib import Path

import run_no_opf_benchmark as runner
import rescore_past_release as past
import gaze_bench_score as score
import scorecard_record as record
from verify_record_scorecards import without_timing


class RecordOnceTests(unittest.TestCase):
    def test_v2_record_replays_independent_v1_scorecard(self):
        fixtures = Path(__file__).parent / "fixtures"
        replay = record.rescore(
            fixtures / "record-once.jsonl.gz", score.SCORED_LABEL_CONTRACT_V1,
            max_workers=1,
        )
        expected = json.loads((fixtures / "record-once-v1.json").read_text())
        replay.pop("observation_record")
        self.assertEqual(without_timing(replay), without_timing(expected))

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
