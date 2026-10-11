"""Field ownership assertions authored independently of detector rules."""
import hashlib
import json
import unittest
from pathlib import Path

import agentic_layers as api
import record_id_ownership_cells as cells


ROOT = Path(__file__).resolve().parents[2]


class RecordIdOwnershipTests(unittest.TestCase):
    def test_previous_corpus_and_contract_remain_reconstructible(self):
        expected = {"dev": "", "test": "d6dac67a85d555f3997f0684a2e58a29570937a82fb16347daeadf4b63967622"}
        for partition, digest in expected.items():
            old = api.records_as_of(16, api.generate(partition))
            if digest:
                self.assertEqual(hashlib.sha256(api.corpus_bytes(old)).hexdigest(), digest)
            self.assertFalse(any(r.surface.startswith("recordids_ownership_") for r in old))
        self.assertEqual(api.corpus_identity(ROOT, 16)[1],
                         "177cdd59eb82ec1700ffc3983261d6b459644f3b8499d4f828dd9f29c8cf1317")

    def test_independent_cells_have_exact_bytes_and_disjoint_partitions(self):
        pools = []
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            self.assertEqual(rows, cells.records(api, partition))
            self.assertEqual(len(rows), len({r.uid for r in rows}))
            self.assertEqual({r.layer for r in rows}, {"A", "D", "R"})
            self.assertEqual({r.family for r in rows}, set(cells.LABELS))
            self.assertTrue(all(r.surface.startswith("recordids_ownership_") for r in rows))
            for record in rows:
                raw = record.text.encode()
                self.assertEqual(bool(record.gold), record.layer != "D")
                for span in (*record.gold, *record.decoys):
                    self.assertEqual(raw[span.start:span.end].decode(), span.value)
            pools.append({g.value for r in rows for g in r.gold})
        self.assertFalse(pools[0] & pools[1])

    def test_grouped_values_and_word_only_controls(self):
        rows = cells.records(api, "test")
        grouped = [r for r in rows if "grouped" in r.surface and r.gold]
        self.assertTrue(grouped)
        self.assertEqual({r.language for r in grouped}, {"en", "de"})
        for row in grouped:
            self.assertTrue(any(ch.isspace() for ch in row.gold[0].value))
            self.assertTrue(any(ch.isalpha() for ch in row.gold[0].value.split()[0]))
            self.assertGreaterEqual(sum(ch.isdigit() for ch in row.gold[0].value), 6)
        controls = [r for r in rows if "placeholder" in r.surface and r.layer == "D"]
        self.assertTrue(controls)
        self.assertTrue(all(any(d.label.startswith("decoy:") for d in r.decoys) for r in controls))

    def test_quoted_scalar_gold_is_the_whole_raw_json_value(self):
        rows = [r for r in cells.records(api, "test") if "quoted" in r.surface and r.layer == "A"]
        self.assertTrue(rows)
        encountered = set()
        for row in rows:
            if "embedded" in row.surface:
                parsed = json.loads(json.loads(row.text)["payload"])
            else:
                parsed = json.loads(row.text)
            value = next(iter(parsed["arguments"].values()))
            self.assertIsInstance(value, str)
            encountered.update(ch for ch in value if ch in "@\t\\\"é")
            self.assertEqual(len(row.gold), 1)
            raw = row.gold[0].value
            self.assertNotEqual(raw, raw.split()[0]) if " " in raw else None
            if "embedded" not in row.surface:
                self.assertEqual(json.loads('"' + raw + '"'), value)
        self.assertLessEqual(set("@\t\\\"é"), encountered)

    def test_adjacent_fields_own_separate_values_and_repeat_the_second(self):
        rows = [r for r in cells.records(api, "test") if "adjacent_personal" in r.surface]
        self.assertTrue(rows)
        for row in rows:
            self.assertGreaterEqual(len(row.gold), 2)
            self.assertNotEqual(row.gold[0].value, row.gold[1].value)
            self.assertLess(row.gold[0].end, row.gold[1].start)
            if row.layer == "R":
                self.assertEqual(len(row.gold), 3)
                self.assertEqual(row.gold[1].value, row.gold[2].value)
                self.assertGreaterEqual(sum(c.isdigit() for c in row.gold[1].value), 6)
        public = [r for r in cells.records(api, "test") if "adjacent_public" in r.surface]
        self.assertTrue(public)
        self.assertEqual({r.layer for r in public}, {"A", "D"})
        self.assertTrue(all(r.decoys for r in public))
        self.assertTrue(all(len(r.gold) == 1 for r in public if r.layer == "A"))

    def test_coverage_contains_all_three_failure_classes_and_cue_resumption(self):
        rows = cells.records(api, "test")
        for family in cells.LABELS:
            selected = [r for r in rows if r.family == family]
            for failure in ("grouped", "quoted", "adjacent"):
                self.assertEqual({r.layer for r in selected if failure in r.surface}, {"A", "D", "R"})
            resumed = [r for r in selected if "resumption" in r.surface]
            self.assertEqual({r.layer for r in resumed}, {"A", "D", "R"})
            self.assertTrue(all(r.decoys for r in resumed))


if __name__ == "__main__":
    unittest.main()
