"""Strict spans match within a fixture before locale metrics are aggregated."""

import unittest

from safety_net_bench_lib import CleanFixture, Fixture, Span, score, score_direct, score_observer


class SafetyNetScoringTest(unittest.TestCase):
    def scores(self, gold, predictions, locales=None):
        fixtures = [
            Fixture(str(i), locale, [], "synthetic", spans)
            for i, (locale, spans) in enumerate(zip(locales or ["Global"] * len(gold), gold))
        ]
        predicted = {str(i): spans for i, spans in enumerate(predictions)}
        # Observer gold lives in clean-text coordinates, independently of direct gold.
        clean = {str(i): CleanFixture("synthetic", [], spans) for i, spans in enumerate(gold)}
        observer_fixtures = [Fixture(f.fixture_id, f.locale, [], f.text, []) for f in fixtures]
        return (
            score(fixtures, predicted, "gold_spans"),
            score_direct(fixtures, predicted),
            score_observer(observer_fixtures, clean, predicted),
        )

    def test_same_offsets_in_wrong_fixture_are_fp_and_fn(self):
        name = Span(0, 5, "Name")
        for result in self.scores([[name], []], [[], [name]]):
            with self.subTest(result=result):
                metrics = result["Global"]["per_class"]["Name"]
                self.assertEqual(metrics, {
                    "support": 1, "predicted": 1, "true_positive": 0,
                    "false_positive": 1, "false_negative": 1,
                    "precision": 0.0, "recall": 0.0, "f1": None,
                })

    def test_coincident_correct_matches_in_separate_fixtures_count_twice(self):
        name = Span(0, 5, "Name")
        for result in self.scores([[name], [name]], [[name], [name]]):
            metrics = result["Global"]["per_class"]["Name"]
            self.assertEqual((metrics["support"], metrics["predicted"], metrics["true_positive"]), (2, 2, 2))
            self.assertEqual((metrics["false_positive"], metrics["false_negative"]), (0, 0))

    def test_duplicate_spans_use_multiset_matching_within_fixture(self):
        name = Span(0, 5, "Name")
        for result in self.scores([[name, name], [name]], [[name], [name, name]]):
            metrics = result["Global"]["per_class"]["Name"]
            self.assertEqual((metrics["support"], metrics["predicted"], metrics["true_positive"]), (3, 3, 2))
            self.assertEqual((metrics["false_positive"], metrics["false_negative"]), (1, 1))
            self.assertEqual((metrics["precision"], metrics["recall"], metrics["f1"]), (0.666667, 0.666667, 0.666667))

    def test_empty_denominators_and_locale_order(self):
        name = Span(0, 5, "Name")
        cases = [([], [], None, None), ([name], [], None, 0.0), ([], [name], 0.0, None)]
        for gold, predicted, precision, recall in cases:
            for result in self.scores([gold], [predicted], ["DeDe"]):
                self.assertEqual(list(result), ["Global", "EnUs", "DeDe"])
                self.assertEqual(result["Global"]["per_class"], {})
                self.assertEqual(result["EnUs"]["per_class"], {})
                self.assertEqual(result["DeDe"]["precision"], precision)
                self.assertEqual(result["DeDe"]["recall"], recall)
                self.assertIsNone(result["DeDe"]["f1"])
        for result in self.scores([], []):
            self.assertTrue(all(m["recall"] is None and m["per_class"] == {} for m in result.values()))

    def test_macro_recall_uses_classes_and_preserves_rounding(self):
        name = Span(0, 5, "Name")
        email = Span(6, 10, "Email")
        wrong_class = Span(0, 5, "Location")
        for result in self.scores([[name, name, name, email]], [[name, email, wrong_class]]):
            metrics = result["Global"]
            self.assertEqual(metrics["precision"], 0.666667)
            self.assertEqual(metrics["recall"], 0.666667)
            self.assertEqual(metrics["f1"], 0.666667)
            self.assertEqual(metrics["per_class"]["Name"]["recall"], 0.333333)
            self.assertIsNone(metrics["per_class"]["Location"]["recall"])
            if "observer_residual_recall" in metrics:
                self.assertEqual(metrics["observer_residual_recall"], 0.666667)
                self.assertEqual(metrics["novel_tp_over_rule_floor"], 0.666667)
                for key in ("agreement_with_rule_floor", "expansion_fraction", "contradiction_fraction"):
                    self.assertIsNone(metrics[key])


if __name__ == "__main__":
    unittest.main()
