import copy
import unittest

import numpy as np

from evaluate import fit_best, metrics, rotate, thresholds
from fixtures import make_row, rows, validate


class ResearchContract(unittest.TestCase):
    def test_byte_metrics_charge_utf8_and_never_credit_refusals(self):
        positive = make_row("test", "fr", "person", 0, "Véra Qenlo", "Bonjour {v}.")
        negative = make_row("test", "fr", "ordinary_word", 0, "Été", "Mot : {v}.")
        score = metrics([positive, negative], np.asarray([0.2, 0.8]), 0.5)
        self.assertEqual(score["missed_bytes"], len("Véra Qenlo".encode()))
        self.assertEqual(score["false_positive_bytes"], len("Été".encode()))
        self.assertEqual((score["tp"], score["fp"], score["fn"]), (0, 1, 1))

    def test_zero_fp_threshold_excludes_ties(self):
        data = [make_row("dev", "en", family, 0, "X", "{v}") for family in ["person", "ordinary_word"]]
        scores = np.asarray([0.7, 0.7])
        selected = thresholds(data, scores)["dev_zero_fp"]
        self.assertGreater(selected, 0.7)
        self.assertEqual(metrics(data, scores, selected)["fp"], 0)
        self.assertEqual(metrics(data, scores, selected)["fn"], 1)

    def test_rotation_preserves_inner_products(self):
        data = np.random.default_rng(7).normal(size=(8, 13)).astype(np.float32)
        rotated = rotate(data, 17)
        np.testing.assert_allclose(rotated @ rotated.T, data @ data.T, rtol=1e-5, atol=1e-5)

    def test_translated_parent_cannot_cross_splits(self):
        data = rows()
        changed = copy.deepcopy(data[0])
        changed["id"] = "corrupt-translated-parent"
        changed["split"] = "test"
        with self.assertRaisesRegex(AssertionError, "split leakage"):
            validate(data + [changed])

    def test_test_features_and_labels_cannot_select_classifier(self):
        data = np.asarray([[-2.], [-1.], [1.], [2.], [-1.5], [1.5], [-100.], [100.]])
        truth = np.asarray([0, 0, 1, 1, 0, 1, 0, 1])
        train = np.arange(8) < 4
        dev = (np.arange(8) >= 4) & (np.arange(8) < 6)
        before, selection, classifier = fit_best(data, truth, train, dev)
        changed = data.copy()
        changed[-2:] *= -1000
        other_truth = truth.copy()
        other_truth[-2:] = 1 - truth[-2:]
        after, other_selection, other_classifier = fit_best(changed, other_truth, train, dev)
        self.assertEqual(selection, other_selection)
        np.testing.assert_array_equal(classifier.coef_, other_classifier.coef_)
        np.testing.assert_array_equal(before[:6], after[:6])


if __name__ == "__main__":
    unittest.main()
