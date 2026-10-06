"""Model-free ownership and mutation checks for the government-ID slice."""
import dataclasses
import hashlib
import json
from pathlib import Path
import re
import sys
import unittest
from unittest import mock
sys.path.insert(0, str(Path(__file__).resolve().parent))
import agentic_layers as a
import government_id_cells as g


class GovernmentCellsTests(unittest.TestCase):
    def test_shapes_and_whole_value_offsets(self):
        for partition in a.PARTITIONS:
            records = g.records(a, partition)
            self.assertEqual({r.family for r in records}, {c.family for c in (*g.CELLS, *g.TWINS)})
            self.assertEqual({c.shape for c in g.CELLS}, set(g.Shape))
            self.assertEqual(len(g.Shape), 20)
            self.assertEqual(sum(r.layer == 'A' for r in records), 80)
            self.assertEqual(sum(r.layer == 'D' for r in records), 84)
            expected = {'dev': '5f3dd32dd69ddeac6041ac18fe79c81bc7d04c7da1cb6029bb5425f3f5d2fe77',
                        'test': '996e4b1d22e044b31e9c3446f3cfb9982491c83e698928f26b2625f1e83c0049'}
            self.assertEqual(hashlib.sha256(a.corpus_bytes(records)).hexdigest(), expected[partition])
            self.assertEqual({c.surface for c in g.CELLS}, {'gov_prose', 'gov_log_kv', 'gov_tool_json', 'gov_tool_result'})
            for r in records:
                (span,) = r.gold or r.decoys
                self.assertEqual(r.text.encode()[span.start:span.end].decode(), span.value)
                if r.surface == 'gov_tool_json':
                    json.loads(r.text)

    def test_population_rejects_a_missing_or_duplicate_record(self):
        records = g.records(a, 'dev')
        for changed in (records[1:], [*records, records[0]]):
            with self.subTest(documents=len(changed)):
                with self.assertRaisesRegex(a.LayerError, 'government population'):
                    g.check(a, changed)

    def test_schema_partition_and_lineage_tampering_fail_closed(self):
        records = g.records(a, 'dev')
        row = records[0]
        changes = {
            'partition': 'test', 'layer': 'D', 'family': 'gov_unknown',
            'surface': 'gov_tool_json', 'group': 'foreign-group',
            'template': 'government/tax_nine/test', 'language': 'en',
            'region': 'US', 'validity': a.BENIGN,
        }
        for field, value in changes.items():
            with self.subTest(field=field):
                changed = [dataclasses.replace(row, **{field: value}), *records[1:]]
                with self.assertRaises(a.LayerError):
                    g.check(a, changed)

    def test_gold_and_decoy_offsets_and_labels_cannot_be_forged(self):
        records = g.records(a, 'test')
        for index in (0, 80):
            row = records[index]
            span = (row.gold or row.decoys)[0]
            for forged in (dataclasses.replace(span, start=span.start + 1),
                           dataclasses.replace(span, label='EMAIL')):
                with self.subTest(layer=row.layer, span=forged):
                    changed = dataclasses.replace(row, **{'gold' if row.gold else 'decoys': [forged]})
                    with self.assertRaises(a.LayerError):
                        g.check(a, [*records[:index], changed, *records[index + 1:]])

    def test_partition_templates_and_all_values_are_disjoint(self):
        for cell in (*g.CELLS, *g.TWINS):
            self.assertNotEqual(cell.templates['dev'], cell.templates['test'])
        values = {p: {s.value for r in g.records(a, p) for s in (*r.gold, *r.decoys)} for p in a.PARTITIONS}
        self.assertFalse(values['dev'] & values['test'])

    def test_growth_and_frozen_v8(self):
        expected = {'dev': '60c3fe121db4ce07b0dfbc2397c1a48a0c1773a5c94eff324fdde597a6aa8ae6',
                    'test': 'ddd234551bcae00ab0f97026fd4b5b6d3d4b4b23cf08b8fa15926e87f598bd5c'}
        for p in a.PARTITIONS:
            records = a.generate(p)
            old = a.records_as_of(8, records)
            self.assertEqual(hashlib.sha256(a.corpus_bytes(old)).hexdigest(), expected[p])
            for layer in ('A', 'D'):
                self.assertLessEqual(sum(r.layer == layer and r.surface.startswith('gov_') for r in records) * 10,
                                     sum(r.layer == layer for r in old))

    def test_broad_and_narrow_mutants_pay_for_each_shape(self):
        for partition in a.PARTITIONS:
            rs = g.records(a, partition)
            for shape in g.Shape:
                for table in (g.BROAD, g.NARROW):
                    costs = [r for r in rs if r.layer == 'D' and any(
                        a._overlaps(table[shape], r.text, *a._char_span(r.text, span)) for span in r.decoys)]
                    self.assertTrue(costs, (partition, shape))
                self.assertTrue(any(r.layer == 'A' and r.family == 'gov_' + shape.value and
                                    re.search(g.NARROW[shape], r.text) for r in rs) or
                                g.CELLS[list(g.Shape).index(shape)].relation == g.Relation.LINE)

    def test_policies_equal_the_committed_mutation_patterns(self):
        root = Path(__file__).resolve().parent / 'fixtures/agentic'
        for name, table in (('broad', g.BROAD), ('narrow', g.NARROW)):
            patterns = re.findall(r"^pattern = '(.*)'$", (root / f'mutant-{name}-government-ids.toml').read_text(), re.M)
            self.assertEqual(sorted(patterns), sorted(table.values()))

    def test_missing_counterweights_fail_generation(self):
        for family in ('gov_twin_tax_nine', 'gov_near_tax_nine'):
            # Removing both costs, or the sole cue-aware cost, must fail closed.
            twins = tuple(c for c in g.TWINS if c.shape != g.Shape.TAX_NINE or (family == 'gov_near_tax_nine' and not c.near_cue))
            with mock.patch.object(g, 'TWINS', twins), self.assertRaisesRegex(a.LayerError, 'D cost'):
                g.records(a, 'test')

    def test_changed_ownership_and_truncated_values_fail(self):
        records = g.records(a, 'dev')
        for cell in (g.CELLS[0], g.TWINS[0]):
            row = next(r for r in records if r.family == cell.family)
            new_text = (re.sub(g.CUES[cell.shape.label], 'Reference', row.text, count=1, flags=re.I) if cell.gold
                        else row.text.replace('Invoice reference', 'Steuer-ID'))
            delta = len(new_text.encode()) - len(row.text.encode())
            span = (row.gold or row.decoys)[0]
            moved = dataclasses.replace(span, start=span.start + delta, end=span.end + delta)
            changed = dataclasses.replace(row, text=new_text, gold=(moved,) if cell.gold else (), decoys=() if cell.gold else (moved,))
            with self.assertRaisesRegex(a.LayerError, 'ownership'):
                g.check(a, [changed])
        row = records[0]
        span = dataclasses.replace(row.gold[0], end=row.gold[0].end - 1, value=row.gold[0].value[:-1])
        with self.assertRaisesRegex(a.LayerError, 'shape'):
            g.check(a, [dataclasses.replace(row, gold=[span])])

    def test_full_ssn_and_steuer_values_are_unassignable(self):
        for partition in a.PARTITIONS:
            for row in g.records(a, partition):
                if row.layer != 'A':
                    continue
                v = row.gold[0].value
                if row.family in ('gov_tax_eleven', 'gov_tax_grouped'):
                    self.assertFalse(a.steuer_id_valid(v))
                    self.assertEqual(row.validity, a.INVALID)
                if row.family.startswith('gov_ssn_') and row.family not in ('gov_ssn_tail', 'gov_ssn_mixed'):
                    self.assertTrue(v.startswith('000'))

    def test_cued_invalid_tax_credit_and_counterweights_are_versioned(self):
        for surface in ('gov_prose', 'gov_log_kv', 'gov_tool_json', 'gov_tool_result'):
            self.assertTrue(a.invalid_twin_credited('TAXNUM', surface))
        self.assertFalse(a.invalid_twin_credited('TAXNUM', 'prose_nocue'))
        added = set(a.guard_families(11)) - set(a.guard_families(10))
        self.assertEqual(added, {'gov_twin_tax_eleven', 'gov_near_tax_eleven',
                                 'gov_twin_tax_grouped', 'gov_near_tax_grouped'})


if __name__ == '__main__':
    unittest.main()
