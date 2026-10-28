import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import ci_gate as gate
import ci_inputs

FULL_OWNERS = {'msrv', 'test-support', 'workspace-shards', 'test-coverage',
               'benchmark-model-free', 'xtask-feature-gates', 'workspace-default',
               'dylint', 'test', 'xtask-gates'}


class LaneSelection(unittest.TestCase):
    def plan(self, *paths, event='pull_request'):
        return gate.make_plan(event, paths, 'enabled')

    def test_readme_requires_full_gate_owners(self):
        plan = self.plan('crates/gaze-document/README.md')
        self.assertEqual(gate.required_jobs(plan, 'integration'), FULL_OWNERS)

    def test_every_declared_input_requires_actual_full_owners(self):
        table = json.loads(ci_inputs.TABLE.read_text())
        for path in table['paths'] + [
                'docs/tutorials/new-page.md', 'crates/gaze-document/custom-readme.md',
                'crates/gaze-types/src/new_module.rs', 'crates/xtask/fixtures/new.json',
                'crates/gaze-types/build.rs', 'crates/gaze/tests/ui/new.rs',
                'crates/gaze/tests/fixtures/new.rs', 'crates/gaze/testdata/new.rs',
                'crates/gaze-recognizers/embedded/new.toml', 'tests/ui/new.stderr']:
            with self.subTest(path=path):
                plan = self.plan(path)
                self.assertEqual(gate.required_jobs(plan, 'integration'), FULL_OWNERS)
                self.assertEqual(gate.required_jobs(plan, 'xtask'),
                                 {'test', 'xtask-feature-gates', 'workspace-default'})
                self.assertEqual(gate.required_jobs(plan, 'test'),
                                 {'test-support', 'workspace-shards', 'test-coverage'})

    def test_full_input_table_has_no_undisclosed_includes_or_gate_reads(self):
        ci_inputs.audit(json.loads(ci_inputs.TABLE.read_text()))

    def test_new_include_and_xtask_read_targets_fail_the_audit(self):
        for reader, before, after in [
                ('crates/demo/src/lib.rs', 'include_str!("../../../inputs/old.txt");',
                 'include_str!(\n r#"../../../inputs/new.txt"#\n);'),
                ('crates/xtask/src/lib.rs',
                 'const INPUT: &str = "inputs/old.txt"; fn run() { fs::read(INPUT); }',
                 'const INPUT: &str = "inputs/new.txt"; fn run() { fs::read(INPUT); }')]:
            with self.subTest(reader=reader), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                for name, content in [(reader, before), ('inputs/old.txt', 'old'),
                                      ('inputs/new.txt', 'new')]:
                    path = root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(content)
                subprocess.run(['git', 'init', '-q', str(root)], check=True)
                subprocess.run(['git', 'add', reader, 'inputs/old.txt', 'inputs/new.txt'],
                               cwd=root, check=True)
                table = ci_inputs.discover(root)
                ci_inputs.audit(table, root)
                (root / reader).write_text(after)
                with self.assertRaisesRegex(ValueError, 'missing paths: inputs/new.txt'):
                    ci_inputs.audit(table, root)

    def test_changed_computed_gate_reads_require_review(self):
        table = json.loads(ci_inputs.TABLE.read_text())
        table['gate_readers'].pop('crates/xtask/src/readme_version_check.rs')
        with self.assertRaisesRegex(ValueError, 'audit computed reads'):
            ci_inputs.audit(table)

    def test_unresolved_include_fails_closed(self):
        with self.assertRaisesRegex(ValueError, 'computed include'):
            list(ci_inputs.includes(Path('src/lib.rs'),
                                    'include!(concat!(env!("OUT_DIR"), "/generated.rs"));',
                                    Path('.')))

    def test_shared_field_observer_selects_both_consumers(self):
        plan = self.plan('crates/gaze/src/test_support.rs')
        self.assertTrue({'gaze-pii', 'gaze-proxy', 'gaze-mcp-core'}.issubset(plan['packages']))

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
        plan = self.plan('crates/gaze-inspection/tests/compile_fail.rs')
        self.assertFalse(plan['integration'])
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

    def test_aggregate_reports_lane_and_rejects_fast_plan_without_activation(self):
        for value in ['', 'enabled']:
            plan = gate.make_plan('pull_request', ['CODE_OF_CONDUCT.md'], value)
            needs = self.needs(plan)
            message = gate.check('pull_request', needs, 'integration', value)
            self.assertIn('PR FAST LANE ONLY' if value else 'FULL INTEGRATION', message)
            if value:
                with self.assertRaises(ValueError):
                    gate.check('pull_request', needs, 'integration', '')
            else:
                self.assertIn('FULL INTEGRATION',
                              gate.check('pull_request', needs, 'integration', 'enabled'))

    def test_full_plan_is_accepted_on_enabled_pr_but_fast_is_rejected_elsewhere(self):
        full = gate.make_plan('pull_request', ['crates/gaze-document/README.md'], 'enabled')
        for context in ['integration', 'test', 'xtask']:
            self.assertIn('FULL INTEGRATION',
                          gate.check('pull_request', self.needs(full), context, 'enabled'))
        fast = gate.make_plan('pull_request', ['CODE_OF_CONDUCT.md'], 'enabled')
        for event in gate.EVENTS:
            for value in ['', 'enabled']:
                if event == 'pull_request' and value == 'enabled':
                    continue
                with self.subTest(event=event, value=value), self.assertRaises(ValueError):
                    gate.check(event, self.needs(fast), 'integration', value)

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
