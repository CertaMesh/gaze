"""Nonzero witnesses for every clock bound and forbidden phase placement."""
from pathlib import Path
import tempfile
import unittest

import perf_attribution as perf
from perf_lifecycle_contract import BOUNDS, CLOCK_PHASES
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
    request['metrics']['request_ns']['ns'] = 1000
    request['recognizers'] = {'r1': {'ns': 1, 'invocations': 1, 'candidates': 1}}
    return [startup, request]


# Each witness violates exactly one bound, so removing it must admit the record.
CHANGES = {
    'policy-ready': {'process_to_policy_ns': 1001},
    'policy-load': {'policy_load_ns': 51},
    'pipeline': {'assembly_ns': 281},
    'ready': {'pipeline_build_ns': 901},
    'daemon-ready': {'daemon_startup_ns': 1001},
    'daemon-build': {'daemon_startup_ns': 449},
    'assembly': {'model_constructor_ns': 181},
    'nym-attach': {'nym_model_constructor_ns': 141},
    'constructor-loads': {'tokenizer_load_ns': 201, 'model_load_ns': 1,
                          'nym_outer_verify_ns': 100, 'nym_attach_ns': 250},
    'constructor-leaves': {'bundle_verify_ns': 171},
}


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


def bound_test(bound):
    def test(self):
        daemon = bound.id.startswith('daemon-')
        rows = records(daemon=daemon)
        record = rows[int(bound.phase == 'request')]
        changes = CHANGES.get(bound.id, {bound.children[0]: 1001})
        for key, value in changes.items():
            record['metrics'][key]['ns'] = value
        violated = []
        for other in BOUNDS:
            if other.phase != bound.phase or (other.daemon_only and not daemon):
                continue
            ns = lambda key: record['metrics'].get(key, {}).get('ns', 0)
            if sum(map(ns, other.children)) > sum(map(ns, other.parents)):
                violated.append(other.id)
        self.assertEqual(violated, [bound.id], 'negative witness must isolate one bound')
        with self.assertRaisesRegex(ValueError, f'attribution mismatch: {bound.id}$'):
            self.admit(rows, daemon=daemon)
    return test


def edge_test(bound, child):
    def test(self):
        daemon = bound.daemon_only
        rows = records(daemon=daemon)
        record = rows[int(bound.phase == 'request')]
        # A single child cannot exceed the complete parent capacity, even when
        # all its serial siblings are nonzero as well.
        capacity = sum(record['metrics'].get(key, {}).get('ns', 0) for key in bound.parents)
        record['metrics'][child]['ns'] = capacity + 1
        with self.assertRaises(ValueError):
            self.admit(rows, daemon=daemon)
    return test


def phase_test(key, phase):
    def test(self):
        rows = records()
        rows[int(phase == 'startup')]['metrics'][key] = {'ns': 1, 'invocations': 1}
        with self.assertRaisesRegex(ValueError, 'clock phase'):
            self.admit(rows)
    return test


for bound in BOUNDS:
    setattr(BoundTests, 'test_bound_' + bound.id.replace('-', '_'), bound_test(bound))
    for child in bound.children:
        setattr(BoundTests, 'test_edge_' + bound.id.replace('-', '_') + '_' + child, edge_test(bound, child))
for key, phase in CLOCK_PHASES.items():
    setattr(BoundTests, 'test_phase_' + key, phase_test(key, phase))


if __name__ == '__main__':
    unittest.main()
