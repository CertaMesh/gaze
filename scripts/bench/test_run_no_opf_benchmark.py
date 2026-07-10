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


def scorecard(leaked: int = 0) -> dict[str, object]:
    uid = "synthetic-runner-1"
    digest = hashlib.sha256(
        json.dumps([uid], separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    fully_covered = 1 if leaked == 0 else 0
    run = {
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
            {"config": config, **copy.deepcopy(run)}
            for config in score.DEFAULT_CONFIGS
        ],
    }


class IntegerComparatorTests(unittest.TestCase):
    def test_empty_and_missing_candidates_fail_closed(self) -> None:
        baseline = scorecard()
        for candidate in ({}, {"schema_version": 3, "runs": []}):
            comparison = score.compare_scorecards(candidate, baseline)
            self.assertFalse(comparison["regression"]["passed"])
            self.assertFalse(comparison["release_readiness"]["passed"])

    def test_integer_leak_ratchet_catches_change_even_if_rate_is_unchanged(self) -> None:
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

    def test_performance_is_informational_by_default(self) -> None:
        baseline = scorecard()
        candidate = copy.deepcopy(baseline)
        candidate["runs"][0]["latency_ms"]["clean_ms"]["p95"] = 2.0
        result = score.compare_performance(
            candidate, baseline, tolerance_percent=10.0
        )
        self.assertFalse(result["passed"])
        self.assertFalse(result["gating"])
        self.assertEqual(result["disposition"], "informational")


class ModelValidationTests(unittest.TestCase):
    def test_missing_model_is_an_actionable_typed_error(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "missing-model"
            with self.assertRaisesRegex(
                runner.ModelBundleError, "model directory does not exist"
            ):
                runner.validate_required_models(repo_root, missing, missing)


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
        with mock.patch.object(score, "run_config", return_value=fake_run) as run_config:
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
