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
        ee.PlannedCase('fixture_a','group_a','synthetic_en',1.,2,30),
        ee.PlannedCase('fixture_b','group_a','synthetic_en',1.,2,30),
        ee.PlannedCase('fixture_c','group_b','synthetic_en',2.,2,30),
    ])


def completed(leak=0, **kw):
    return ee.DocRecord(observed_metrics=ee.OBSERVED_METRICS, outcome='COMPLETED', gold_occurrences=2, gold_bytes=30, gold_bytes_surviving=leak, **kw)


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


class AuthoritativePlanTests(unittest.TestCase):
    def test_plan_bounds_and_record_conflicts(self):
        for field in ('gold_bytes', 'gold_occurrences'):
            for plan, record_value, observed in [(0,30,8),(2,30,3),(2,1,0),(2,3,0)]:
                with self.subTest(field=field, plan=plan, record_value=record_value):
                    inv = ee.PlannedInventory([ee.PlannedCase('a','g','synthetic_en',1., **{field:plan})])
                    metric = 'gold_bytes_surviving_egress' if field == 'gold_bytes' else 'gold_occurrences_surviving_egress'
                    value_field = ee.METRIC_FIELDS[metric]
                    record = ee.DocRecord(outcome='COMPLETED', observed_metrics=frozenset((metric,)), **{field:record_value,value_field:observed})
                    with self.assertRaises(ep.ReceiptRefused, msg='plan-record-bound'):
                        ee.PrivateEvaluator(inv).add('candidate','a',record)

    def test_plan_bounds_without_record_denominators(self):
        inv = ee.PlannedInventory([ee.PlannedCase('a','g','synthetic_en',1.,2,8)])
        for full,partial,unknown,bytes_ in [(2,1,0,0),(1,1,1,0),(0,0,3,0),(0,0,0,9)]:
            record = ee.DocRecord(outcome='COMPLETED', observed_metrics=ee.OBSERVED_METRICS,
                gold_occurrences_surviving=full, gold_occurrences_partially_surviving=partial,
                gold_occurrences_attribution_not_measured=unknown, gold_bytes_surviving=bytes_)
            with self.assertRaises(ep.ReceiptRefused, msg='plan-crossfield-bound'):
                ee.PrivateEvaluator(inv).add('candidate','a',record)
        e = ee.PrivateEvaluator(inv)
        try:
            e.add('candidate','a',ee.DocRecord(outcome='COMPLETED', observed_metrics=ee.OBSERVED_METRICS,
                gold_occurrences_surviving=1,gold_occurrences_partially_surviving=1,gold_bytes_surviving=8))
        except ep.ReceiptRefused:
            self.fail('plan-exact-boundary')
        self.assertTrue(e.aggregate('base') == {'gold_occurrences_planned':2,'gold_bytes_planned':8}, 'missing-record-plan')


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
        e = ee.PrivateEvaluator(inventory());e.add('base','fixture_a',ee.DocRecord(observed_metrics=frozenset(('gold_bytes_surviving_egress','gold_occurrences_partially_surviving_egress')),outcome='UNKNOWN_EGRESS',gold_bytes=30,gold_bytes_surviving=8,gold_occurrences=2,gold_occurrences_partially_surviving=1))
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
        inv = ee.PlannedInventory([ee.PlannedCase('a','a','synthetic_en',1.,2,30),ee.PlannedCase('b','b','synthetic_de',3.)])
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

    def test_unknown_predicate_truth_table_and_mixed_coverage(self):
        metrics = ('gold_bytes_surviving_egress','gold_occurrences_surviving_egress','gold_occurrences_partially_surviving_egress')
        for values in itertools.product((None,0,1), repeat=3):
            # Attribution is deliberately observed, but cannot prove survival zero.
            observed = frozenset(m for m,v in zip(metrics,values) if v is not None) | {'gold_occurrences_attribution_not_measured'}
            kw = {ee.METRIC_FIELDS[m]:v for m,v in zip(metrics,values) if v is not None}
            r0 = ee.DocRecord(outcome='UNKNOWN_EGRESS', observed_metrics=observed, gold_occurrences=2, gold_bytes=30,
                gold_occurrences_attribution_not_measured=int(sum(v or 0 for v in values[1:]) < 2), **kw)
            known = any(v == 1 for v in values) or all(v == 0 for v in values)
            for mixed in (False, True):
                e = ee.PrivateEvaluator(inventory()); e.add('candidate','fixture_a',r0)
                if mixed: e.add('candidate','fixture_b',ee.DocRecord(outcome='UNKNOWN_EGRESS'))
                r = self.export(evaluator=e); m = 'unknown_egress_lower_bound_cases'
                self.assertTrue((m in r['counts']) == known, 'unknown-predicate-availability')
                if known:
                    self.assertTrue(r['counts'][m] == int(any(v == 1 for v in values)), 'unknown-predicate-count')
                self.assertTrue(r['derivations'][m] == ('observed_subset_lower_bound' if known and mixed else 'private_authored_records' if known else 'not_measured'), 'unknown-predicate-grade')
                self.assertTrue(r['gate_results']['unknown_egress_lower_bound'] == ('PASS' if known and not mixed else 'NOT_EVALUABLE'), 'unknown-predicate-gate')
                self.assertTrue(all(v == 'NOT_EVALUABLE' for v in r['intervals'].values()), 'unknown-predicate-interval')

    def test_planned_counts_have_no_paired_estimand(self):
        for metric in ee.PLAN_METRICS:
            self.assertTrue(paired().paired_interval(metric,declaration()) == 'NOT_EVALUABLE', 'planned-no-estimand')
        r = self.export()
        for metric in ee.PLAN_METRICS:
            self.assertTrue(r['intervals'].get(metric,'NOT_EVALUABLE') == 'NOT_EVALUABLE', 'planned-no-estimand')

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
        e.add('candidate','fixture_a',ee.DocRecord(observed_metrics=frozenset(('gold_bytes_surviving_egress','gold_occurrences_partially_surviving_egress')),outcome='UNKNOWN_EGRESS',gold_bytes=30,gold_bytes_surviving=8))
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
        from test_evidence_protocol import private_child
        def exercise():
            e = paired(); e.finalize(); e.aggregate('candidate')
            self.export(evaluator=e)
            try: e.add('base','synthetic-private-canary',completed())
            except ep.ReceiptRefused as error: assert str(error) in ep.REFUSAL_CODES, 'closed-exception'
            custody = json.loads(CUSTODY.read_text()); custody['membership_order'].reverse()
            try: self.export(custody=custody)
            except ep.ReceiptRefused as error: assert str(error) in ep.REFUSAL_CODES, 'closed-exception'
        status, out, err, files = private_child(exercise)
        self.assertTrue(status != 2 << 8, 'evaluator-closed-exception')
        self.assertTrue(status == 0, 'evaluator-child-success')
        self.assertTrue(not out, 'evaluator-stdout-boundary')
        self.assertTrue(not err, 'evaluator-stderr-boundary')
        self.assertTrue(not files, 'evaluator-file-boundary')

    @patch.object(ep, 'validate_receipt', side_effect=lambda r, *args, **kwargs: r)
    def test_availability_identity_and_full_derivation_map(self, _validator):
        # Inspect the producer's declarations independently; binding has separate consumer tests.
        e = ee.PrivateEvaluator(inventory())
        for state in ['NOT_STARTED','UNKNOWN_EGRESS','FAILED_CLOSED_NO_EGRESS']:
            e = ee.PrivateEvaluator(inventory()); e.add('candidate','fixture_a',ee.DocRecord(outcome=state))
            r = self.export(evaluator=e)
            self.assertTrue(set(r['counts']) == ee.PLAN_METRICS, 'missing-observation-counts')
            self.assertTrue(r['counts']['gold_bytes_planned'] == 90, 'inventory-plan-count')
            expected = dict.fromkeys(ep.METRIC_IDS, 'not_measured')
            for m in ee.PLAN_METRICS: expected[m] = 'planned_inventory'
            self.assertTrue(r['derivations'] == expected, 'evaluator-declared-grades')
        r = self.export()
        self.assertTrue(r['route_id'] == 'evaluator.private.v1' and r['cell_id'] == 'synthetic.evaluator.v1' and r['policy_identity'] == 'authored.records.v1', 'evaluator-identity')
        self.assertTrue(r['route_status']['mcp.rmcp.duplex.v1'] == 'NOT_IMPLEMENTED', 'evaluator-no-route-claim')
        expected = dict.fromkeys(ep.METRIC_IDS, 'not_measured')
        for m in ee.PLAN_METRICS: expected[m] = 'planned_inventory'
        for m in ee.OBSERVED_METRICS: expected[m] = 'private_authored_records'
        self.assertTrue(r['derivations'] == expected, 'evaluator-declared-grades')
        self.assertTrue(r['counts']['false_positive_bytes'] == 0 and 'leaf_restore_exact' not in r['counts'], 'observed-zero-no-restore')
        self.assertTrue(r['gate_results']['unknown_egress_lower_bound'] == 'NOT_EVALUABLE' and r['gate_results']['rejection_is_not_protection'] == 'NOT_EVALUABLE', 'unexercised-evaluator-gates')
        template = golden(); template['derivations'] = dict.fromkeys(ep.METRIC_IDS,'route_native')
        r = paired().export_receipt(template,json.loads(CUSTODY.read_text()),attestation_probe={'revision':'0'*40,'dirty':False})
        self.assertTrue(r['derivations'] == expected and r['route_id'] == 'evaluator.private.v1', 'caller-cannot-misattribute')

    def test_nonzero_authored_metrics_and_subset_grade(self):
        e = ee.PrivateEvaluator(inventory())
        e.add('candidate','fixture_a',completed(gold_occurrences_attribution_not_measured=1,false_positive_occurrences=2,false_positive_bytes=7))
        r = self.export(evaluator=e)
        for m,n in [('gold_occurrences_attribution_not_measured',1),('false_positive_occurrences',2),('false_positive_bytes',7)]:
            self.assertTrue(r['counts'][m] == n and r['derivations'][m] == 'observed_subset_lower_bound', 'nonzero-authored-subset')
        self.assertTrue(all(v == 'NOT_EVALUABLE' for v in r['intervals'].values()), 'subset-not-exact')

    def test_every_authored_count_has_a_nonzero_driver_and_plan_is_independent(self):
        e = ee.PrivateEvaluator(inventory())
        e.add('candidate','fixture_a',completed(8,gold_occurrences_surviving=1,gold_occurrences_partially_surviving=1,false_positive_occurrences=2,false_positive_bytes=7))
        a = e.aggregate('candidate')
        for m in ['gold_occurrences_surviving_egress','gold_occurrences_partially_surviving_egress','gold_bytes_surviving_egress','false_positive_occurrences','false_positive_bytes']:
            self.assertTrue(a[m] > 0, 'authored-nonzero-driver')
        missing = ee.PrivateEvaluator(inventory())
        r = self.export(evaluator=missing)
        self.assertTrue(r['counts'] == {'gold_occurrences_planned':6,'gold_bytes_planned':90}, 'both-arms-missing-plan')
        no_plan = ee.PlannedInventory([ee.PlannedCase('fixture_a','g','synthetic_en',1.)])
        e = ee.PrivateEvaluator(no_plan); e.add('candidate','fixture_a',completed())
        self.assertTrue(not ee.PLAN_METRICS.intersection(e.aggregate('candidate')), 'record-not-plan-authority')

    def test_empty_or_missing_membership_is_refused(self):
        for custody in [{}, {'membership_order':[]}]:
            c = json.loads(CUSTODY.read_text()); c.pop('membership_order'); c.update(custody)
            with self.assertRaises(ep.ReceiptRefused): self.export(evaluator=ee.PrivateEvaluator(ee.PlannedInventory([])),custody=c)
        c = json.loads(CUSTODY.read_text()); c.pop('membership_order')
        with self.assertRaises(ep.ReceiptRefused): self.export(custody=c)


class CommitmentConsumptionTests(unittest.TestCase):
    def test_evaluator_loads_committed_class_table(self):
        with patch.object(ep, 'load_class_commitments', side_effect=ep.ClassCommitmentRefused('class_commitment_invalid')) as loader:
            with self.assertRaises(ep.ClassCommitmentRefused): ee.PrivateEvaluator(inventory())
        self.assertTrue(loader.call_count == 1)


def run_rust_mutation_proof():
    """Opt-in under the live machine lease; touch only the committed test file."""
    import os
    import subprocess
    import re
    root = Path(__file__).resolve().parents[2]
    source_path = root/'crates/gaze-mcp-rmcp/tests/evidence_route.rs'
    source = source_path.read_text()
    cargo = os.environ.get('GAZE_EVIDENCE_CARGO') or subprocess.check_output(
        ['rustup', 'which', '--toolchain', '1.96.0', 'cargo'], text=True).strip()
    # Each row executes just its declared target, not an inferred whole-suite kill set.
    cases = [
        ('MUT-NO-PAYLOAD', [('r.is_error != Some(true) => "COMPLETED"','(r.is_error == Some(true) || r.is_error != Some(true)) => "COMPLETED"')], 'undeclared_carrier_has_positive_no_payload_and_unobserved_leaves'),
        ('MUT-VOCAB-RUST', [('const METRIC_IDS: &[&str] = &[','const METRIC_IDS: &[&str] = &["mutation-only",')], 'vocabularies_match_committed_artifact'),
        ('MUT-RUST-WALKER', [('fn walk(node: &Value, path: &str, rules: &Value) -> bool {','fn walk(node: &Value, path: &str, rules: &Value) -> bool { if path != "$" {return true;}')], 'rust_walker_rejects_nested_paths_types_and_forbidden_counts'),
        ('MUT-OBSERVER-RAW-GAP', [('            mode,\n            session: session.clone(),','            mode: if mode == 0 {1} else {mode},\n            session: session.clone(),')], 'protected_success_and_golden_receipt'),
        ('MUT-EMITTER-FORBIDDEN-COUNT', [('    assert!(receipt_allowlisted(&r), "emitter-conformance");','    r["counts"]["protection_trace_items"] = json!(0);\n    assert!(receipt_allowlisted(&r), "emitter-conformance");')], 'protected_success_and_golden_receipt'),
        ('MUT-FAILED-FINISH', [('async fn finish_call(&self, _: CallHandle, _: SnapshotRef)', 'async fn finish_call(&self, handle: CallHandle, _: SnapshotRef)'), ('if self.fail_finish {','if self.fail_finish {\n            self.fail_call(handle, FailureReason::Other { message: "synthetic".into() }).await?;')], 'response_conflict_rolls_back_but_failed_finish_retains_mappings'),
        ('MUT-ROLLBACK', [('if self.mode == 3 && !ctx.manifest.spans.is_empty() {','if self.mode == 3 && !ctx.manifest.spans.is_empty() {\n            must(self.session.tokenize(&PiiClass::Email, FRESH));')], 'response_conflict_rolls_back_but_failed_finish_retains_mappings'),
        ('MUT-LEAK-COMPUTED', [('self.add("gold_occurrences_surviving_egress", 1);','self.add("gold_occurrences_surviving_egress", 0);')], 'controlled_four_slot_occurrence_oracle'),
        ('MUT-OCCURRENCE-ORACLE', [('if observed.matches("[[g]]").count() != 1 || observed.matches("[[/g]]").count() != 1 {\n            return Verdict::Unknown;','if observed.matches("[[g]]").count() != 1 || observed.matches("[[/g]]").count() != 1 {\n            return Verdict::Full;')], 'controlled_four_slot_occurrence_oracle'),
        ('MUT-STRING-BYTES', [('if r.text == expected {','if must(serde_json::from_str::<Value>(&r.text)) == must(serde_json::from_str::<Value>(expected)) {')], 'json_text_string_bytes_are_stricter_than_semantic_equality'),
        ('MUT-RAW-VALUE-SWAP', [('self.add("egress_raw_value_mismatches", 1);','self.add("egress_raw_value_mismatches", 0);')], 'integrity_analogues_have_independent_nonzero_falsifiers'),
        ('MUT-TOKEN-CORRUPTION', [('self.add("egress_token_restore_failures", 1);','self.add("egress_token_restore_failures", 0);')], 'integrity_analogues_have_independent_nonzero_falsifiers'),
        ('MUT-CANARY-RUST', [('    let wire = must(serde_json::to_string(&r));','    eprintln!("{EMAIL}");\n    let wire = must(serde_json::to_string(&r));')], 'private_failure_canary_captures_stdout_stderr_and_files'),
    ]
    cases.extend([
        ('MUT-ROUTE-AVAILABILITY', [('COUNTING_GRADES.contains(&grade) && (!c.0.contains_key(m) || incomplete)', 'false')], 'missing_measurements_and_ambiguous_only_gates'),
        ('MUT-EXTRA-SURFACES', [('if no_payload_surfaces(r)', 'if true')], 'no_payload_classifier_rejects_extra_surfaces'),
        ('MUT-GATE-INTEGRITY', [('    assert!(receipt_allowlisted(&r), "emitter-conformance");', '    r["gate_results"]["egress_integrity_analogues"] = json!("PASS");\n    assert!(receipt_allowlisted(&r), "emitter-conformance");')], 'integrity_analogues_have_independent_nonzero_falsifiers'),
        ('MUT-GATE-STRING', [('    assert!(receipt_allowlisted(&r), "emitter-conformance");', '    r["gate_results"]["string_byte_reversibility"] = json!("PASS");\n    assert!(receipt_allowlisted(&r), "emitter-conformance");')], 'json_text_string_bytes_are_stricter_than_semantic_equality'),
        ('MUT-GATE-GOLD', [('    assert!(receipt_allowlisted(&r), "emitter-conformance");', '    r["gate_results"]["gold_survival_oracle"] = json!("PASS");\n    assert!(receipt_allowlisted(&r), "emitter-conformance");')], 'missing_measurements_and_ambiguous_only_gates'),
        ('MUT-GATE-FP', [('    assert!(receipt_allowlisted(&r), "emitter-conformance");', '    r["gate_results"]["false_positive_negative_control"] = json!("PASS");\n    assert!(receipt_allowlisted(&r), "emitter-conformance");')], 'missing_measurements_and_ambiguous_only_gates'),
    ])
    cases.extend([('MUT-RAW-COVERAGE-COUNT', [('"egress_raw_value_mismatches" => c.1.get("raw_compared") != c.1.get("restore"),', '"egress_raw_value_mismatches" => false,')], 'r2_mixed_raw_comparison_coverage'), ('MUT-RAW-COVERAGE-GATE', [('\n        c.1.get("raw_compared").copied().unwrap_or(0) != restores', '\n        false')], 'r2_mixed_raw_comparison_coverage'), ('MUT-NEGATIVE-COVERAGE-COUNT', [('\n                c.1.get("negative_compared") != c.1.get("negative")', '\n                false')], 'r2_negative_predicate_coverage'), ('MUT-NEGATIVE-COVERAGE-GATE', [('\n        c.1.get("negative_compared") != c.1.get("negative")', '\n        false')], 'r2_negative_predicate_coverage'), ('MUT-PRODUCER-GRADE-RUST', [('allowed_derivation(route, m, text(g))', '(allowed_derivation(route, m, text(g)) || true)')], 'r2_producer_grade_and_identity_binding'), ('MUT-PRODUCER-CELL-RUST', [('    if !match route {', '    if false && !match route {')], 'r2_producer_grade_and_identity_binding'), ('MUT-PLAN-INTERVAL-RUST', [(') && v != "NOT_EVALUABLE"', ') && false && v != "NOT_EVALUABLE"')], 'r2_planned_interval_refused')])
    env = os.environ.copy()
    env.update(RUSTUP_TOOLCHAIN='1.96.0', RUSTC=str(Path(cargo).with_name('rustc')), RUSTDOC=str(Path(cargo).with_name('rustdoc')))
    markers = {
        'MUT-NO-PAYLOAD':'positive-no-payload', 'MUT-VOCAB-RUST':'vocabulary-mirror',
        'MUT-RUST-WALKER':'nested-path-type-refusal', 'MUT-OBSERVER-RAW-GAP':'completed-single-carrier',
        'MUT-EMITTER-FORBIDDEN-COUNT':'emitter-conformance', 'MUT-FAILED-FINISH':'failed-finish-retains-committed',
        'MUT-ROLLBACK':'rollback-no-losing-mappings', 'MUT-LEAK-COMPUTED':'four-exact-verdicts',
        'MUT-OCCURRENCE-ORACLE':'four-exact-verdicts', 'MUT-STRING-BYTES':'string-bytes-discriminate',
        'MUT-RAW-VALUE-SWAP':'independent-slot-swap', 'MUT-TOKEN-CORRUPTION':'restore-error-counted',
        'MUT-CANARY-RUST':'private-output-canary',
    }
    markers.update({'MUT-ROUTE-AVAILABILITY':'absent-measurements', 'MUT-EXTRA-SURFACES':'extra-surface-unknown', 'MUT-GATE-INTEGRITY':'swap-gate-fail', 'MUT-GATE-STRING':'string-gate-fail', 'MUT-GATE-GOLD':'unperformed-gate', 'MUT-GATE-FP':'unperformed-gate'})
    markers.update({'MUT-RAW-COVERAGE-COUNT': 'raw-incomplete-count', 'MUT-RAW-COVERAGE-GATE': 'raw-coverage-gate', 'MUT-NEGATIVE-COVERAGE-COUNT': 'negative-incomplete-count', 'MUT-NEGATIVE-COVERAGE-GATE': 'negative-coverage-gate', 'MUT-PRODUCER-GRADE-RUST': 'producer-metric-grade', 'MUT-PRODUCER-CELL-RUST': 'producer-cell-policy', 'MUT-PLAN-INTERVAL-RUST': 'planned-interval-refused'})
    results = []
    for identifier,edits,target in cases:
        mutated = source
        for old,new in edits:
            if old not in mutated: raise AssertionError('mutation-site-missing')
            mutated = mutated.replace(old,new)
        try:
            source_path.write_text(mutated)
            result = subprocess.run([cargo,'test','--offline','--locked','-p','gaze-mcp-rmcp','--test','evidence_route','--','--exact',target,'--test-threads=1'],cwd=root,env=env,capture_output=True)
            output = result.stdout + result.stderr
            # Compilation failure, panic in an unrelated test, or zero collection is no proof.
            killed = result.returncode != 0 and b'running 1 test' in output and ('test '+target+' ... FAILED').encode() in output and markers[identifier].encode() in output
            row = dict(id=identifier,killed=killed,failure_marker=markers[identifier],tests_run=1 if b'running 1 test' in output else 0,kill_set=[target] if killed else [])
            results.append(row)
            print(json.dumps(row,sort_keys=True),flush=True)
        finally:
            source_path.write_text(source)
        if not killed: break
    assert source_path.read_text() == source, 'mutation-restoration'
    return results


if __name__ == '__main__':
    import sys
    if '--rust-mutation-proof' in sys.argv:
        rows = run_rust_mutation_proof()
        raise SystemExit(not all(r['killed'] for r in rows))
    unittest.main()
