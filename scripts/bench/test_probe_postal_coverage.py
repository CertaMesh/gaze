"""Postal attribution evidence rejects refusal and mismatched producer output."""

import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import agentic_layers
import probe_postal_coverage as probe


class PostalProbeTests(unittest.TestCase):
    def responses(self, all_new_cells=False):
        return [{'fixture_id': r.uid, 'final_protection_trace': []}
                for r in agentic_layers._coverage_records('test') if all_new_cells or r.family == 'postal_de']

    def measure(self, responses, all_new_cells=False):
        with tempfile.TemporaryDirectory() as directory:
            policy = Path(directory) / 'policy.toml'
            policy.write_text('[locale]\nactive = ["de-DE"]\n')
            producer = SimpleNamespace(stdout='\n'.join(map(json.dumps, responses)))
            with patch.object(probe.subprocess, 'run', return_value=producer):
                return probe.measure(Path('clean_for_bench'), policy, all_new_cells=all_new_cells)

    def test_evidence_has_no_source_values(self):
        result = self.measure(self.responses())
        self.assertEqual(len(result['documents']), 20)
        self.assertEqual(result['locale_chain'], ['de-DE'])
        self.assertTrue(all(set(d) == {'uid', 'layer', 'trace'} for d in result['documents']))

    def test_all_new_cells_are_recorded_without_values(self):
        result = self.measure(self.responses(True), True)
        self.assertEqual(len(result["documents"]), 680)
        self.assertTrue(all(set(d) == {"uid", "layer", "trace"} for d in result["documents"]))

    def test_refusal_cannot_be_recorded_as_empty_protection(self):
        responses = self.responses()
        responses[0].pop('final_protection_trace')
        with self.assertRaisesRegex(ValueError, 'refused'):
            self.measure(responses)

    def test_missing_and_reordered_responses_fail(self):
        responses = self.responses()
        with self.assertRaisesRegex(ValueError, 'count'):
            self.measure(responses[:-1])
        with self.assertRaisesRegex(ValueError, 'different document'):
            self.measure(list(reversed(responses)))


if __name__ == '__main__':
    unittest.main()
