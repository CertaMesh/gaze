#!/usr/bin/env python3

import sys
import socket
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import openpii_gaze_bench as benchmark
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
