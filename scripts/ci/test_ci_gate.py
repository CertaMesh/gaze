import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import ci_gate as gate


class LaneSelection(unittest.TestCase):
    def plan(self, *paths, event='pull_request'):
        return gate.make_plan(event, paths, 'enabled')

    def test_documentation_only_is_lightweight(self):
        plan = self.plan('CODE_OF_CONDUCT.md', 'LICENSE-MIT')
        self.assertEqual(plan['packages'], [])
        self.assertFalse(plan['msrv'])
        self.assertFalse(plan['bench'])
        self.assertFalse(plan['dylint'])

    def test_rust_consumed_inputs_fall_back_wide(self):
        for path in [
                'docs/how-to/rust-library.md', 'docs/reference/policy.md',
                'docs/reference/policy.example.toml', 'CHANGELOG.md', 'README.md',
                'UPGRADE.md', 'docs/reference/redaction-classes.md',
                'docs/reference/benchmarks/README.md',
                'docs/explanation/how-gaze-works.md', 'docs/tutorials/getting-started.md',
                'docs/reference/cli.md', 'docs/new-input.md',
                'scripts/bench/fixtures/evidence/mcp_route_v1.golden.json',
                'scripts/bench/fixtures/jwt_payload/policy.toml',
                'scripts/bench/trace-lineage-vocabulary.json',
                'tests/fixtures/new-input.md', 'dylint.toml',
                'scripts/fetch/fetch-ner-model.sh']:
            with self.subTest(path=path):
                plan = self.plan(path)
                self.assertTrue(plan['wide'])
                self.assertEqual(set(plan['packages']), set(gate.workspace()[0].values()))

    def test_crate_local_inputs_select_their_consumers(self):
        for path, owner in [
                ('crates/gaze/tests/fixtures/session_snapshot_v3_text_only.bin', 'gaze-pii'),
                ('crates/gaze-cli/README.md', 'gaze-cli'),
                ('crates/gaze-recognizers/embedded/core.toml', 'gaze-pii'),
                ('crates/gaze-proxy-dashboard/assets/index.html', 'gaze-cli')]:
            with self.subTest(path=path):
                self.assertIn(owner, self.plan(path)['packages'])

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
                     '.cargo/config.toml', 'rust-toolchain.toml', 'new/input.dat',
                     'fixtures/embedded.md', 'scripts/bench/validator_recall_probe/Cargo.toml',
                     'scripts/bench/validator_recall_probe/src/main.rs']:
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

    def test_only_exact_enabled_value_allows_fast_prs(self):
        full_owners = {'msrv', 'test-support', 'workspace-shards', 'test-coverage',
                       'benchmark-model-free', 'xtask-feature-gates', 'workspace-default',
                       'dylint', 'test', 'xtask-gates'}
        for value in ['', 'disabled', 'true', 'ENABLED', 'enabled ', 'enabled']:
            for event in gate.EVENTS:
                with self.subTest(value=value, event=event):
                    plan = gate.make_plan(event, ['CODE_OF_CONDUCT.md'], value)
                    full = event != 'pull_request' or value != 'enabled'
                    self.assertEqual(plan['integration'], full)
                    if full:
                        self.assertEqual(gate.required_jobs(plan, 'integration'), full_owners)
                        self.assertEqual(set(plan['packages']), set(gate.workspace()[0].values()))
                    else:
                        self.assertEqual(plan['packages'], [])

    def test_unset_variable_defaults_to_full_on_docs_only_pr(self):
        plan = gate.make_plan('pull_request', ['CODE_OF_CONDUCT.md'])
        self.assertTrue(plan['integration'])
        self.assertTrue(plan['wide'])


class Aggregate(unittest.TestCase):
    def needs(self, plan):
        needs = {job: {'result': 'skipped'} for job in gate.ALL_JOBS}
        needs['plan'] = {'result': 'success', 'outputs': gate.outputs(plan)}
        for job in gate.required_jobs(plan, 'integration'):
            needs[job] = {'result': 'success'}
        return needs

    def test_every_mandatory_failure_cancel_skip_or_missing_is_red(self):
        for fast_lane, event in [(value, event) for value in ['', 'enabled']
                                 for event in gate.EVENTS]:
            for paths in [('CODE_OF_CONDUCT.md',), ('README.md',),
                          ('crates/gaze-types/src/lib.rs',),
                          ('scripts/bench/score.py',), ('.github/workflows/test.yml',)]:
                plan = gate.make_plan(event, paths, fast_lane)
                needs = self.needs(plan)
                gate.check(event, needs, 'integration', fast_lane)
                for job in gate.required_jobs(plan, 'integration') | {'plan'}:
                    for result in ['failure', 'cancelled', 'skipped', '', None]:
                        broken = json.loads(json.dumps(needs))
                        if result is None:
                            del broken[job]
                        else:
                            broken[job]['result'] = result
                        with self.subTest(event=event, job=job, result=result):
                            with self.assertRaises(ValueError):
                                gate.check(event, broken, 'integration', fast_lane)

    def test_missing_or_malformed_plan_output_is_red(self):
        plan = gate.make_plan('pull_request', ['CODE_OF_CONDUCT.md'], 'enabled')
        for key in gate.outputs(plan):
            for value in [None, 'invalid']:
                needs = self.needs(plan)
                if value is None:
                    del needs['plan']['outputs'][key]
                else:
                    needs['plan']['outputs'][key] = value
                with self.assertRaises(ValueError):
                    gate.check('pull_request', needs, 'integration', 'enabled')

    def test_optional_jobs_may_skip_but_never_fail(self):
        plan = gate.make_plan('pull_request', ['CODE_OF_CONDUCT.md'], 'enabled')
        needs = self.needs(plan)
        for result in ['failure', 'cancelled']:
            needs['dylint']['result'] = result
            with self.assertRaises(ValueError):
                gate.check('pull_request', needs, 'integration', 'enabled')

    def test_legacy_contexts_require_their_owners_in_each_lane(self):
        for fast_lane, event in [(value, event) for value in ['', 'enabled']
                                 for event in ['pull_request', 'merge_group']]:
            plan = gate.make_plan(event, ['CODE_OF_CONDUCT.md'], fast_lane)
            for context in ['test', 'xtask']:
                needs = self.needs(plan)
                gate.check(event, needs, context, fast_lane)
                for job in gate.required_jobs(plan, context):
                    needs[job]['result'] = 'skipped'
                    with self.assertRaises(ValueError):
                        gate.check(event, needs, context, fast_lane)
                    needs[job]['result'] = 'success'

    def test_aggregate_reports_lane_and_rejects_activation_mismatch(self):
        for value in ['', 'enabled']:
            plan = gate.make_plan('pull_request', ['CODE_OF_CONDUCT.md'], value)
            needs = self.needs(plan)
            message = gate.check('pull_request', needs, 'integration', value)
            self.assertIn('PR FAST LANE ONLY' if value else 'FULL INTEGRATION', message)
            with self.assertRaises(ValueError):
                gate.check('pull_request', needs, 'integration', '' if value else 'enabled')

    def test_cli_uses_activation_variable_for_planning_and_checking(self):
        with tempfile.TemporaryDirectory() as temporary:
            for value in [None, 'disabled', 'ENABLED', 'enabled']:
                with self.subTest(value=value):
                    env = dict(os.environ, GITHUB_OUTPUT=str(Path(temporary) / 'outputs'),
                               GITHUB_STEP_SUMMARY=str(Path(temporary) / 'summary'),
                               EVENT_NAME='pull_request')
                    env.pop('GAZE_CI_FAST_LANE', None)
                    if value is not None:
                        env['GAZE_CI_FAST_LANE'] = value
                    command = [sys.executable, str(gate.ROOT / 'scripts/ci/ci_gate.py')]
                    result = subprocess.run(
                        command + ['plan', '--event', 'pull_request', '--paths-json',
                                   '["CODE_OF_CONDUCT.md"]'],
                        env=env, check=True, capture_output=True, text=True)
                    plan = json.loads(result.stdout)
                    self.assertEqual(plan['integration'], value != 'enabled')
                    env['NEEDS_JSON'] = json.dumps(self.needs(plan))
                    result = subprocess.run(command + ['check', '--context', 'integration'],
                                            env=env, check=True, capture_output=True, text=True)
                    self.assertIn('PR FAST LANE ONLY' if value == 'enabled' else 'FULL INTEGRATION',
                                  result.stdout)


if __name__ == '__main__':
    unittest.main()
