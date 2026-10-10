import hashlib
import json
import unittest
from pathlib import Path

import agentic_layers as agentic


ROOT = Path(__file__).resolve().parents[2]
V14_HASHES = {
    "dev": "fa9df2412b489d2c94981f145e09666a0ff6009130597bd8022cba80b2415380",
    "test": "1e07150bb9ba391488355b5212f0797bfd511b8095c8e3e2ebd6936627e26bec",
}


class RecordIdScalarSyntaxTests(unittest.TestCase):
    def test_v14_corpus_remains_byte_identical(self):
        for partition, expected in V14_HASHES.items():
            records = agentic.records_as_of(14, agentic.generate(partition))
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(records)).hexdigest(), expected)
        contract = agentic.load_contract(ROOT, version=14)
        frozen = ROOT / agentic.HISTORICAL_CONTRACTS[14]
        self.assertEqual(hashlib.sha256(frozen.read_bytes()).hexdigest(), "ba4ade507783cd521d11aba13ff4992f0250d4df3fe1fd34a759f4e1ab2bdf19")
        self.assertEqual(json.loads(frozen.read_text())["corpus"]["generator_version"], 14)
        self.assertLessEqual({"CUSTOMER_ID", "EMPLOYEE_ID", "RECORD_ID"}, contract.scored_labels)

    def test_numeric_json_values_are_numbers_with_exact_gold_and_repeats(self):
        for partition in agentic.PARTITIONS:
            rows = agentic.record_id_syntax.records(agentic, partition)
            self.assertEqual(len(rows), 175)
            self.assertEqual({layer: sum(r.layer == layer for r in rows) for layer in ("A", "D", "R")},
                             {"A": 30, "D": 115, "R": 30})
            self.assertEqual(len({r.uid for r in rows}), 175)
            for record in rows:
                self.assertEqual(bool(record.gold), record.layer != "D")
                self.assertEqual(bool(record.decoys), record.layer == "D")
                for span in (*record.gold, *record.decoys):
                    self.assertEqual(record.text.encode()[span.start:span.end].decode(), span.value)
                if record.layer == "A":
                    value, = json.loads(record.text)["arguments"].values()
                    self.assertIs(type(value), int)
                    self.assertEqual(record.gold[0].value, str(value))
                    self.assertEqual(len(str(value)), 6)
                elif record.layer == "R":
                    self.assertEqual(len(record.gold), 3)
                    self.assertEqual(len({span.value for span in record.gold}), 1)

    def test_json_keywords_and_quoted_schema_labels_are_benign(self):
        for partition in agentic.PARTITIONS:
            rows = agentic.record_id_syntax.records(agentic, partition)
            for family in agentic.record_id_syntax.LABELS:
                for keyword in ("null", "true", "false"):
                    cells = [r for r in rows if r.family == family and r.surface == "recordids_syntax_json_" + keyword]
                    self.assertEqual(len(cells), 10)
                    for record in cells:
                        value, = json.loads(record.text)["arguments"].values()
                        self.assertTrue(value is None or type(value) is bool)
                        self.assertFalse(record.gold)
                        self.assertEqual(record.decoys[0].value, keyword)
            quoted = [r for r in rows if "quoted_label" in r.surface]
            self.assertEqual(len(quoted), 25)
            self.assertTrue(all(not r.gold and r.decoys for r in quoted))
            self.assertTrue(any(r.text == '[session A "customer ID" token]' for r in quoted))

    def test_positive_values_and_template_ids_are_partition_disjoint(self):
        rows = [agentic.record_id_syntax.records(agentic, part) for part in agentic.PARTITIONS]
        pools = [{span.value for r in partition for span in r.gold} for partition in rows]
        templates = [{r.template for r in partition} for partition in rows]
        self.assertFalse(pools[0] & pools[1])
        self.assertFalse(templates[0] & templates[1])


if __name__ == "__main__":
    unittest.main()
