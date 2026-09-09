import contextlib
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import evidence_protocol as ep

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = Path(__file__).parent / 'fixtures/evidence/mcp_route_v1.golden.json'
CUSTODY = Path(__file__).parent / 'fixtures/evidence/custody_record_v1.json'
CLASS_TABLE = ROOT / 'docs/reference/benchmarks/class-commitments-v1.json'


def golden():
    return json.loads(FIXTURE.read_text())['receipt']


def declaration():
    return dict(confidence_level=.5, resample_count=16, seed=4, strata=['synthetic_en'], weighting='inventory_group', multiplicity_treatment='synthetic_none', coverage_target=.8, acceptance_limit=.1)


def interval():
    return dict(point=0., low=0., high=1., method_id='grouped_paired_percentile_v1', conditional=False, basis='paired_completed')


class ReceiptTests(unittest.TestCase):
    def setUp(self):
        self.r = golden()

    def refuse(self, code):
        with self.assertRaises(ep.ReceiptRefused) as ctx:
            ep.validate_structure(self.r)
        self.assertTrue(ctx.exception.code == code, 'refusal-code')

    def test_golden_receipt_with_all_non_counting_derivations_is_accepted(self):
        self.assertTrue(ep.validate_structure(self.r) == self.r)

    def test_unknown_top_level_key_is_refused(self):
        self.r['clean_text'] = 'private'
        self.refuse('unknown_path')

    def test_unknown_nested_key_is_refused(self):
        self.r['not_measured']['extra'] = []
        self.refuse('value_out_of_vocabulary')

    def candidate(self):
        self.r['arm_id'] = 'candidate'
        self.r['asymmetric_outcome_table'] = {a:{b:int(a == b == 'COMPLETED') for b in ep.OUTCOME_STATES} for a in ep.OUTCOME_STATES}

    def test_unknown_key_inside_asymmetric_table_is_refused(self):
        self.candidate()
        self.r['asymmetric_outcome_table']['COMPLETED']['extra'] = 0
        self.refuse('value_out_of_vocabulary')

    def test_wrong_leaf_type_is_refused(self):
        self.r['planned_case_count'] = '1'
        self.refuse('wrong_type')

    def test_bool_where_int_required_is_refused(self):
        self.r['planned_case_count'] = True
        self.refuse('wrong_type')

    def test_nested_container_types_even_empty_are_checked(self):
        for key in ('outcomes','counts','derivations','not_measured','analysis_declaration','intervals'):
            with self.subTest(key=key):
                self.r = golden(); self.r[key] = []
                self.refuse('wrong_type')
        self.r = golden(); self.r['not_measured']['metrics'] = {}
        self.refuse('wrong_type')

    def test_non_finite_interval_bound_is_refused(self):
        self.r['intervals']['gold_bytes_surviving_egress'] = interval()
        self.r['intervals']['gold_bytes_surviving_egress']['low'] = float('nan')
        self.refuse('non_finite_number')

    def test_unknown_metric_id_in_counts_is_refused(self):
        self.r['counts']['extra'] = 1
        self.refuse('value_out_of_vocabulary')

    def test_unknown_gate_id_in_gate_results_is_refused(self):
        self.r['gate_results']['extra'] = 'PASS'
        self.refuse('value_out_of_vocabulary')

    def test_free_form_error_code_is_refused(self):
        self.r['error_codes']['private'] = 1
        self.refuse('value_out_of_vocabulary')

    def test_unknown_stratum_in_declaration_is_refused(self):
        self.r['analysis_declaration'] = declaration()
        self.r['analysis_declaration']['strata'] = ['private']
        self.refuse('value_out_of_vocabulary')

    def test_unknown_derivation_value_is_refused(self):
        self.r['derivations']['tool_invocations'] = 'private'
        self.refuse('value_out_of_vocabulary')

    def test_readable_population_handle_in_custody_is_still_refused(self):
        self.r['population_handle'] = 'readable-fixture'
        with self.assertRaises(ep.ReceiptRefused) as c:
            ep.validate_receipt(self.r, {'population_handle':'readable-fixture'}, attestation_probe={'revision':'0'*40,'dirty':False})
        self.assertTrue(c.exception.code == 'handle_shape_invalid')

    def test_all_five_states_are_mandatory(self):
        del self.r['outcomes']['NOT_STARTED']
        self.refuse('outcome_identity_violation')

    def test_planned_count_must_equal_outcome_sum(self):
        self.r['planned_case_count'] = 2
        self.refuse('outcome_identity_violation')

    def test_candidate_margin_is_checked(self):
        self.candidate(); self.r['asymmetric_outcome_table']['COMPLETED']['COMPLETED'] = 2
        self.refuse('outcome_identity_violation')

    def test_asymmetric_table_forbidden_on_base_arm(self):
        self.candidate(); self.r['arm_id'] = 'base'
        self.refuse('outcome_identity_violation')

    def test_non_counting_grades_refuse_counts(self):
        for grade in ep.DERIVATIONS - ep.COUNTING_GRADES:
            self.r = golden()
            metric = next(k for k,v in self.r['derivations'].items() if v == grade)
            self.r['counts'][metric] = 0
            self.refuse('counted_non_measurement')

    def test_actual_measurement_requires_count(self):
        del self.r['counts']['tool_invocations']
        self.refuse('uncounted_measurable_metric')

    def test_metric_without_declared_derivation_is_refused(self):
        del self.r['derivations']['protection_trace_items']
        self.refuse('derivation_coverage_incomplete')

    def test_blocked_gate_absent_from_blocked_gates_is_refused(self):
        self.r['not_measured']['blocked_gates'].pop()
        self.refuse('derivation_conflict')

    def test_not_measured_inventory_is_exact(self):
        self.r['not_measured']['metrics'].pop()
        self.refuse('derivation_conflict')

    def test_gate_coverage_must_be_complete(self):
        del self.r['gate_results']['vocabulary_closure']
        self.refuse('gate_coverage_incomplete')

    def test_emitted_receipt_carrying_a_stamped_key_is_refused(self):
        for key in ep.STAMPED_KEYS:
            self.r = golden(); self.r[key] = True
            self.refuse('stamped_key_in_emitted_receipt')

    def test_missing_claim_scope_is_refused(self):
        del self.r['claim_scope']
        self.refuse('missing_mandatory_key')

    def test_non_t1_claim_scope_is_refused(self):
        self.r['claim_scope'] = 'full_corpus'
        self.refuse('value_out_of_vocabulary')

    def test_interval_without_declaration_is_refused(self):
        for missing in [None, *ep.DECLARATION_FIELDS]:
            self.r = golden(); self.r['intervals']['gold_bytes_surviving_egress'] = interval()
            if missing is not None:
                self.r['analysis_declaration'] = declaration(); del self.r['analysis_declaration'][missing]
            self.refuse('interval_without_declaration')

    def test_unknown_with_no_fragment_still_blocks_exact_interval(self):
        self.r['analysis_declaration'] = declaration()
        self.r['outcomes']['COMPLETED'] = 0; self.r['outcomes']['UNKNOWN_EGRESS'] = 1
        self.r['intervals']['gold_bytes_surviving_egress'] = interval()
        self.refuse('lower_bound_reported_as_exact')

    def test_full_cell_requires_complete_known_outcomes(self):
        self.r['analysis_declaration'] = declaration()
        self.r['intervals']['tool_invocations'] = interval()
        self.r['intervals']['tool_invocations'].update(basis='full_cell', conditional=True)
        self.refuse('conditional_reported_as_full_cell')

    def test_observer_coverage_is_not_inferred_for_unobserved_leaves(self):
        self.r['counts']['observer_leaves_unobserved'] = 1
        self.refuse('observer_coverage_incomplete')

    def test_duplicate_json_key_is_refused(self):
        with self.assertRaises(ep.ReceiptRefused) as c:
            ep.loads_receipt('{"counts":{},"counts":{}}')
        self.assertTrue(c.exception.code == 'duplicate_json_key')


class StampTests(unittest.TestCase):
    def validate(self, **kw):
        return ep.validate_receipt(golden(), json.loads(CUSTODY.read_text()), **kw)

    def test_external_receipt_never_stamps_producer_or_local_membership(self):
        r = self.validate(attestation_probe={'revision':'0'*40,'dirty':False})
        self.assertFalse('local_membership_order_verified' in r)
        self.assertTrue(r['gate_results']['producer_membership_order_proof'] == 'BLOCKED')
        self.assertTrue(r['gate_results']['build_attestation_clean_source'] == 'NOT_EVALUABLE')

    def test_placeholder_source_revision_is_refused(self):
        with self.assertRaises(ep.ReceiptRefused) as c:
            self.validate(attestation_probe={'revision':'placeholder','dirty':False})
        self.assertTrue(c.exception.code == 'attestation_shape_invalid')

    def test_repo_root_is_required_outside_fixture_probe(self):
        with self.assertRaises(ep.ReceiptRefused): self.validate()

    def test_clean_probe_cannot_override_live_dirty_tree(self):
        with patch.object(ep.legacy, 'git_metadata', return_value={'revision':'1'*40,'dirty':True}) as helper:
            r = self.validate(repo_root=ROOT, attestation_probe={'revision':'0'*40,'dirty':False})
        self.assertTrue(helper.call_count == 1)
        self.assertTrue(r['gate_results']['build_attestation_clean_source'] == 'FAIL')

    def test_live_repository_attestation_binds_shape_and_gate(self):
        r = self.validate(repo_root=ROOT)
        stamp = r['build_attestation']
        self.assertTrue(len(stamp['source_revision']) == 40)
        self.assertTrue(r['gate_results']['build_attestation_clean_source'] == ('FAIL' if stamp['dirty'] else 'PASS'))


class AggregationTests(unittest.TestCase):
    def test_all_blocked_is_not_evaluable_not_pass(self):
        self.assertTrue(ep.overall(dict.fromkeys(ep.GATE_IDS, 'BLOCKED')) == 'NOT_EVALUABLE')

    def test_fail_dominates_blocked(self):
        gates = dict.fromkeys(ep.GATE_IDS, 'BLOCKED'); gates['vocabulary_closure'] = 'FAIL'
        self.assertTrue(ep.overall(gates) == 'FAIL')

    def test_all_pass_is_pass(self):
        self.assertTrue(ep.overall(dict.fromkeys(ep.GATE_IDS, 'PASS')) == 'PASS')


class ClassCommitmentTests(unittest.TestCase):
    def test_fixture_loads(self):
        self.assertTrue(len(ep.load_class_commitments(CLASS_TABLE)) == 2)

    def bad(self, mutate):
        data = json.loads(CLASS_TABLE.read_text()); mutate(data)
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)/'fixture.json'; p.write_text(json.dumps(data))
            with self.assertRaises(ep.ClassCommitmentRefused): ep.load_class_commitments(p)

    def test_unknown_label_region_pair_is_a_load_error(self):
        self.bad(lambda d: d['fixture_rows'][0].update(region='unknown'))

    def test_missing_mandatory_row_field_is_a_load_error(self):
        self.bad(lambda d: d['fixture_rows'][0].pop('rationale'))

    def test_duplicate_label_region_pair_is_a_load_error(self):
        self.bad(lambda d: d['fixture_rows'].append(d['fixture_rows'][0]))

    def test_untrusted_schema_cannot_extend_vocabulary(self):
        def mutate(d):
            d['schema']['label_regions']['UNKNOWN'] = ['unknown']
            d['fixture_rows'][0].update(gold_label='UNKNOWN', region='unknown')
        self.bad(mutate)


class VocabularyMirrorTests(unittest.TestCase):
    def test_python_vocabularies_equal_committed_artifact(self):
        committed = json.loads(FIXTURE.read_text())['vocabularies']
        self.assertTrue(set(committed) == set(ep.VOCABULARIES))
        for key, values in ep.VOCABULARIES.items():
            self.assertTrue(set(committed[key]) == set(values), 'vocabulary-mirror')


class CanaryTests(unittest.TestCase):
    def test_failures_do_not_emit_private_values_or_files(self):
        secret = 'synthetic-private-canary'; token = '<synthetic-canary_1>'
        stdout, stderr = io.StringIO(), io.StringIO()
        with tempfile.TemporaryDirectory() as d, contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            before = set(Path(d).iterdir())
            bad = golden(); bad['not_measured']['extra'] = secret
            calls = [lambda: ep.validate_structure(golden()), lambda: ep.validate_structure(bad),
                     lambda: ep.loads_receipt(secret * 30000), lambda: ep.loads_receipt('{"x":"'+secret),
                     lambda: ep.ReceiptRefused(secret), lambda: ep.ReceiptRefused({'x':token})]
            for call in calls:
                try: call()
                except ep.ReceiptRefused as exc:
                    self.assertTrue(str(exc) in ep.REFUSAL_CODES, 'closed-exception')
            self.assertTrue(set(Path(d).iterdir()) == before, 'no-private-files')
        self.assertTrue(secret not in stdout.getvalue()+stderr.getvalue() and token not in stdout.getvalue()+stderr.getvalue(), 'private-output-canary')


class DirectBoundaryTests(unittest.TestCase):
    def test_closed_dynamic_map_keys(self):
        for path in ('$.counts','$.gate_results'):
            with self.assertRaises(ep.ReceiptRefused): ep.walk({'private':0},path)

    def test_empty_container_types(self):
        with self.assertRaises(ep.ReceiptRefused): ep.walk([], '$.counts')

    def test_closed_dynamic_values(self):
        for value,path in [('private','$.derivations.*'),('private','$.analysis_declaration.strata.[]')]:
            with self.assertRaises(ep.ReceiptRefused): ep.walk(value,path)


def run_mutation_proof():
    """Explicit opt-in; in-memory mutants, one mechanism-specific target per run."""
    import inspect
    import textwrap
    import evidence_eval as ee
    import test_evidence_eval  # noqa: F401
    mutations = [
        ('MUT-PATH-TOP',ep,'validate_structure',[('    walk(r)','    pass')],'test_evidence_protocol.ReceiptTests.test_unknown_top_level_key_is_refused'),
        ('MUT-PATH-NESTED',ep,'walk',[("    require(path in RECEIPT_PATHS", "    if path != '$': return\n    require(path in RECEIPT_PATHS")],'test_evidence_protocol.ReceiptTests.test_unknown_nested_key_is_refused'),
        ('MUT-LEAF-TYPES',ep,'walk',[("    if kind == 'nullable_object'", "    if kind not in ('object','nullable_object','interval','array'): return\n    if kind == 'nullable_object'")],'test_evidence_protocol.ReceiptTests.test_bool_where_int_required_is_refused'),
        ('MUT-VOCAB-CLOSURE-MAPS',ep,'walk',[("require(key in allowed, 'value_out_of_vocabulary')","pass")],'test_evidence_protocol.DirectBoundaryTests.test_closed_dynamic_map_keys'),
        ('MUT-VOCAB-CLOSURE-VALUES',ep,'walk',[("require(node in allowed, 'value_out_of_vocabulary')","pass")],'test_evidence_protocol.DirectBoundaryTests.test_closed_dynamic_values'),
        ('MUT-HANDLE-OPACITY',ep,'walk',[("require(type(node) is str and re.fullmatch('[0-9a-f]{32,64}', node) is not None, 'handle_shape_invalid')","pass")],'test_evidence_protocol.ReceiptTests.test_readable_population_handle_in_custody_is_still_refused'),
        ('MUT-ATTESTATION-BINDING',ep,'validate_receipt',[("require(type(meta['revision']) is str and re.fullmatch('[0-9a-f]{40}', meta['revision']) is not None and type(meta['dirty']) is bool, 'attestation_shape_invalid')","pass")],'test_evidence_protocol.StampTests.test_placeholder_source_revision_is_refused'),
        ('MUT-OUTCOME-IDENTITY',ep,'check_outcome_identities',[("require(sum(o.values()) == r['planned_case_count'], 'outcome_identity_violation')","pass")],'test_evidence_protocol.ReceiptTests.test_planned_count_must_equal_outcome_sum'),
        ('MUT-AGGREGATION',ep,'overall',[("if any(v != 'PASS' for v in gates.values()):","if False:")],'test_evidence_protocol.AggregationTests.test_all_blocked_is_not_evaluable_not_pass'),
        ('MUT-VALIDATOR-ACCEPTS-FORBIDDEN-COUNT',ep,'validate_structure',[("require(all(d[k] in COUNTING_GRADES for k in c), 'counted_non_measurement')","pass")],'test_evidence_protocol.ReceiptTests.test_non_counting_grades_refuse_counts'),
        ('MUT-BLOCKED-BOOKKEEPING',ep,'validate_structure',[("require(set(r['not_measured']['blocked_gates']) == {k for k,v in r['gate_results'].items() if v == 'BLOCKED'}, 'derivation_conflict')","pass")],'test_evidence_protocol.ReceiptTests.test_blocked_gate_absent_from_blocked_gates_is_refused'),
        ('MUT-COUNTING-SUBSET',ep,'validate_structure',[("require(all(k in c for k,v in d.items() if v in COUNTING_GRADES), 'uncounted_measurable_metric')","pass")],'test_evidence_protocol.ReceiptTests.test_actual_measurement_requires_count'),
        ('MUT-INTERVAL-DECLARATION',ep,'validate_structure',[("require(declaration_valid(r['analysis_declaration']), 'interval_without_declaration')","pass")],'test_evidence_protocol.ReceiptTests.test_interval_without_declaration_is_refused'),
        ('MUT-UNKNOWN-INTERVAL',ep,'validate_structure',[("require(not (r['outcomes']['UNKNOWN_EGRESS'] and metric in LEAK_FAMILY_METRIC_IDS), 'lower_bound_reported_as_exact')","pass"),("require(interval['conditional'], 'conditional_reported_as_full_cell')","pass")],'test_evidence_protocol.ReceiptTests.test_unknown_with_no_fragment_still_blocks_exact_interval'),
        ('MUT-FULL-CELL-BASIS',ep,'validate_structure',[("require(not interval['conditional'] and not r['outcomes']['UNKNOWN_EGRESS'] and not r['outcomes']['NOT_STARTED'], 'conditional_reported_as_full_cell')","pass")],'test_evidence_protocol.ReceiptTests.test_full_cell_requires_complete_known_outcomes'),
        ('MUT-CLASS-LOAD',ep,'load_class_commitments',[("pair in {('EMAIL','global'), ('PHONE','de')} and pair not in seen","pair not in seen")],'test_evidence_protocol.ClassCommitmentTests.test_unknown_label_region_pair_is_a_load_error'),
        ('MUT-OBSERVER-COVERAGE',ep,'validate_structure',[("require(r['gate_results']['source_attribution_events'] != 'PASS', 'observer_coverage_incomplete')","pass")],'test_evidence_protocol.ReceiptTests.test_observer_coverage_is_not_inferred_for_unobserved_leaves'),
        ('MUT-CLAIM-SCOPE',ep,'validate_structure',[("MANDATORY_KEYS <= set(r)","MANDATORY_KEYS - {'claim_scope'} <= set(r)")],'test_evidence_protocol.ReceiptTests.test_missing_claim_scope_is_refused'),
        ('MUT-QUANTILE',ee,'quantile',[("math.ceil(probability * len(samples))-1","math.ceil(probability * len(samples))")],'test_evidence_eval.IntervalArithmeticTests.test_non_constant_deltas_match_hand_computed_quantiles'),
        ('MUT-CONDITIONALITY',ee.PrivateEvaluator,'paired_interval',[("conditional=any(self.outcomes(a)['COMPLETED'] != len(self.inventory) for a in ep.ARM_IDS)","conditional=False")],'test_evidence_eval.PairingTests.test_conditional_flag_set_when_remainder_non_empty'),
        ('MUT-UNKNOWN-LOWER-BOUND',ee.PrivateEvaluator,'aggregate',[("for metric,field in METRIC_FIELDS.items()}","for metric,field in METRIC_FIELDS.items()}\n        result['gold_bytes_surviving_egress'] = 0")],'test_evidence_eval.PairingTests.test_unknown_egress_observed_fragment_is_retained_as_lower_bound'),
        ('MUT-LOCAL-MEMBERSHIP-PROOF',ee.PrivateEvaluator,'export_receipt',[("tuple(custody.get('membership_order', ())) == self.inventory.keys()","True")],'test_evidence_eval.ExportTests.test_wrong_local_order_fails_membership_proof'),
        ('MUT-REJECTION-CREDIT',ee.PrivateEvaluator,'protected_case_count',[("r.outcome == 'COMPLETED' and r.entities > 0 and r.entities == r.entities_fully_covered","r.outcome == 'FAILED_CLOSED_NO_EGRESS'")],'test_evidence_eval.PairingTests.test_failed_closed_is_not_protection'),
        ('MUT-GROUPING',ee.PrivateEvaluator,'draw_resample',[("result.extend(groups[rng.choice(ids)])","result.append(rng.choice(groups[rng.choice(ids)]))")],'test_evidence_eval.GroupingTests.test_resample_draws_groups_not_records'),
    ]
    mutations.extend([
        ('MUT-OUTCOME-COVERAGE',ep,'check_outcome_identities',[("    o = r['outcomes']", "    o = r['outcomes']\n    o.setdefault('NOT_STARTED', 0)")],'test_evidence_protocol.ReceiptTests.test_all_five_states_are_mandatory'),
        ('MUT-INVENTORY',ee.PrivateEvaluator,'add',[("        self.inventory.case(key)","        pass")],'test_evidence_eval.PlannedInventoryTests.test_unknown_key_is_refused'),
        ('MUT-DECLARATION-ABSENT',ee.PrivateEvaluator,'paired_interval',[("        self.finalize()", "        if declaration is None: declaration = __import__('test_evidence_protocol').declaration()\n        self.finalize()")],'test_evidence_eval.DeclarationTests.test_absent_declaration_is_not_evaluable'),
        ('MUT-DECLARATION-PARTIAL',ee.PrivateEvaluator,'paired_interval',[("        self.finalize()", "        if type(declaration) is dict: declaration.setdefault('confidence_level', .5)\n        self.finalize()")],'test_evidence_eval.DeclarationTests.test_partial_declaration_is_not_evaluable'),
        ('MUT-PAIR-HONESTY',ee.PrivateEvaluator,'paired_completed_keys',[("if all(self._records[a][k].outcome == 'COMPLETED' for a in ep.ARM_IDS)","if any(self._records[a][k].outcome == 'COMPLETED' for a in ep.ARM_IDS)")],'test_evidence_eval.PairingTests.test_missing_pair_leaves_intersection_and_is_not_zero_leak'),
        ('MUT-STAMP-SEPARATION',ep,'validate_structure',[("require(not STAMPED_KEYS.intersection(r), 'stamped_key_in_emitted_receipt')","pass"),("    walk(r)","    walk({k:v for k,v in r.items() if k not in STAMPED_KEYS})")],'test_evidence_protocol.ReceiptTests.test_emitted_receipt_carrying_a_stamped_key_is_refused'),
        ('MUT-DERIVATION-COVERAGE',ep,'validate_structure',[("require(set(d) == METRIC_IDS, 'derivation_coverage_incomplete')","pass"),("require(set(r['not_measured']['metrics']) == {k for k,v in d.items() if v == 'not_measured'}, 'derivation_conflict')","pass")],'test_evidence_protocol.ReceiptTests.test_metric_without_declared_derivation_is_refused'),
        ('MUT-CLASS-FIELDS',ep,'load_class_commitments',[("require(type(row) is dict and set(row) == required, 'class_commitment_invalid')","pass"),("all(type(row[k]) is str and row[k] for k in ('partial_scope','rationale'))","all(type(row.get(k,'fixture')) is str and row.get(k,'fixture') for k in ('partial_scope','rationale'))")],'test_evidence_protocol.ClassCommitmentTests.test_missing_mandatory_row_field_is_a_load_error'),
        ('MUT-CANARY-PRIVATE-VALUE',ep,'loads_receipt',[("    require(type(text) is str", "    print('synthetic-private-canary')\n    require(type(text) is str")],'test_evidence_protocol.CanaryTests.test_failures_do_not_emit_private_values_or_files'),
        ('MUT-CONTAINER-TYPES',ep,'walk',[("    if kind == 'nullable_object'", "    if kind == 'object' and node == []: return\n    if kind == 'nullable_object'")],'test_evidence_protocol.DirectBoundaryTests.test_empty_container_types'),
    ])
    results=[]
    for identifier,owner,name,edits,target in mutations:
        original=getattr(owner,name); source=textwrap.dedent(inspect.getsource(original))
        for old,new in edits:
            if old not in source: raise AssertionError('mutation-site-missing')
            source=source.replace(old,new)
        namespace=original.__globals__.copy()
        exec(compile(source,'<evidence-mutation>','exec'),namespace)
        setattr(owner,name,namespace[name])
        try:
            suite=unittest.defaultTestLoader.loadTestsFromName(target)
            output=io.StringIO(); result=unittest.TextTestRunner(stream=output).run(suite)
            killed=bool(result.failures) and not result.errors and result.testsRun > 0
            results.append({'id':identifier,'killed':killed,'tests_run':result.testsRun,'kill_set':[test.id() for test,_ in result.failures],'errors':len(result.errors)})
        finally: setattr(owner,name,original)
        if not killed: break
    if all(row['killed'] for row in results):
        vocab = copy.deepcopy(ep.VOCABULARIES)
        vocab['METRIC_IDS'] = vocab['METRIC_IDS'] | {'mutation-only'}
        with patch.object(ep, 'VOCABULARIES', vocab):
            target='test_evidence_protocol.VocabularyMirrorTests.test_python_vocabularies_equal_committed_artifact'
            result=unittest.TextTestRunner(stream=io.StringIO()).run(unittest.defaultTestLoader.loadTestsFromName(target))
        results.append({'id':'MUT-VOCAB-PY','killed':bool(result.failures) and not result.errors,'tests_run':result.testsRun,'kill_set':[test.id() for test,_ in result.failures],'errors':len(result.errors)})
    return results


if __name__ == '__main__':
    import sys
    if '--mutation-proof' in sys.argv:
        rows=run_mutation_proof()
        print(json.dumps(rows,sort_keys=True))
        raise SystemExit(not all(r['killed'] for r in rows))
    unittest.main()
