import copy
import json
from pathlib import Path
import subprocess
import sys
import unittest

import external_quality_receipt as receipt

class AggregateOnlyTests(unittest.TestCase):
    def counts(self):
        return receipt.quality_counts({
            "pipeline_availability": {"attempted_documents": 2, "completed_documents": 2, "failed_closed_documents": 0,
                                      "errors": object(), "document_ids": object()},
            "pipeline_contract": {"documents": 2, "restore_exact_documents": 1, "restore_success_decisions": 2,
                                  "manifest_valid_documents": 1, "failure_details": object()},
            "text": object(), "spans": object(), "process": object(), "failed_closed_population": object(),
        })

    def value(self):
        pins = {name: "a" * (40 if name.endswith("revision") else 64) for name in receipt.RECEIPTS}
        pins['inputs_sha256'] = {name: 'b' * 64 for name in receipt.INPUTS['sealed']}
        return {"schema_version": 1, "benchmark": "sealed", "receipts": pins,
                "quality": {name: self.counts() for name in receipt.SETS['sealed']}}

    def test_reads_only_fixed_counters_and_outputs_closed_schema(self):
        value = self.value()
        receipt.validate_receipt(value)
        self.assertEqual(json.loads(json.dumps(value)), value)
        for row in value['quality'].values():
            self.assertEqual(set(row), receipt.COUNTERS)
            self.assertTrue(all(type(count) is int for count in row.values()))

    def test_extra_fields_anywhere_are_rejected(self):
        for path in ((), ('receipts',), ('receipts', 'inputs_sha256'), ('quality',), ('quality', 'openpii/all')):
            with self.subTest(path=path):
                value = self.value(); target = value
                for key in path: target = target[key]
                target['document_ids'] = ['synthetic-private-marker']
                with self.assertRaises(ValueError): receipt.validate_receipt(value)

    def test_detail_text_and_invalid_counts_are_rejected(self):
        for bad in ('synthetic-private-marker', True, -1, 1.5):
            with self.subTest(bad=bad):
                value = self.value()
                value['quality']['openpii/all']['attempted_documents'] = bad
                with self.assertRaises(ValueError): receipt.validate_receipt(value)
        value = self.value(); value['receipts']['driver_sha256'] = 'synthetic-private-marker'
        with self.assertRaises(ValueError): receipt.validate_receipt(value)

    def test_inconsistent_counts_are_rejected(self):
        for field in ('attempted_documents', 'completed_documents', 'contract_documents', 'restore_failures', 'manifest_invalid_documents'):
            value = self.value(); value['quality']['openpii/all'][field] += 1
            with self.subTest(field=field), self.assertRaises(ValueError): receipt.validate_receipt(value)

    def test_driver_output_is_discarded_even_on_exception(self):
        code = '''
import os, subprocess, sys
from external_quality_receipt import discard_driver_output
try:
    with discard_driver_output():
        print('synthetic-private-marker', end='')
        print('synthetic-private-marker', end='', file=sys.stderr)
        os.write(1, b'synthetic-private-marker')
        subprocess.run([sys.executable, '-c', 'print("synthetic-private-marker")'], check=True)
        raise RuntimeError('synthetic-private-marker')
except RuntimeError:
    print('FIXED_CODE')
'''
        result = subprocess.run([sys.executable, '-c', code], cwd=Path(receipt.__file__).parent,
                                capture_output=True, text=True, check=True)
        self.assertEqual(result.stdout, 'FIXED_CODE\n')
        self.assertEqual(result.stderr, '')

if __name__ == '__main__': unittest.main()
