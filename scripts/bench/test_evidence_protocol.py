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
