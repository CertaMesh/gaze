"""Nonzero witnesses for every clock bound and forbidden phase placement."""
from pathlib import Path
from unittest.mock import patch
import tempfile
import re
import unittest

import perf_attribution as perf
from perf_lifecycle_contract import CLOCK_CHECKS, CLOCK_GRAPH, CLOCK_PHASES, RECOGNIZERS, REQUEST_ASSEMBLY, clock_values, violates_check
from test_perf_attribution import trace


POLICY = '[ner]\nenabled=true\n[dob_judge]\nenabled=true\n[safety_net]\nbackend="nym"\n'


def records(*, daemon=False):
    startup = trace('startup', 0, daemon=daemon, models=3, nym=True, dob=True)
    values = {
        'process_to_ready_ns': 1000, 'process_to_policy_ns': 50, 'policy_load_ns': 20,
        'pipeline_build_ns': 300, 'assembly_ns': 200, 'recognizer_construction_ns': 10,
        'rule_construction_ns': 10, 'nym_attach_ns': 150, 'model_constructor_ns': 100,
        'nym_model_constructor_ns': 100, 'nym_outer_verify_ns': 10, 'tokenizer_load_ns': 20,
        'model_load_ns': 20, 'bundle_verify_ns': 30, 'daemon_startup_ns': 600 if daemon else 0,
    }
    for key, value in values.items():
        startup['metrics'][key]['ns'] = value
    request = trace()
    request['metrics'].update({key: {'ns': 1, 'invocations': 1} for key, phase in CLOCK_PHASES.items()
                               if phase == ('request',)})
    for key, value in {'request_ns': 1000, 'registry_ns': 100, 'safety_model_ns': 100,
                       'nym_stable_ns': 40, 'nym_neutral_ns': 40, 'nym_tokenize_ns': 10,
                       'nym_mutex_wait_ns': 10, 'nym_ort_ns': 10, 'resolve1_ns': 10,
                       'sweep_ns': 10, 'resolve2_ns': 10, 'safety_view_build_ns': 10,
                       'manifest_ns': 10, 'audit_ns': 10, 'rules_ns': 10}.items():
        request['metrics'][key]['ns'] = value
    request['recognizers'] = {
        'r1': {'ns': 10, 'invocations': 1, 'candidates': 1, 'evidence': 'rule', 'model': 'none'},
        'r2': {'ns': 10, 'invocations': 1, 'candidates': 1, 'evidence': 'learned', 'model': 'ner'},
    }
    request['metrics']['rules_ns'].update(invocations=1, candidates=1)
    if daemon:
        for key, value in {'assembly_ns': 100, 'recognizer_construction_ns': 20,
                           'rule_construction_ns': 20}.items():
            request['metrics'][key] = {'ns': value, 'invocations': 1}
    return [startup, request]


# Independent full-graph witnesses: every edge still fits; exactly one serial
# group fails. These values deliberately retain nonzero siblings/descendants.
SERIAL_CHANGES = {
    'ready-build': {'pipeline_build_ns': 901},
    'ready-policy': {'process_to_policy_ns': 950},
    'pipeline': {'assembly_ns': 281},
    'daemon-build': {'daemon_startup_ns': 449},
    'assembly': {'model_constructor_ns': 181},
    'nym-attach': {'nym_model_constructor_ns': 141},
    'constructor-loads': {'tokenizer_load_ns': 101, 'model_load_ns': 100, 'bundle_verify_ns': 1},
    'constructor-leaves': {'bundle_verify_ns': 171},
    'request-sweep': {'sweep_ns': 761},
    'request-resolve': {'resolve2_ns': 761},
    'request-assembly': {'recognizer_construction_ns': 81},
    'registry-recognizers': {RECOGNIZERS: 60},
    'recognizer-work': {'rules_ns': 18},
    'nym-views': {'nym_stable_ns': 60, 'nym_neutral_ns': 41},
    'nym-inference': {'nym_tokenize_ns': 30, 'nym_mutex_wait_ns': 30, 'nym_ort_ns': 21},
}


def violated_checks(rows, *, daemon):
    return [check.id for check in CLOCK_CHECKS
            if not (check.daemon_only and not daemon)
            and violates_check(rows[int(check.phase == 'request')], check)]


class BoundTests(unittest.TestCase):
    def admit(self, rows, *, daemon=False):
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / 'policy'
            policy.write_text(POLICY)
            return perf.validate_lifecycle(rows, 1, daemon=daemon, policy=policy, required=True)

    def test_all_nonzero_clocks_are_admitted(self):
        for daemon in (False, True):
            with self.subTest(daemon=daemon):
                self.admit(records(daemon=daemon), daemon=daemon)

    def test_graph_covers_every_clock_and_only_its_own_phase(self):
        roots = {'startup': 'process_to_ready_ns', 'request': 'request_ns'}
        self.assertEqual(len({g.id for g in CLOCK_GRAPH}), len(CLOCK_GRAPH))
        self.assertEqual(len({c.id for c in CLOCK_CHECKS}), len(CLOCK_CHECKS))
        phases = {**CLOCK_PHASES, RECOGNIZERS: ('request',)}
        for group in CLOCK_GRAPH:
            self.assertTrue(group.hooks)
            self.assertTrue(set(group.children).isdisjoint(group.parents))
            self.assertTrue(all(group.phase in phases[k] for k in (*group.children, *group.parents)))
        for phase, root in roots.items():
            parents = {k: set() for k, allowed in phases.items() if phase in allowed}
            for group in CLOCK_GRAPH:
                if group.phase == phase:
                    for child in group.children:
                        parents[child].update(group.parents)
            def ancestors(key, path):
                self.assertNotIn(key, path)
                return {key} | set().union(*(ancestors(p, path | {key}) for p in parents[key]))
            for key in parents:
                self.assertIn(root, ancestors(key, set()))

    def test_reviewer_recognizer_outside_registry(self):
        rows = records()
        rows[1]['recognizers']['r1']['ns'] = 900
        with self.assertRaises(ValueError):
            self.admit(rows)

    def test_reviewer_serial_recognizers_outside_request(self):
        rows = records()
        for metric in rows[1]['recognizers'].values():
            metric['ns'] = 700
        with self.assertRaises(ValueError):
            self.admit(rows)

    def test_reviewer_rules_outside_recognizer_total(self):
        rows = records()
        rows[1]['metrics']['rules_ns']['ns'] = 50
        with self.assertRaises(ValueError):
            self.admit(rows)

    def test_reviewer_ner_cannot_borrow_another_recognizer(self):
        rows = records()
        rows[1]['recognizers']['r3'] = dict(rows[1]['recognizers']['r2'], model='dob')
        rows[1]['metrics']['ner_tokenize_ns']['ns'] = 13
        with self.assertRaisesRegex(ValueError, 'ner-owner-capacity'):
            self.admit(rows)

    def test_reviewer_rules_must_equal_rule_spans(self):
        rows = records()
        rows[1]['metrics']['rules_ns']['ns'] = 11
        with self.assertRaisesRegex(ValueError, 'rule-owner-ns'):
            self.admit(rows)

    def test_rule_owner_invocations(self):
        rows = records()
        rows[1]['metrics']['rules_ns']['invocations'] = 2
        with self.assertRaisesRegex(ValueError, 'rule-owner-invocations'):
            self.admit(rows)

    def test_rule_owner_candidates(self):
        rows = records()
        rows[1]['metrics']['rules_ns']['candidates'] = 2
        with self.assertRaisesRegex(ValueError, 'rule-owner-candidates'):
            self.admit(rows)

    def test_ner_owner_serial_nonzero_leaves(self):
        rows = records()
        for key in ('ner_tokenize_ns', 'ner_mutex_wait_ns', 'ner_ort_ns'):
            rows[1]['metrics'][key]['ns'] = 4
        self.assertEqual(violated_checks(rows, daemon=False), ['recognizer-work-serial'])
        # Extra Rule capacity keeps the aggregate bound satisfied while NER alone overflows.
        rows[1]['recognizers']['r3'] = dict(rows[1]['recognizers']['r1'], ns=10)
        rows[1]['metrics']['rules_ns'].update(ns=20, invocations=2, candidates=2)
        # Pool is 30, rules + NER is 32; raise only the DOB capacity to make pooling pass.
        rows[1]['recognizers']['r4'] = dict(rows[1]['recognizers']['r2'], ns=10, model='dob')
        self.assertEqual(violated_checks(rows, daemon=False), [])
        with self.assertRaisesRegex(ValueError, 'ner-owner-capacity'):
            self.admit(rows)

    def test_recognizer_role(self):
        rows = records()
        rows[1]['recognizers']['r3'] = dict(rows[1]['recognizers']['r2'], model='other')
        with self.assertRaisesRegex(ValueError, 'recognizer-role'):
            self.admit(rows)

    def test_zero_duration_ner_hooks_still_require_an_owner(self):
        rows = records()
        del rows[1]['recognizers']['r2']
        for key in ('ner_tokenize_ns', 'ner_mutex_wait_ns', 'ner_ort_ns'):
            rows[1]['metrics'][key]['ns'] = 0
        with self.assertRaisesRegex(ValueError, 'ner-owner-capacity'):
            self.admit(rows)

    def test_one_model_owner_across_all_requests(self):
        import copy
        rows = records(daemon=True)
        second = copy.deepcopy(rows[1])
        second['ordinal'] = 2
        second['recognizers']['r3'] = second['recognizers'].pop('r2')
        rows.append(second)
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / 'policy'
            policy.write_text(POLICY)
            with self.assertRaisesRegex(ValueError, 'recognizer-role'):
                perf.validate_lifecycle(rows, 2, daemon=True, policy=policy, required=True)

    def test_stable_recognizer_role(self):
        rows = records(daemon=True)
        import copy
        second = copy.deepcopy(rows[1])
        second['ordinal'] = 2
        second['recognizers']['r1'], second['recognizers']['r2'] = (
            second['recognizers']['r2'], second['recognizers']['r1'])
        rows.append(second)
        with tempfile.TemporaryDirectory() as tmp:
            policy = Path(tmp) / 'policy'
            policy.write_text(POLICY)
            with self.assertRaisesRegex(ValueError, 'stable-recognizer-role'):
                perf.validate_lifecycle(rows, 2, daemon=True, policy=policy, required=True)

    def test_invalid_and_duplicate_model_roles_refuse(self):
        for evidence, model in (('learned', 'none'), ('rule', 'ner'), ('learned', 'other')):
            rows = records()
            rows[1]['recognizers']['r3'] = dict(rows[1]['recognizers']['r2'], evidence=evidence, model=model)
            # Other guards are independently covered: exercise this admission guard alone.
            with patch.object(perf, 'OWNERSHIP_CHECKS', ('recognizer-role',)):
                with self.assertRaisesRegex(ValueError, 'recognizer-role'):
                    self.admit(rows)
        for model in ('ner', 'dob'):
            rows = records()
            for ordinal in ('r3', 'r4'):
                rows[1]['recognizers'][ordinal] = dict(rows[1]['recognizers']['r2'], model=model)
            with self.assertRaises(ValueError):
                self.admit(rows)

    def test_ner_only_ner_nym_and_ner_nym_dob_shapes_admit(self):
        for nym, dob in ((False, False), (True, False), (True, True)):
            for daemon in (False, True):
                rows = records(daemon=daemon)
                startup = rows[0]
                models = 1 + int(dob)
                counts = {'model_constructor_ns': models, 'nym_model_constructor_ns': int(nym),
                          'nym_outer_verify_ns': int(nym), 'nym_attach_ns': int(nym),
                          'bundle_verify_ns': models + 2 * int(nym),
                          'model_load_ns': models + int(nym), 'tokenizer_load_ns': models + int(nym)}
                for key, count in counts.items():
                    startup['metrics'][key]['invocations'] = count
                    if not count:
                        startup['metrics'][key]['ns'] = 0
                if dob:
                    rows[1]['recognizers']['r3'] = dict(rows[1]['recognizers']['r2'], model='dob')
                with tempfile.TemporaryDirectory() as tmp:
                    policy = Path(tmp) / 'policy'
                    policy.write_text('[ner]\nenabled=true\n' + ('[dob_judge]\nenabled=true\n' if dob else '')
                                      + ('[safety_net]\nbackend="nym"\n' if nym else ''))
                    perf.validate_lifecycle(rows, 1, daemon=daemon, policy=policy, required=True)

    def test_unconfigured_models_refuse(self):
        rows = records()
        with patch.object(perf, 'OWNERSHIP_CHECKS', ('recognizer-role',)):
            # Keep model constructor checks out of this role-specific unit.
            from perf_lifecycle_contract import violates_ownership
            self.assertTrue(violates_ownership(rows[1], 'recognizer-role', config={}, roles={}))

    def test_missing_or_unexpected_role_metadata_refuses(self):
        for field, value in (('evidence', 'private'), ('model', 'private'), ('model', 1)):
            rows = records()
            rows[1]['recognizers']['r1'][field] = value
            with self.assertRaisesRegex(ValueError, 'invalid timer metadata'):
                self.admit(rows)
        rows = records()
        del rows[1]['recognizers']['r1']['evidence']
        with self.assertRaisesRegex(ValueError, 'invalid timer metadata'):
            self.admit(rows)

    def test_daemon_context_assembly_is_admitted(self):
        rows = records(daemon=True)
        for key, ns in {'assembly_ns': 100, 'recognizer_construction_ns': 50,
                        'rule_construction_ns': 10}.items():
            rows[1]['metrics'][key] = {'ns': ns, 'invocations': 1}
        self.admit(rows, daemon=True)

    def test_reviewer_startup_serial_witness(self):
        rows = records()
        rows[0]['metrics']['process_to_policy_ns']['ns'] = 950
        with self.assertRaisesRegex(ValueError, 'ready-policy-serial'):
            self.admit(rows)

    def test_reviewer_nested_views_witness(self):
        rows = records()
        for key, value in {'safety_model_ns': 10, 'nym_stable_ns': 700, 'nym_neutral_ns': 700}.items():
            rows[1]['metrics'][key]['ns'] = value
        with self.assertRaises(ValueError):
            self.admit(rows)

    def test_reviewer_nested_inference_witness(self):
        rows = records()
        for key, value in {'nym_stable_ns': 1, 'nym_neutral_ns': 1,
                           'nym_tokenize_ns': 700, 'nym_ort_ns': 700}.items():
            rows[1]['metrics'][key]['ns'] = value
        with self.assertRaises(ValueError):
            self.admit(rows)

    def test_overlapping_sweep_and_resolution_are_not_summed(self):
        rows = records()
        rows[1]['metrics']['sweep_ns']['ns'] = 700
        rows[1]['metrics']['resolve2_ns']['ns'] = 700
        self.admit(rows)

    def test_overlapping_validators_are_not_summed(self):
        rows = records()
        rows[1]['metrics']['validators_ns']['ns'] = 999
        self.admit(rows)

    def test_overlapping_pipeline_and_policy_prefix_are_not_summed(self):
        rows = records()
        for key, value in {'process_to_policy_ns': 900, 'pipeline_build_ns': 900,
                           'assembly_ns': 30, 'model_constructor_ns': 10,
                           'nym_attach_ns': 50, 'nym_model_constructor_ns': 20,
                           'tokenizer_load_ns': 5, 'model_load_ns': 5, 'bundle_verify_ns': 5}.items():
            rows[0]['metrics'][key]['ns'] = value
        self.admit(rows)

    def test_recognizer_bound(self):
        rows = records()
        rows[1]['recognizers']['r1']['ns'] = 1001
        with self.assertRaisesRegex(ValueError, 'registry-recognizers'):
            self.admit(rows)

    def test_one_shot_request_assembly_is_refused(self):
        rows = records()
        for key in REQUEST_ASSEMBLY:
            rows[1]['metrics'][key] = {'ns': 0, 'invocations': 1}
        with self.assertRaisesRegex(ValueError, 'clock phase'):
            self.admit(rows)

    def test_request_assembly_hooks_are_complete_and_once_each(self):
        for key in REQUEST_ASSEMBLY:
            for count in (0, 2):
                rows = records(daemon=True)
                rows[1]['metrics'][key]['invocations'] = count
                for clock in REQUEST_ASSEMBLY:
                    rows[1]['metrics'][clock]['ns'] = 0
                with self.assertRaisesRegex(ValueError, 'request assembly hook'):
                    self.admit(rows, daemon=True)

    def test_resident_models_cannot_initialize_in_daemon_requests(self):
        for key in perf.LIFECYCLE - REQUEST_ASSEMBLY:
            rows = records(daemon=True)
            rows[1]['metrics'][key] = {'ns': 1, 'invocations': 1}
            with self.assertRaisesRegex(ValueError, 'clock phase'):
                self.admit(rows, daemon=True)

    def test_request_model_invocations_are_refused_even_at_zero_duration(self):
        rows = records(daemon=True)
        rows[1]['metrics']['model_load_ns'] = {'ns': 0, 'invocations': 1}
        with self.assertRaisesRegex(ValueError, 'clock phase'):
            self.admit(rows, daemon=True)

    def test_startup_work_counters_are_refused(self):
        rows = records()
        rows[0]['counters'] = {'input_bytes': 1}
        with self.assertRaisesRegex(ValueError, 'startup processing'):
            self.admit(rows)

    def test_startup_recognizers_are_refused(self):
        rows = records()
        rows[0]['recognizers'] = {'r1': {'ns': 1, 'invocations': 1, 'candidates': 1, 'evidence': 'rule', 'model': 'none'}}
        with self.assertRaisesRegex(ValueError, 'startup processing'):
            self.admit(rows)

    def test_nonzero_clock_requires_invocation(self):
        rows = records()
        rows[1]['metrics']['registry_ns']['invocations'] = 0
        with self.assertRaisesRegex(ValueError, 'without an invocation'):
            self.admit(rows)


def set_clock(record, key, value):
    if key == RECOGNIZERS:
        for metric in record['recognizers'].values():
            metric['ns'] = value
    else:
        record['metrics'].setdefault(key, {'ns': 0, 'invocations': 1})['ns'] = value


def serial_test(group):
    def test(self):
        daemon = group.daemon_only
        rows = records(daemon=daemon)
        record = rows[int(group.phase == 'request')]
        for key, value in SERIAL_CHANGES[group.id].items():
            set_clock(record, key, value)
        self.assertEqual(violated_checks(rows, daemon=daemon), [group.id + '-serial'])
        with patch.object(perf, 'OWNERSHIP_CHECKS', ()):
            with self.assertRaisesRegex(ValueError, group.id + '-serial$'):
                self.admit(rows, daemon=daemon)
    return test


def parent_daemon(check):
    return (check.daemon_only or "daemon_startup_ns" in check.children
            or (check.phase == 'request' and any(k in REQUEST_ASSEMBLY for k in check.children)))


def parent_witness(check):
    rows = records(daemon=parent_daemon(check))
    record = rows[int(check.phase == 'request')]
    capacity = sum(ns for key in check.parents for ns in clock_values(record, key))
    set_clock(record, check.children[0], capacity + 1)
    return rows


def parent_test(check, *, isolated):
    def test(self):
        rows = parent_witness(check)
        if isolated:
            # Serial sums imply their individual edges. Exercise each redundant
            # parent check alone through real lifecycle admission so a mutation
            # cannot hide behind a serial sum (or another ancestor edge).
            with patch.object(perf, 'CLOCK_CHECKS', (check,)), patch.object(perf, 'OWNERSHIP_CHECKS', ()):
                with self.assertRaisesRegex(ValueError, re.escape(check.id) + '$'):
                    self.admit(rows, daemon=parent_daemon(check))
        else:
            with self.assertRaises(ValueError):
                self.admit(rows, daemon=parent_daemon(check))
    return test


def phase_test(key, phase):
    def test(self):
        rows = records()
        rows[int(phase == 'request')]['metrics'][key] = {'ns': 1, 'invocations': 1}
        with self.assertRaisesRegex(ValueError, 'clock phase'):
            self.admit(rows)
    return test


def one_shot_assembly_phase_test(key):
    def test(self):
        # A candidate-only count isolates the phase guard from duration and hook-count guards.
        for field in ('candidates', 'invocations', 'ns'):
            rows = records()
            rows[1]['metrics'][key] = {'ns': 0, 'invocations': 0, 'candidates': 0}
            rows[1]['metrics'][key][field] = 1
            with self.assertRaisesRegex(ValueError, 'clock phase'):
                self.admit(rows)
    return test


for key in REQUEST_ASSEMBLY:
    setattr(BoundTests, 'test_one_shot_phase_' + key, one_shot_assembly_phase_test(key))


for group in CLOCK_GRAPH:
    if group.relationship == 'serial':
        setattr(BoundTests, 'test_serial_' + group.id.replace('-', '_'), serial_test(group))
for check in CLOCK_CHECKS:
    if check.kind == 'parent':
        name = check.id.replace('-', '_')
        setattr(BoundTests, 'test_parent_' + name, parent_test(check, isolated=True))
        setattr(BoundTests, 'test_full_graph_' + name, parent_test(check, isolated=False))
for key, phases in CLOCK_PHASES.items():
    for forbidden in {'startup', 'request'} - set(phases):
        setattr(BoundTests, 'test_phase_' + key, phase_test(key, forbidden))


if __name__ == '__main__':
    unittest.main()
