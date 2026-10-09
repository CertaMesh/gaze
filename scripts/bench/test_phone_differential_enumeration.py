"""The supplemental phone enumeration remains deterministic and complete."""
import collections
import unittest
import phone_differential_enumeration as enumeration


class PhoneEnumerationTests(unittest.TestCase):
    def test_5600_generated_shapes_have_correct_byte_gold_and_digit_subsets(self):
        cases = enumeration.cases()
        self.assertEqual(cases, enumeration.cases())
        self.assertEqual(len(cases), 5600)
        self.assertEqual(collections.Counter(row[0] for row in cases), {
            'de_national': 900, 'de_international': 900, 'gb_cued': 900,
            'us_national': 900, 'de_adjacent_tail': 300, 'benign_dotted': 600,
            'benign_grouped': 400, 'fr_dotted': 100, 'fr_dotted_cued': 100,
            'gb_00': 100, 'gb_national_cued': 100, 'gb_reserved': 100,
            'gb_trunk': 100, 'us_001': 100})
        for family, text, gold, digits, _ in cases:
            self.assertLessEqual(digits, gold)
            self.assertTrue(all(0 <= byte < len(text.encode()) for byte in gold))
            self.assertTrue(all(48 <= text.encode()[byte] <= 57 for byte in digits))
            self.assertEqual(bool(gold), not family.startswith('benign_'))
