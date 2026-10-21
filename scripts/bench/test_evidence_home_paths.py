"""Tracked gzip evidence carries no absolute home path; scrubbed archives equal their originals."""
import copy
import gzip
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import evidence_home_paths as evidence
import export_portable_record as export


def _gz(rows) -> bytes:
    text = "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in rows)
    return gzip.compress(text.encode("utf-8"), mtime=0)


HEADER = {"kind": "header", "scorecard": {"runner_provenance": {"policy": {
    "path": "/Users/someone/anvil/workspaces/proj/tree/target/gate/policy.toml",
    "sha256": "a" * 64}}, "runs": [{"leaked": 7}]}}
ROWS = [{"kind": "observation", "document_id": "d1", "spans": [[1, 4]]},
        {"kind": "observation", "document_id": "d2", "spans": []}]


class TrackedEvidenceTests(unittest.TestCase):
    def test_every_tracked_gz_is_free_of_home_paths(self):
        self.assertTrue(evidence.tracked_gz(), "git ls-files found no .gz evidence")
        self.assertEqual(evidence.scan(), {})

    def test_the_receipts_prove_every_scrubbed_archive_equals_its_original(self):
        self.assertEqual(evidence.verify_receipts(), 6)


class PlantedPathTests(unittest.TestCase):
    def planted_repo(self, text: str) -> Path:
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        (root / "clean.jsonl.gz").write_bytes(_gz([{"path": "target/gate/policy.toml"}]))
        (root / "planted.jsonl.gz").write_bytes(gzip.compress(text.encode("utf-8")))
        subprocess.run(["git", "-C", str(root), "add", "."], check=True)
        return root

    def test_planted_home_paths_fail_the_scan(self):
        for text in ('{"path":"/Users/someone/policy.toml"}',
                     '{"path":"/home/someone/policy.toml"}',
                     '{"path":"C:\\\\Users\\\\someone\\\\policy.toml"}',
                     "C:\\Users\\someone\\policy.toml"):
            with self.subTest(text=text):
                root = self.planted_repo(text)
                self.assertEqual(evidence.scan(root), {"planted.jsonl.gz": 1})

    def test_the_check_command_exits_nonzero_on_a_planted_path(self):
        root = self.planted_repo('{"path":"/Users/someone/policy.toml"}')
        original = evidence.ROOT
        evidence.ROOT = root
        try:
            self.assertEqual(evidence.main(["check"]), 1)
        finally:
            evidence.ROOT = original

    def test_json_escaped_slashes_fail_scan_and_check(self):
        for path in ("/Users/someone/policy.toml", "/home/someone/policy.toml"):
            with self.subTest(path=path):
                text = json.dumps({"nested": [{"path": path}]}).replace("/", r"\/")
                root = self.planted_repo(text)
                self.assertEqual(evidence.scan(root), {"planted.jsonl.gz": 1})
                original = evidence.ROOT
                evidence.ROOT = root
                try:
                    self.assertEqual(evidence.main(["check"]), 1)
                finally:
                    evidence.ROOT = original

    def test_windows_case_variants_fail_scan_and_check(self):
        for component in ("users", "USERS", "UsErS"):
            for separator in ("\\", "/"):
                path = separator.join(("C:", component, "someone", "policy.toml"))
                for text in (path, json.dumps({"path": path})):
                    with self.subTest(text=text):
                        root = self.planted_repo(text)
                        self.assertEqual(evidence.scan(root), {"planted.jsonl.gz": 1})
                        original = evidence.ROOT
                        evidence.ROOT = root
                        try:
                            self.assertEqual(evidence.main(["check"]), 1)
                        finally:
                            evidence.ROOT = original

    def test_portable_paths_pass_the_scan(self):
        self.assertEqual(evidence.home_path_hits(_gz([{"p": "~/gaze-bench/policy.toml"}])), 0)

    def test_json_in_strings_fails_scan_check_and_receipt_comparison(self):
        for prefix in ("/Users/", "/home/"):
            for escape in (r"\/", r"\u002F"):
                inner = json.dumps({"path": prefix + "someone/policy.toml"}).replace("/", escape)
                for depth in (1, 4):
                    payload = inner
                    for _ in range(depth - 1):
                        payload = json.dumps({"payload": payload})
                    with self.subTest(prefix=prefix, escape=escape, depth=depth):
                        row = {"payload": payload}
                        root = self.planted_repo(json.dumps(row))
                        self.assertGreater(evidence.scan(root).get("planted.jsonl.gz", 0), 0)
                        original = evidence.ROOT
                        evidence.ROOT = root
                        try:
                            self.assertEqual(evidence.main(["check"]), 1)
                        finally:
                            evidence.ROOT = original
                        published = export.portable_header(HEADER)
                        self.assertIn("published record still carries a home path",
                                      evidence.compare_records(_gz([HEADER, row]),
                                                               _gz([published, row]),
                                                               CompareRecordsTests.FIELD))

    def test_json_in_strings_depth_limit_fails_closed(self):
        payload = json.dumps({"path": "target/policy.toml"})
        for _ in range(3):
            payload = json.dumps({"payload": payload})
        self.assertEqual(evidence.home_path_hits(_gz([{"payload": payload}])), 0)
        payload = json.dumps({"payload": payload})
        row = {"payload": payload}
        root = self.planted_repo(json.dumps(row))
        self.assertGreater(evidence.scan(root).get("planted.jsonl.gz", 0), 0)
        original = evidence.ROOT
        evidence.ROOT = root
        try:
            self.assertEqual(evidence.main(["check"]), 1)
        finally:
            evidence.ROOT = original
        self.assertIn("published record still carries a home path",
                      evidence.compare_records(_gz([HEADER, row]),
                                               _gz([export.portable_header(HEADER), row]),
                                               CompareRecordsTests.FIELD))

    def test_bounded_portable_json_in_strings_passes(self):
        for payload in ('ordinary text', '{not JSON}', json.dumps({"path": "~/policy.toml"}),
                        json.dumps([json.dumps({"path": "target/policy.toml"})])):
            with self.subTest(payload=payload):
                self.assertEqual(evidence.home_path_hits(_gz([{"payload": payload}])), 0)

    def test_unc_homes_fail_scan_check_and_receipt_comparison(self):
        for prefix in (r"\\server", r"\\?\UNC\server"):
            for component in ("Users", "users", "USERS"):
                path = prefix + "\\" + component + r"\someone\policy.toml"
                for text in (path, json.dumps({"path": path})):
                    with self.subTest(text=text):
                        root = self.planted_repo(text)
                        self.assertGreater(evidence.scan(root).get("planted.jsonl.gz", 0), 0)
                        original = evidence.ROOT
                        evidence.ROOT = root
                        try:
                            self.assertEqual(evidence.main(["check"]), 1)
                        finally:
                            evidence.ROOT = original
                row = {"path": path}
                self.assertIn("published record still carries a home path",
                              evidence.compare_records(_gz([HEADER, row]),
                                                       _gz([export.portable_header(HEADER), row]),
                                                       CompareRecordsTests.FIELD))


class CompareRecordsTests(unittest.TestCase):
    FIELD = ["scorecard.runner_provenance.policy.path"]

    def published(self, header=None, rows=None) -> bytes:
        header = copy.deepcopy(HEADER) if header is None else header
        header["scorecard"]["runner_provenance"]["policy"]["path"] = "target/gate/policy.toml"
        return _gz([header, *(ROWS if rows is None else rows)])

    def test_only_the_path_field_changed(self):
        self.assertEqual(evidence.compare_records(_gz([HEADER, *ROWS]), self.published(), self.FIELD), [])

    def test_a_changed_observation_is_reported(self):
        rows = copy.deepcopy(ROWS)
        rows[1]["spans"] = [[0, 1]]
        problems = evidence.compare_records(_gz([HEADER, *ROWS]), self.published(rows=rows), self.FIELD)
        self.assertIn("observation row 2 differs", problems)

    def test_a_changed_non_path_header_field_is_reported(self):
        header = copy.deepcopy(HEADER)
        header["scorecard"]["runner_provenance"]["policy"]["sha256"] = "b" * 64
        problems = evidence.compare_records(_gz([HEADER, *ROWS]), self.published(header), self.FIELD)
        self.assertIn("header field scorecard differs", problems)

    def test_an_unscrubbed_path_is_reported(self):
        problems = evidence.compare_records(_gz([HEADER, *ROWS]), _gz([HEADER, *ROWS]), self.FIELD)
        self.assertIn("published record still carries a home path", problems)

    def test_allowed_path_field_cannot_hide_escaped_or_case_variant_home(self):
        for path in ("/Users/someone/policy.toml", "/home/someone/policy.toml",
                     r"C:\users\someone\policy.toml", r"C:\USERS\someone\policy.toml",
                     "C:/users/someone/policy.toml", "C:/USERS/someone/policy.toml"):
            with self.subTest(path=path):
                header = copy.deepcopy(HEADER)
                header["scorecard"]["runner_provenance"]["policy"]["path"] = path
                published = gzip.compress(gzip.decompress(_gz([header, *ROWS])).replace(b"/", b"\\/"))
                problems = evidence.compare_records(_gz([HEADER, *ROWS]), published, self.FIELD)
                self.assertIn("published record still carries a home path", problems)


class PortablePathTests(unittest.TestCase):
    def test_anvil_worktree_policy_becomes_repo_relative(self):
        self.assertEqual(export.portable_path(
            "/Users/someone/anvil/workspaces/proj/tree/target/gate/setup.toml"), "target/gate/setup.toml")

    def test_any_users_home_becomes_a_tilde_path(self):
        self.assertEqual(export.portable_path("/home/someone/gaze-bench/p.toml"), "~/gaze-bench/p.toml")
        self.assertEqual(export.portable_path("C:\\Users\\someone\\bench\\p.toml"), "~/bench/p.toml")

    def test_other_values_pass_through(self):
        self.assertEqual(export.portable_path("target/gate/policy.toml"), "target/gate/policy.toml")
        self.assertEqual(export.portable_path("/opt/bench/p.toml"), "/opt/bench/p.toml")


if __name__ == "__main__":
    unittest.main()
