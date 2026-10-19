import copy
from pathlib import Path
import tempfile
import unittest

import export_portable_record as export
import scorecard_record


class PortableExportTests(unittest.TestCase):
    def test_only_policy_path_changes(self):
        header = {'scorecard': {'runner_provenance': {'policy': {
            'path': str(Path.home() / 'synthetic/policy.toml'), 'sha256': 'a' * 64}},
            'gaze': {'revision': 'b' * 40}, 'runs': [{'aggregate': 42}]}, 'documents': []}
        original = copy.deepcopy(header)
        expected = copy.deepcopy(header)
        expected['scorecard']['runner_provenance']['policy']['path'] = '~/synthetic/policy.toml'
        self.assertEqual(export.portable_header(header), expected)
        self.assertEqual(header, original)

    def test_nonportable_policy_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ValueError):
                export.portable_header({'scorecard': {'runner_provenance': {'policy': {'path': str(Path(tmp) / 'policy.toml')}}}})

    def test_publication_copy_preserves_every_observation_and_pins_both_files(self):
        source = Path(__file__).resolve().parents[2] / 'docs/reference/benchmarks/observations-v0.17.0.jsonl.gz'
        header, observations = scorecard_record._read(source)
        header['scorecard'].setdefault('runner_provenance', {})['policy'] = {
            'path': str(Path.home() / 'synthetic/policy.toml'), 'sha256': 'a' * 64}
        with tempfile.TemporaryDirectory() as tmp:
            capture, published = Path(tmp) / 'capture.jsonl.gz', Path(tmp) / 'published.jsonl.gz'
            scorecard_record._write_rows(capture, [header, *observations])
            receipt = export.export_record(capture, published)
            normalized, result = scorecard_record._read(published)
            self.assertEqual(result, observations)
            self.assertEqual(normalized, export.portable_header(header))
            self.assertEqual(receipt['source_sha256'], export.sha256(capture))
            self.assertEqual(receipt['exported_sha256'], export.sha256(published))
            self.assertTrue(receipt['observation_rows_unchanged'])
            self.assertEqual(receipt['metadata_normalization'], 'policy-path-home-prefix')
