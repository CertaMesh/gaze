#!/usr/bin/env python3

import copy
import hashlib
import io
import json
import socket
import sys
import tempfile
import tomllib
import types
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaze_bench_score as benchmark
import openpii_gaze_bench as openpii_benchmark
import dataiku_en_de_gaze_bench as dataiku_benchmark
import opf_daemon


EXPECTED_BUILTIN_SOURCE_IDS = frozenset(
    {
        "ner",
        "kiji-distilbert",
        "kiji-distilbert-ort",
        "kiji-distilbert-tract",
        "kiji-distilbert-candle",
        "kiji-distilbert-subprocess",
        "openai-privacy-filter",
        "openai-privacy-filter-subprocess",
    }
)
BUILTIN_SOURCE_LITERAL_PATHS = {
    "ner": "crates/gaze-recognizers/src/ner/recognizer.rs",
    "kiji-distilbert": (
        "crates/gaze-recognizers/src/safety_net/kiji_distilbert/mod.rs"
    ),
    "kiji-distilbert-ort": (
        "crates/gaze-recognizers/src/safety_net/kiji_distilbert/backend/ort.rs"
    ),
    "kiji-distilbert-tract": (
        "crates/gaze-recognizers/src/safety_net/kiji_distilbert/backend/tract.rs"
    ),
    "kiji-distilbert-candle": (
        "crates/gaze-recognizers/src/safety_net/kiji_distilbert/backend/candle.rs"
    ),
    "kiji-distilbert-subprocess": (
        "crates/gaze-recognizers/src/safety_net/kiji_distilbert/backend/subprocess.rs"
    ),
    "openai-privacy-filter": (
        "crates/gaze-recognizers/src/safety_net/openai_filter/mod.rs"
    ),
    "openai-privacy-filter-subprocess": (
        "crates/gaze-recognizers/src/safety_net/openai_filter/backend/subprocess.rs"
    ),
}


def assert_builtin_source_literals(
    source_ids: list[str], authoritative_source_text: dict[str, str]
) -> None:
    for source_id in source_ids:
        relative_path = BUILTIN_SOURCE_LITERAL_PATHS.get(source_id)
        if relative_path is None:
            raise AssertionError(
                f"declared built-in source ID has no authoritative source mapping: "
                f"{source_id}"
            )
        source_text = authoritative_source_text.get(relative_path)
        if source_text is None or f'"{source_id}"' not in source_text:
            raise AssertionError(
                f"declared built-in source ID lacks a literal in its authoritative "
                f"source file {relative_path}: {source_id}"
            )


class OffsetTests(unittest.TestCase):
    def test_character_offsets_convert_to_utf8_byte_offsets(self) -> None:
        self.assertEqual(benchmark.char_to_byte_offsets("aé中"), [0, 1, 3, 6])

    def test_adjacent_and_overlapping_intervals_are_merged(self) -> None:
        self.assertEqual(
            benchmark.merge_intervals([(5, 9), (0, 3), (3, 6), (12, 14)]),
            [(0, 9), (12, 14)],
        )


class MetricTests(unittest.TestCase):
    def document(self) -> benchmark.Document:
        text = "alice@example.invalid is synthetic"
        email_end = len("alice@example.invalid".encode())
        return benchmark.Document(
            uid="synthetic-1",
            text=text,
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(benchmark.Span(0, email_end, "EMAIL"),),
        )

    def test_partial_mask_is_counted_as_a_leak(self) -> None:
        document = self.document()
        metrics = benchmark.MetricAccumulator()
        metrics.add(document, [benchmark.Span(0, 5, "email")])
        result = metrics.result()

        self.assertEqual(result["utf8_bytes"]["true_positive"], 5)
        self.assertEqual(
            result["utf8_bytes"]["leaked"],
            len("alice@example.invalid".encode()) - 5,
        )
        self.assertEqual(result["entities"]["fully_covered"], 0)
        self.assertEqual(result["entities"]["overlapped"], 1)
        self.assertEqual(result["documents_without_leaks"], 0)

    def test_primary_coverage_is_label_agnostic(self) -> None:
        document = self.document()
        email_end = document.spans[0].end
        metrics = benchmark.MetricAccumulator()
        metrics.add(document, [benchmark.Span(0, email_end, "name")])
        result = metrics.result()

        self.assertEqual(result["utf8_bytes"]["recall"], 1.0)
        self.assertEqual(result["entities"]["full_coverage_recall"], 1.0)
        self.assertEqual(result["zero_leak_document_rate"], 1.0)

    def test_prediction_covering_the_full_entity_is_fully_covered(self) -> None:
        document = self.document()
        email_end = document.spans[0].end
        metrics = benchmark.MetricAccumulator()
        metrics.add(document, [benchmark.Span(0, email_end + 1, "name")])
        result = metrics.result()

        self.assertEqual(result["entities"]["fully_covered"], 1)
        self.assertEqual(result["entities"]["overlapped"], 1)
        self.assertEqual(result["entities"]["exact_boundary"], 0)
        self.assertEqual(result["documents_without_leaks"], 1)

    def test_adjacent_prediction_does_not_protect_or_overlap_entity(self) -> None:
        document = self.document()
        email_end = document.spans[0].end
        metrics = benchmark.MetricAccumulator()
        metrics.add(document, [benchmark.Span(email_end, email_end + 1, "email")])
        result = metrics.result()

        self.assertEqual(result["utf8_bytes"]["true_positive"], 0)
        self.assertEqual(result["entities"]["fully_covered"], 0)
        self.assertEqual(result["entities"]["overlapped"], 0)
        self.assertEqual(result["prediction_spans"]["overlapped_gold"], 0)
        self.assertEqual(result["documents_without_leaks"], 0)


class ContractTests(unittest.TestCase):
    def test_contract_score_does_not_compensate_restore_failure(self) -> None:
        accumulator = benchmark.ContractAccumulator()
        accumulator.add(
            {
                "restore": {"exact": False, "decision": "success"},
                "manifest_integrity": {
                    "spans": 1,
                    "invalid_clean_bounds": 0,
                    "invalid_raw_bounds": 0,
                    "overlapping_clean_spans": 0,
                    "non_monotonic_raw_spans": 0,
                    "token_restore_failures": 0,
                    "raw_value_mismatches": 1,
                },
                "initial_safety_net_stats": {
                    "suspect_count": 1,
                    "uncovered_count": 1,
                    "partial_bleed_count": 0,
                    "class_mismatch_count": 0,
                },
                "strict_would_reject": True,
                "post_policy_safety_net_stats": {
                    "suspect_count": 0,
                },
            }
        )
        result = accumulator.result()

        self.assertEqual(result["restore_exact_rate"], 0.0)
        self.assertEqual(result["restore_success_decision_rate"], 1.0)
        self.assertEqual(result["manifest_valid_document_rate"], 0.0)
        self.assertEqual(result["strict_acceptance_rate"], 0.0)
        self.assertEqual(result["post_policy_zero_suspect_rate"], 1.0)

    def test_typed_pipeline_error_only_counts_as_failed_closed_availability(
        self,
    ) -> None:
        """A typed failure bypasses score/contract accumulation in the current scorer."""
        document = benchmark.Document(
            uid="synthetic-failure",
            text="alice@example.invalid",
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(benchmark.Span(0, len("alice@example.invalid"), "EMAIL"),),
        )
        response = {
            "fixture_id": document.uid,
            "pipeline_error_code": "safety_net_fallback_residual_suspect",
            "pipeline_error_stage": "clean",
            "timing": {"total_ms": 1.25},
        }
        process = mock.Mock()
        process.stdin = io.StringIO()
        process.stdout = io.StringIO(json.dumps(response) + "\n")
        process.wait.return_value = 0
        repo_root = Path(benchmark.__file__).resolve().parents[2]

        with tempfile.TemporaryDirectory(dir=repo_root) as temporary:
            with mock.patch.object(benchmark.subprocess, "Popen", return_value=process):
                result = benchmark.run_config(
                    repo_root=repo_root,
                    binary=Path(temporary) / "synthetic-runner",
                    config="rule-floor-extended",
                    documents=[document],
                    model_dir=Path(temporary),
                    kiji_model_dir=Path(temporary),
                    opf_command=None,
                    opf_checkpoint=None,
                    opf_daemon_socket=None,
                    threshold=0.3,
                    diagnostics_dir=Path(temporary),
                )

        self.assertEqual(result["metrics"]["documents"], 0)
        self.assertEqual(result["pipeline_contract"]["documents"], 0)
        self.assertEqual(result["pipeline_contract"]["restore_exact_rate"], 1.0)
        self.assertEqual(
            result["pipeline_contract"]["manifest_valid_document_rate"], 1.0
        )
        self.assertEqual(result["pipeline_contract"]["strict_acceptance_rate"], 0.0)
        self.assertEqual(
            result["pipeline_availability"],
            {
                "attempted_documents": 1,
                "completed_documents": 0,
                "completion_rate": 0.0,
                "failed_closed_documents": 1,
                "errors": {"safety_net_fallback_residual_suspect": 1},
                "error_stages": {"clean": 1},
            },
        )
        self.assertEqual(result["scored_population"]["document_ids"], [])
        self.assertEqual(
            result["failed_closed_population"]["excluded_documents"],
            [
                {
                    "document_id": "synthetic-failure",
                    "reason": "safety_net_fallback_residual_suspect",
                    "stage": "clean",
                }
            ],
        )
        self.assertEqual(result["latency_ms"], {})
        self.assertIsNone(result["process"]["documents_per_second"])


class ResponseValidationTests(unittest.TestCase):
    def document(self) -> benchmark.Document:
        return benchmark.Document(
            uid="synthetic-response",
            text="synthetic text",
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(),
        )

    def success_response(self) -> dict[str, object]:
        return {
            "fixture_id": "synthetic-response",
            "clean_text": "synthetic text",
            "manifest_spans": [],
            "pre_safety_text_len": None,
            "pre_safety_manifest_spans": None,
            "leak_suspects": [],
            "safety_net_mode": "off",
            "strict_would_reject": False,
            "initial_safety_net_stats": {
                "suspect_count": 0,
                "uncovered_count": 0,
                "partial_bleed_count": 0,
                "class_mismatch_count": 0,
                "locale_skipped_count": 0,
            },
            "post_policy_safety_net_stats": None,
            "restore": {
                "exact": True,
                "decision": "success",
                "unknown_token_count": 0,
                "manifest_bypass_count": 0,
                "fresh_pii_detected_count": 0,
                "phase_execution_mask": 1,
            },
            "manifest_integrity": {
                "spans": 0,
                "invalid_clean_bounds": 0,
                "invalid_raw_bounds": 0,
                "overlapping_clean_spans": 0,
                "non_monotonic_raw_spans": 0,
                "token_restore_failures": 0,
                "raw_value_mismatches": 0,
            },
            "timing": {
                "clean_ms": 1.0,
                "restore_ms": 0.3,
                "post_policy_scan_ms": None,
            },
            "final_protection_trace": [],
        }

    def trace_document(self) -> benchmark.Document:
        return benchmark.Document(
            uid="synthetic-response",
            text="é synthetic",
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(benchmark.Span(0, 2, "NAME"),),
        )

    def tokenize_response(self) -> dict[str, object]:
        response = self.success_response()
        response["manifest_spans"] = [
            {
                "raw_start": 0,
                "raw_end": 2,
                "clean_start": 0,
                "clean_end": 8,
                "class": "name",
            }
        ]
        response["manifest_integrity"]["spans"] = 1
        response["final_protection_trace"] = [
            {
                "raw_start": 0,
                "raw_end": 2,
                "class": "name",
                "action": "tokenize",
                "provenance": {
                    "stage": "primary_pipeline",
                    "decision": "policy",
                    "source_ids": ["rule.name"],
                },
            }
        ]
        return response

    def test_missing_required_field_fails_closed(self) -> None:
        response = self.success_response()
        response.pop("restore")
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "missing fields"
        ):
            benchmark.validate_response(self.document(), response)

    def test_missing_final_trace_fails_closed(self) -> None:
        response = self.success_response()
        response.pop("final_protection_trace")
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "missing fields"
        ):
            benchmark.validate_response(self.document(), response)

    def test_unknown_field_fails_closed(self) -> None:
        response = self.success_response()
        response["unratified_wire_field"] = 1
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "unknown fields"
        ):
            benchmark.validate_response(self.document(), response)

    def test_wrong_typed_nested_field_fails_closed(self) -> None:
        response = self.success_response()
        response["manifest_integrity"]["spans"] = True
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "expected an integer"
        ):
            benchmark.validate_response(self.document(), response)

    def test_success_timing_requires_the_exact_ratified_fields(self) -> None:
        response = self.success_response()
        response["timing"]["total_ms"] = 1.0
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "unknown fields"
        ):
            benchmark.validate_response(self.document(), response)

        for field in ("pass1_ms", "pass2_ms", "pass3_ms"):
            with self.subTest(field=field):
                response = self.success_response()
                response["timing"][field] = 1.0
                with self.assertRaisesRegex(
                    benchmark.ResponseValidationError, "unknown fields"
                ):
                    benchmark.validate_response(self.document(), response)

        response = self.success_response()
        response["timing"].pop("clean_ms")
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "missing fields"
        ):
            benchmark.validate_response(self.document(), response)

    def test_success_timing_only_allows_null_for_post_policy_scan(self) -> None:
        benchmark.validate_response(self.document(), self.success_response())

        for field in ("clean_ms", "restore_ms"):
            with self.subTest(field=field):
                response = self.success_response()
                response["timing"][field] = None
                with self.assertRaisesRegex(
                    benchmark.ResponseValidationError, "expected a finite number"
                ):
                    benchmark.validate_response(self.document(), response)

    def test_success_timing_rejects_non_finite_boolean_and_negative_values(
        self,
    ) -> None:
        for field in ("clean_ms", "restore_ms", "post_policy_scan_ms"):
            for value in (True, -0.1, float("nan"), float("inf")):
                with self.subTest(field=field, value=value):
                    response = self.success_response()
                    response["timing"][field] = value
                    with self.assertRaises(benchmark.ResponseValidationError):
                        benchmark.validate_response(self.document(), response)

    def test_success_latency_and_throughput_use_measured_clean_ms_only(self) -> None:
        first = self.success_response()
        first["timing"] = {
            "clean_ms": 2.0,
            "restore_ms": 10.0,
            "post_policy_scan_ms": None,
        }
        second = self.success_response()
        second["fixture_id"] = "synthetic-response-2"
        second["timing"] = {
            "clean_ms": 3.0,
            "restore_ms": 20.0,
            "post_policy_scan_ms": 7.0,
        }
        process = mock.Mock()
        process.stdin = io.StringIO()
        process.stdout = io.StringIO(
            "\n".join((json.dumps(first), json.dumps(second))) + "\n"
        )
        process.wait.return_value = 0
        repo_root = Path(benchmark.__file__).resolve().parents[2]
        second_document = benchmark.Document(
            uid="synthetic-response-2",
            text="synthetic text",
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(),
        )

        with tempfile.TemporaryDirectory(dir=repo_root) as temporary:
            with mock.patch.object(benchmark.subprocess, "Popen", return_value=process):
                result = benchmark.run_config(
                    repo_root=repo_root,
                    binary=Path(temporary) / "synthetic-runner",
                    config="production-full-stack",
                    documents=[self.document(), second_document],
                    model_dir=Path(temporary),
                    kiji_model_dir=Path(temporary),
                    opf_command=None,
                    opf_checkpoint=None,
                    opf_daemon_socket=None,
                    threshold=0.5,
                    diagnostics_dir=Path(temporary) / "diagnostics",
                )

        self.assertEqual(result["latency_ms"]["clean_ms"]["mean"], 2.5)
        self.assertEqual(result["latency_ms"]["restore_ms"]["mean"], 15.0)
        self.assertEqual(result["latency_ms"]["post_policy_scan_ms"]["mean"], 7.0)
        self.assertNotIn("total_ms", result["latency_ms"])
        self.assertNotIn("pass1_ms", result["latency_ms"])
        self.assertNotIn("pass2_ms", result["latency_ms"])
        self.assertNotIn("pass3_ms", result["latency_ms"])
        self.assertEqual(result["process"]["documents_per_second"], 400.0)

    def test_warmup_correctness_and_pipeline_errors_are_logged_not_counted(
        self,
    ) -> None:
        document = self.document()
        warmup_error = {
            "fixture_id": document.uid,
            "pipeline_error_stage": "clean",
            "pipeline_error_code": "safety_net_fallback_residual_suspect",
            "timing": {"total_ms": 1.25},
        }
        warmup_rejection = self.success_response()
        scored_rejection = self.success_response()
        for response in (warmup_rejection, scored_rejection):
            response["leak_suspects"] = [
                {
                    "clean_start": 0,
                    "clean_end": 2,
                    "action_start": 0,
                    "action_end": 2,
                    "class": "name",
                    "safety_net_id": "safety.name",
                    "kind": "uncovered",
                }
            ]
            response["initial_safety_net_stats"].update(
                {"suspect_count": 1, "uncovered_count": 1}
            )
            response["strict_would_reject"] = True
        process = mock.Mock()
        process.stdin = io.StringIO()
        process.stdout = io.StringIO(
            "\n".join(
                json.dumps(response)
                for response in (warmup_error, warmup_rejection, scored_rejection)
            )
            + "\n"
        )
        process.wait.return_value = 0
        repo_root = Path(benchmark.__file__).resolve().parents[2]

        with tempfile.TemporaryDirectory(dir=repo_root) as temporary:
            with mock.patch.object(benchmark.subprocess, "Popen", return_value=process):
                result = benchmark.run_config(
                    repo_root=repo_root,
                    binary=Path(temporary) / "synthetic-runner",
                    config="full-stack-kiji-resolve",
                    documents=[document],
                    model_dir=Path(temporary),
                    kiji_model_dir=Path(temporary),
                    opf_command=None,
                    opf_checkpoint=None,
                    opf_daemon_socket=None,
                    threshold=0.3,
                    diagnostics_dir=Path(temporary) / "diagnostics",
                    warmup_count=2,
                )

        self.assertEqual(result["pipeline_contract"]["documents"], 1)
        self.assertEqual(
            result["pipeline_contract"]["strict_would_reject_documents"], 1
        )
        self.assertEqual(result["pipeline_contract"]["strict_acceptance_rate"], 0.0)
        self.assertEqual(result["pipeline_availability"]["failed_closed_documents"], 0)
        discarded = result["process"]["discarded_warmup_samples"]
        self.assertEqual(len(discarded), 2)
        self.assertEqual(
            discarded[0]["pipeline_error_code"],
            "safety_net_fallback_residual_suspect",
        )
        self.assertEqual(discarded[0]["pipeline_error_stage"], "clean")
        self.assertEqual(discarded[1]["correctness"]["strict_rejections"], 1)

    def test_unknown_pipeline_error_timing_field_fails_closed(self) -> None:
        response = {
            "fixture_id": "synthetic-response",
            "pipeline_error_stage": "clean",
            "pipeline_error_code": "safety_net_runtime_other",
            "timing": {"total_ms": 1.0, "extra_ms": 0.1},
        }
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "unknown fields"
        ):
            benchmark.validate_response(self.document(), response)

    def test_pipeline_error_reason_and_stage_are_closed_sets(self) -> None:
        response = {
            "fixture_id": "synthetic-response",
            "pipeline_error_stage": "clean",
            "pipeline_error_code": "safety_net_runtime_subprocess_timeout",
            "timing": {"total_ms": 1.0},
        }
        self.assertEqual(
            benchmark.validate_response(self.document(), copy.deepcopy(response)),
            response,
        )

        unknown_reason = copy.deepcopy(response)
        unknown_reason["pipeline_error_code"] = "synthetic_unknown_reason"
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "unknown closed reason"
        ):
            benchmark.validate_response(self.document(), unknown_reason)

        unknown_stage = copy.deepcopy(response)
        unknown_stage["pipeline_error_stage"] = "synthetic_unknown_stage"
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "unknown closed stage"
        ):
            benchmark.validate_response(self.document(), unknown_stage)

    def test_tokenize_trace_must_match_final_manifest_one_to_one(self) -> None:
        response = self.tokenize_response()
        response["manifest_spans"][0]["class"] = "email"
        with self.assertRaisesRegex(benchmark.ResponseValidationError, "agree 1:1"):
            benchmark.validate_response(self.trace_document(), response)

    def test_trace_rejects_unknown_item_and_provenance_fields(self) -> None:
        response = self.tokenize_response()
        response["final_protection_trace"][0]["raw_value"] = "synthetic"
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "unknown fields"
        ):
            benchmark.validate_response(self.trace_document(), response)

        response = self.tokenize_response()
        response["final_protection_trace"][0]["provenance"].pop("source_ids")
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "missing fields"
        ):
            benchmark.validate_response(self.trace_document(), response)

        response = self.tokenize_response()
        response["final_protection_trace"][0]["provenance"]["extra"] = "metadata"
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "unknown fields"
        ):
            benchmark.validate_response(self.trace_document(), response)

    def test_trace_rejects_wrong_types_and_invalid_closed_enum_combinations(
        self,
    ) -> None:
        response = self.tokenize_response()
        response["final_protection_trace"][0]["raw_start"] = False
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "expected an integer"
        ):
            benchmark.validate_response(self.trace_document(), response)

        response = self.tokenize_response()
        response["final_protection_trace"][0]["action"] = "mask"
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "unknown action"
        ):
            benchmark.validate_response(self.trace_document(), response)

        response = self.tokenize_response()
        response["final_protection_trace"][0]["provenance"]["decision"] = "resolve"
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "invalid.*combination"
        ):
            benchmark.validate_response(self.trace_document(), response)

    def test_trace_source_ids_are_non_empty_sorted_and_duplicate_free(self) -> None:
        for source_ids in (
            [],
            ["rule.z", "rule.a"],
            ["rule.a", "rule.a"],
            [1],
        ):
            with self.subTest(source_ids=source_ids):
                response = self.tokenize_response()
                response["final_protection_trace"][0]["provenance"]["source_ids"] = (
                    source_ids
                )
                with self.assertRaises(benchmark.ResponseValidationError):
                    benchmark.validate_response(self.trace_document(), response)

    def test_trace_rejects_noncanonical_classes_and_pii_bearing_source_ids(
        self,
    ) -> None:
        response = self.tokenize_response()
        response["manifest_spans"][0]["class"] = "not-canonical"
        response["final_protection_trace"][0]["class"] = "not-canonical"
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "canonical PiiClass"
        ):
            benchmark.validate_response(self.trace_document(), response)

        for source_id in ("alice@example.invalid", "+1-555-0142", "Dr. Schmidt"):
            with self.subTest(source_id=source_id):
                response = self.tokenize_response()
                response["final_protection_trace"][0]["provenance"]["source_ids"] = [
                    source_id
                ]
                with self.assertRaisesRegex(
                    benchmark.ResponseValidationError, "metadata-only stable identifier"
                ):
                    benchmark.validate_response(self.trace_document(), response)

    def test_trace_accepts_producer_canonical_custom_class_and_stable_source_ids(
        self,
    ) -> None:
        response = self.tokenize_response()
        response["manifest_spans"][0]["class"] = "custom:family:payment-card-or-iban"
        item = response["final_protection_trace"][0]
        item["class"] = "custom:family:payment-card-or-iban"
        item["provenance"]["source_ids"] = [
            "kiji",
            "rule.iban",
        ]
        benchmark.validate_response(self.trace_document(), response)

    def test_trace_rejects_source_id_reproducing_protected_request_content(
        self,
    ) -> None:
        for protected_value, source_id in (
            ("order-1234", "order-1234"),
            ("order-1234", "rule-order-1234-source"),
            ("davlan", "ner.davlan"),
        ):
            with self.subTest(protected_value=protected_value, source_id=source_id):
                document = benchmark.Document(
                    uid="synthetic-response",
                    text=protected_value,
                    language="en",
                    region="US",
                    source_dataset="unit-test",
                    spans=(benchmark.Span(0, len(protected_value), "ORDER"),),
                )
                response = self.tokenize_response()
                response["manifest_spans"][0]["raw_end"] = len(protected_value)
                item = response["final_protection_trace"][0]
                item["raw_end"] = len(protected_value)
                item["provenance"]["source_ids"] = [source_id]
                with self.assertRaisesRegex(
                    benchmark.ResponseValidationError,
                    "reproduces protected request content",
                ):
                    benchmark.validate_response(document, response)

    def test_trace_accepts_committed_id_despite_bounded_protected_segment(
        self,
    ) -> None:
        protected_value = "de"
        document = benchmark.Document(
            uid="synthetic-response",
            text=protected_value,
            language="de",
            region="DE",
            source_dataset="unit-test",
            spans=(benchmark.Span(0, len(protected_value), "POSTAL"),),
        )
        response = self.tokenize_response()
        response["manifest_spans"][0]["raw_end"] = len(protected_value)
        item = response["final_protection_trace"][0]
        item["raw_end"] = len(protected_value)
        item["provenance"]["source_ids"] = ["postal.de"]

        benchmark.validate_response(document, response)

    def test_trace_accepts_builtin_id_equal_to_protected_content(self) -> None:
        protected_value = "ner"
        document = benchmark.Document(
            uid="synthetic-response",
            text=protected_value,
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(benchmark.Span(0, len(protected_value), "ORDER"),),
        )
        response = self.tokenize_response()
        response["manifest_spans"][0]["raw_end"] = len(protected_value)
        item = response["final_protection_trace"][0]
        item["raw_end"] = len(protected_value)
        item["provenance"]["source_ids"] = ["ner"]

        benchmark.validate_response(document, response)

    def test_trace_rejects_novel_id_with_bounded_protected_segment(self) -> None:
        protected_value = "de"
        document = benchmark.Document(
            uid="synthetic-response",
            text=protected_value,
            language="de",
            region="DE",
            source_dataset="unit-test",
            spans=(benchmark.Span(0, len(protected_value), "POSTAL"),),
        )
        response = self.tokenize_response()
        response["manifest_spans"][0]["raw_end"] = len(protected_value)
        item = response["final_protection_trace"][0]
        item["raw_end"] = len(protected_value)
        item["provenance"]["source_ids"] = ["novel.de.source"]

        with self.assertRaisesRegex(
            benchmark.ResponseValidationError,
            "reproduces protected request content",
        ):
            benchmark.validate_response(document, response)

    def test_trace_accepts_unbounded_source_id_substring(self) -> None:
        protected_value = "avl"
        document = benchmark.Document(
            uid="synthetic-response",
            text=protected_value,
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(benchmark.Span(0, len(protected_value), "ORDER"),),
        )
        response = self.tokenize_response()
        response["manifest_spans"][0]["raw_end"] = len(protected_value)
        item = response["final_protection_trace"][0]
        item["raw_end"] = len(protected_value)
        item["provenance"]["source_ids"] = ["davlan"]

        benchmark.validate_response(document, response)

    def test_all_ratified_trace_combinations_validate(self) -> None:
        combinations = (
            ("primary_pipeline", "policy", "tokenize"),
            ("safety_net", "resolve", "tokenize"),
            ("safety_net", "redact", "redact"),
            ("safety_net", "fallback_redact", "redact"),
        )
        for stage, decision, action in combinations:
            with self.subTest(stage=stage, decision=decision, action=action):
                response = self.tokenize_response()
                item = response["final_protection_trace"][0]
                item["action"] = action
                item["provenance"]["stage"] = stage
                item["provenance"]["decision"] = decision
                if action == "redact":
                    response["manifest_spans"] = []
                    response["manifest_integrity"]["spans"] = 0
                    response["restore"]["exact"] = False
                benchmark.validate_response(self.trace_document(), response)

    def test_trace_spans_use_original_utf8_boundaries_and_are_disjoint(self) -> None:
        response = self.tokenize_response()
        response["final_protection_trace"][0]["raw_end"] = 1
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "char boundaries"
        ):
            benchmark.validate_response(self.trace_document(), response)

        response = self.tokenize_response()
        response["manifest_spans"].append(
            {
                "raw_start": 2,
                "raw_end": 4,
                "clean_start": 8,
                "clean_end": 16,
                "class": "name",
            }
        )
        response["final_protection_trace"].append(
            {
                "raw_start": 0,
                "raw_end": 4,
                "class": "name",
                "action": "tokenize",
                "provenance": {
                    "stage": "safety_net",
                    "decision": "resolve",
                    "source_ids": ["safety.name"],
                },
            }
        )
        with self.assertRaises(benchmark.ResponseValidationError):
            benchmark.validate_response(self.trace_document(), response)

    def test_pipeline_error_response_must_not_contain_final_trace(self) -> None:
        response = {
            "fixture_id": "synthetic-response",
            "pipeline_error_stage": "clean",
            "pipeline_error_code": "pipeline_error",
            "timing": {"total_ms": 1.0},
            "final_protection_trace": [],
        }
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "unknown fields"
        ):
            benchmark.validate_response(self.document(), response)

    def test_telemetry_output_disagreement_fails_closed(self) -> None:
        response = self.tokenize_response()
        response["manifest_integrity"]["spans"] = 0
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "telemetry/output disagreement"
        ):
            benchmark.validate_response(self.trace_document(), response)

        response = self.success_response()
        response["initial_safety_net_stats"]["suspect_count"] = 1
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "telemetry/output disagreement"
        ):
            benchmark.validate_response(self.document(), response)

    def test_redact_protects_safety_but_cannot_claim_exact_restore(self) -> None:
        response = self.success_response()
        response["restore"]["exact"] = False
        response["final_protection_trace"] = [
            {
                "raw_start": 0,
                "raw_end": 2,
                "class": "name",
                "action": "redact",
                "provenance": {
                    "stage": "safety_net",
                    "decision": "redact",
                    "source_ids": ["safety.name"],
                },
            }
        ]
        document = self.trace_document()
        validated = benchmark.validate_response(document, response)
        predictions = benchmark.final_trace_predictions(document, validated)
        metrics = benchmark.MetricAccumulator()
        metrics.add(document, predictions)
        contract = benchmark.ContractAccumulator()
        contract.add(validated)
        self.assertEqual(metrics.result()["utf8_bytes"]["leaked"], 0)
        self.assertEqual(contract.result()["redact_actions"], 1)
        self.assertEqual(contract.result()["restore_exact_rate"], 0.0)

        response["restore"]["exact"] = True
        with self.assertRaisesRegex(
            benchmark.ResponseValidationError, "cannot be exactly"
        ):
            benchmark.validate_response(document, response)

    def test_predictions_ignore_pre_safety_manifests_and_leak_suspects(self) -> None:
        response = self.success_response()
        response["leak_suspects"] = [
            {
                "clean_start": 0,
                "clean_end": 2,
                "action_start": 0,
                "action_end": 2,
                "class": "name",
                "safety_net_id": "safety.name",
                "kind": "uncovered",
            }
        ]
        response["initial_safety_net_stats"].update(
            {"suspect_count": 1, "uncovered_count": 1}
        )
        response["strict_would_reject"] = True
        document = self.trace_document()
        validated = benchmark.validate_response(document, response)
        self.assertEqual(benchmark.final_trace_predictions(document, validated), [])

    def test_safety_net_protection_uses_producer_authored_raw_interval(self) -> None:
        document = benchmark.Document(
            uid="synthetic-response",
            text="aaaaabbbbbccccc",
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(),
        )
        response = self.success_response()
        response["manifest_spans"] = [
            {
                "raw_start": 5,
                "raw_end": 10,
                "clean_start": 5,
                "clean_end": 8,
                "class": "name",
            },
            {
                "raw_start": 11,
                "raw_end": 14,
                "clean_start": 9,
                "clean_end": 12,
                "class": "location",
            },
        ]
        response["manifest_integrity"]["spans"] = 2
        response["final_protection_trace"] = [
            {
                "raw_start": 5,
                "raw_end": 10,
                "class": "name",
                "action": "tokenize",
                "provenance": {
                    "stage": "primary_pipeline",
                    "decision": "policy",
                    "source_ids": ["rule.name"],
                },
            },
            {
                "raw_start": 11,
                "raw_end": 14,
                "class": "location",
                "action": "tokenize",
                "provenance": {
                    "stage": "safety_net",
                    "decision": "resolve",
                    "source_ids": ["safety.location"],
                },
            },
        ]
        validated = benchmark.validate_response(document, response)
        predictions = benchmark.final_trace_predictions(document, validated)
        self.assertEqual(predictions[0], benchmark.Span(5, 10, "name"))
        self.assertEqual(predictions[1], benchmark.Span(11, 14, "location"))


class SamplingTests(unittest.TestCase):
    def documents(self) -> list[benchmark.Document]:
        strata = (
            ("en", "US"),
            ("en", "US"),
            ("en", "GB"),
            ("en", "GB"),
            ("de", "DE"),
            ("de", "DE"),
        )
        return [
            benchmark.Document(
                uid=f"synthetic-sample-{index}",
                text="synthetic text",
                language=language,
                region=region,
                source_dataset="unit-test",
                spans=(),
            )
            for index, (language, region) in enumerate(strata)
        ]

    def test_seed_reproducibility_is_independent_of_input_order(self) -> None:
        documents = self.documents()
        first, first_report = benchmark.stratified_sample(documents, 3, seed=41)
        second, second_report = benchmark.stratified_sample(
            list(reversed(documents)), 3, seed=41
        )
        self.assertEqual(
            [document.uid for document in first], [document.uid for document in second]
        )
        self.assertEqual(
            first_report["evaluated_document_ids_digest"],
            second_report["evaluated_document_ids_digest"],
        )

    def test_full_population_order_is_seeded_and_input_order_independent(self) -> None:
        documents = self.documents()
        first, first_report = benchmark.stratified_sample(documents, None, seed=41)
        second, second_report = benchmark.stratified_sample(
            list(reversed(documents)), None, seed=41
        )
        self.assertEqual(
            [document.uid for document in first],
            [document.uid for document in second],
        )
        self.assertEqual(
            first_report["evaluated_document_ids"],
            second_report["evaluated_document_ids"],
        )

    def test_sampling_represents_each_language_region_stratum_when_budget_allows(
        self,
    ) -> None:
        selected, _ = benchmark.stratified_sample(self.documents(), 3, seed=7)
        self.assertEqual(
            {(document.language, document.region) for document in selected},
            {("de", "DE"), ("en", "GB"), ("en", "US")},
        )

    def test_largest_remainder_does_not_preallocate_small_strata(self) -> None:
        documents = [
            benchmark.Document(
                uid=f"synthetic-common-{index}",
                text="synthetic text",
                language="en",
                region="US",
                source_dataset="unit-test",
                spans=(),
            )
            for index in range(100)
        ]
        documents.append(
            benchmark.Document(
                uid="synthetic-rare-1",
                text="synthetic text",
                language="de",
                region="DE",
                source_dataset="unit-test",
                spans=(),
            )
        )
        selected, _ = benchmark.stratified_sample(documents, 2, seed=7)
        self.assertEqual(
            [(document.language, document.region) for document in selected],
            [("en", "US"), ("en", "US")],
        )

    def test_sampling_reports_available_and_evaluated_populations(self) -> None:
        _, report = benchmark.stratified_sample(self.documents(), 3, seed=7)
        self.assertEqual(report["available_population"]["documents"], 6)
        self.assertEqual(report["evaluated_population"]["documents"], 3)
        self.assertEqual(len(report["evaluated_document_ids"]), 3)


class ScorecardComparisonTests(unittest.TestCase):
    def scorecard(
        self, leaked: int, evaluated_ids: list[str] | None = None
    ) -> dict[str, object]:
        recall = 1.0 if leaked == 0 else 0.5
        ids = evaluated_ids or ["synthetic-comparison-1"]
        documents = len(ids)
        fully_covered = documents if leaked == 0 else documents - 1
        digest = hashlib.sha256(
            json.dumps(ids, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        ).hexdigest()
        run = {
            "scored_population": benchmark.identified_document_population(ids),
            "failed_closed_population": {
                **benchmark.identified_document_population([]),
                "excluded_documents": [],
                "reason_counts": {},
                "stage_counts": {},
            },
            "metrics": {
                "documents": documents,
                "zero_leak_document_rate": recall,
                "documents_without_leaks": fully_covered,
                "documents_with_false_positives": 0,
                "utf8_bytes": {
                    "pii": documents * 2,
                    "leaked": leaked,
                    "false_positive": 0,
                    "recall": recall,
                    "precision": 1.0,
                },
                "entities": {
                    "gold": documents,
                    "fully_covered": fully_covered,
                    "full_coverage_recall": recall,
                },
            },
            "pipeline_contract": {
                "documents": documents,
                "restore_exact_documents": documents,
                "restore_exact_rate": 1.0,
                "restore_success_decisions": documents,
                "manifest_valid_documents": documents,
                "manifest_valid_document_rate": 1.0,
                "manifest_integrity_errors": {},
                "strict_would_reject_documents": 0,
                "post_policy_scanned_documents": documents,
                "post_policy_suspects": 0,
                "redact_actions": 0,
            },
            "pipeline_availability": {
                "attempted_documents": documents,
                "completed_documents": documents,
                "failed_closed_documents": 0,
                "errors": {},
                "error_stages": {},
                "completion_rate": 1.0,
            },
            "latency_ms": {"clean_ms": {"p95": 1.0}},
        }
        return {
            "schema_version": 3,
            "dataset": {
                "repository": "synthetic/comparison",
                "revision": "synthetic-revision",
                "file": "synthetic.jsonl",
                "integrity": {"sha256": "0" * 64},
                "evaluated_population": {"documents": len(ids)},
                "sampling": {
                    "strategy": benchmark.SAMPLING_STRATEGY,
                    "seed": 7,
                    "evaluated_document_ids": ids,
                    "evaluated_document_ids_digest": {
                        "algorithm": "sha256",
                        "value": digest,
                    },
                },
            },
            "runs": [
                {"config": config, **copy.deepcopy(run)}
                for config in benchmark.DEFAULT_CONFIGS
            ],
        }

    def test_regression_and_release_readiness_are_distinct(self) -> None:
        history = self.scorecard(leaked=1)
        comparison = benchmark.compare_scorecards(history, copy.deepcopy(history))
        self.assertTrue(comparison["regression"]["passed"])
        self.assertFalse(comparison["release_readiness"]["passed"])

    def test_empty_candidate_fails_regression_and_release_readiness(self) -> None:
        baseline = self.scorecard(leaked=0)
        candidate = copy.deepcopy(baseline)
        candidate["runs"] = []
        comparison = benchmark.compare_scorecards(candidate, baseline)
        self.assertFalse(comparison["regression"]["passed"])
        self.assertFalse(comparison["release_readiness"]["passed"])

    def test_missing_candidate_runs_fail_regression_and_release_readiness(self) -> None:
        baseline = self.scorecard(leaked=0)
        candidate = copy.deepcopy(baseline)
        candidate.pop("runs")
        comparison = benchmark.compare_scorecards(candidate, baseline)
        self.assertFalse(comparison["regression"]["passed"])
        self.assertFalse(comparison["release_readiness"]["passed"])

    def test_mismatched_population_fails_regression_not_candidate_readiness(
        self,
    ) -> None:
        baseline = self.scorecard(leaked=0)
        candidate = self.scorecard(leaked=0, evaluated_ids=["synthetic-other-1"])
        comparison = benchmark.compare_scorecards(candidate, baseline)
        self.assertFalse(comparison["regression"]["passed"])
        self.assertTrue(comparison["release_readiness"]["passed"])

    def test_release_readiness_uses_the_production_candidate_cell(self) -> None:
        candidate = self.scorecard(leaked=0)
        nonproduction = candidate["runs"][0]
        nonproduction["metrics"]["zero_leak_document_rate"] = 0.0
        nonproduction["metrics"]["utf8_bytes"]["leaked"] = 1
        nonproduction["metrics"]["utf8_bytes"]["recall"] = 0.0
        nonproduction["metrics"]["entities"]["full_coverage_recall"] = 0.0
        comparison = benchmark.compare_scorecards(candidate, copy.deepcopy(candidate))
        self.assertTrue(comparison["regression"]["passed"])
        self.assertTrue(comparison["release_readiness"]["passed"])


class DataikuSelectionTests(unittest.TestCase):
    def test_load_documents_counts_selected_english_and_german_rows(self) -> None:
        rows = [
            {
                "language": "English",
                "country": "United States",
                "text": "alice@example.invalid",
                "privacy_mask": [
                    {
                        "start": 0,
                        "end": len("alice@example.invalid"),
                        "value": "alice@example.invalid",
                        "label": "EMAIL",
                    }
                ],
            },
            {
                "language": "German",
                "country": "Germany",
                "text": "Dr. Schmidt",
                "privacy_mask": [
                    {
                        "start": 0,
                        "end": len("Dr. Schmidt"),
                        "value": "Dr. Schmidt",
                        "label": "GIVENNAME",
                    }
                ],
            },
            {
                "language": "French",
                "country": "France",
                "text": "synthetic row",
                "privacy_mask": [],
            },
        ]
        table = types.SimpleNamespace(to_pylist=lambda: rows)
        parquet = types.ModuleType("pyarrow.parquet")
        parquet.read_table = mock.Mock(return_value=table)
        pyarrow = types.ModuleType("pyarrow")
        pyarrow.__path__ = []
        pyarrow.parquet = parquet

        with mock.patch.dict(
            sys.modules,
            {"pyarrow": pyarrow, "pyarrow.parquet": parquet},
        ):
            with mock.patch.object(dataiku_benchmark, "verify_dataset"):
                with mock.patch.object(dataiku_benchmark, "DATASET_ROWS", len(rows)):
                    documents, report = dataiku_benchmark.load_documents(
                        Path("synthetic.parquet")
                    )

        self.assertEqual(len(documents), 2)
        self.assertEqual([document.language for document in documents], ["en", "de"])
        self.assertEqual(report["integrity"]["rows"], 3)
        self.assertEqual(report["selection"]["documents"], 2)
        self.assertEqual(report["selection"]["languages"], {"de": 1, "en": 1})

    def test_max_documents_reports_available_and_evaluated_populations(self) -> None:
        """The pre-registered baseline move reports the sampled population exactly."""
        documents = [
            benchmark.Document(
                uid=f"synthetic-{index}",
                text="alice@example.invalid",
                language=language,
                region=region,
                source_dataset="unit-test",
                spans=(benchmark.Span(0, len("alice@example.invalid"), "EMAIL"),),
            )
            for index, (language, region) in enumerate(
                (("en", "US"), ("de", "DE")), start=1
            )
        ]
        dataset_report = {
            "integrity": {"rows": 2},
            "selection": {"documents": 2, "languages": {"de": 1, "en": 1}},
        }

        with tempfile.TemporaryDirectory() as temporary:
            temporary_path = Path(temporary)
            model_dir = temporary_path / "model"
            model_dir.mkdir()
            output_path = temporary_path / "scorecard.json"
            args = types.SimpleNamespace(
                dataset=temporary_path / "synthetic.parquet",
                output=output_path,
                model_dir=model_dir,
                kiji_model_dir=temporary_path / "kiji-model",
                threshold=0.3,
                max_documents=1,
                opf_command=None,
                opf_checkpoint=None,
                opf_daemon_socket=None,
                config=["rule-floor-extended"],
                no_download=True,
                skip_build=True,
            )
            with mock.patch.object(dataiku_benchmark, "parse_args", return_value=args):
                with mock.patch.object(dataiku_benchmark, "verify_dataset"):
                    with mock.patch.object(
                        dataiku_benchmark,
                        "load_documents",
                        return_value=(documents, dataset_report),
                    ):
                        with mock.patch.object(
                            benchmark,
                            "run_config",
                            return_value={"config": "rule-floor-extended"},
                        ) as run_config:
                            with mock.patch.object(
                                benchmark,
                                "git_metadata",
                                return_value={"revision": "synthetic", "dirty": False},
                            ):
                                self.assertEqual(dataiku_benchmark.main(), 0)

            result = json.loads(output_path.read_text(encoding="utf-8"))

        evaluated_documents = run_config.call_args.args[3]
        self.assertEqual(len(evaluated_documents), 1)
        self.assertEqual(result["parameters"]["max_documents"], 1)
        self.assertEqual(result["dataset"]["selection"]["documents"], 1)
        self.assertEqual(result["dataset"]["available_population"]["documents"], 2)
        self.assertEqual(result["dataset"]["evaluated_population"]["documents"], 1)
        self.assertEqual(result["schema_version"], 3)


class ConfigTests(unittest.TestCase):
    def test_committed_source_id_vocabulary_loads_rulepack_and_model_ids(self) -> None:
        self.assertIn("postal.de", benchmark.COMMITTED_SOURCE_ID_VOCABULARY)
        self.assertIn(
            "davlan-mbert-ner-hrl-onnx",
            benchmark.COMMITTED_SOURCE_ID_VOCABULARY,
        )
        self.assertTrue(
            EXPECTED_BUILTIN_SOURCE_IDS.issubset(
                benchmark.COMMITTED_SOURCE_ID_VOCABULARY
            )
        )

    def test_builtin_source_id_declaration_matches_committed_source_literals(
        self,
    ) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        with (repo_root / "scripts/bench/no_opf_models.toml").open("rb") as handle:
            models = tomllib.load(handle)
        declared = models["builtin_source_ids"]["ids"]
        self.assertEqual(len(declared), len(EXPECTED_BUILTIN_SOURCE_IDS))
        self.assertEqual(frozenset(declared), EXPECTED_BUILTIN_SOURCE_IDS)
        self.assertEqual(
            frozenset(BUILTIN_SOURCE_LITERAL_PATHS),
            EXPECTED_BUILTIN_SOURCE_IDS,
        )
        authoritative_source_text = {
            relative_path: (repo_root / relative_path).read_text(encoding="utf-8")
            for relative_path in set(BUILTIN_SOURCE_LITERAL_PATHS.values())
        }

        assert_builtin_source_literals(declared, authoritative_source_text)
        with self.assertRaisesRegex(AssertionError, "fabricated-built-in-source"):
            assert_builtin_source_literals(
                [*declared, "fabricated-built-in-source"],
                authoritative_source_text,
            )

    def test_builtin_source_id_coherence_rejects_stripped_authoritative_literal(
        self,
    ) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        source_id = "kiji-distilbert"
        relative_path = BUILTIN_SOURCE_LITERAL_PATHS[source_id]
        source_text = (repo_root / relative_path).read_text(encoding="utf-8")
        literal = f'"{source_id}"'
        self.assertIn(literal, source_text)
        stripped_source_text = source_text.replace(literal, "")
        self.assertNotIn(literal, stripped_source_text)

        with self.assertRaisesRegex(AssertionError, source_id):
            assert_builtin_source_literals(
                [source_id],
                {
                    relative_path: stripped_source_text,
                    "unrelated/test_fixture.rs": literal,
                },
            )

    def test_committed_source_id_vocabulary_load_failure_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(
                benchmark.SourceIdVocabularyError,
                "committed rulepack directory is missing",
            ):
                benchmark.load_committed_source_id_vocabulary(Path(temporary))

    def test_builtin_source_id_declaration_failures_fail_closed(self) -> None:
        declarations = {
            "missing": "",
            "malformed": "builtin_source_ids = []\n",
            "empty": "[builtin_source_ids]\nids = []\n",
        }
        for name, declaration in declarations.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                repo_root = Path(temporary)
                rulepack_dir = repo_root / "crates/gaze-recognizers/embedded"
                rulepack_dir.mkdir(parents=True)
                (rulepack_dir / "synthetic.toml").write_text(
                    '[[recognizers]]\nid = "synthetic-rule"\n',
                    encoding="utf-8",
                )
                model_path = repo_root / "scripts/bench/no_opf_models.toml"
                model_path.parent.mkdir(parents=True)
                model_path.write_text(
                    f'{declaration}[synthetic_model]\nid = "synthetic-model"\n',
                    encoding="utf-8",
                )
                with self.assertRaisesRegex(
                    benchmark.SourceIdVocabularyError,
                    "builtin_source_ids|built-in source-ID declaration",
                ):
                    benchmark.load_committed_source_id_vocabulary(repo_root)

    def test_all_no_opf_config_names_are_present_and_spelled_exactly(self) -> None:
        expected = (
            "rule-floor-extended",
            "pass2-ner",
            "full-stack-kiji-resolve",
        )
        self.assertEqual(benchmark.DEFAULT_CONFIGS, expected)
        self.assertTrue(set(expected).issubset(dataiku_benchmark.CONFIG_CHOICES))

        argv = ["benchmark"]
        for config in expected:
            argv.extend(("--config", config))
        for module in (openpii_benchmark, dataiku_benchmark):
            with mock.patch.object(sys, "argv", argv):
                self.assertEqual(tuple(module.parse_args().config), expected)


class OpfDaemonTests(unittest.TestCase):
    def test_receive_all_enforces_the_requested_limit(self) -> None:
        sender, receiver = socket.socketpair()
        with sender, receiver:
            sender.sendall(b"abc")
            sender.shutdown(socket.SHUT_WR)
            with self.assertRaisesRegex(ValueError, "byte limit"):
                opf_daemon.receive_all(receiver, 2)

    def test_socket_parent_is_private(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            socket_path = Path(temporary) / "private" / "opf.sock"
            opf_daemon.validate_socket_parent(socket_path)
            self.assertEqual(socket_path.parent.stat().st_mode & 0o777, 0o700)

    def test_socket_path_length_fails_before_creation(self) -> None:
        with self.assertRaisesRegex(ValueError, "shorter than 100 bytes"):
            opf_daemon.validate_socket_parent(Path("x" * 100))


if __name__ == "__main__":
    unittest.main()
