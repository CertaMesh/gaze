"""Model-free provenance checks for the invisible-separator extension."""
import hashlib
import json
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
                    if row.family == "invisible_card_twin":
                        prefix, suffix = (("Schema example: ", "; incomplete.") if partition == "dev"
                                          else ("Public format fixture = ", ". Not assigned."))
                        digits = row.text.removeprefix(prefix).removesuffix(suffix).replace(separator, "")
                        doubled = [int(digit) * (2 if index % 2 else 1)
                                   for index, digit in enumerate(reversed(digits))]
                        self.assertNotEqual(sum(n - 9 if n > 9 else n for n in doubled) % 10, 0)

    def test_cued_tax_gold_and_counterweights_participate_in_credit_gate(self):
        self.assertIn("invisible_card_twin", agentic.guard_families(agentic.GENERATOR_VERSION))
        self.assertIn("invisible_tax_twin", agentic.guard_families(agentic.GENERATOR_VERSION))
        self.assertNotIn("invisible_card_twin", agentic.guard_families(26))
        self.assertNotIn("invisible_tax_twin", agentic.guard_families(26))
        for surface in cells.CUED_SURFACES:
            self.assertTrue(agentic.invalid_twin_credited("TAXNUM", surface))

    def test_integrated_history_reconstructs_byte_identically(self):
        # v25/v26 are the existing corpus pins; v27/v28 also include structured_.
        pins = {
            25: ("a88025bd7c995e3633738debf8b8bab45c8c7232bf222a4d72a9af36fc67ca59",
                 "133f8452d01fd517c77087e539ba9dc253e30a1886668bfab3e1631cc9ff6c32"),
            26: ("cbc9a1bbfc899c50b35c78e0ce1e02518b3a8f5ecfeb759ec5e6dde0b6432b25",
                 "f8b9dc58de45dcc6ec8de0e911dc0d5f52682889ec912cbf4b120a03e56409bc"),
            27: ("a1b00d3d8281b54feabe1c04c9a490738bc415d9f9b94b75ae9eb1b1c172e4e1",
                 "dbee6dc0184c49df384b9f2949b29af81e58c14ae1db0043df8aadc8f1c41a81"),
            28: ("ad447042cd7403b599b217c40b77851f8c07dc59c99d19bddd7a21586ca55c99",
                 "db979087d95fce766e649ba0f4bc701a62b4c6b2e3975767348e163e942e8b9f"),
        }
        root = Path(__file__).resolve().parents[2]
        for index, partition in enumerate(("dev", "test")):
            current = agentic.generate(partition)
            for version, digests in pins.items():
                with self.subTest(partition=partition, version=version):
                    previous = agentic.records_as_of(version, current)
                    self.assertEqual(hashlib.sha256(agentic.corpus_bytes(previous)).hexdigest(), digests[index])
                    agentic.load_contract(root, version=version)
        frozen = root / "docs/reference/benchmarks/scored-labels-agentic-generator-v26.json"
        self.assertEqual(hashlib.sha256(frozen.read_bytes()).hexdigest(),
                         "11bb0105c99e61e254d31c9cf7d93e333ccfc79f2086499490fa37a95277a660")

    def test_partitions_share_no_new_text_or_value(self):
        dev, test = (cells.records(agentic, partition) for partition in agentic.PARTITIONS)
        self.assertFalse({row.text for row in dev} & {row.text for row in test})
        self.assertFalse({span.value for row in dev for span in row.gold}
                         & {span.value for row in test for span in row.gold})

    def test_imported_cells_and_dev_phone_correction_are_pinned(self):
        pins = {"dev": "d0ddf954642df5e32612286d9efd418447ee7ca7831754adf258cfb9b06e4f0d",
                "test": "c16e596db95282e23fd72c02f4d04aab1de8da0afd94e532462311a56605f22f"}
        for partition, digest in pins.items():
            rows = cells.records(agentic, partition)
            self.assertEqual(len(rows), 12530)
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(rows)).hexdigest(), digest)

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

    def test_labelled_values_and_repeats_are_whole_source_gold(self):
        for partition in agentic.PARTITIONS:
            for row in cells.records(agentic, partition):
                if row.layer == "D" or row.surface.startswith("invisible_boundary_"):
                    continue
                first, *repeat = row.text.split("\n")
                if row.surface.endswith("_json"):
                    payload = json.loads(first)
                    fields = payload.get("result", payload)
                    expected = next(value for value in fields.values() if isinstance(value, str))
                elif partition == "dev":
                    expected = first.split(": ", 1)[1].removesuffix("; approved.")
                else:
                    expected = first.split(" = ", 1)[1].removesuffix(". End.")
                self.assertEqual([span.value for span in row.gold], [expected] * (1 + len(repeat)), row.uid)
                if repeat:
                    self.assertEqual(repeat, [("Again: " if partition == "dev" else "Copy = ") + expected])
                if row.surface.startswith("invisible_joined_"):
                    self.assertEqual(len(expected), 23)

    def test_boundary_shapes_have_whole_value_gold_and_formatting_counterweights(self):
        for partition in agentic.PARTITIONS:
            rows = [row for row in cells.records(agentic, partition)
                    if row.surface.startswith("invisible_boundary_")]
            self.assertEqual({row.layer for row in rows}, {"A", "R", "D"})
            self.assertEqual({span.label for row in rows for span in row.gold},
                             {"CREDITCARDNUMBER", "IBAN", "ETHADDRESS", "EMAIL", "TELEPHONENUM"})
            for shape in ("before", "after", "inside", "mixed"):
                selected = [row for row in rows if row.surface.endswith("_" + shape)]
                self.assertTrue(selected)
                self.assertTrue(all(row.gold for row in selected))
            self.assertEqual({row.family for row in rows if row.layer == "D"},
                             {"formatting_" + name for name in ("emoji_zwj", "emoji_vs16", "emoji_tag_flag",
                                                                "persian", "arabic", "indic", "rtl")})
            for row in rows:
                for span in row.gold:
                    self.assertEqual(row.text.encode()[span.start:span.end].decode(), span.value)
                self.assertEqual(len(row.gold), {"A": 1, "D": 0, "R": 2}[row.layer])
                if row.layer == "D":
                    continue
                shape = row.surface.rsplit("_", 1)[1]
                first = row.text.split("\n", 1)[0]
                gold = row.gold[0]
                before = first.encode()[:gold.start].decode()
                after = first.encode()[gold.end:].decode()
                if shape in {"before", "mixed"}:
                    word = "item" if partition == "dev" else "entry"
                    self.assertTrue(before.startswith(word))
                    self.assertEqual(len(before.removeprefix(word)), 1)
                else:
                    self.assertEqual(before, "Value: ")
                if shape in {"after", "mixed"}:
                    word = "paid" if partition == "dev" else "ready"
                    self.assertTrue(after.endswith(word + "; end."))
                    self.assertEqual(len(after.removesuffix(word + "; end.")), 1)
                else:
                    self.assertEqual(after, "; end.")
                # Formatting inside the value is gold, not merely present nearby.
                controls = [c for c in gold.value if not c.isascii()]
                self.assertEqual(len(controls), int(shape in {"inside", "mixed"}), row.uid)
