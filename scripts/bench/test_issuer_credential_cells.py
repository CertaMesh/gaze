"""Keep issuer positive/negative attribution independent of detector output."""
import hashlib
import unittest

import agentic_layers as api
import issuer_credential_cells as cells


class IssuerCredentialCellsTests(unittest.TestCase):
    def test_positive_and_counterweight_cells_and_source_offsets(self):
        records = cells.records(api, "test")
        self.assertEqual(len(records), 4200)
        self.assertEqual({r.layer for r in records}, {"A", "D", "R"})
        for record in records:
            self.assertEqual(len(record.gold), {"A": 1, "D": 0, "R": 2}[record.layer])
            for gold in record.gold:
                self.assertEqual(record.text.encode()[gold.start:gold.end].decode(), gold.value)
                self.assertEqual(gold.label, "SECURITYTOKEN")
        self.assertTrue(any(r.surface.endswith("truncated_nested_json") for r in records))
        self.assertTrue(any(r.family == "generic_sk" and r.layer == "D" for r in records))
        self.assertEqual(sum(r.layer == "D" and r.family == "code_identifier" for r in records), 90)
        self.assertEqual(sum(r.layer == "D" and r.family == "tutorial" for r in records), 20)
        self.assertTrue(any(r.surface.endswith("pem_comma") for r in records))
        self.assertTrue(any(r.surface.endswith("pem_space") for r in records))
        encoded = [r for r in records if r.surface.endswith("pem_url")]
        self.assertEqual(len(encoded), 70)
        self.assertTrue(all("BEGIN%20" in r.text for r in encoded))

    def test_gold_owns_complete_source_values_but_not_wrappers(self):
        for partition in api.PARTITIONS:
            wrappers = (["({V})", '{"output":"{V}"}', "GH_TOKEN_{V}", "令牌{V}已隐藏", "Bearer%20{V}"]
                        if partition == "dev" else
                        ["[{V}]", '{"result":{"value":"{V}"}}', "BUILD_{V}", "输出{V}结束", "auth%3D{V}"])
            for row in cells.records(api, partition):
                if row.layer == "D":
                    self.assertFalse(row.gold)
                    continue
                surface = row.surface.removeprefix("issuer_credentials_")
                expected = wrappers[1]
                if surface.startswith("bare_"):
                    expected = wrappers[int(surface.removeprefix("bare_"))]
                elif surface == "repeat":
                    expected = wrappers[0] + " " + wrappers[1]
                elif surface == "boundary":
                    expected = wrappers[0]
                elif surface == "embedded_refresh":
                    expected = wrappers[1].replace("{V}", "wordxoxe.{V}")
                source = row.text.encode()
                for gold in reversed(row.gold):
                    source = source[:gold.start] + b"{V}" + source[gold.end:]
                self.assertEqual(source.decode(), expected, row.uid)
                if surface.startswith(("pem_", "truncated_")):
                    self.assertTrue(row.gold[0].value.startswith("-----BEGIN"))
                    self.assertEqual("-----END" in row.gold[0].value, surface.startswith("pem_"))

    def test_partitions_use_different_values_and_rendered_templates(self):
        dev, test = [cells.records(api, p) for p in api.PARTITIONS]
        self.assertFalse({r.text for r in dev} & {r.text for r in test})
        self.assertFalse({s.value for r in dev for s in r.gold} &
                         {s.value for r in test for s in r.gold})

    def test_source_cells_stay_byte_identical(self):
        for partition, expected in {
            "dev": "a65138d3ca650dde00154f6c0ee5290e745fad74b12eb3abdd8d1f96e8591ce9",
            "test": "a7720d99fa90ec6987777d82c15c1805999cb3feae46ca41e59cae4681390268",
        }.items():
            rows = cells.records(api, partition)
            self.assertEqual(hashlib.sha256(api.corpus_bytes(rows)).hexdigest(), expected)
