import contextlib
from dataclasses import replace
import io
import itertools
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import evidence_eval as ee
import evidence_protocol as ep
from test_evidence_protocol import declaration, golden, CUSTODY


def inventory():
    return ee.PlannedInventory([
        ee.PlannedCase('fixture_a','group_a','synthetic_en',1.),
        ee.PlannedCase('fixture_b','group_a','synthetic_en',1.),
        ee.PlannedCase('fixture_c','group_b','synthetic_en',2.),
    ])


def completed(leak=0, **kw):
    return ee.DocRecord(outcome='COMPLETED', gold_occurrences=2, gold_bytes=30, gold_bytes_surviving=leak, **kw)


def paired():
    e = ee.PrivateEvaluator(inventory())
    for k, b, c in [('fixture_a',0,3),('fixture_b',0,5),('fixture_c',9,0)]:
        e.add('base',k,completed(b)); e.add('candidate',k,completed(c))
    return e


class PlannedInventoryTests(unittest.TestCase):
    def test_unknown_key_is_refused(self):
        with self.assertRaises(ep.ReceiptRefused): ee.PrivateEvaluator(inventory()).add('base','unknown',completed())

    def test_duplicate_add_is_refused(self):
        e = paired()
        with self.assertRaises(ep.ReceiptRefused): e.add('base','fixture_a',completed())

    def test_conflicting_group_metadata_is_refused(self):
        for attribute, value in [('stratum','synthetic_de'),('weight',2.)]:
            a = ee.PlannedCase('a','same','synthetic_en',1.)
            with self.assertRaises(ep.ReceiptRefused): ee.PlannedInventory([a,replace(a,key='b',**{attribute:value})])

    def test_duplicate_inventory_and_invalid_weights_are_refused(self):
        a = ee.PlannedCase('a','g','synthetic_en',1.)
        with self.assertRaises(ep.ReceiptRefused): ee.PlannedInventory([a,a])
        for w in [True,0,-1,float('inf'),float('nan')]:
            with self.assertRaises(ep.ReceiptRefused): ee.PlannedInventory([replace(a,weight=w)])

    def test_case_absent_from_both_arms_is_not_started_in_both(self):
        e = ee.PrivateEvaluator(inventory()).finalize()
        self.assertTrue(e.asymmetric_outcome_table()['NOT_STARTED']['NOT_STARTED'] == 3)

    def test_margins_and_total_reconcile_to_the_inventory(self):
        e = paired().finalize(); e.reconcile()
        self.assertTrue(sum(sum(row.values()) for row in e.asymmetric_outcome_table().values()) == 3)

    def test_unreconciled_population_is_refused(self):
        e = paired().finalize(); e._records['base']['extra'] = completed()
        with self.assertRaises(ep.ReceiptRefused): e.reconcile()

    def test_records_cannot_regroup_or_overwrite_after_finalize(self):
        e = paired().finalize()
        with self.assertRaises(ep.ReceiptRefused): e.add('base','fixture_a',completed())
        with self.assertRaises(TypeError): ee.DocRecord(group_id='private')

    def test_invalid_record_counts_are_refused(self):
        for r in [completed(-1), completed(gold_occurrences_surviving=3), completed(restore_exact=True), completed(false_positive_bytes=True)]:
            with self.assertRaises(ep.ReceiptRefused): ee.PrivateEvaluator(inventory()).add('base','fixture_a',r)


class PairingTests(unittest.TestCase):
    def test_missing_pair_leaves_intersection_and_is_not_zero_leak(self):
        e = ee.PrivateEvaluator(inventory())
        e.add('base','fixture_a',completed(10));e.add('candidate','fixture_a',completed(4))
        e.add('base','fixture_c',completed(30))
        result = e.paired_interval('gold_bytes_surviving_egress', declaration())
        self.assertTrue(len(e.paired_completed_keys()) == 1 and result['point'] == -6)

    def test_conditional_flag_set_when_remainder_non_empty(self):
        e = ee.PrivateEvaluator(inventory())
        e.add('base','fixture_a',completed());e.add('candidate','fixture_a',completed())
        self.assertTrue(e.paired_interval('gold_bytes_surviving_egress',declaration())['conditional'])
        self.assertTrue(e.asymmetric_outcome_table()['NOT_STARTED']['NOT_STARTED'] == 2)

    def test_failed_closed_is_not_protection(self):
        e = ee.PrivateEvaluator(inventory());e.add('base','fixture_a',ee.DocRecord(outcome='FAILED_CLOSED_NO_EGRESS',entities=1))
        self.assertTrue(e.protected_case_count('base') == 0)
        with self.assertRaises(ep.ReceiptRefused): ee.PrivateEvaluator(inventory()).add('base','fixture_a',ee.DocRecord(outcome='FAILED_CLOSED_NO_EGRESS',entities=1,entities_fully_covered=1))

    def test_unknown_egress_is_neither_protected_nor_zero(self):
        e = ee.PrivateEvaluator(inventory());e.add('base','fixture_a',ee.DocRecord(outcome='UNKNOWN_EGRESS'))
        self.assertTrue(e.protected_case_count('base') == 0)
        self.assertTrue(e.paired_interval('gold_bytes_surviving_egress',declaration()) == 'NOT_EVALUABLE')

    def test_unknown_egress_observed_fragment_is_retained_as_lower_bound(self):
        e = ee.PrivateEvaluator(inventory());e.add('base','fixture_a',ee.DocRecord(outcome='UNKNOWN_EGRESS',gold_bytes=30,gold_bytes_surviving=8,gold_occurrences=1,gold_occurrences_partially_surviving=1))
        a = e.aggregate('base')
        self.assertTrue(a['gold_bytes_surviving_egress'] == 8 and a['unknown_egress_lower_bound_cases'] == 1)
        self.assertTrue(e.protected_case_count('base') == 0)

    def test_unknown_remainder_blocks_leak_interval_even_with_completed_pair(self):
        e = ee.PrivateEvaluator(inventory());e.add('base','fixture_a',completed());e.add('candidate','fixture_a',completed())
        e.add('base','fixture_b',ee.DocRecord(outcome='UNKNOWN_EGRESS'))
        self.assertTrue(e.paired_interval('gold_bytes_surviving_egress', declaration()) == 'NOT_EVALUABLE')

    def test_protection_counter_can_be_nonzero(self):
        e = ee.PrivateEvaluator(inventory());e.add('base','fixture_a',completed(entities=1,entities_fully_covered=1))
        self.assertTrue(e.protected_case_count('base') == 1)


class DeclarationTests(unittest.TestCase):
    def test_absent_declaration_is_not_evaluable(self):
        self.assertTrue(paired().paired_interval('gold_bytes_surviving_egress',None) == 'NOT_EVALUABLE')

    def test_partial_declaration_is_not_evaluable(self):
        d = declaration();del d['confidence_level']
        self.assertTrue(paired().paired_interval('gold_bytes_surviving_egress',d) == 'NOT_EVALUABLE')

    def test_each_required_field_individually_gates(self):
        for k in ep.DECLARATION_FIELDS:
            d = declaration();del d[k]
            self.assertTrue(paired().paired_interval('gold_bytes_surviving_egress',d) == 'NOT_EVALUABLE')

    def test_types_ranges_and_stratum_coverage_gate(self):
        for k, value in [('confidence_level',1),('confidence_level',float('nan')),('resample_count',True),('resample_count',0),('seed',-1),('strata',['synthetic_de']),('coverage_target',2),('acceptance_limit',-1),('weighting','unknown')]:
            d = declaration();d[k] = value
            self.assertTrue(paired().paired_interval('gold_bytes_surviving_egress',d) == 'NOT_EVALUABLE')


class GroupingTests(unittest.TestCase):
    def test_resample_draws_groups_not_records(self):
        e = paired()
        self.assertTrue(e.resample_unit_count() == 2)
        seen = set()
        for i in range(30):
            draw = e.draw_resample(1,i)
            self.assertTrue(draw.count('fixture_a') == draw.count('fixture_b'))
            seen.add(len(draw))
        self.assertTrue(seen == {2,3,4})

    def test_strata_and_weights_are_preserved_within_a_resample(self):
        inv = ee.PlannedInventory([ee.PlannedCase('a','a','synthetic_en',1.),ee.PlannedCase('b','b','synthetic_de',3.)])
        e = ee.PrivateEvaluator(inv)
        for k, delta in [('a',2),('b',10)]:
            e.add('base',k,completed()); e.add('candidate',k,completed(delta))
        d = declaration();d['strata'] = ['synthetic_en','synthetic_de']
        for i in range(10): self.assertTrue(set(e.draw_resample(0,i)) == {'a','b'})
        result = e.paired_interval('gold_bytes_surviving_egress',d)
        self.assertTrue(result['point'] == result['low'] == result['high'] == 8)

    def test_populations_are_evaluated_separately_with_no_pooled_headline(self):
        a,b = paired(),ee.PrivateEvaluator(inventory())
        self.assertTrue(a.paired_interval('gold_bytes_surviving_egress',declaration())['point'] == -14/3)
        self.assertTrue(b.paired_interval('gold_bytes_surviving_egress',declaration()) == 'NOT_EVALUABLE')


class IntervalArithmeticTests(unittest.TestCase):
    def test_non_constant_deltas_match_hand_computed_quantiles(self):
        self.assertTrue(ee.quantile([-9.,-14/3,-14/3,4.],.25) == -9.)
        self.assertTrue(ee.quantile([-9.,-14/3,-14/3,4.],.75) == -14/3)

    def test_exhaustive_oracle_agrees_with_the_sampler(self):
        e = paired();d = declaration();d['resample_count'] = 4
        draws = [list(itertools.chain.from_iterable((['fixture_a','fixture_b'] if g == 0 else ['fixture_c']) for g in pair)) for pair in itertools.product(range(2), repeat=2)]
        expected = [-9.,-14/3,-14/3,4.]
        with patch.object(e,'draw_resample',side_effect=draws):
            result = e.paired_interval('gold_bytes_surviving_egress',d)
        self.assertTrue(result['point'] == -14/3 and result['low'] == expected[0] and result['high'] == expected[2])
        actual = {round(e._estimate('gold_bytes_surviving_egress',e.draw_resample(2,i)),8) for i in range(100)}
        self.assertTrue(actual == {round(x,8) for x in expected})

    def test_legacy_byte_helpers_overlap_adjacency_unicode(self):
        for spans, expected in [([(0,4),(2,8)],8), ([(0,4),(4,8)],8), ([(0,2),(4,8)],6), ([(0,len('äö'.encode())),(2,6)],6)]:
            self.assertTrue(ee.surviving_bytes(spans) == expected)
            self.assertTrue(ee.surviving_bytes(spans) == ee.interval_length(ee.merge_intervals(spans)))


class ExportTests(unittest.TestCase):
    def export(self, evaluator=None, custody=None):
        return (evaluator or paired()).export_receipt(golden(),custody or json.loads(CUSTODY.read_text()),declaration=declaration(),attestation_probe={'revision':'0'*40,'dirty':False})

    def test_local_membership_order_is_actually_stamped(self):
        r = self.export()
        self.assertTrue(r['local_membership_order_verified'] and r['local_membership_order_proof_method'] == 'private_membership_order_v1')
        self.assertTrue(r['gate_results']['producer_membership_order_proof'] == 'BLOCKED')
        self.assertTrue(r['gate_results']['build_attestation_clean_source'] == 'NOT_EVALUABLE')

    def test_wrong_local_order_fails_membership_proof(self):
        c = json.loads(CUSTODY.read_text());c['membership_order'].reverse()
        with self.assertRaises(ep.ReceiptRefused) as error: self.export(custody=c)
        self.assertTrue(error.exception.code == 'membership_order_proof_failed')

    def test_only_aggregates_leave_private_boundary(self):
        r = self.export(); text = json.dumps(r)
        self.assertTrue(all(k not in text for k in inventory().keys()))
        self.assertTrue('group_a' not in text and 'group_b' not in text)

    def test_unknown_fragment_export_retains_lower_bound_without_interval(self):
        e = ee.PrivateEvaluator(inventory())
        e.add('candidate','fixture_a',ee.DocRecord(outcome='UNKNOWN_EGRESS',gold_bytes=30,gold_bytes_surviving=8))
        r = self.export(evaluator=e)
        self.assertTrue(r['counts']['gold_bytes_surviving_egress'] == 8)
        self.assertTrue(r['intervals']['gold_bytes_surviving_egress'] == 'NOT_EVALUABLE')

    def test_unsupported_class_leaks_remain_in_totals(self):
        e = ee.PrivateEvaluator(inventory());e.add('candidate','fixture_a',completed(8,pii_class='custom:phone',region='de'))
        self.assertTrue(e.aggregate('candidate')['gold_bytes_surviving_egress'] == 8)
        self.assertTrue(e.class_cross_tab('candidate')[('custom:phone','de')] == 8)

    def test_private_records_have_no_value_repr(self):
        c = ee.PlannedCase('synthetic-private-canary','g','synthetic_en',1.)
        self.assertTrue(c.key not in repr(c))

    def test_evaluator_canary_no_output_or_file_writes(self):
        out,err = io.StringIO(),io.StringIO()
        with tempfile.TemporaryDirectory() as d, contextlib.redirect_stdout(out),contextlib.redirect_stderr(err),patch('builtins.open',side_effect=AssertionError('unexpected-file-open')):
            e = paired();e.finalize(); e.aggregate('candidate')
            try: e.add('base','synthetic-private-canary',completed())
            except ep.ReceiptRefused as error: self.assertTrue(str(error) in ep.REFUSAL_CODES)
            self.assertTrue(not list(Path(d).iterdir()))
        self.assertTrue(not out.getvalue() and not err.getvalue(), 'private-output-canary')
