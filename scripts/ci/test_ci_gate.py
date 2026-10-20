import json
from pathlib import Path
import unittest

import ci_gate as gate


class LaneSelection(unittest.TestCase):
    def plan(self, *paths, event='pull_request'):
        return gate.make_plan(event, paths)

    def test_documentation_only_is_lightweight(self):
        plan = self.plan('README.md', 'docs/guides/setup.md')
        self.assertEqual(plan['packages'], [])
        self.assertFalse(plan['msrv'])
        self.assertFalse(plan['bench'])
        self.assertFalse(plan['dylint'])

    def test_rust_includes_every_reverse_dependency(self):
        plan = self.plan('crates/gaze-inspection/src/lib.rs')
        self.assertIn('gaze-inspection', plan['packages'])
        self.assertIn('gaze-proxy-dashboard', plan['packages'])
        self.assertIn('gaze-cli', plan['packages'])
        self.assertTrue(plan['msrv'])
        self.assertTrue(plan['dylint'])

    def test_benchmark_docs_and_evidence_trigger_python(self):
        for path in ['scripts/bench/score.py', 'docs/reference/benchmarks/README.md',
                     'docs/reference/benchmarks/release-history.json']:
            with self.subTest(path=path):
                self.assertTrue(self.plan(path)['bench'])

    def test_manifests_ci_and_unknown_paths_fall_back_wide(self):
        for path in ['Cargo.lock', 'crates/gaze-types/Cargo.toml',
                     '.github/workflows/test.yml', 'scripts/ci/ci_gate.py',
                     '.cargo/config.toml', 'rust-toolchain.toml', 'new/input.dat']:
            with self.subTest(path=path):
                plan = self.plan(path)
                self.assertTrue(plan['wide'])
                self.assertEqual(set(plan['packages']), set(gate.workspace()[0].values()))
                self.assertTrue(all(plan[key] for key in ['msrv', 'bench', 'dylint']))

    def test_lint_and_audit_boundary_run_dylint(self):
        for path in ['lint/dylint/src/lib.rs', 'crates/gaze-audit/src/lib.rs',
                     'crates/gaze-types/src/logger.rs', 'crates/gaze-cli/src/restore.rs']:
            self.assertTrue(self.plan(path)['dylint'], path)

    def test_integration_ignores_path_filter(self):
        for event in ['merge_group', 'push', 'workflow_dispatch']:
            plan = self.plan('README.md', event=event)
            self.assertTrue(plan['integration'])
            self.assertTrue(all(plan[key] for key in ['msrv', 'bench', 'dylint']))
        with self.assertRaises(ValueError):
            self.plan('README.md', event='unknown')

    def test_empty_diff_fails_wide(self):
        self.assertTrue(self.plan()['wide'])


class Aggregate(unittest.TestCase):
    def needs(self, plan):
        needs = {job: {'result': 'skipped'} for job in gate.ALL_JOBS}
        needs['plan'] = {'result': 'success', 'outputs': gate.outputs(plan)}
        for job in gate.required_jobs(plan, 'integration'):
            needs[job] = {'result': 'success'}
        return needs

    def test_every_mandatory_failure_cancel_skip_or_missing_is_red(self):
        for event in ['merge_group', 'push', 'workflow_dispatch', 'pull_request']:
            for paths in [('README.md',), ('crates/gaze-types/src/lib.rs',),
                          ('scripts/bench/score.py',), ('.github/workflows/test.yml',)]:
                plan = gate.make_plan(event, paths)
                needs = self.needs(plan)
                gate.check(event, needs, 'integration')
                for job in gate.required_jobs(plan, 'integration') | {'plan'}:
                    for result in ['failure', 'cancelled', 'skipped', '', None]:
                        broken = json.loads(json.dumps(needs))
                        if result is None:
                            del broken[job]
                        else:
                            broken[job]['result'] = result
                        with self.subTest(event=event, job=job, result=result):
                            with self.assertRaises(ValueError):
                                gate.check(event, broken, 'integration')

    def test_missing_or_malformed_plan_output_is_red(self):
        plan = gate.make_plan('pull_request', ['README.md'])
        for key in gate.outputs(plan):
            for value in [None, 'invalid']:
                needs = self.needs(plan)
                if value is None:
                    del needs['plan']['outputs'][key]
                else:
                    needs['plan']['outputs'][key] = value
                with self.assertRaises(ValueError):
                    gate.check('pull_request', needs, 'integration')

    def test_optional_jobs_may_skip_but_never_fail(self):
        plan = gate.make_plan('pull_request', ['README.md'])
        needs = self.needs(plan)
        for result in ['failure', 'cancelled']:
            needs['dylint']['result'] = result
            with self.assertRaises(ValueError):
                gate.check('pull_request', needs, 'integration')

    def test_legacy_contexts_require_their_owners_in_each_lane(self):
        for event in ['pull_request', 'merge_group']:
            plan = gate.make_plan(event, ['README.md'])
            for context in ['test', 'xtask']:
                needs = self.needs(plan)
                gate.check(event, needs, context)
                for job in gate.required_jobs(plan, context):
                    needs[job]['result'] = 'skipped'
                    with self.assertRaises(ValueError):
                        gate.check(event, needs, context)
                    needs[job]['result'] = 'success'


if __name__ == '__main__':
    unittest.main()
