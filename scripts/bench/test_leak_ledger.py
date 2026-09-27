"""Leak ledger: each cause lands where it should, and check refuses drift."""

import copy
import gzip
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import leak_ledger as ledger


COMPATIBLE = frozenset({
    ("custom:iban", "IBAN"), ("custom:tax_id", "TAXNUM"), ("name", "FIRSTNAME"),
})


def item(start, end, klass, recognizer, **extra):
    return {"raw_start": start, "raw_end": end, "class": klass,
            "recognizer_id": recognizer, "score": 0.7, "decided_by": "None", **extra}


def trace(start, end, klass, *sources, stage="primary_pipeline"):
    return {"raw_start": start, "raw_end": end, "class": klass, "action": "tokenize",
            "provenance": {"stage": stage, "decision": "policy", "source_ids": list(sources)}}


def pool(detected=(), resolved=(), vetoed=(), locale_gated=()):
    return {"detected": list(detected), "resolved": list(resolved),
            "vetoed": list(vetoed), "locale_gated": list(locale_gated)}


def row(start, end, label, covered=0):
    return {"layer": "C", "document_id": "doc", "label": label, "start": start,
            "end": end, "leaked": end - start - covered, "covered": covered}


# One synthetic document; each test places its gold span on a different value.
TEXT = b"IBAN DE00 1111 2222 3333 44 and Steuer 12 345 678 901, Jane again Jane. IBAN 7623"
IBAN = (5, 27)
TAX = (39, 53)
JANE_FIRST, JANE_SECOND = (55, 59), (66, 70)


def classify(gold, trace_items=(), pool_items=None, losers=()):
    return ledger.classify(gold, TEXT, list(trace_items), pool_items or pool(), COMPATIBLE, losers)


class CauseTests(unittest.TestCase):
    def test_partial_span_is_d_even_when_a_candidate_was_vetoed(self):
        result = classify(
            row(*IBAN, "IBAN", covered=10), [trace(5, 15, "custom:iban", "iban.structural")],
            pool(vetoed=[item(*IBAN, "custom:credit_card", "card.structural", reason="LuhnFailed")]),
        )
        self.assertEqual(result["cause"], "d")
        self.assertEqual(result["detail"]["by"][0]["sources"], "iban.structural")

    def test_validator_veto_is_b_with_rule_and_reason(self):
        result = classify(row(*TAX, "TAXNUM"), pool_items=pool(
            detected=[item(*TAX, "custom:tax_id", "steuer_id.de")],
            vetoed=[item(*TAX, "custom:tax_id", "steuer_id.de", reason="DeSteuerIdMod1110Failed")],
        ))
        self.assertEqual((result["cause"], result["detail"]["kind"]), ("b", "validator"))
        self.assertEqual(result["detail"]["recognizer"], "steuer_id.de")
        self.assertEqual(result["detail"]["reason"], "DeSteuerIdMod1110Failed")

    def test_lost_candidate_is_c_with_winner_and_audit_tier(self):
        result = classify(row(*TAX, "TAXNUM"), pool_items=pool(
            detected=[item(*TAX, "custom:tax_id", "tax.cue"), item(33, 53, "custom:phone", "phone.de")],
            resolved=[item(33, 53, "custom:phone", "phone.de")],
        ), losers=[("tax.cue", "ClassPriority"), ("tax.cue", "ValidatorVeto")])
        self.assertEqual(result["cause"], "c")
        self.assertEqual(result["detail"]["loser"], "tax.cue")
        self.assertEqual(result["detail"]["winner_class"], "custom:phone")
        self.assertEqual(result["detail"]["tier"], "ClassPriority")

    def test_lost_candidate_without_an_audit_row_says_unlogged(self):
        result = classify(row(*TAX, "TAXNUM"), pool_items=pool(
            detected=[item(*TAX, "custom:tax_id", "tax.cue")],
            resolved=[item(33, 53, "custom:phone", "phone.de")],
        ))
        self.assertEqual(result["detail"]["tier"], "unlogged")

    def test_candidate_dropped_with_no_overlapping_winner_is_b_pre_resolution(self):
        result = classify(row(*TAX, "TAXNUM"), pool_items=pool(
            detected=[item(*TAX, "custom:tax_id", "tax.cue")],
        ))
        self.assertEqual((result["cause"], result["detail"]["kind"]), ("b", "pre_resolution_drop"))

    def test_only_an_out_of_chain_recognizer_matching_is_b_locale_gate(self):
        result = classify(row(*TAX, "TAXNUM"), pool_items=pool(
            locale_gated=[item(*TAX, "custom:tax_id", "tax.fr")],
        ))
        self.assertEqual((result["cause"], result["detail"]["kind"]), ("b", "locale_gate"))
        self.assertEqual(result["detail"]["recognizer"], "tax.fr")

    def test_unprotected_repeat_of_a_protected_value_is_e(self):
        result = classify(
            row(*JANE_SECOND, "FIRSTNAME"), [trace(*JANE_FIRST, "name", "ner")],
        )
        self.assertEqual(result["cause"], "e")
        self.assertEqual(result["detail"]["copy_sources"], ["ner"])

    def test_repeat_inside_a_longer_word_is_not_a_protected_copy(self):
        # `an` of `and` recurs inside the protected `Jane`; that is no copy of it.
        result = classify(row(28, 30, "FIRSTNAME"), [trace(*JANE_FIRST, "name", "ner")])
        self.assertEqual(result["cause"], "a")

    def test_junk_shaped_gold_without_candidate_is_f(self):
        result = classify(row(77, 81, "IBAN"))
        self.assertEqual(result["cause"], "f")
        self.assertEqual(result["junk_shape"], "identifier_four_or_fewer_alphanumerics")

    def test_no_candidate_is_a_with_the_closest_compatible_recognizer(self):
        result = classify(row(*TAX, "TAXNUM"), [trace(5, 27, "custom:iban", "iban.structural")],
                          pool(detected=[item(56, 59, "name", "ner")]))
        self.assertEqual(result["cause"], "a")
        self.assertEqual(result["detail"]["closest"]["recognizer"], "ner")
        self.assertEqual(result["detail"]["closest"]["gap"], 3)
        self.assertFalse(result["detail"]["closest"]["compatible"])

    def test_no_candidate_far_from_everything_names_nothing(self):
        result = classify(row(*TAX, "TAXNUM"))
        self.assertEqual((result["cause"], result["detail"]["closest"]), ("a", None))

    def test_primary_winner_missing_from_the_trace_is_g(self):
        winner = item(*TAX, "custom:tax_id", "tax.cue")
        result = classify(row(*TAX, "TAXNUM"), pool_items=pool(detected=[winner], resolved=[winner]))
        self.assertEqual(result["cause"], "g")


class ShapeAndAuditTests(unittest.TestCase):
    def test_junk_shapes(self):
        self.assertEqual(ledger.junk_shape("IBAN", b"IBAN"), "identifier_without_digit")
        self.assertEqual(ledger.junk_shape("TAXNUM", b"7623"), "identifier_four_or_fewer_alphanumerics")
        self.assertEqual(ledger.junk_shape("PHONENUMBER", b"--"), "no_alphanumeric")
        self.assertIsNone(ledger.junk_shape("BUILDINGNUM", b"12"))
        self.assertIsNone(ledger.junk_shape("TAXNUM", b"12 345 678 901"))

    def test_audit_loser_rows_parse(self):
        rows = [
            'RedactionEntry { source: "a", recognizer_id: Some("postal.at_ch"), class: Custom("postal_code"), '
            'conflict_loser: true, decided_by: ContainmentPrecedence, created_at: 0 }',
            'RedactionEntry { source: "b", recognizer_id: Some("email.global"), class: Email, '
            'conflict_loser: false, decided_by: None, created_at: 0 }',
        ]
        self.assertEqual(ledger.audit_losers(rows), [("postal.at_ch", "ContainmentPrecedence")])


class CheckTests(unittest.TestCase):
    """`check` against the committed ledger, and against tampered copies."""

    @classmethod
    def setUpClass(cls):
        cls.index, cls.record_path, cls.rows = ledger.load()
        _, cls.expected = ledger.leaked_spans(cls.record_path)

    def test_committed_ledger_checks(self):
        totals, body = ledger.derive()
        self.assertEqual(set(totals), {"1", "2", "3"})
        original = ledger.DOC.read_text(encoding="utf-8")
        self.assertEqual(ledger.apply(original, body), original)

    def test_rows_must_equal_the_record_leaks(self):
        ledger.validate_rows(self.rows, self.expected)
        dropped = self.rows[1:]
        with self.assertRaises(ledger.LedgerError):
            ledger.validate_rows(dropped, self.expected)
        shifted = copy.deepcopy(self.rows)
        shifted[0]["leaked"] += 1
        with self.assertRaises(ledger.LedgerError):
            ledger.validate_rows(shifted, self.expected)

    def test_partial_cause_must_agree_with_coverage(self):
        rows = copy.deepcopy(self.rows)
        target = next(item for item in rows if item["covered"] == 0)
        target["cause"] = "d"
        with self.assertRaisesRegex(ledger.LedgerError, "partial-span"):
            ledger.validate_rows(rows, self.expected)

    def test_unknown_cause_is_refused(self):
        rows = copy.deepcopy(self.rows)
        rows[0]["cause"] = "z"
        with self.assertRaisesRegex(ledger.LedgerError, "unknown cause"):
            ledger.validate_rows(rows, self.expected)

    def test_label_totals_must_equal_the_scorecard(self):
        rows = copy.deepcopy(self.rows)
        target = next(item for item in rows if item["layer"] == "C" and item["label"] == "TAXNUM")
        target["label"] = "SURNAME"
        with self.assertRaisesRegex(ledger.LedgerError, "ledger vs scorecard"):
            ledger.reconcile(rows, self.record_path, ledger._contracts(), ledger._layer_contract())

    def test_tampered_rows_file_fails_its_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in (ledger.INDEX.relative_to(ledger.ROOT),
                             Path(self.index["record"]["file"]), Path(self.index["rows"]["file"])):
                (root / relative).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ledger.ROOT / relative, root / relative)
            rows_path = root / self.index["rows"]["file"]
            with gzip.open(rows_path, "at", encoding="utf-8") as stream:
                stream.write(json.dumps(self.rows[0]) + "\n")
            with self.assertRaisesRegex(ledger.LedgerError, "pinned SHA-256"):
                ledger.load(root)


if __name__ == "__main__":
    unittest.main()
