"""Gold freeze is explicit, inventory checked, and never edits observations."""
import copy
import tempfile
import unittest
from pathlib import Path

import agentic_layers as layers
import gaze_bench_score as score
import scorecard_record as record

ROOT = Path(__file__).resolve().parents[2]


class FrozenGoldTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = ROOT / 'docs/reference/benchmarks/observations-v0.15.1-agentic.jsonl.gz'
        cls.header, cls.observations = record._read(cls.source)
        cls.contract = score.load_scored_label_contract(ROOT / 'docs/reference/benchmarks/scored-labels-v2.json')

    def changed_header(self):
        header = copy.deepcopy(self.header)
        for row in header['documents']:
            if row['layer'] == 'C':
                for verdict in row['validator']['gold_validation']:
                    if verdict['applicable']:
                        verdict['validator_passed'] = not verdict['validator_passed']
                        return header
        self.fail('fixture has no applicable validator')

    def test_only_layer_c_verdicts_change_and_inventory_mismatch_is_refused(self):
        changed = self.changed_header()
        original = copy.deepcopy(changed)
        self.assertEqual(record.freeze_gold_header(changed, self.header), 1)
        for row, old in zip(changed['documents'], original['documents'], strict=True):
            if row['layer'] != 'C':
                self.assertEqual(row, old)
            else:
                self.assertEqual({k: v for k, v in row.items() if k != 'validator'},
                                 {k: v for k, v in old.items() if k != 'validator'})
                self.assertEqual(row['validator']['predictions'], old['validator']['predictions'])
        self.assertEqual(changed['documents'], self.header['documents'])
        for mutation in ('gold', 'document', 'verdict_key', 'corpus'):
            with self.subTest(mutation=mutation):
                bad = self.changed_header()
                if mutation == 'gold':
                    bad['documents'][0]['gold'][0][1] += 1
                elif mutation == 'document':
                    bad['documents'].pop()
                elif mutation == 'verdict_key':
                    next(d for d in bad['documents'] if d['layer'] == 'C' and d['gold'])['validator']['gold_validation'][0]['end'] += 1
                else:
                    bad['corpus_sha256'] = 'wrong'
                with self.assertRaises(record.RecordError):
                    record.freeze_gold_header(bad, self.header)

    def test_explicit_freeze_replays_both_views_and_dropping_it_refuses_comparison(self):
        with tempfile.TemporaryDirectory() as directory:
            candidate = Path(directory) / 'candidate.jsonl.gz'
            record._write_rows(candidate, [self.changed_header(), *self.observations])
            base_card = record.rescore(self.source, self.contract, max_workers=1)
            candidate_card = record.rescore(candidate, self.contract, max_workers=1)
            # Mutation: deleting the explicit freeze must leave the strict guard intact.
            ordinary = layers.gate(base_card, candidate_card, allow_legacy_policy_inputs=True)
            self.assertEqual(ordinary['verdict'], 'not_comparable')
            self.assertEqual(ordinary['differing'], ['layer_c_gold_validity'])
            result = layers.gate_frozen_records(base_card, candidate_card, self.source, candidate, self.contract, allow_legacy_policy_inputs=True)
            frozen = result['frozen_gold_verdicts']
            self.assertTrue(frozen['observations_unchanged'])
            self.assertEqual(frozen['changed_verdicts']['base'], {'base': 0, 'candidate': 1})
            self.assertEqual(frozen['changed_verdicts']['candidate'], {'base': 1, 'candidate': 0})
            self.assertNotEqual(result['verdict'], 'not_comparable')
            self.assertNotEqual(frozen['candidate_verdict_view']['verdict'], 'not_comparable')
            _, replayed = record._read(candidate)
            self.assertEqual(replayed, self.observations)
            self.assertIn('Candidate-verdict sensitivity view', layers.gate_markdown(result))

    def test_freeze_does_not_override_other_identity_changes(self):
        from test_agentic_layers import _scorecard
        base, candidate = _scorecard(dict.fromkeys("CADR", 0)), _scorecard(dict.fromkeys("CADR", 0))
        candidate['layers']['gold_validity']['C']['value'] = 'b' * 64
        candidate['parameters']['policy_sha256'] = 'different'
        result = layers.gate_frozen_records(base, candidate, Path('absent'), Path('absent'), self.contract)
        self.assertEqual(result['verdict'], 'not_comparable')
        self.assertIn('policy_sha256', result['differing'])
