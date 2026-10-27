"""Model-free provenance checks for the invisible-separator extension."""
import hashlib
from pathlib import Path
import unittest

import agentic_layers as agentic
import invisible_separator_cells as cells


class InvisibleSeparatorCellsTests(unittest.TestCase):
    def test_each_separator_has_independent_gold_repeats_and_counterweights(self):
        for partition in agentic.PARTITIONS:
            rows = cells.records(agentic, partition)
            for name, separator in cells.SEPARATORS.items():
                selected = [row for row in rows if row.surface.startswith("invisible_" + name + "_")]
                self.assertEqual({row.layer for row in selected}, {"A", "D", "R"})
                self.assertTrue({"invisible_card_twin", "invisible_tax_twin", "invisible_phone_twin",
                                 "invisible_iban_twin", "invisible_email_twin"}
                                <= {row.family for row in selected if row.layer == "D"})
                self.assertEqual({span.label for row in selected for span in row.gold},
                                 {"TAXNUM", "IBAN", "TELEPHONENUM", "CREDITCARDNUMBER", "EMAIL"})
                for row in selected:
                    self.assertIn(separator, row.text)
                    self.assertEqual(len(row.gold), {"A": 1, "D": 0, "R": 2}[row.layer])
                    for span in (*row.gold, *row.decoys):
                        self.assertEqual(row.text.encode()[span.start:span.end].decode(), span.value)
                        self.assertIn(separator, span.value)

    def test_cued_tax_gold_and_counterweights_participate_in_credit_gate(self):
        self.assertIn("invisible_card_twin", agentic.guard_families(agentic.GENERATOR_VERSION))
        self.assertIn("invisible_tax_twin", agentic.guard_families(agentic.GENERATOR_VERSION))
        self.assertNotIn("invisible_card_twin", agentic.guard_families(25))
        for surface in cells.CUED_SURFACES:
            self.assertTrue(agentic.invalid_twin_credited("TAXNUM", surface))

    def test_old_generation_stays_byte_identical(self):
        pins = {"dev": "a88025bd7c995e3633738debf8b8bab45c8c7232bf222a4d72a9af36fc67ca59",
                "test": "133f8452d01fd517c77087e539ba9dc253e30a1886668bfab3e1631cc9ff6c32"}
        for partition, digest in pins.items():
            previous = agentic.records_as_of(25, agentic.generate(partition))
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(previous)).hexdigest(), digest)

    def test_partitions_share_no_new_text_or_value(self):
        dev, test = (cells.records(agentic, partition) for partition in agentic.PARTITIONS)
        self.assertFalse({row.text for row in dev} & {row.text for row in test})
        self.assertFalse({span.value for row in dev for span in row.gold}
                         & {span.value for row in test for span in row.gold})

    def test_v27_reconstructs_byte_identically(self):
        pins = {"dev": "1feeb7efe720f1c4a3ec773764121b7cfe33495e50f30135d0f2a3ec6097b13e",
                "test": "7a93d8e6097ec5936047854b561e647208a8865e160ea553b7127799e2f4c638"}
        for partition, digest in pins.items():
            previous = agentic.records_as_of(27, agentic.generate(partition))
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(previous)).hexdigest(), digest)
            agentic.load_contract(Path(__file__).resolve().parents[2], version=27)

    def test_bom_counterweights_include_leading_and_embedded_code(self):
        for partition in agentic.PARTITIONS:
            rows = cells.records(agentic, partition)
            code = [row for row in rows if row.family in {"leading_code", "embedded_code"}]
            self.assertEqual(len(code), 2 * agentic.DOCS_PER_FAMILY)
            self.assertTrue(all(row.layer == "D" and not row.gold for row in code))
            self.assertTrue(all("\ufeff" in row.text for row in code))
            self.assertTrue(all(row.text.startswith("\ufeff") for row in code if row.family == "leading_code"))

    def test_joined_overlong_values_have_gold_repeats_and_public_counterweights(self):
        for partition in agentic.PARTITIONS:
            rows = cells.records(agentic, partition)
            for name, separator in cells.JOINERS.items():
                selected = [row for row in rows if row.surface.startswith("invisible_joined_" + name + "_")]
                self.assertEqual({row.layer for row in selected}, {"A", "D", "R"})
                self.assertEqual({span.label for row in selected for span in row.gold}, {"NATIONALID", "TAXNUM"})
                for row in selected:
                    self.assertIn(separator, row.text)
                    self.assertEqual(len(row.gold), {"A": 1, "D": 0, "R": 2}[row.layer])
