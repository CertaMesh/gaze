"""Keep issuer positive/negative attribution independent of detector output."""
import hashlib
import unittest

import agentic_layers as api
import issuer_credential_cells as cells


class IssuerCredentialCellsTests(unittest.TestCase):
    def test_positive_and_counterweight_cells_and_source_offsets(self):
        records = cells.records(api, "test")
        self.assertEqual(len(records), 3190)
        self.assertEqual({r.layer for r in records}, {"A", "D", "R"})
        for record in records:
            self.assertEqual(bool(record.gold), record.layer != "D")
            for gold in record.gold:
                self.assertEqual(record.text.encode()[gold.start:gold.end].decode(), gold.value)
        self.assertTrue(any(r.surface.endswith("truncated_nested_json") for r in records))
        self.assertTrue(any(r.family == "generic_sk" and r.layer == "D" for r in records))

    def test_partitions_use_different_values_and_rendered_templates(self):
        dev, test = [cells.records(api, p) for p in api.PARTITIONS]
        self.assertFalse({r.text for r in dev} & {r.text for r in test})
        self.assertFalse({s.value for r in dev for s in r.gold} &
                         {s.value for r in test for s in r.gold})

    def test_previous_generator_reconstructs_byte_exactly(self):
        for partition, expected in {
            "dev": "a88025bd7c995e3633738debf8b8bab45c8c7232bf222a4d72a9af36fc67ca59",
            "test": "133f8452d01fd517c77087e539ba9dc253e30a1886668bfab3e1631cc9ff6c32",
        }.items():
            previous = api.records_as_of(25, api.generate(partition))
            self.assertEqual(hashlib.sha256(api.corpus_bytes(previous)).hexdigest(), expected)
