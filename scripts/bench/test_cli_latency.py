#!/usr/bin/env python3
"""Model-free tests for scripts/bench/cli-latency.py."""

from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
_SPEC = importlib.util.spec_from_file_location("cli_latency", HERE / "cli-latency.py")
assert _SPEC is not None and _SPEC.loader is not None
latency = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(latency)

ECHO_JSONL = "import sys\nfor line in sys.stdin:\n    sys.stdout.write(line)\n    sys.stdout.flush()\n"

# A stand-in for `gaze proxy serve`: forwards chat requests to --upstream-openai and replies
# with the upstream body. FAKE_PROXY_MODE=refuse answers 422 for texts containing REFUSE,
# garble alters every reply, crash answers 500.
FAKE_PROXY = """\
import json, os, sys, urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer
args = sys.argv[1:]
assert args[:2] == ["proxy", "serve"], args
bind = args[args.index("--bind") + 1]
upstream = args[args.index("--upstream-openai") + 1]
mode = os.environ.get("FAKE_PROXY_MODE", "")
class Proxy(BaseHTTPRequestHandler):
    def reply(self, status, body):
        self.send_response(status)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def do_GET(self):
        self.reply(200, b"ok")
    def do_POST(self):
        raw = self.rfile.read(int(self.headers["Content-Length"]))
        text = json.loads(raw)["messages"][-1]["content"]
        if mode == "crash":
            return self.reply(500, b"{}")
        if mode == "refuse" and "REFUSE" in text:
            return self.reply(422, b'{"error":"Refused"}')
        request = urllib.request.Request(upstream + self.path, data=raw, headers={"Content-Type": "application/json"})
        body = urllib.request.urlopen(request).read()
        if mode == "garble":
            body = body.replace(b"hello", b"HELLO")
        self.reply(200, body)
    def log_message(self, *_):
        pass
host, port = bind.split(":")
HTTPServer((host, int(port)), Proxy).serve_forever()
"""


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


class ProxyArmTest(unittest.TestCase):
    def setUp(self) -> None:
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.gaze = Path(scratch.name) / "gaze"
        self.gaze.write_text(f"#!{sys.executable}\n{FAKE_PROXY}", encoding="utf-8")
        self.gaze.chmod(0o755)
        self.policy = Path(scratch.name) / "policy.toml"

    def run_arm(self, mode: str, texts: list[str]) -> dict[str, object]:
        env = {**os.environ, "FAKE_PROXY_MODE": mode}
        return latency.proxy_arm(self.gaze, self.policy, texts, env)

    def test_first_request_is_cold_and_every_reply_restores(self) -> None:
        result = self.run_arm("", ["hello one", "hello two", "hello three"])
        self.assertIsNotNone(result["first_request_cold_ms"])
        self.assertEqual(result["warm"]["n"], 3)
        self.assertEqual(result["status_counts"], {"200": 3})
        self.assertEqual(result["restored_exact"], 3)
        self.assertGreater(result["peak_rss_mib"], 1.0)

    def test_refusals_are_counted_not_restored(self) -> None:
        result = self.run_arm("refuse", ["hello", "REFUSE this", "hello again"])
        self.assertEqual(result["status_counts"], {"200": 2, "422": 1})
        self.assertEqual(result["restored_exact"], 2)

    def test_a_reply_that_does_not_restore_fails_the_run(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "did not restore"):
            self.run_arm("garble", ["hello", "hello"])

    def test_any_other_status_fails_the_run(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "answered 500"):
            self.run_arm("crash", ["hello"])


class ArgumentsTest(unittest.TestCase):
    def test_defaults(self) -> None:
        args = latency.parse_args(["--out", "x.json"])
        self.assertEqual(args.documents, 30)
        self.assertFalse(args.smoke)
        self.assertIsNone(args.baseline_root)
        self.assertTrue(latency.parse_args(["--out", "x.json", "--smoke"]).smoke)

    def test_baseline_version_selects_a_known_tag(self) -> None:
        self.assertEqual(latency.parse_args(["--out", "x.json"]).baseline_version, "v0.14.0")
        args = latency.parse_args(["--out", "x.json", "--baseline-version", "v0.15.0"])
        self.assertEqual(args.baseline_version, "v0.15.0")
        with self.assertRaises(SystemExit):
            latency.parse_args(["--out", "x.json", "--baseline-version", "v0.13.0"])

    def test_baseline_commits_are_the_release_tags(self) -> None:
        for version, commit in latency.BASELINE_COMMITS.items():
            with self.subTest(version=version):
                tag = subprocess.run(
                    ["git", "-C", str(REPO), "rev-parse", "--verify", "--quiet", f"{version}^{{commit}}"],
                    capture_output=True, text=True,
                )
                if tag.returncode != 0:
                    self.skipTest(f"tag {version} is not fetched in this checkout")
                self.assertEqual(tag.stdout.strip(), commit)


if __name__ == "__main__":
    unittest.main()
