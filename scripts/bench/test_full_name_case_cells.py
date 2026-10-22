"""Model-free coverage and canonical-scoring checks for full-name case cells."""
import hashlib
import json
import unittest
from collections import Counter
from pathlib import Path

import agentic_layers as api
import full_name_case_cells as cells
import gaze_bench_score as score


class FullNameCaseTests(unittest.TestCase):
    def test_case_locale_shape_surface_and_seed_matrix(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            self.assertEqual(rows, cells.records(api, partition))
            self.assertEqual(len(rows), len({r.uid for r in rows}))
            self.assertEqual(Counter(r.layer for r in rows), {"A": 72, "R": 144, "D": 261})
            for language in cells.REGIONS:
                for size in (2, 3):
                    for case in cells.CASES:
                        for surface in cells.SURFACES:
                            prefix = f"fullnames_{language}_{size}part_{case}_{surface}_"
                            selected = [r for r in rows if r.surface.startswith(prefix) and r.layer != "D"]
                            self.assertEqual({r.surface.rsplit('_', 2)[-2] + '_' + r.surface.rsplit('_', 1)[-1]
                                              if r.layer == 'R' else 'first' for r in selected},
                                             {"first", "rule_seed", "ner_seed"})
                            for r in selected:
                                self.assertEqual(len(r.gold), size * (2 if r.layer == 'R' else 1))
                                self.assertEqual([g.label for g in r.gold[:size]],
                                                 ["GIVENNAME"] * (size - 1) + ["SURNAME"])
                                if r.layer == 'R':
                                    self.assertEqual([g.value for g in r.gold[size:]],
                                                     [cells.CASES[case](g.value) for g in r.gold[:size]])
                                    self.assertEqual(r.text.startswith('reply to '), r.surface.endswith('rule_seed'))

    def test_current_contract_accepts_all_new_gold(self):
        prepared = api.prepare(Path(__file__).resolve().parents[2])
        self.assertEqual(prepared.manifest["generator_version"], api.GENERATOR_VERSION)

    def test_raw_unicode_offsets_and_tool_argument_strings(self):
        for partition in api.PARTITIONS:
            for row in cells.records(api, partition):
                for span in (*row.gold, *row.decoys):
                    self.assertEqual(row.text.encode()[span.start:span.end].decode(), span.value)
                self.assertEqual(bool(row.gold), row.layer != 'D')
                if 'tool_json' in row.surface:
                    text = row.text.split('\n', 1)[-1] if row.layer == 'R' or row.surface.endswith(('rule_seed','ner_seed')) else row.text
                    payload = json.loads(json.loads(text)['arguments'])
                    spans = row.gold or row.decoys
                    parts = len(spans) // (2 if row.surface.endswith(('rule_seed','ner_seed')) else 1)
                    self.assertEqual(next(iter(payload.values())), ' '.join(g.value for g in spans[-parts:]))

    def test_partitions_disjoint_and_v19_frozen(self):
        values = [{g.value.casefold() for r in cells.records(api, p) for g in r.gold} for p in api.PARTITIONS]
        self.assertFalse(values[0] & values[1])
        pins = {'dev': '1bef59f4bff3a1660600407d70ed3132228d07d6c5edefb0609d9b648acbead1',
                'test': '4f096686fdea809eb6089060e468de99147770160a208219aac1b4406f058813'}
        for partition, digest in pins.items():
            self.assertEqual(hashlib.sha256(api.corpus_bytes(api.records_as_of(19,api.generate(partition)))).hexdigest(),digest)
        root=Path(__file__).resolve().parents[2]
        self.assertEqual(hashlib.sha256((root/api.HISTORICAL_CONTRACTS[19]).read_bytes()).hexdigest(),
                         "c23c3a978df0bb5b7d002eaf2e4e3759d7b41851f12201cf9a70371621bc356a")

    def test_counterweights_include_same_cues_and_surname_nouns(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api,partition)
            for row in rows:
                if row.layer == 'A':
                    self.assertTrue(any(d.layer=='D' and d.template==row.template for d in rows))
            for language, noun in cells.NOUNS[partition].items():
                self.assertTrue(any(d.layer=='D' and noun in d.text and not d.gold for d in rows))
            if partition=='test':
                for phrase in ('contact technical support','reply to the team','dear customer'):
                    self.assertTrue(any(d.layer=='D' and d.text==phrase for d in rows))

    def test_canonical_scorer_sees_missing_repeat_partial_name_and_benign_catch(self):
        for row in cells.records(api,'test'):
            doc=row.to_document()
            accumulator=score.MetricAccumulator()
            # Simulate a case-sensitive detector catching only the seed.
            spans = row.gold[:len(row.gold)//2] if row.layer=='R' else ()
            accumulator.add(doc,[score.Span(g.start,g.end,g.label) for g in spans])
            metrics=accumulator.result()['utf8_bytes']
            self.assertEqual(metrics['leaked']>0,row.layer!='D')
            if row.layer=='D':
                accumulator=score.MetricAccumulator()
                accumulator.add(doc,[score.Span(d.start,d.end,'GIVENNAME') for d in row.decoys])
                self.assertGreater(accumulator.result()['utf8_bytes']['false_positive'],0)
            else:
                accumulator=score.MetricAccumulator()
                accumulator.add(doc,[score.Span(g.start,g.end,g.label) for g in row.gold if g.label=='GIVENNAME'])
                self.assertGreater(accumulator.result()['utf8_bytes']['leaked'],0)


if __name__=='__main__':
    unittest.main()
