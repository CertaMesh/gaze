"""Subprocess checks for comparison provenance refusals; no timing or models."""
from pathlib import Path
import os
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class ComparisonRefusalTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='gaze-compare-refusal-')
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        scripts = self.root / 'scripts/bench'
        scripts.mkdir(parents=True)
        for path in (ROOT / 'scripts/bench').glob('*.py'):
            shutil.copyfile(path, scripts / path.name)
        contract = Path('docs/reference/benchmarks/class-commitments-v1.json')
        (self.root / contract).parent.mkdir(parents=True)
        shutil.copyfile(ROOT / contract, self.root / contract)
        rulepacks = Path('crates/gaze-recognizers/embedded')
        (self.root / rulepacks).mkdir(parents=True)
        for path in (ROOT / rulepacks).glob('*.toml'):
            shutil.copyfile(path, self.root / rulepacks / path.name)
        shutil.copyfile(ROOT / 'scripts/bench/no_opf_models.toml', scripts / 'no_opf_models.toml')
        self.env = dict(os.environ, GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull)
        self.git('init', '--quiet')
        self.git('add', 'scripts/bench', str(contract), str(rulepacks))
        self.git('-c', 'user.name=Synthetic Fixture', '-c', 'user.email=fixture@example.invalid',
                 'commit', '--quiet', '-m', 'synthetic comparison fixture')
        self.revision = self.git('rev-parse', 'HEAD').stdout.strip()

    def git(self, *args):
        return subprocess.run(['git', *args], cwd=self.root, env=self.env,
                              text=True, capture_output=True, check=True)

    def refused(self, revision, message, mode='parity'):
        result = subprocess.run(
            [sys.executable, 'scripts/bench/compare_evidence_eval.py',
             '--baseline-revision', revision, '--mode', mode],
            cwd=self.root, env=self.env, text=True, capture_output=True,
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn(message, result.stderr)
        self.assertNotIn('Traceback', result.stderr)
        self.assertEqual(result.stdout, '', 'refused provenance must not emit evidence')

    def test_mutable_and_abbreviated_revisions_are_refused(self):
        self.git('tag', 'fixture-tag')
        for revision in ('HEAD', 'fixture-tag', 'refs/heads/main', self.revision[:8]):
            with self.subTest(revision=revision):
                self.refused(revision, 'full lowercase commit hash')

    def test_unknown_full_revision_is_refused_cleanly(self):
        self.refused('0' * 40, 'baseline commit is unavailable locally')

    def test_identical_sources_are_refused(self):
        self.refused(self.revision, 'sources are identical')

    def test_contract_drift_is_refused(self):
        path = self.root / 'docs/reference/benchmarks/class-commitments-v1.json'
        path.write_bytes(path.read_bytes() + b'\n')
        self.refused(self.revision, 'class contracts differ')

    def test_each_shared_dependency_drift_is_refused(self):
        for name in ('evidence_protocol.py', 'gaze_bench_score.py', 'bench_subprocess.py'):
            with self.subTest(dependency=name):
                path = self.root / 'scripts/bench' / name
                original = path.read_bytes()
                try:
                    path.write_bytes(original + b'\n# synthetic dependency drift\n')
                    self.refused(self.revision, f'dependency differ: scripts/bench/{name}')
                finally:
                    path.write_bytes(original)

    def test_timing_without_lease_is_refused(self):
        self.refused(self.revision, 'timing requires --machine-lease', mode='timing')

    def test_import_time_data_drift_is_refused(self):
        rulepack = next((self.root / 'crates/gaze-recognizers/embedded').glob('*.toml'))
        for path in (rulepack, self.root / 'scripts/bench/no_opf_models.toml'):
            with self.subTest(data=path.name):
                original = path.read_bytes()
                try:
                    path.write_bytes(original + b'\n# synthetic data drift\n')
                    self.refused(self.revision, 'initialization data differ')
                finally:
                    path.write_bytes(original)
        extra = rulepack.parent / 'synthetic-added.toml'
        extra.write_text('')
        self.refused(self.revision, 'rulepack inventories differ')
        extra.unlink()
        rulepack.unlink()
        self.refused(self.revision, 'rulepack inventories differ')


if __name__ == '__main__':
    unittest.main()
