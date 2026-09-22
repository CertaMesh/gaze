#!/usr/bin/env python3
"""Contract v3 gold-gap evidence script: replay identity and audit statistics."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gold_gap_evidence as evidence


def row(uid, text, gold, trace, excluded=(), negative=None, error=None) -> dict:
    raw = text.encode("utf-8")

    def locate(value, label, occurrence=0):
        start = -1
        for _ in range(occurrence + 1):
            start = raw.index(value.encode("utf-8"), start + 1)
        return [start, start + len(value.encode("utf-8")), label]

    trace_items = []
    for value, predicted_class, occurrence in trace:
        start, end, _ = locate(value, "", occurrence)
        trace_items.append({"s": start, "e": end, "class": predicted_class})
    return {
        "uid": uid,
        "language": "en",
        "negative_category": negative,
        "text": text,
        "gold": [locate(*item) for item in gold],
        "excluded": [locate(*item) for item in excluded],
        "neutral": [],
        "error": error,
        "trace": trace_items,
    }


ROWS = [
    row(
        "doc-1",
        "My name is Emma Clarke. Emma likes Berlin. City: Berlin. pw hunter22",
        [("Emma", "FIRSTNAME"), ("Clarke", "SURNAME"), ("Berlin", "CITY", 1)],
        [
            ("Emma Clarke", "name", 0),
            ("Emma", "name", 1),
            ("Berlin", "location", 0),
            ("Berlin", "location", 1),
            ("hunter22", "custom:password", 0),
        ],
        excluded=[("hunter22", "PASSWORD")],
    ),
    row("neg-1", "Emma is a word here.", [], [("Emma", "name", 0)], negative="plain"),
    row("doc-err", "Emma Clarke.", [("Emma", "FIRSTNAME")], [], error="pipeline_failed"),
]


class ReplayTests(unittest.TestCase):
    def test_replay_keeps_v2_numbers_and_reports_the_column(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trace.jsonl"
            path.write_text("\n".join(json.dumps(item) for item in ROWS) + "\n")
            result = evidence.replay(path)
        holdout = result["cells"]["holdout"]
        self.assertEqual(result["failed_closed_documents"], 1)
        self.assertEqual(holdout["leaked_bytes_v2"], holdout["leaked_bytes_v3"])
        # "Emma" repeat and the unlabelled first "Berlin" are credited.
        self.assertEqual(holdout["gold_gap_protected_bytes"], 4 + 6)
        self.assertEqual(
            holdout["gold_gap_protected_bytes_by_label"], {"CITY": 6, "FIRSTNAME": 4}
        )
        self.assertEqual(
            holdout["false_positive_bytes_after_gold_gap"],
            holdout["false_positive_bytes_v2"] - 10,
        )
        self.assertEqual(result["cells"]["negatives"]["gold_gap_protected_bytes"], 0)

    def test_eligibility_set_is_what_the_scorer_credits(self) -> None:
        trace = [evidence.TraceDocument(item) for item in ROWS]
        population = evidence.eligible(trace)
        self.assertEqual(
            [(e["byte_start"], e["byte_end"], e["gold_label"], e["predicted_class"]) for e in population],
            [(24, 28, "FIRSTNAME", "name"), (35, 41, "CITY", "location")],
        )
        self.assertEqual(population[1]["attributed_gold_span"], [49, 55])


class StatisticsTests(unittest.TestCase):
    def test_acceptance_count_meets_the_declared_bound(self) -> None:
        bound = evidence.clopper_pearson_upper(evidence.MAX_FAILURES, evidence.SAMPLE_SIZE)
        self.assertLessEqual(bound, evidence.MAX_UPPER_BOUND)
        self.assertGreater(
            evidence.clopper_pearson_upper(evidence.MAX_FAILURES + 1, evidence.SAMPLE_SIZE),
            evidence.MAX_UPPER_BOUND,
        )
        # Known values: 0 of 200 gives 1 - 0.05 ** (1 / 200).
        self.assertAlmostEqual(
            evidence.clopper_pearson_upper(0, 200), 1 - 0.05 ** (1 / 200), places=9
        )

    def test_largest_remainder_respects_caps_and_total(self) -> None:
        allocation = evidence.largest_remainder(10, {"a": 5, "b": 3, "c": 2}, {"a": 4, "b": 9, "c": 9})
        self.assertEqual(sum(allocation.values()), 10)
        self.assertLessEqual(allocation["a"], 4)
        self.assertEqual(allocation, {"a": 4, "b": 4, "c": 2})

    def test_questions_are_label_appropriate(self) -> None:
        self.assertIn("same person", evidence.question("SURNAME"))
        self.assertIn("same place", evidence.question("CITY"))
        self.assertIn("same organisation", evidence.question("COMPANYNAME"))
        self.assertIn("same personal datum", evidence.question("ZIP"))


if __name__ == "__main__":
    unittest.main()
