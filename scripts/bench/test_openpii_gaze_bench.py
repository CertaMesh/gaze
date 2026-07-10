#!/usr/bin/env python3

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import openpii_gaze_bench as benchmark


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


if __name__ == "__main__":
    unittest.main()
