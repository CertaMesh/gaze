"""Producer-boundary checks independent of detector quote recognition."""
import csv
import io
import json
import sqlite3
import unicodedata
import unittest
from contextlib import closing

import agentic_layers as api
from field_wrappers import WRAPPERS
import record_id_wrapper_cells as cells


class WrapperCellsTests(unittest.TestCase):
    def test_inventory_covers_all_unicode_opening_punctuation(self):
        inventory = {opening for opening, _ in WRAPPERS}
        for number in range(0x110000):
            char = chr(number)
            if unicodedata.category(char) in ("Ps", "Pi"):
                self.assertIn(char, inventory)
        self.assertTrue(set('`\"\'「『„‚«‹') <= inventory)

    def test_genuine_csv_cells_and_private_public_gold(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            self.assertEqual(len(rows), len(WRAPPERS) * 21 * 3 * 3)
            self.assertEqual(len(rows), len({row.uid for row in rows}))
            self.assertEqual({row.family for row in rows}, set(cells.LABELS))
            for row in rows:
                self.assertEqual(bool(row.gold), row.layer != "D")
                self.assertTrue(any(span.label == "public_column" for span in row.decoys))
                for span in (*row.gold, *row.decoys):
                    self.assertEqual(row.text.encode()[span.start:span.end].decode(), span.value)
                if row.layer == "R":
                    self.assertEqual(row.gold[0].value, row.gold[1].value)
                    text = row.text.split('\n', 1)[1]
                else:
                    text = row.text
                if "_csv_" in row.surface:
                    parsed = list(csv.reader(io.StringIO(text)))
                    self.assertEqual(len(parsed), 1)
                    self.assertEqual(len(parsed[0]), 2)
                    self.assertEqual(parsed[0][1], cells.PUBLIC[partition][2])
                    self.assertIn('=', parsed[0][0])
                    if row.layer != "D":
                        raw = row.gold[-1].value
                        self.assertIn(raw.replace('""', '"'), parsed[0][0])
                if "_sql_literal_" in row.surface:
                    with closing(sqlite3.connect(":memory:")) as db:
                        private, public = db.execute(text).fetchone()
                        self.assertIn('=', private)
                        self.assertEqual(public, cells.PUBLIC[partition][2])
                        if row.gold:
                            self.assertIn(row.gold[-1].value.replace("''", "'"), private)
                if "_json_depth" in row.surface:
                    parsed = json.loads(text)
                    while isinstance(parsed, dict):
                        self.assertEqual(parsed["public_version"], cells.PUBLIC[partition][2])
                        note = parsed["note"]
                        try:
                            parsed = json.loads(note)
                        except json.JSONDecodeError:
                            self.assertIn('=', note)
                            break

    def test_pre_v27_inputs_exclude_new_wrappers(self):
        rows = api.generate("test")
        self.assertTrue(any(row.surface.startswith('recordids_wrappers_') for row in rows))
        self.assertFalse(any(row.surface.startswith('recordids_wrappers_') for row in api.records_as_of(26, rows)))
