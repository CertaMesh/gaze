#!/usr/bin/env python3

import io
import json
import socket
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import openpii_gaze_bench as benchmark
import dataiku_en_de_gaze_bench as dataiku_benchmark
import opf_daemon


class OffsetTests(unittest.TestCase):
    def test_character_offsets_convert_to_utf8_byte_offsets(self) -> None:
        self.assertEqual(benchmark.char_to_byte_offsets("aé中"), [0, 1, 3, 6])

    def test_adjacent_and_overlapping_intervals_are_merged(self) -> None:
        self.assertEqual(
            benchmark.merge_intervals([(5, 9), (0, 3), (3, 6), (12, 14)]),
            [(0, 9), (12, 14)],
        )

    def test_safety_net_action_maps_through_existing_token(self) -> None:
        document = benchmark.Document(
            uid="synthetic-map",
            text="aaaaabbbbbccccc",
            language="en",
            region="US",
            source_dataset="unit-test",
            spans=(),
        )
        manifest = [
            {
                "raw_start": 5,
                "raw_end": 10,
                "clean_start": 5,
                "clean_end": 8,
                "class": "name",
            }
        ]

        plain = benchmark.map_clean_actions_to_raw(
            document,
            13,
            manifest,
            [{"action_start": 9, "action_end": 12, "class": "location"}],
        )
        overlapping_token = benchmark.map_clean_actions_to_raw(
            document,
            13,
            manifest,
            [{"action_start": 6, "action_end": 7, "class": "name"}],
        )

        self.assertEqual(plain, [benchmark.Span(11, 14, "location")])
        self.assertEqual(overlapping_token, [benchmark.Span(5, 10, "name")])


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
            "pipeline_error_code": "POLICY_REJECTED",
            "pipeline_error_stage": "safety_net",
            "timing": {"total_ms": 1.25, "pass_2_ms": None},
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
                "errors": {"POLICY_REJECTED": 1},
                "error_stages": {"safety_net": 1},
            },
        )
        self.assertEqual(result["latency_ms"]["total_ms"]["median"], 1.25)


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

    def test_max_documents_truncates_after_frozen_selection_report(self) -> None:
        """Baseline debt to fix later: sampled runs report the full selection."""
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
        self.assertEqual(result["dataset"]["selection"]["documents"], 2)


class ConfigTests(unittest.TestCase):
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
        for module in (benchmark, dataiku_benchmark):
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
