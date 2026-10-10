"""Pin the original supplemental inputs and JSONL Unicode framing."""
import collections
import hashlib
import json
import unittest
import phone_differential_enumeration as enumeration


class PhoneEnumerationTests(unittest.TestCase):
    def test_original_5600_inputs_and_gold_are_unchanged(self):
        cases = enumeration.cases()
        self.assertEqual(cases, enumeration.cases())
        self.assertEqual(len(cases), 5600)
        self.assertEqual(collections.Counter(row[0] for row in cases), {
            'de_national': 900, 'de_international': 900, 'gb_cued': 900,
            'us_national': 900, 'de_adjacent_tail': 300, 'benign_dotted': 600,
            'benign_grouped': 400, 'fr_dotted': 100, 'fr_dotted_cued': 100,
            'gb_00': 100, 'gb_national_cued': 100, 'gb_reserved': 100,
            'gb_trunk': 100, 'us_001': 100})
        payload = json.dumps([[family, text, sorted(gold)] for family, text, gold in cases],
                             ensure_ascii=False, separators=(',', ':')).encode()
        self.assertEqual(hashlib.sha256(payload).hexdigest(),
                         '06362e1e1df0b3babbedd85f2f7a70fc37757b1c20a175425485b5200e97062d')
        for family, text, gold in cases:
            self.assertTrue(all(0 <= byte < len(text.encode()) for byte in gold))
            self.assertEqual(bool(gold), not family.startswith('benign_'))

    def test_unicode_line_separator_inside_json_string_is_not_a_record_boundary(self):
        response = {'clean_text': 'left\u2028right', 'session_id': 'fixture'}
        payload = json.dumps(response, ensure_ascii=False) + '\n'
        self.assertEqual(enumeration.parse_responses(payload), [response])
