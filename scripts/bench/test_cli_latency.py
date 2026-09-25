#!/usr/bin/env python3
"""Model-free tests for scripts/bench/cli-latency.py."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
_SPEC = importlib.util.spec_from_file_location("cli_latency", HERE / "cli-latency.py")
assert _SPEC is not None and _SPEC.loader is not None
latency = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(latency)

ECHO_JSONL = "import sys\nfor line in sys.stdin:\n    sys.stdout.write(line)\n    sys.stdout.flush()\n"


class SummaryTest(unittest.TestCase):
    def test_percentiles_sort_and_use_nearest_rank(self) -> None:
        samples = [float(value) for value in range(100, 0, -1)]
        self.assertEqual(
            latency.summary(samples),
            {"n": 100, "p50_ms": 50.5, "p95_ms": 95.0, "max_ms": 100.0, "mean_ms": 50.5},
        )
        # 10 samples: nearest rank is ceil(9.5) = the 10th value, not the 9th.
        self.assertEqual(latency.summary([float(value) for value in range(1, 11)])["p95_ms"], 10.0)

    def test_no_samples_is_none(self) -> None:
        self.assertIsNone(latency.summary([]))


class ConversationTest(unittest.TestCase):
    def test_turn_k_resends_turns_one_to_k(self) -> None:
        texts = [f"message {index}" for index in range(12)]
        turns = latency.conversation_turns(texts)
        self.assertEqual(len(turns), latency.CONVERSATION_TURNS)
        self.assertEqual(turns[0], "message 0")
        self.assertEqual(turns[2], "message 0\n\nmessage 1\n\nmessage 2")
        for earlier, later in zip(turns, turns[1:]):
            self.assertTrue(later.startswith(earlier + "\n\n"))

    def test_fewer_documents_than_turns(self) -> None:
        self.assertEqual(len(latency.conversation_turns(["a", "b", "c"])), 3)


class QuietHostTest(unittest.TestCase):
    TABLE = [
        (1, 0, "launchd"),
        (10, 1, "zsh -c uv run python cli-latency.py --out x  # cargo build --release"),
        (11, 10, "python cli-latency.py"),
        (12, 11, "target/release/examples/clean_for_bench --config policy-file"),
        (13, 12, "rustc --crate-name child_of_the_workload"),
        (20, 1, "cargo build --workspace"),
        (21, 1, "rustc --crate-name gaze"),
        (22, 1, "vim notes.txt"),
    ]

    def test_own_tree_is_ancestors_and_descendants(self) -> None:
        self.assertEqual(latency.own_tree(self.TABLE, 11), {1, 10, 11, 12, 13})

    def test_only_foreign_matches_count(self) -> None:
        foreign = latency.foreign_processes(self.TABLE, 11)
        self.assertEqual([row.split()[0] for row in foreign], ["20", "21"])

    def test_verdict(self) -> None:
        quiet = {"quiet": True, "foreign_processes": []}
        noisy = {"quiet": False, "foreign_processes": []}
        foreign = {"quiet": False, "foreign_processes": ["20 cargo build"]}
        self.assertEqual(latency.verdict(quiet, quiet, smoke=False), "valid")
        # The workload's own load at the end is expected; only a foreign process fails it.
        self.assertEqual(latency.verdict(quiet, noisy, smoke=False), "valid")
        self.assertTrue(latency.verdict(noisy, quiet, smoke=False).startswith("timing invalid"))
        self.assertTrue(latency.verdict(quiet, foreign, smoke=False).startswith("timing invalid"))
        self.assertEqual(latency.verdict(quiet, quiet, smoke=True), "smoke run, not a timing claim")

    def test_the_live_probe_has_every_field(self) -> None:
        probe = latency.host_probe("now")
        for key in ("load_1m", "load_5m", "load_15m", "foreign_processes", "quiet"):
            self.assertIn(key, probe)


class ChildProcessTest(unittest.TestCase):
    def test_run_once_reports_exit_code_and_peak_rss(self) -> None:
        elapsed, code, stderr, rss = latency.run_once(
            [sys.executable, "-c", "import sys; sys.stderr.write(sys.stdin.read()); sys.exit(3)"], "hello", {}
        )
        self.assertEqual(code, 3)
        self.assertEqual(stderr, "hello")
        self.assertGreater(elapsed, 0.0)
        self.assertGreater(rss, 1 << 20)

    def test_jsonl_child_round_trips_and_reports_peak_rss(self) -> None:
        child = latency.Child([sys.executable, "-c", ECHO_JSONL], {})
        elapsed, response = child.request({"session_id": "s", "text": "t"})
        self.assertEqual(response, {"session_id": "s", "text": "t"})
        self.assertGreater(elapsed, 0.0)
        self.assertGreater(child.close(), 1 << 20)

    def test_a_failing_child_fails_the_run(self) -> None:
        child = latency.Child([sys.executable, "-c", "import sys; sys.stdin.read(); sys.exit(2)"], {})
        with self.assertRaisesRegex(RuntimeError, "exited 2"):
            child.close()


class ArgumentsTest(unittest.TestCase):
    def test_defaults(self) -> None:
        args = latency.parse_args(["--out", "x.json"])
        self.assertEqual(args.documents, 30)
        self.assertFalse(args.smoke)
        self.assertIsNone(args.baseline_root)
        self.assertTrue(latency.parse_args(["--out", "x.json", "--smoke"]).smoke)

    def test_baseline_commit_is_the_v0_14_0_tag(self) -> None:
        tag = subprocess.run(
            ["git", "-C", str(REPO), "rev-parse", "--verify", "--quiet", "v0.14.0^{commit}"],
            capture_output=True, text=True,
        )
        if tag.returncode != 0:
            self.skipTest("tag v0.14.0 is not fetched in this checkout")
        self.assertEqual(tag.stdout.strip(), latency.V0_14_0_COMMIT)


if __name__ == "__main__":
    unittest.main()
