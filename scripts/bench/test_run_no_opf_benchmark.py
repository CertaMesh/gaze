#!/usr/bin/env python3

import copy
import hashlib
import json
import sys
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaze_bench_score as score
import run_no_opf_benchmark as runner


DAVLAN_ARTIFACTS = (
    "config.json",
    "labels.json",
    "model.onnx",
    "special_tokens_map.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "vocab.txt",
)
DAVLAN_HF_REPO = "onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX"
DAVLAN_HF_COMMIT = "cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8"
DAVLAN_BUNDLE_SHA = "7b0b9d0d200bf7f3a39654257f8723998316600852edff8404834eb7edfc5c16"


def scorecard(leaked: int = 0) -> dict[str, object]:
    uid = "synthetic-runner-1"
    digest = hashlib.sha256(
        json.dumps([uid], separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    fully_covered = 1 if leaked == 0 else 0
    run = {
        "scored_population": score.identified_document_population([uid]),
        "failed_closed_population": {
            **score.identified_document_population([]),
            "excluded_documents": [],
            "reason_counts": {},
            "stage_counts": {},
        },
        "metrics": {
            "documents": 1,
            "documents_without_leaks": fully_covered,
            "documents_with_false_positives": 0,
            "utf8_bytes": {
                "pii": 2,
                "leaked": leaked,
                "false_positive": 0,
            },
            "entities": {"gold": 1, "fully_covered": fully_covered},
        },
        "pipeline_contract": {
            "documents": 1,
            "restore_exact_documents": 1,
            "restore_success_decisions": 1,
            "manifest_valid_documents": 1,
            "manifest_integrity_errors": {},
            "strict_would_reject_documents": 0,
            "post_policy_scanned_documents": 1,
            "post_policy_suspects": 0,
            "redact_actions": 0,
        },
        "pipeline_availability": {
            "attempted_documents": 1,
            "completed_documents": 1,
            "failed_closed_documents": 0,
            "errors": {},
            "error_stages": {},
        },
        "per_language": {},
        "per_label_recall": {},
        "per_negative_category": {},
        "latency_ms": {"clean_ms": {"p95": 1.0}},
        "process": {
            "cold_start_to_first_validated_response_ms": 2.0,
            "warmup_count": 1,
            "discarded_warmup_samples": [{"sample": 1, "clean_ms": 0.5}],
        },
    }
    return {
        "schema_version": 3,
        "dataset": {
            "repository": "synthetic/runner",
            "revision": "synthetic-v1",
            "file": "synthetic.jsonl",
            "integrity": {"sha256": "0" * 64},
            "evaluated_population": {"documents": 1},
            "sampling": {
                "strategy": score.SAMPLING_STRATEGY,
                "seed": 7,
                "evaluated_document_ids": [uid],
                "evaluated_document_ids_digest": {
                    "algorithm": "sha256",
                    "value": digest,
                },
            },
        },
        "runs": [
            {"config": config, **copy.deepcopy(run)} for config in score.DEFAULT_CONFIGS
        ],
    }


def set_population_split(
    card: dict[str, object], *, scored_id: str, failed_id: str
) -> None:
    document_ids = sorted([scored_id, failed_id])
    card["dataset"]["evaluated_population"]["documents"] = 2
    sampling = card["dataset"]["sampling"]
    sampling["evaluated_document_ids"] = document_ids
    sampling["evaluated_document_ids_digest"] = score.document_ids_digest(document_ids)
    reason = "safety_net_fallback_residual_suspect"
    for run in card["runs"]:
        run["scored_population"] = score.identified_document_population([scored_id])
        run["failed_closed_population"] = {
            **score.identified_document_population([failed_id]),
            "excluded_documents": [
                {"document_id": failed_id, "reason": reason, "stage": "clean"}
            ],
            "reason_counts": {reason: 1},
            "stage_counts": {"clean": 1},
        }
        run["pipeline_availability"] = {
            "attempted_documents": 2,
            "completed_documents": 1,
            "failed_closed_documents": 1,
            "errors": {reason: 1},
            "error_stages": {"clean": 1},
        }


class IntegerComparatorTests(unittest.TestCase):
    def test_empty_and_missing_candidates_fail_closed(self) -> None:
        baseline = scorecard()
        for candidate in ({}, {"schema_version": 3, "runs": []}):
            comparison = score.compare_scorecards(candidate, baseline)
            self.assertFalse(comparison["regression"]["passed"])
            self.assertFalse(comparison["release_readiness"]["passed"])

    def test_integer_leak_ratchet_catches_change_even_if_rate_is_unchanged(
        self,
    ) -> None:
        baseline = scorecard()
        candidate = copy.deepcopy(baseline)
        candidate["runs"][0]["metrics"]["utf8_bytes"]["leaked"] = 1
        comparison = score.compare_scorecards(candidate, baseline)
        self.assertFalse(comparison["regression"]["passed"])
        self.assertTrue(
            any(
                failure.get("gate") == "leaked_labeled_utf8_bytes"
                for failure in comparison["regression"]["failures"]
            )
        )

    def test_success_count_decrease_is_a_zero_tolerance_regression(self) -> None:
        baseline = scorecard()
        candidate = copy.deepcopy(baseline)
        candidate["runs"][0]["metrics"]["documents_without_leaks"] = 0
        comparison = score.compare_scorecards(candidate, baseline)
        self.assertFalse(comparison["regression"]["passed"])
        self.assertTrue(
            any(
                failure.get("gate") == "documents_without_leaks"
                for failure in comparison["regression"]["failures"]
            )
        )

    def test_pipeline_telemetry_disagreement_fails_both_verdicts(self) -> None:
        baseline = scorecard()
        candidate = copy.deepcopy(baseline)
        production = candidate["runs"][-1]["pipeline_availability"]
        production["failed_closed_documents"] = 1
        production["completed_documents"] = 0
        production["errors"] = {}
        production["error_stages"] = {}
        comparison = score.compare_scorecards(candidate, baseline)
        self.assertFalse(comparison["regression"]["passed"])
        self.assertFalse(comparison["release_readiness"]["passed"])

    def test_equal_cardinality_scored_membership_swap_names_added_and_removed_ids(
        self,
    ) -> None:
        baseline = scorecard()
        candidate = copy.deepcopy(baseline)
        set_population_split(baseline, scored_id="synthetic-a", failed_id="synthetic-b")
        set_population_split(
            candidate, scored_id="synthetic-b", failed_id="synthetic-a"
        )

        comparison = score.compare_scorecards(candidate, baseline)
        failures = [
            failure
            for failure in comparison["regression"]["failures"]
            if failure.get("gate") == "scored_population_identity_match"
        ]

        self.assertFalse(comparison["regression"]["passed"])
        self.assertEqual(len(failures), len(score.DEFAULT_CONFIGS), failures)
        self.assertEqual(failures[0]["added_document_ids"], ["synthetic-b"])
        self.assertEqual(failures[0]["removed_document_ids"], ["synthetic-a"])

    def test_missing_typed_failure_reason_breaks_reconciliation(self) -> None:
        card = scorecard()
        set_population_split(card, scored_id="synthetic-a", failed_id="synthetic-b")
        run = card["runs"][0]
        run["failed_closed_population"]["reason_counts"] = {}

        with self.assertRaisesRegex(
            ValueError, "reason counts do not reconcile to excluded documents"
        ):
            score._run_population_provenance(
                run,
                context="mutation",
                expected_document_ids=["synthetic-a", "synthetic-b"],
            )

    def test_failure_reason_counts_sum_exactly_to_failed_total(self) -> None:
        card = scorecard()
        set_population_split(card, scored_id="synthetic-a", failed_id="synthetic-b")
        run = card["runs"][0]

        score._run_population_provenance(
            run,
            context="reconciled",
            expected_document_ids=["synthetic-a", "synthetic-b"],
        )

        self.assertEqual(
            sum(run["failed_closed_population"]["reason_counts"].values()),
            run["pipeline_availability"]["failed_closed_documents"],
        )

    def test_diagnostics_carries_reconciled_typed_failure_breakdown(self) -> None:
        card = scorecard()
        set_population_split(card, scored_id="synthetic-a", failed_id="synthetic-b")

        emitted = runner.diagnostics(card)
        failed_population = emitted["runs"][0]["failed_closed_population"]

        self.assertEqual(
            failed_population["reason_counts"],
            {"safety_net_fallback_residual_suspect": 1},
        )
        self.assertEqual(
            sum(failed_population["reason_counts"].values()),
            failed_population["documents"],
        )

    def test_legacy_schema_v3_without_run_identity_is_readable_but_not_comparable(
        self,
    ) -> None:
        legacy = scorecard()
        for run in legacy["runs"]:
            run.pop("scored_population")
            run.pop("failed_closed_population")

        comparison = score.compare_scorecards(legacy, copy.deepcopy(legacy))

        self.assertFalse(comparison["regression"]["passed"])
        self.assertTrue(
            any(
                failure.get("gate") == "candidate_run_population_provenance"
                for failure in comparison["regression"]["failures"]
            )
        )

    def test_strict_rejections_are_bucketed_ratcheted_and_release_blocking(
        self,
    ) -> None:
        baseline = scorecard()
        candidate = copy.deepcopy(baseline)
        candidate["runs"][0]["pipeline_contract"]["strict_would_reject_documents"] = 1

        counts = score._run_correctness_counts(candidate["runs"][0])
        comparison = score.compare_scorecards(candidate, baseline)

        self.assertEqual(counts["strict_rejections"], 1)
        self.assertFalse(comparison["regression"]["passed"])
        self.assertFalse(comparison["release_readiness"]["passed"])
        for verdict in ("regression", "release_readiness"):
            self.assertTrue(
                any(
                    failure.get("config") == score.DEFAULT_CONFIGS[0]
                    and failure.get("gate") == "strict_rejections"
                    for failure in comparison[verdict]["failures"]
                )
            )

    def test_performance_is_informational_by_default(self) -> None:
        baseline = scorecard()
        candidate = copy.deepcopy(baseline)
        candidate["runs"][0]["latency_ms"]["clean_ms"]["p95"] = 2.0
        result = score.compare_performance(candidate, baseline, tolerance_percent=10.0)
        self.assertFalse(result["passed"])
        self.assertFalse(result["gating"])
        self.assertEqual(result["disposition"], "informational")


class ModelValidationTests(unittest.TestCase):
    def write_davlan_bundle(self, bundle: Path) -> runner.ModelPin:
        bundle.mkdir()
        manifest_lines = []
        for name in DAVLAN_ARTIFACTS:
            artifact = bundle / name
            artifact.write_bytes(f"synthetic {name} bytes".encode("utf-8"))
            manifest_lines.append(f"{score.sha256_file(artifact)}  {name}\n")
        manifest = bundle / "SHA256SUMS"
        manifest.write_text("".join(manifest_lines), encoding="utf-8")
        return runner.ModelPin(
            "davlan-mbert-ner-hrl-onnx",
            bundle,
            "SHA256SUMS",
            score.sha256_file(manifest),
            hf_repo=DAVLAN_HF_REPO,
            hf_commit=DAVLAN_HF_COMMIT,
            runtime="onnxruntime",
        )

    def test_production_davlan_pin_is_separate_and_onnx_specific(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        davlan, kiji = runner.load_model_pins(
            repo_root,
            Path("/synthetic/davlan"),
            Path("/synthetic/kiji"),
        )
        self.assertEqual(davlan.model_id, "davlan-mbert-ner-hrl-onnx")
        self.assertEqual(davlan.hf_repo, DAVLAN_HF_REPO)
        self.assertEqual(davlan.hf_commit, DAVLAN_HF_COMMIT)
        self.assertEqual(davlan.expected_sha256, DAVLAN_BUNDLE_SHA)
        self.assertEqual(davlan.runtime, "onnxruntime")
        self.assertEqual(kiji.model_id, "kiji-distilbert")

    def test_missing_model_is_an_actionable_typed_error(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "missing-model"
            with self.assertRaisesRegex(
                runner.ModelBundleError, "model directory does not exist"
            ):
                runner.validate_required_models(repo_root, missing, missing)

    def test_davlan_exact_surface_manifest_validates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pin = self.write_davlan_bundle(Path(tmp) / "davlan")
            result = runner.validate_model_bundle(pin)
        self.assertEqual(result["digest_kind"], "SHA256SUMS")
        self.assertEqual(result["observed_sha256"], pin.expected_sha256)
        self.assertEqual(result["verified_artifacts"], len(DAVLAN_ARTIFACTS))
        self.assertEqual(result["hf_repo"], DAVLAN_HF_REPO)
        self.assertEqual(result["hf_commit"], DAVLAN_HF_COMMIT)
        self.assertEqual(result["bundle_sha"], pin.expected_sha256)

    def test_davlan_transformers_bundle_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "davlan"
            pin = self.write_davlan_bundle(bundle)
            (bundle / "model.onnx").unlink()
            (bundle / "pytorch_model.bin").write_bytes(
                b"synthetic transformers weights"
            )
            with self.assertRaisesRegex(
                runner.ModelBundleError, "unexpected bundle surface"
            ):
                runner.validate_model_bundle(pin)

    def test_davlan_safetensors_bundle_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "davlan"
            pin = self.write_davlan_bundle(bundle)
            (bundle / "model.safetensors").write_bytes(b"synthetic alternate weights")
            with self.assertRaisesRegex(
                runner.ModelBundleError, "unexpected bundle surface"
            ):
                runner.validate_model_bundle(pin)

    def test_davlan_extra_bundle_material_fails_closed(self) -> None:
        for extra in ("pytorch_model.bin", "README.md"):
            with self.subTest(extra=extra), tempfile.TemporaryDirectory() as tmp:
                bundle = Path(tmp) / "davlan"
                pin = self.write_davlan_bundle(bundle)
                unexpected = bundle / extra
                unexpected.parent.mkdir(parents=True, exist_ok=True)
                unexpected.write_bytes(b"synthetic unexpected bytes")
                with self.assertRaisesRegex(
                    runner.ModelBundleError, "unexpected bundle surface"
                ):
                    runner.validate_model_bundle(pin)

    def test_davlan_subdirectory_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "davlan"
            pin = self.write_davlan_bundle(bundle)
            (bundle / "cache").mkdir()
            with self.assertRaisesRegex(
                runner.ModelBundleError, "unexpected bundle surface"
            ):
                runner.validate_model_bundle(pin)

    def test_davlan_missing_runtime_artifact_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "davlan"
            pin = self.write_davlan_bundle(bundle)
            (bundle / "vocab.txt").unlink()
            with self.assertRaisesRegex(
                runner.ModelBundleError, "unexpected bundle surface"
            ):
                runner.validate_model_bundle(pin)

    def test_davlan_symlinked_artifact_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bundle = root / "davlan"
            pin = self.write_davlan_bundle(bundle)
            artifact = bundle / "vocab.txt"
            artifact.unlink()
            target = root / "synthetic-vocab-target.txt"
            target.write_bytes(b"synthetic vocab.txt bytes")
            artifact.symlink_to(target)
            with self.assertRaisesRegex(runner.ModelBundleError, "symlink"):
                runner.validate_model_bundle(pin)

    def test_davlan_manifest_digest_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "davlan"
            pin = self.write_davlan_bundle(bundle)
            wrong_pin = runner.ModelPin(
                pin.model_id,
                pin.path,
                pin.digest_kind,
                "0" * 64,
            )
            with self.assertRaisesRegex(
                runner.ModelBundleError, "checksum manifest digest mismatch"
            ):
                runner.validate_model_bundle(wrong_pin)

    def test_davlan_manifest_must_list_exact_runtime_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "davlan"
            pin = self.write_davlan_bundle(bundle)
            manifest = bundle / "SHA256SUMS"
            lines = manifest.read_text(encoding="utf-8").splitlines()
            lines[-1] = lines[0]
            manifest.write_text("\n".join(lines) + "\n", encoding="utf-8")
            updated_pin = runner.ModelPin(
                pin.model_id,
                pin.path,
                pin.digest_kind,
                score.sha256_file(manifest),
            )
            with self.assertRaisesRegex(runner.ModelBundleError, "must list exactly"):
                runner.validate_model_bundle(updated_pin)

    def test_davlan_malformed_manifest_line_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "davlan"
            pin = self.write_davlan_bundle(bundle)
            manifest = bundle / "SHA256SUMS"
            lines = manifest.read_text(encoding="utf-8").splitlines()
            lines[0] = "not-a-sha  config.json"
            manifest.write_text("\n".join(lines) + "\n", encoding="utf-8")
            updated_pin = runner.ModelPin(
                pin.model_id,
                pin.path,
                pin.digest_kind,
                score.sha256_file(manifest),
            )
            with self.assertRaisesRegex(runner.ModelBundleError, "malformed"):
                runner.validate_model_bundle(updated_pin)

    def test_davlan_unsafe_manifest_path_fails_closed(self) -> None:
        for unsafe in ("../config.json", "/config.json"):
            with self.subTest(unsafe=unsafe), tempfile.TemporaryDirectory() as tmp:
                bundle = Path(tmp) / "davlan"
                pin = self.write_davlan_bundle(bundle)
                manifest = bundle / "SHA256SUMS"
                lines = manifest.read_text(encoding="utf-8").splitlines()
                digest = lines[0].split()[0]
                lines[0] = f"{digest}  {unsafe}"
                manifest.write_text("\n".join(lines) + "\n", encoding="utf-8")
                updated_pin = runner.ModelPin(
                    pin.model_id,
                    pin.path,
                    pin.digest_kind,
                    score.sha256_file(manifest),
                )
                with self.assertRaisesRegex(runner.ModelBundleError, "unsafe path"):
                    runner.validate_model_bundle(updated_pin)

    def test_davlan_artifact_digest_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "davlan"
            pin = self.write_davlan_bundle(bundle)
            (bundle / "config.json").write_bytes(b"synthetic corrupted config")
            with self.assertRaisesRegex(
                runner.ModelBundleError, "artifact digest mismatch"
            ):
                runner.validate_model_bundle(pin)

    def test_kiji_manifest_validation_remains_unchanged(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "kiji"
            bundle.mkdir()
            artifact = bundle / "model.onnx"
            artifact.write_bytes(b"synthetic kiji model bytes")
            manifest = bundle / "SHA256SUMS"
            manifest.write_text(
                f"{score.sha256_file(artifact)}  model.onnx\n",
                encoding="utf-8",
            )
            pin = runner.ModelPin(
                "kiji-distilbert",
                bundle,
                "SHA256SUMS",
                score.sha256_file(manifest),
            )
            result = runner.validate_model_bundle(pin)
        self.assertEqual(result["verified_artifacts"], 1)

    def test_present_model_with_mismatched_manifest_digest_fails_closed(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "present-model"
            bundle.mkdir()
            artifact = bundle / "model.onnx"
            artifact.write_bytes(b"synthetic model bytes")
            manifest = bundle / "SHA256SUMS"
            manifest.write_text(
                f"{score.sha256_file(artifact)}  model.onnx\n",
                encoding="utf-8",
            )
            pin = runner.ModelPin(
                "synthetic-present-model",
                bundle,
                "SHA256SUMS",
                "0" * 64,
            )
            with mock.patch.object(runner, "load_model_pins", return_value=(pin,)):
                with self.assertRaisesRegex(
                    runner.ModelBundleError, "checksum manifest digest mismatch"
                ):
                    runner.validate_required_models(repo_root, bundle, bundle)

    def test_present_model_with_mismatched_artifact_digest_fails_closed(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as tmp:
            bundle = Path(tmp) / "present-model"
            bundle.mkdir()
            artifact = bundle / "model.onnx"
            artifact.write_bytes(b"synthetic corrupted model bytes")
            manifest = bundle / "SHA256SUMS"
            manifest.write_text(f"{'0' * 64}  model.onnx\n", encoding="utf-8")
            pin = runner.ModelPin(
                "synthetic-present-model",
                bundle,
                "SHA256SUMS",
                score.sha256_file(manifest),
            )
            with mock.patch.object(runner, "load_model_pins", return_value=(pin,)):
                with self.assertRaisesRegex(
                    runner.ModelBundleError, "artifact digest mismatch"
                ):
                    runner.validate_required_models(repo_root, bundle, bundle)


class ProfileIsolationTests(unittest.TestCase):
    def document(self) -> score.Document:
        return score.Document(
            uid="synthetic-no-opf-1",
            text="synthetic text",
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(),
        )

    def test_full_profile_never_passes_opf_even_when_environment_is_set(self) -> None:
        fake_environment = {
            "PATH": "/synthetic/bin",
            "GAZE_OPENAI_FILTER_OPF": "/tmp/fake-opf",
            "OPF_CHECKPOINT": "/tmp/fake-checkpoint",
            "GAZE_OPF_DAEMON_SOCKET": "/tmp/fake.sock",
        }
        fake_run = scorecard()["runs"][0]
        with mock.patch.object(
            score, "run_config", return_value=fake_run
        ) as run_config:
            _, provenance = runner.execute_measurements(
                repo_root=Path("/synthetic/repo"),
                binary=Path("/synthetic/clean_for_bench"),
                documents=[self.document()],
                davlan_model=Path("/synthetic/davlan"),
                kiji_model=Path("/synthetic/kiji"),
                threshold=0.3,
                diagnostics_dir=Path("/synthetic/diagnostics"),
                warmup_count=1,
                measured_repetitions=1,
                source_environment=fake_environment,
            )
        self.assertEqual(run_config.call_count, len(score.DEFAULT_CONFIGS))
        for call in run_config.call_args_list:
            self.assertEqual(call.args[6:9], (None, None, None))
            passed_environment = call.kwargs["base_environment"]
            self.assertEqual(passed_environment, {"PATH": "/synthetic/bin"})
        serialized = json.loads(json.dumps(provenance))
        first = serialized[0]["runs"][0]
        self.assertEqual(first["warmup_count"], 1)
        self.assertEqual(len(first["discarded_warmup_samples"]), 1)
        self.assertEqual(first["cold_start_to_first_validated_response_ms"], 2.0)

    def test_quick_sampling_is_reproducible(self) -> None:
        documents = [
            score.Document(
                uid=f"synthetic-sample-{index}",
                text="synthetic text",
                language="en" if index % 2 else "de",
                region="US" if index % 2 else "DE",
                source_dataset="unit-test",
                spans=(),
            )
            for index in range(20)
        ]
        first, _ = score.stratified_sample(documents, 7, seed=41)
        second, _ = score.stratified_sample(list(reversed(documents)), 7, seed=41)
        self.assertEqual(
            [document.uid for document in first],
            [document.uid for document in second],
        )

    def test_repetitions_require_exact_strict_rejection_counts(self) -> None:
        runs_by_config = {run["config"]: run for run in scorecard()["runs"]}
        call_count = 0

        def fake_run_config(*args: object, **kwargs: object) -> dict[str, object]:
            nonlocal call_count
            config = str(args[2])
            run = copy.deepcopy(runs_by_config[config])
            if (
                call_count >= len(score.DEFAULT_CONFIGS)
                and config == score.DEFAULT_CONFIGS[0]
            ):
                run["pipeline_contract"]["strict_would_reject_documents"] = 1
            call_count += 1
            return run

        with mock.patch.object(score, "run_config", side_effect=fake_run_config):
            with self.assertRaisesRegex(
                runner.RepetitionMismatchError,
                "integer correctness counts",
            ):
                runner.execute_measurements(
                    repo_root=Path("/synthetic/repo"),
                    binary=Path("/synthetic/clean_for_bench"),
                    documents=[self.document()],
                    davlan_model=Path("/synthetic/davlan"),
                    kiji_model=Path("/synthetic/kiji"),
                    threshold=0.3,
                    diagnostics_dir=Path("/synthetic/diagnostics"),
                    warmup_count=0,
                    measured_repetitions=2,
                    source_environment={"PATH": "/synthetic/bin"},
                )


class BaselineGuardTests(unittest.TestCase):
    def args(self, **overrides: object) -> Namespace:
        values = {
            "profile": "full",
            "accept_baseline": Path("/tmp/synthetic-baseline.json"),
            "compare_baseline": Path("/tmp/synthetic-baseline.json"),
            "accept_baseline_confirm": None,
        }
        values.update(overrides)
        return Namespace(**values)

    def test_accept_baseline_requires_exact_review_confirmation(self) -> None:
        result = {"passed": True}
        with self.assertRaisesRegex(
            runner.BaselineAcceptanceError, "requires --accept-baseline-confirm"
        ):
            runner.accept_baseline(self.args(), scorecard(), result, result)

    def test_quick_profile_can_never_replace_baseline(self) -> None:
        result = {"passed": True}
        with self.assertRaisesRegex(
            runner.BaselineAcceptanceError, "only a full-profile"
        ):
            runner.accept_baseline(
                self.args(
                    profile="quick",
                    accept_baseline_confirm=runner.BASELINE_CONFIRMATION,
                ),
                scorecard(),
                result,
                result,
            )

    def test_initialization_cannot_overwrite_an_existing_baseline(self) -> None:
        result = {"passed": True}
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            baseline.write_text("{}\n", encoding="utf-8")
            with self.assertRaisesRegex(
                runner.BaselineAcceptanceError, "refusing to overwrite"
            ):
                runner.accept_baseline(
                    self.args(
                        accept_baseline=baseline,
                        compare_baseline=None,
                        accept_baseline_confirm=runner.BASELINE_CONFIRMATION,
                    ),
                    scorecard(),
                    {"passed": None},
                    result,
                )

    def test_reviewed_release_ready_initialization_writes_new_baseline(self) -> None:
        result = {"passed": True}
        with tempfile.TemporaryDirectory() as tmp:
            baseline = Path(tmp) / "baseline.json"
            runner.accept_baseline(
                self.args(
                    accept_baseline=baseline,
                    compare_baseline=None,
                    accept_baseline_confirm=runner.BASELINE_CONFIRMATION,
                ),
                scorecard(),
                {"passed": None},
                result,
            )
            self.assertEqual(json.loads(baseline.read_text()), scorecard())


if __name__ == "__main__":
    unittest.main()
