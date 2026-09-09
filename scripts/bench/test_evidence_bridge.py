"""Model-free tests by discovery; real rmcp proof requires explicit integration mode."""
from __future__ import annotations

import contextlib
import copy
from dataclasses import replace
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

import evidence_bridge as bridge
import evidence_eval as ee
from bench_subprocess import BenchSubprocess, ProducerFailure, TransportLimits

CANARY = 'synthetic-private-bridge-canary'
HERE = Path(__file__).resolve()
ROOT = HERE.parents[2]


def frame(ordinal=0, arm='candidate'):
    value = 0 if arm == 'base' else (16 if ordinal == 0 else 8)
    return dict(format=bridge.FORMAT, kind='observation', arm=arm, ordinal=ordinal,
                scenario=bridge.SCENARIO, outcome='COMPLETED',
                gold=[dict(slot='a', verdict='protected' if not value else 'full' if value == 16 else 'partial', surviving_bytes=value),
                      dict(slot='b', verdict='protected', surviving_bytes=0)],
                negative=[dict(slot='n', verdict='full', false_positive_bytes=0)],
                restore=[dict(slot=s, decision_success=True, exact=not (s == 'a' and value == 8)) for s in ('a', 'b', 'n')])


def mapped(value):
    return bridge.map_observation(value, bridge.PLANS[value['ordinal']], value['arm'])


def cell_with(value):
    evaluator = ee.PrivateEvaluator(bridge.inventory())
    for p in bridge.PLANS:
        for arm in ('base', 'candidate'):
            row = value if p.ordinal == value['ordinal'] and arm == value['arm'] else frame(p.ordinal, arm)
            evaluator.add(arm, p.key, mapped(row).record)
    return evaluator


class ModelTests(unittest.TestCase):
    def test_shared_source_and_scorer(self):
        route = (ROOT/'crates/gaze-mcp-rmcp/tests/evidence_route.rs').read_text()
        example = (ROOT/'crates/gaze-mcp-rmcp/examples/evidence_bridge.rs').read_text()
        support = (ROOT/'crates/gaze-mcp-rmcp/tests/support/evidence_harness.rs').read_text()
        self.assertIn('#[path = "support/evidence_harness.rs"]', route)
        self.assertIn('#[path = "../tests/support/evidence_harness.rs"]', example)
        self.assertNotIn('fn occurrence(', route + example)
        self.assertNotIn('struct Counts(', route + example)
        self.assertEqual(support.count('fn occurrence('), 1)
        self.assertIn('counts.score(', example)
        self.assertIn('counts.negative(', example)
        self.assertIn('counts.restore(', example)
        self.assertIn('../../../../scripts/bench/fixtures/evidence/mcp_route_v1.golden.json', support)

    def test_full_numeric_cell(self):
        e = cell_with(frame())
        observations = {(a, p.ordinal): mapped(frame(p.ordinal, a)) for p in bridge.PLANS for a in ('base', 'candidate')}
        self.assertTrue(bridge.validate_numeric(e, observations).numeric_verified, 'numeric-model')
        self.assertEqual(e.paired_interval(bridge.GOLD_BYTES, bridge.declaration())['point'], 10)
        self.assertEqual([e.inventory.case(p.key).gold_bytes for p in bridge.PLANS], [32, 32])

    def test_full_coverage_operands(self):
        side = mapped(frame())
        self.assertEqual(side.record.observed_metrics, ee.OBSERVED_METRICS, 'full-coverage-operands')

    def test_wrong_pair_and_arm_are_discriminated(self):
        for swapped_keys, swapped_arms, expected in ((True, False, 14), (False, True, -10)):
            e = ee.PrivateEvaluator(bridge.inventory())
            for p in bridge.PLANS:
                for arm in ('base', 'candidate'):
                    target = bridge.PLANS[1-p.ordinal] if swapped_keys else p
                    target_arm = ('candidate' if arm == 'base' else 'base') if swapped_arms else arm
                    e.add(target_arm, target.key, mapped(frame(p.ordinal, arm)).record)
            self.assertEqual(e.paired_interval(bridge.GOLD_BYTES, bridge.declaration())['point'], expected)

    def test_unknown_retains_measured_bytes_without_exact_credit(self):
        v = frame(); v['gold'][1]['verdict'] = 'unknown'
        side = mapped(v)
        self.assertEqual((side.measured_gold_bytes, side.unknown), (16, 1), 'unknown-side-preserved')
        self.assertEqual(side.gold_coverage, bridge.Coverage.ATTRIBUTION, 'unknown-unavailable')
        self.assertNotIn(bridge.GOLD_BYTES, side.record.observed_metrics, 'unknown-no-protected-credit')
        self.assertIn(bridge.ATTRIBUTION, side.record.observed_metrics)
        self.assertEqual(side.record.gold_bytes_surviving, 0)
        self.assertEqual(cell_with(v).paired_interval(bridge.GOLD_BYTES, bridge.declaration()), 'NOT_EVALUABLE', 'unknown-no-interval')

    def test_missing_gold_is_not_protected_or_unknown(self):
        for remaining in (0, 1):
            v = frame(); v['gold'] = v['gold'][:remaining]
            side = mapped(v)
            self.assertNotIn(bridge.GOLD_BYTES, side.record.observed_metrics, 'missing-gold-unavailable')
            self.assertNotIn(bridge.ATTRIBUTION, side.record.observed_metrics)
            self.assertEqual(side.gold_coverage, bridge.Coverage.PARTIAL if remaining else bridge.Coverage.UNAVAILABLE)
            self.assertEqual(cell_with(v).paired_interval(bridge.GOLD_BYTES, bridge.declaration()), 'NOT_EVALUABLE')

    def test_negative_uncompared_and_absent_are_unavailable(self):
        for verdict in ('uncompared', None):
            v = frame()
            if verdict: v['negative'][0]['verdict'] = verdict
            else: v['negative'] = []
            side = mapped(v)
            self.assertFalse(side.record.observed_metrics & bridge.NEGATIVE, 'negative-unavailable')
        for verdict in ('partial', 'unknown'):
            v = frame(); v['negative'][0]['verdict'] = verdict
            with self.assertRaises(ProducerFailure): mapped(v)
        v = frame(); v['negative'][0].update(verdict='protected', false_positive_bytes=12)
        self.assertEqual(mapped(v).record.false_positive_bytes, 12)

    def test_restore_requires_complete_nonempty_actual_reduction(self):
        for size in (0, 1, 2):
            v = frame(); v['restore'] = v['restore'][:size]
            self.assertFalse(mapped(v).record.restore_exact, 'restore-incomplete')
            self.assertFalse(mapped(v).record.restore_decision_success, 'restore-decision-incomplete')
        v = frame(); v['restore'] = []
        side = bridge.map_observation(v, replace(bridge.PLANS[0], restore=()), 'candidate')
        self.assertFalse(side.record.restore_exact, 'restore-empty-plan')
        v = frame(); v['restore'][0].update(exact=False, decision_success=False)
        self.assertFalse(mapped(v).record.restore_exact)
        self.assertFalse(mapped(v).record.restore_decision_success)
        self.assertFalse(mapped(frame(1)).record.restore_exact)
        self.assertTrue(mapped(frame(1)).record.restore_decision_success)

    def test_noncompleted_and_missing_arm_have_separate_inventory(self):
        for state in ('FAILED_CLOSED_NO_EGRESS', 'UNKNOWN_EGRESS'):
            v = frame(); v.update(outcome=state, gold=[], negative=[], restore=[])
            side = mapped(v)
            self.assertFalse(side.record.restore_exact or side.record.restore_decision_success)
            self.assertFalse(side.record.observed_metrics)
            e = cell_with(v)
            self.assertEqual(e.outcomes('candidate')[state], 1)
            if state == 'UNKNOWN_EGRESS':
                self.assertEqual(e.paired_interval(bridge.GOLD_BYTES, bridge.declaration()), 'NOT_EVALUABLE')
        e = ee.PrivateEvaluator(bridge.inventory())
        e.add('base', bridge.PLANS[0].key, mapped(frame(0, 'base')).record)
        e.finalize()
        self.assertEqual(e.outcomes('candidate')['NOT_STARTED'], 2)
        self.assertEqual(e.paired_interval(bridge.GOLD_BYTES, bridge.declaration()), 'NOT_EVALUABLE')

    def test_duplicate_unknown_slots_refused(self):
        for kind in ('gold', 'negative', 'restore'):
            for mutation in ('duplicate', 'unknown'):
                v = frame()
                if mutation == 'duplicate': v[kind] = [v[kind][0], v[kind][0]]
                else: v[kind][0]['slot'] = 'extra'
                with self.assertRaises(ProducerFailure, msg='slot-refusal'): mapped(v)

    def test_closed_fields_and_identity(self):
        for name in ('gold_bytes', 'group', 'weight', 'key', 'stratum', 'pii_class', 'region', 'payload'):
            v = frame(); v[name] = 'Email' if name == 'pii_class' else 0
            with self.assertRaises(ProducerFailure, msg='extra-field-refusal'): mapped(v)
        for name, value in (('ordinal', True), ('ordinal', 1), ('arm', 'base'), ('scenario', 'extra'), ('format', 'extra'), ('kind', 'refused')):
            v = frame(); v[name] = value
            with self.assertRaises(ProducerFailure, msg='identity-refusal'):
                bridge.map_observation(v, bridge.PLANS[0], 'candidate')

    def test_closed_values_and_types(self):
        for value in (True, 1.0, -1, 1 << 64, None, '16'):
            v = frame(); v['gold'][0]['surviving_bytes'] = value
            with self.assertRaises(ProducerFailure): mapped(v)
        for verdict, count in (('full', 15), ('partial', 7), ('partial', 16), ('protected', 1), ('unknown', 1)):
            v = frame(); v['gold'][0].update(verdict=verdict, surviving_bytes=count)
            with self.assertRaises(ProducerFailure): mapped(v)
        for value in (1, None, 'true'):
            v = frame(); v['restore'][0]['exact'] = value
            with self.assertRaises(ProducerFailure): mapped(v)
        for state in ('FAILED_CLOSED_NO_EGRESS', 'UNKNOWN_EGRESS'):
            v = frame(); v['outcome'] = state
            with self.assertRaises(ProducerFailure): mapped(v)

    def test_private_repr(self):
        for value in (bridge.PLANS[0], mapped(frame()), mapped(frame()).record):
            self.assertNotIn('case0', repr(value))
            self.assertNotIn('measured_gold', repr(value))


def limits():
    return TransportLimits(handshake_seconds=2, exchange_seconds=2, invocation_seconds=10,
                           finish_seconds=.5, terminate_seconds=.1, reap_seconds=.5)


def child(mode):
    def emit(value):
        os.write(1, json.dumps(value).encode() + b'\n')
    emit(dict(format=bridge.FORMAT, kind='ready'))
    for index, line in enumerate(sys.stdin.buffer):
        request = json.loads(line)
        if mode == 'malformed-second' and index == 1:
            os.write(1, b'{bad\n'); return
        value = frame(request['ordinal'], request['arm'])
        if mode == 'coalesced':
            wire = json.dumps(value).encode() + b'\n'
            os.write(1, wire + wire); return
        if mode == 'refused':
            emit(dict(format=bridge.FORMAT, kind='refused', code='protocol')); return
        emit(value)
    if mode == 'trailing': os.write(1, b'{}\n')
    if mode == 'nonzero': sys.exit(7)


class BoundaryTests(unittest.TestCase):
    def owner(self, mode='valid'):
        return BenchSubprocess([sys.executable, str(HERE), '--child', mode], limits=limits())

    def assert_reaped(self, owner):
        try:
            self.assertIsNotNone(owner.process.returncode, 'direct-child-reaped')
            self.assertTrue(all(s.closed for s in (owner.process.stdin, owner.process.stdout, owner.process.stderr)), 'pipes-closed')
        finally:
            if owner.process.poll() is None:
                owner.process.kill(); owner.process.wait(timeout=2)
            for stream in (owner.process.stdin, owner.process.stdout, owner.process.stderr): stream.close()

    def refuse(self, owner, **kwargs):
        result = None
        try:
            caught = None
            try:
                result = bridge.run(sys.executable, _owner=owner, **kwargs)
            except BaseException as error:
                caught = error
            self.assertIs(type(caught), ProducerFailure, 'whole-cell-closed-abort')
            self.assertIsNone(result, 'no-staged-success')
            self.assertIsNone(caught.__cause__)
            self.assertIsNone(caught.__context__)
            self.assertNotIn(CANARY, str(caught))
            self.assertNotIn(CANARY, repr(caught))
        finally:
            if owner.process is not None: self.assert_reaped(owner)

    def test_three_request_second_failure_never_reaches_estimator(self):
        owner = self.owner('malformed-second')
        with mock.patch.object(bridge, 'validate_numeric', wraps=bridge.validate_numeric) as validate:
            self.refuse(owner, _order=(('base', bridge.PLANS[0]), ('candidate', bridge.PLANS[0]), ('base', bridge.PLANS[1])))
            self.assertEqual(validate.call_count, 0, 'no-estimator-after-exchange-failure')

    def test_framing_refusal_trailing_and_nonzero_abort(self):
        for mode in ('coalesced', 'refused', 'trailing', 'nonzero'):
            with self.subTest(mode=mode): self.refuse(self.owner(mode))

    def test_mapper_evaluator_and_cancellation_are_closed(self):
        for target, error in (('map_observation', RuntimeError(CANARY)), ('validate_numeric', RuntimeError(CANARY)), ('map_observation', KeyboardInterrupt(CANARY))):
            with mock.patch.object(bridge, target, side_effect=error):
                out, err = io.StringIO(), io.StringIO()
                with tempfile.TemporaryDirectory() as directory:
                    owner = self.owner(); owner.cwd = directory
                    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err): self.refuse(owner)
                    self.assertEqual(list(Path(directory).iterdir()), [])
                self.assertNotIn(CANARY, out.getvalue() + err.getvalue())
        owner = self.owner()
        with mock.patch.object(ee.PrivateEvaluator, 'add', side_effect=RuntimeError(CANARY)):
            self.refuse(owner)

    def test_preflight_handshake_and_cleanup_inside_boundary(self):
        with mock.patch.object(bridge.os.path, 'isfile', side_effect=RuntimeError(CANARY)):
            with self.assertRaises(ProducerFailure, msg='preflight-boundary'): bridge.run(sys.executable)
        owner = self.owner()
        with mock.patch.object(owner, 'receive_handshake', side_effect=RuntimeError(CANARY)):
            self.refuse(owner)
        owner = self.owner()
        cleanup = owner._cleanup
        def fail_cleanup():
            cleanup()
            raise RuntimeError(CANARY)
        with mock.patch.object(owner, '_cleanup', side_effect=fail_cleanup): self.refuse(owner)

    def test_cleanup_failure_aborts(self):
        owner = self.owner()
        cleanup = owner._cleanup
        def failed():
            cleanup()
            return False
        with mock.patch.object(owner, '_cleanup', side_effect=failed): self.refuse(owner)

    def test_finish_failure_and_deadlines_abort(self):
        owner = self.owner()
        with mock.patch.object(owner, 'finish', side_effect=ProducerFailure('producer_exit', 'finish')):
            self.refuse(owner)
        for method in ('check_message_deadline', 'check_deadline'):
            owner = self.owner()
            with mock.patch.object(owner, method, side_effect=ProducerFailure('deadline', 'payload')):
                self.refuse(owner)

    def test_evaluator_boundary(self):
        with mock.patch.object(ee.PrivateEvaluator, 'add', side_effect=RuntimeError(CANARY)):
            self.refuse(self.owner())

    def test_message_deadline_after_mapping(self):
        owner = self.owner()
        owner.limits = replace(owner.limits, exchange_seconds=.05)
        original = bridge.map_observation
        def delayed(*args):
            result = original(*args)
            time.sleep(.08)
            return result
        with mock.patch.object(bridge, 'map_observation', side_effect=delayed): self.refuse(owner)

    def test_invocation_deadline_after_estimator(self):
        owner = self.owner()
        original = bridge.validate_numeric
        def delayed(*args):
            result = original(*args)
            owner.invocation_deadline = time.monotonic() - 1
            return result
        with mock.patch.object(bridge, 'validate_numeric', side_effect=delayed), mock.patch.object(owner, 'finish', wraps=owner.finish) as finish:
            self.refuse(owner)
            self.assertEqual(finish.call_count, 0, 'deadline-before-finish')

    def test_parent_key_and_arm_binding(self):
        seen = []
        original = ee.PrivateEvaluator.add
        def capture(evaluator, arm, key, record):
            seen.append((arm, key, record.gold_bytes_surviving))
            return original(evaluator, arm, key, record)
        owner = self.owner()
        try:
            with mock.patch.object(ee.PrivateEvaluator, 'add', new=capture):
                try:
                    bridge.run(sys.executable, _owner=owner)
                except ProducerFailure as error:
                    if (error.code, error.phase) != ('protocol', 'payload'): raise
            self.assertEqual(seen, [('base', 'case0', 0), ('candidate', 'case0', 16),
                                    ('base', 'case1', 0), ('candidate', 'case1', 8)], 'parent-identity-binding')
        finally:
            if owner.process is not None: self.assert_reaped(owner)

    def test_duplicate_identity_aborts(self):
        self.refuse(self.owner(), _order=(('base', bridge.PLANS[0]), ('base', bridge.PLANS[0])))

    def test_authored_exports_are_never_called(self):
        with mock.patch.object(ee.PrivateEvaluator, 'export_receipt', side_effect=AssertionError('export-called')) as export, \
             mock.patch.object(ee.PrivateEvaluator, 'aggregate', side_effect=AssertionError('aggregate-called')) as aggregate, \
             mock.patch.object(ee.PrivateEvaluator, 'protected_case_count', side_effect=AssertionError('protected-called')) as protected:
            owner = self.owner()
            result = None
            try:
                result = bridge.run(sys.executable, _owner=owner)
            except ProducerFailure:
                pass
            self.assertEqual(export.call_count + aggregate.call_count + protected.call_count, 0, 'no-authored-export')
            self.assertIsNotNone(result, 'private-success')
            self.assertEqual(export.call_count + aggregate.call_count + protected.call_count, 0, 'no-authored-export')
            self.assert_reaped(owner)

    def test_correct_reorder_passes(self):
        order = tuple(reversed(tuple((a, p) for p in bridge.PLANS for a in ('base', 'candidate'))))
        owner = self.owner()
        self.assertTrue(bridge.run(sys.executable, _owner=owner, _order=order).numeric_verified, 'correct-reorder')
        self.assert_reaped(owner)


class RealBridgeTests:
    binary: str

    def test_real_shared_numeric_observations(self):
        with BenchSubprocess([self.binary]) as owner:
            self.assertEqual(owner.receive_handshake(), dict(format=bridge.FORMAT, kind='ready'))
            for p in bridge.PLANS:
                for arm in ('base', 'candidate'):
                    value = owner.exchange(bridge.request_for(p, arm))
                    expected = 0 if arm == 'base' else 16 if p.ordinal == 0 else 8
                    self.assertEqual(sum(r['surviving_bytes'] for r in value['gold']), expected, 'shared-numeric-observation')
                    self.assertEqual({r['slot'] for r in value['gold']}, {'a', 'b'}, 'shared-slot-coverage')
                    mapped(value)
                    owner.check_message_deadline()
            owner.finish()

    def test_real_estimator_and_reorder(self):
        for order in (None, tuple((a, p) for p in reversed(bridge.PLANS) for a in ('candidate', 'base'))):
            self.assertTrue(bridge.run(self.binary, _order=order).numeric_verified, 'real-estimator-point')


def mutation_proof(binary):
    """Actual unique-site edits; only named assertion failures count as kills."""
    import re
    python_source = ROOT/'scripts/bench/evidence_bridge.py'
    rust_source = ROOT/'crates/gaze-mcp-rmcp/tests/support/evidence_harness.rs'
    python_cases = [
        ('wrong-known-key', [('evaluator.add(arm, plan.key, side.record)', 'evaluator.add(arm, PLANS[1-plan.ordinal].key, side.record)')], 'BoundaryTests.test_parent_key_and_arm_binding', 'parent-identity-binding'),
        ('arm-swap', [('evaluator.add(arm, plan.key, side.record)', 'evaluator.add("candidate" if arm == "base" else "base", plan.key, side.record)')], 'BoundaryTests.test_parent_key_and_arm_binding', 'parent-identity-binding'),
        ('skipped-gold-slot', [("gold = rows(frame['gold'], gold_plan", "gold = rows(frame['gold'][:1], gold_plan")], 'ModelTests.test_full_coverage_operands', 'full-coverage-operands'),
        ('duplicate-slot', [('and slot not in result', 'and True')], 'ModelTests.test_duplicate_unknown_slots_refused', 'slot-refusal'),
        ('unknown-as-protected', [('if unknown:', 'if False:')], 'ModelTests.test_unknown_retains_measured_bytes_without_exact_credit', 'unknown-unavailable'),
        ('negative-uncompared-complete', [('if uncompared:', 'if False:')], 'ModelTests.test_negative_uncompared_and_absent_are_unavailable', 'negative-unavailable'),
        ('restore-incomplete-true', [("restore_resolved = rc == Coverage.COMPLETE and bool(plan.restore) and state == 'COMPLETED'", "restore_resolved = state == 'COMPLETED'")], 'ModelTests.test_restore_requires_complete_nonempty_actual_reduction', 'restore-incomplete'),
        ('extra-denominator-accepted', [('set(value) == set(keys)', 'True')], 'ModelTests.test_closed_fields_and_identity', 'extra-field-refusal'),
        ('mapper-boundary-bypass', [('@producer_boundary\ndef run(', 'def run(')], 'BoundaryTests.test_mapper_evaluator_and_cancellation_are_closed', 'whole-cell-closed-abort'),
        ('evaluator-boundary-bypass', [('@producer_boundary\ndef run(', 'def run(')], 'BoundaryTests.test_evaluator_boundary', 'whole-cell-closed-abort'),
        ('transport-catch-continue', [('            frame = child.exchange(request_for(plan, arm))', '            try:\n                frame = child.exchange(request_for(plan, arm))\n            except ProducerFailure:\n                continue')], 'BoundaryTests.test_three_request_second_failure_never_reaches_estimator', 'no-estimator-after-exchange-failure'),
        ('finish-bypass', [('        child.finish()', '        child.finished = True')], 'BoundaryTests.test_framing_refusal_trailing_and_nonzero_abort', 'whole-cell-closed-abort'),
        ('cleanup-bypass', [('    with owner as child:', '    child = owner.__enter__()\n    if True:')], 'BoundaryTests.test_cleanup_failure_aborts', 'whole-cell-closed-abort'),
        ('authored-export', [('        staged = validate_numeric(evaluator, observations)', '        evaluator.export_receipt(None, None)\n        staged = validate_numeric(evaluator, observations)')], 'BoundaryTests.test_authored_exports_are_never_called', 'no-authored-export'),
        ('message-deadline-bypass', [('            child.check_message_deadline()', '            pass')], 'BoundaryTests.test_message_deadline_after_mapping', 'whole-cell-closed-abort'),
        ('invocation-deadline-bypass', [('        staged = validate_numeric(evaluator, observations)\n        child.check_deadline()', '        staged = validate_numeric(evaluator, observations)')], 'BoundaryTests.test_invocation_deadline_after_estimator', 'deadline-before-finish'),
    ]
    rust_cases = [
        ('route-bytes-zeroed', [('self.add("gold_bytes_surviving_egress", expected.len());', 'self.add("gold_bytes_surviving_egress", 0);'), ('self.add("gold_bytes_surviving_egress", n);', 'self.add("gold_bytes_surviving_egress", 0);')]),
        ('constant-observations', [('let v = occurrence(session, observed, expected, anchor);', 'let v = occurrence(session, observed, expected, anchor);\n        let _ = v;\n        let v = Verdict::Protected;')]),
    ]
    cargo = subprocess.check_output(['rustup', 'which', '--toolchain', '1.96.0', 'cargo'], text=True).strip()
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')
    env['PYTHONPYCACHEPREFIX'] = str(Path(tempfile.gettempdir())/'bridge-mutation-unused-cache')
    def build():
        result = subprocess.run([cargo, 'build', '--offline', '--locked', '-p', 'gaze-mcp-rmcp', '--example', 'evidence_bridge'],
                                cwd=ROOT, env=env, capture_output=True, timeout=180)
        if result.returncode:
            raise AssertionError('mutation-build-failed-not-a-kill')
    results = []
    cases = [(python_source, name, edits, target, marker, False) for name, edits, target, marker in python_cases]
    cases += [(rust_source, name, edits, 'test_real_shared_numeric_observations', 'shared-numeric-observation', True) for name, edits in rust_cases]
    for source, name, edits, target, marker, rust in cases:
        original = source.read_text()
        mutated = original
        for old, new in edits:
            if mutated.count(old) != 1: raise AssertionError('mutation-site-not-unique:' + name)
            mutated = mutated.replace(old, new, 1)
        try:
            source.write_text(mutated)
            if rust:
                build()
                command = [sys.executable, str(HERE), '--integration', '--binary', binary, '--test', target]
            else:
                command = [sys.executable, '-m', 'unittest', '-v', 'test_evidence_bridge.' + target]
            result = subprocess.run(command, cwd=HERE.parent, env=env, capture_output=True, timeout=30)
            output = result.stdout + result.stderr
            killed = (result.returncode != 0 and marker.encode() in output
                      and re.search(rb'FAILED \(failures=[1-9][0-9]*\)', output) is not None
                      and b'Ran 1 test' in output and b'ERROR:' not in output)
            row = dict(id=name, killed=killed, kill_set=[target] if killed else [], assertion_marker=marker)
            results.append(row)
            print(json.dumps(row, sort_keys=True), flush=True)
        finally:
            source.write_text(original)
            if rust: build()
        if not killed: raise AssertionError('mutation-survived-or-invalid:' + name)
    return results


def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--integration', action='store_true')
    parser.add_argument('--binary')
    parser.add_argument('--child')
    parser.add_argument('--test')
    parser.add_argument('--mutation-proof', action='store_true')
    args = parser.parse_args()
    if args.child:
        child(args.child); return
    if args.mutation_proof:
        if not args.binary or not os.path.isfile(args.binary) or not os.access(args.binary, os.X_OK):
            parser.error('--mutation-proof requires an executable --binary')
        mutation_proof(str(Path(args.binary).resolve())); return
    if args.integration:
        if not args.binary or not os.path.isfile(args.binary) or not os.access(args.binary, os.X_OK):
            parser.error('--integration requires an executable --binary')
        class Integration(RealBridgeTests, unittest.TestCase):
            binary = args.binary
        suite = unittest.TestSuite([Integration(args.test)]) if args.test else unittest.defaultTestLoader.loadTestsFromTestCase(Integration)
    else:
        if args.binary: parser.error('--binary requires --integration')
        suite = unittest.defaultTestLoader.loadTestsFromModule(sys.modules[__name__])
    raise SystemExit(not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful())


if __name__ == '__main__':
    main()
