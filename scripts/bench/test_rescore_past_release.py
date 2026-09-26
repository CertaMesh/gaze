#!/usr/bin/env python3
"""Guards of scripts/bench/rescore_past_release.py that need no benchmark run."""

from __future__ import annotations

import hashlib
import subprocess
import tempfile
import unittest
from pathlib import Path

import rescore_past_release as rescore


class ModelBundleTest(unittest.TestCase):
    def test_a_bundle_whose_digest_differs_from_the_pin_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "SHA256SUMS").write_text("abc  model.onnx\n", encoding="utf-8")
            digest = hashlib.sha256(b"abc  model.onnx\n").hexdigest()
            self.assertEqual(
                rescore.model_bundle(f"kiji={tmp}={digest}")["observed_sha256"], digest
            )
            with self.assertRaisesRegex(rescore.PastReleaseError, "does not match"):
                rescore.model_bundle(f"kiji={tmp}={'0' * 64}")

    def test_a_missing_bundle_is_refused(self):
        with self.assertRaisesRegex(rescore.PastReleaseError, "missing"):
            rescore.model_bundle(f"kiji=/nonexistent-bundle={'0' * 64}")


class ReleaseCheckoutTest(unittest.TestCase):
    def test_a_dirty_release_checkout_is_refused_before_anything_runs(self):
        with tempfile.TemporaryDirectory() as tmp:
            subprocess.run(["git", "init", "-q", tmp], check=True)
            (Path(tmp) / "untracked.txt").write_text("x", encoding="utf-8")
            args = rescore.parse_args(
                [
                    "--release-root", tmp,
                    "--binary", str(Path(tmp) / "clean_for_bench"),
                    "--binary-profile", "debug",
                    "--configs", "rule-floor-extended",
                    "--output", str(Path(tmp) / "out.json"),
                ]
            )
            with self.assertRaisesRegex(rescore.PastReleaseError, "dirty"):
                rescore.run(args)

    def test_a_dirty_harness_checkout_is_refused_before_anything_runs(self):
        with tempfile.TemporaryDirectory() as release, tempfile.TemporaryDirectory() as harness:
            for root in (release, harness):
                subprocess.run(["git", "init", "-q", root], check=True)
            scorer = Path(harness) / "gaze_bench_score.py"
            scorer.write_text("committed", encoding="utf-8")
            subprocess.run(["git", "-C", harness, "add", "-A"], check=True)
            subprocess.run(
                ["git", "-C", harness, "-c", "user.name=t", "-c", "user.email=t@example.invalid",
                 "-c", "commit.gpgsign=false", "commit", "-qm", "t"],
                check=True,
            )
            scorer.write_text("edited, not committed", encoding="utf-8")
            args = rescore.parse_args(
                [
                    "--release-root", release,
                    "--binary", str(Path(release) / "clean_for_bench"),
                    "--binary-profile", "debug",
                    "--configs", "rule-floor-extended",
                    "--output", str(Path(release) / "out.json"),
                ]
            )
            original = rescore.HARNESS_ROOT
            rescore.HARNESS_ROOT = Path(harness)
            try:
                with self.assertRaisesRegex(rescore.PastReleaseError, "harness checkout is dirty"):
                    rescore.run(args)
            finally:
                rescore.HARNESS_ROOT = original

    def test_opf_configs_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            subprocess.run(["git", "init", "-q", tmp], check=True)
            binary = Path(tmp) / "clean_for_bench"
            args = rescore.parse_args(
                [
                    "--release-root", tmp,
                    "--binary", str(binary),
                    "--binary-profile", "debug",
                    "--configs", "full-stack-opf-resolve",
                    "--output", str(Path(tmp) / "out.json"),
                ]
            )
            binary.write_text("", encoding="utf-8")
            subprocess.run(["git", "-C", tmp, "add", "-A"], check=True)
            subprocess.run(
                ["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@example.invalid",
                 "-c", "commit.gpgsign=false", "commit", "-qm", "t"],
                check=True,
            )
            with self.assertRaisesRegex(rescore.PastReleaseError, "OPF-free"):
                rescore.run(args)


if __name__ == "__main__":
    unittest.main()
