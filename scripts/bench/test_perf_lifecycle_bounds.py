"""Nonzero witnesses for every clock bound and forbidden phase placement."""
from pathlib import Path
from unittest.mock import patch
import tempfile
import unittest

import perf_attribution as perf
from perf_lifecycle_contract import CLOCK_CHECKS, CLOCK_GRAPH, CLOCK_PHASES
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
                               if phase == 'request'})
    for key, value in {'request_ns': 1000, 'registry_ns': 100, 'safety_model_ns': 100,
                       'nym_stable_ns': 40, 'nym_neutral_ns': 40, 'nym_tokenize_ns': 10,
                       'nym_mutex_wait_ns': 10, 'nym_ort_ns': 10, 'resolve1_ns': 10,
                       'sweep_ns': 10, 'resolve2_ns': 10, 'safety_view_build_ns': 10,
                       'manifest_ns': 10, 'audit_ns': 10}.items():
        request['metrics'][key]['ns'] = value
    request['recognizers'] = {'r1': {'ns': 1, 'invocations': 1, 'candidates': 1}}
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
    'registry': {'rules_ns': 98},
    'nym-views': {'nym_stable_ns': 60, 'nym_neutral_ns': 41},
    'nym-inference': {'nym_tokenize_ns': 30, 'nym_mutex_wait_ns': 30, 'nym_ort_ns': 21},
}


def violated_checks(rows, *, daemon):
    return [check.id for check in CLOCK_CHECKS
            if not (check.daemon_only and not daemon)
            and sum(rows[int(check.phase == 'request')]['metrics'][k]['ns'] for k in check.children)
            > sum(rows[int(check.phase == 'request')]['metrics'][k]['ns'] for k in check.parents)]


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
        roots = {'process_to_ready_ns', 'request_ns'}
        self.assertEqual({k for g in CLOCK_GRAPH for k in g.children}, set(CLOCK_PHASES) - roots)
        self.assertEqual(len({g.id for g in CLOCK_GRAPH}), len(CLOCK_GRAPH))
        self.assertEqual(len({c.id for c in CLOCK_CHECKS}), len(CLOCK_CHECKS))
        for group in CLOCK_GRAPH:
            self.assertTrue(group.hooks)
            self.assertTrue(set(group.children).isdisjoint(group.parents))
            self.assertTrue(all(CLOCK_PHASES[k] == group.phase for k in (*group.children, *group.parents)))
        # Every metric reaches its phase root; no detached subgraph or cycle.
        parents = {k: set() for k in CLOCK_PHASES}
        for group in CLOCK_GRAPH:
            for child in group.children:
                parents[child].update(group.parents)
        def ancestors(key, path):
            self.assertNotIn(key, path)
            return {key} | set().union(*(ancestors(p, path | {key}) for p in parents[key]))
        for key, phase in CLOCK_PHASES.items():
            self.assertIn('request_ns' if phase == 'request' else 'process_to_ready_ns', ancestors(key, set()))

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
        with self.assertRaisesRegex(ValueError, 'recognizer duration'):
            self.admit(rows)

    def test_startup_work_counters_are_refused(self):
        rows = records()
        rows[0]['counters'] = {'input_bytes': 1}
        with self.assertRaisesRegex(ValueError, 'startup processing'):
            self.admit(rows)

    def test_startup_recognizers_are_refused(self):
        rows = records()
        rows[0]['recognizers'] = {'r1': {'ns': 1, 'invocations': 1, 'candidates': 1}}
        with self.assertRaisesRegex(ValueError, 'startup processing'):
            self.admit(rows)

    def test_nonzero_clock_requires_invocation(self):
        rows = records()
        rows[1]['metrics']['rules_ns']['invocations'] = 0
        with self.assertRaisesRegex(ValueError, 'without an invocation'):
            self.admit(rows)


def serial_test(group):
    def test(self):
        daemon = group.daemon_only
        rows = records(daemon=daemon)
        record = rows[int(group.phase == 'request')]
        for key, value in SERIAL_CHANGES[group.id].items():
            record['metrics'][key]['ns'] = value
        self.assertEqual(violated_checks(rows, daemon=daemon), [group.id + '-serial'])
        with self.assertRaisesRegex(ValueError, group.id + '-serial$'):
            self.admit(rows, daemon=daemon)
    return test


def parent_daemon(check):
    return check.daemon_only or "daemon_startup_ns" in check.children


def parent_witness(check):
    rows = records(daemon=parent_daemon(check))
    record = rows[int(check.phase == 'request')]
    capacity = sum(record['metrics'][key]['ns'] for key in check.parents)
    record['metrics'][check.children[0]]['ns'] = capacity + 1
    return rows


def parent_test(check, *, isolated):
    def test(self):
        rows = parent_witness(check)
        if isolated:
            # Serial sums imply their individual edges. Exercise each redundant
            # parent check alone through real lifecycle admission so a mutation
            # cannot hide behind a serial sum (or another ancestor edge).
            with patch.object(perf, 'CLOCK_CHECKS', (check,)):
                with self.assertRaisesRegex(ValueError, check.id + '$'):
                    self.admit(rows, daemon=parent_daemon(check))
        else:
            with self.assertRaises(ValueError):
                self.admit(rows, daemon=parent_daemon(check))
    return test


def phase_test(key, phase):
    def test(self):
        rows = records()
        rows[int(phase == 'startup')]['metrics'][key] = {'ns': 1, 'invocations': 1}
        with self.assertRaisesRegex(ValueError, 'clock phase'):
            self.admit(rows)
    return test


for group in CLOCK_GRAPH:
    if group.relationship == 'serial':
        setattr(BoundTests, 'test_serial_' + group.id.replace('-', '_'), serial_test(group))
for check in CLOCK_CHECKS:
    if check.kind == 'parent':
        name = check.id.replace('-', '_')
        setattr(BoundTests, 'test_parent_' + name, parent_test(check, isolated=True))
        setattr(BoundTests, 'test_full_graph_' + name, parent_test(check, isolated=False))
for key, phase in CLOCK_PHASES.items():
    setattr(BoundTests, 'test_phase_' + key, phase_test(key, phase))


if __name__ == '__main__':
    unittest.main()
