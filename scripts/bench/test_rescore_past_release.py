#!/usr/bin/env python3
"""Guards of scripts/bench/rescore_past_release.py that need no benchmark run."""

from __future__ import annotations

import hashlib
import subprocess
import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

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
    def test_policy_dependencies_are_available_for_rescored_cards(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            policy = root / "policy.toml"
            policy.write_text('[policy.rulepacks]\npaths=["pack.toml"]\n', encoding="utf-8")
            (root / "pack.toml").write_text('[[recognizers]]\nid="songs"\n[recognizers.match]\nkind="dictionary"\nterms_file="songs.txt"\n', encoding="utf-8")
            (root / "songs.txt").write_text("synthetic song\n", encoding="utf-8")
            provenance = rescore.policy_dependencies(policy, root)
            self.assertEqual(set(provenance["files"]), {
                "policy.rulepacks.paths[0]",
                "policy.rulepacks.paths[0].recognizers[0].terms_file",
            })
            self.assertIsNone(rescore.policy_dependencies(None, root))

    def test_rescored_card_writes_policy_dependency_identity(self):
        with tempfile.TemporaryDirectory() as release, tempfile.TemporaryDirectory() as harness:
            root = Path(release)
            harness_root = Path(harness)
            policy = root / "policy.toml"
            policy.write_text('[policy.rulepacks]\npaths=["pack.toml"]\n', encoding="utf-8")
            release_pack = root / "pack.toml"
            release_pack.write_text('name="release"\n', encoding="utf-8")
            harness_pack = harness_root / "pack.toml"
            harness_pack.write_text('name="harness"\n', encoding="utf-8")
            binary = root / "clean_for_bench"
            binary.write_bytes(b"binary")
            args = rescore.parse_args([
                "--release-root", str(root), "--binary", str(binary),
                "--binary-profile", "debug", "--configs", "policy-file",
                "--policy", str(policy), "--output", str(root / "card.json"),
            ])
            writer = mock.Mock()
            writer.write.return_value = {}
            write_json = mock.Mock()
            returns = [
                (rescore.subprocess, "run", SimpleNamespace(stdout="")),
                (rescore.runner, "validate_required_models", []),
                (rescore.runner, "load_scored_label_contract", {}),
                (rescore.dataiku, "verify_dataset", None),
                (rescore.dataiku, "load_documents", ([], {})),
                (rescore.runner, "load_negative_documents", ([], {})),
                (rescore.score, "stratified_sample", ([], {})),
                (rescore.score, "apply_scored_label_contract", []),
                (rescore.score, "build_validator_probe", root / "probe"),
                (rescore.score, "collect_validator_measurements", {}),
                (rescore.score, "validator_gold_census", {}),
                (rescore.records, "filter_measurements", {}),
                (rescore.runner, "composite_dataset_report", ({}, {"integrity": {"sha256": "a" * 64}})),
                (rescore.score, "run_config", {}),
                (rescore.score, "scored_label_contract_report", {}),
                (rescore.score, "assemble_scorecard", {"parameters": {"profile": "full"}}),
            ]
            with ExitStack() as stack:
                stack.enter_context(mock.patch.object(rescore, "HARNESS_ROOT", harness_root))
                for target, name, value in returns:
                    stack.enter_context(mock.patch.object(target, name, return_value=value))
                stack.enter_context(mock.patch.object(rescore.records, "RecordWriter", return_value=writer))
                stack.enter_context(mock.patch.object(rescore.runner, "write_json", write_json))
                rescore.run(args)
            card = write_json.call_args.args[1]
            self.assertEqual(card["runner_provenance"]["policy_dependencies"],
                             rescore.policy_dependencies(policy, root))
            digest = card["runner_provenance"]["policy_dependencies"]["files"]["policy.rulepacks.paths[0]"]
            self.assertEqual(digest, hashlib.sha256(release_pack.read_bytes()).hexdigest())
            self.assertNotEqual(digest, hashlib.sha256(harness_pack.read_bytes()).hexdigest())

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
