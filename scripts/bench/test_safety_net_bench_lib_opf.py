#!/usr/bin/env python3
"""Model-free tests: the OPF bench scorer never silently scores part of a multi-line fixture."""

from __future__ import annotations

import importlib.util
import json
import sys
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location(
    "safety_net_bench_lib", Path(__file__).with_name("safety_net_bench_lib.py")
)
assert SPEC is not None and SPEC.loader is not None
lib = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = lib
SPEC.loader.exec_module(lib)


def output(text: str, *spans: tuple[str, int, int]) -> str:
    return json.dumps(
        {
            "text": text,
            "detected_spans": [
                {"label": label, "start": start, "end": end} for label, start, end in spans
            ],
        }
    )


def span_texts(text: str, spans: list) -> list[str]:
    data = text.encode("utf-8")
    return [data[span.start : span.end].decode("utf-8") for span in spans]


class OpfOutputToSpansTest(unittest.TestCase):
    def test_whole_multi_line_text_maps_every_span(self) -> None:
        text = "\n\nContact John Smith.\nGrüße Zoë"
        spans = lib.opf_output_to_spans(
            "f", text, output(text, ("private_person", 10, 20), ("private_person", 28, 31))
        )
        self.assertEqual(span_texts(text, spans), ["John Smith", "Zoë"])

    def test_crlf_and_lone_cr_offsets_count_translated_newlines(self) -> None:
        text = "Hi,\r\nJohn Smith\rZoë\r\n"
        view = "Hi,\nJohn Smith\nZoë\n"
        spans = lib.opf_output_to_spans(
            "f", text, output(view, ("private_person", 4, 14), ("private_person", 15, 18))
        )
        self.assertEqual(span_texts(text, spans), ["John Smith", "Zoë"])

    def test_line_split_output_fails_loudly(self) -> None:
        text = "Contact John Smith.\nEmail jane.doe@example.invalid"
        stdout = (
            output("Contact John Smith.", ("private_person", 8, 18))
            + "\n"
            + output("Email jane.doe@example.invalid", ("private_email", 6, 30))
        )
        with self.assertRaisesRegex(RuntimeError, "exactly one JSON document"):
            lib.opf_output_to_spans("f", text, stdout)

    def test_skipped_leading_blank_lines_fail_loudly(self) -> None:
        text = "\n\nJohn Smith"
        with self.assertRaisesRegex(RuntimeError, "different text"):
            lib.opf_output_to_spans("f", text, output("John Smith", ("private_person", 0, 10)))

    def test_output_without_echoed_text_fails_loudly(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "different text"):
            lib.opf_output_to_spans(
                "f", "John Smith", json.dumps([{"label": "private_person", "start": 0, "end": 10}])
            )

    def test_offset_past_the_text_fails_loudly(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "out-of-bounds"):
            lib.opf_output_to_spans(
                "f", "Zoë", output("Zoë", ("private_person", 0, 4))
            )


if __name__ == "__main__":
    unittest.main()
