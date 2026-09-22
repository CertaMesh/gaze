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

    def test_sample_entry_carries_the_class_that_earned_the_credit(self) -> None:
        # The earlier `location` span holds the ZIP repeat but touches gold,
        # so it is blocked; the credit belongs to `custom:postal_code`.
        text = "PLZ: 10115 und Berlin 10115 end"
        trace_row = row(
            "doc-zip",
            text,
            [("10115", "ZIP"), ("Berlin", "CITY")],
            [("10115", "custom:postal_code", 1)],
        )
        trace_row["trace"].insert(
            0,
            {"s": text.index("10115"), "e": text.index(" end"), "class": "location"},
        )
        population = evidence.eligible([evidence.TraceDocument(trace_row)])
        self.assertEqual(
            [(e["byte_start"], e["gold_label"], e["predicted_class"]) for e in population],
            [(22, "ZIP", "custom:postal_code")],
        )
        self.assertEqual(population[0]["attributed_gold_span"], [5, 10])


class AmbiguityTests(unittest.TestCase):
    def signals(self, rows) -> dict:
        trace = [evidence.TraceDocument(item) for item in rows]
        with tempfile.TemporaryDirectory() as directory:
            wordlist = Path(directory) / "words"
            wordlist.write_text("may\nhouse\n", encoding="utf-8")
            found, _ = evidence.ambiguity_signals(trace, evidence.eligible(trace), wordlist)
        return {key[1:]: reasons for key, reasons in found.items()}

    def test_german_noun_homonym_is_flagged(self) -> None:
        # German nouns are always capitalised, so no lowercase use exists.
        text = "Wohnort: Essen. Das Essen ist fertig."
        found = self.signals(
            [row("doc-essen", text, [("Essen", "CITY")], [("Essen", "location", 1)])]
        )
        self.assertEqual(found, {(20, 25): ["german_noun_surname_seed"]})

    def test_unlabelled_use_in_another_document_is_flagged(self) -> None:
        text = "Wohnort: Lemgo. Lemgo ist klein."
        rows = [
            row("doc-a", text, [("Lemgo", "CITY")], [("Lemgo", "location", 1)]),
            row("doc-b", "Die Lemgo-Werke bauen Pumpen.", [], []),
        ]
        self.assertEqual(self.signals(rows), {(16, 21): ["unlabelled_use_elsewhere"]})
        # Where the other document labels the value, it is no evidence.
        rows[1] = row("doc-b", "Stadt: Lemgo.", [("Lemgo", "CITY")], [])
        self.assertEqual(self.signals(rows), {(16, 21): []})

    def test_english_dictionary_signal_is_kept(self) -> None:
        text = "May Example sent it. Due in May."
        found = self.signals(
            [row("doc-may", text, [("May", "FIRSTNAME")], [("May", "name", 1)])]
        )
        self.assertEqual(
            found, {(28, 31): ["english_dictionary_word", "german_noun_surname_seed"]}
        )


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

    def test_every_non_empty_sub_stratum_is_drawn(self) -> None:
        # The committed v3 shape that left COUNTRY and STATE plain unsampled.
        groups = {
            "FIRSTNAME": (106, 468),
            "SURNAME": (146, 403),
            "CITY": (262, 119),
            "COUNTRY": (17, 2),
            "STATE": (16, 24),
            "STREET": (0, 78),
            "ZERO": (0, 0),
        }
        allocation, split = evidence.allocate(187, groups)
        self.assertEqual(sum(allocation.values()), 187)
        for label, sizes in groups.items():
            for size, drawn in zip(sizes, split[label], strict=True):
                with self.subTest(label):
                    self.assertEqual(bool(size), bool(drawn))
            self.assertEqual(sum(split[label]), allocation[label])
        for label in evidence.SAMPLE_FLOORS:
            self.assertGreaterEqual(allocation[label], evidence.SAMPLE_FLOORS[label])

    def test_a_zero_draw_stratum_fails_the_weight_check(self) -> None:
        strata = [
            {"gold_label": "COUNTRY", "sub_stratum": "ambiguous", "population": 1, "sampled": 1},
            {"gold_label": "COUNTRY", "sub_stratum": "plain", "population": 18, "sampled": 0},
        ]
        entries = [{"design_weight": 1.0}]
        with self.assertRaisesRegex(SystemExit, "COUNTRY/plain"):
            evidence.check_design_weights(entries, strata, 19)
        strata[1]["sampled"] = 2
        entries += [{"design_weight": 9.0}, {"design_weight": 9.0}]
        evidence.check_design_weights(entries, strata, 19)

    def test_questions_are_label_appropriate(self) -> None:
        self.assertIn("same person", evidence.question("SURNAME"))
        self.assertIn("same place", evidence.question("CITY"))
        self.assertIn("same organisation", evidence.question("COMPANYNAME"))
        self.assertIn("same personal datum", evidence.question("ZIP"))


if __name__ == "__main__":
    unittest.main()
