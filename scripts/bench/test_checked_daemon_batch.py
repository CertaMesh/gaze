"""Enumeration proofs must reject failed observations before scoring either arm."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from bench_subprocess import ProducerFailure, TransportLimits
from checked_daemon_batch import run_daemon_batch
import phone_differential_enumeration as phone
import iban_trailing_word_enumeration as iban
import collision_settled_enumeration as collision
import family_action_policy_matrix as family


class CheckedDaemonTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.policy = self.root / 'policy.toml'
        self.policy.write_text('')
        self.docs = [{'text': 'first'}, {'text': 'second'}]

    def daemon(self, mode):
        binary = self.root / 'daemon'
        binary.write_text('''#!/opt/homebrew/bin/python3.13
import json, sys
requests = [json.loads(line) for line in sys.stdin]
responses = [{'session_id': r['session_id'], 'clean_text': r['text'], 'manifest': []} for r in requests]
mode = MODE
if mode == 'error':
    responses = [{'session_id': r['session_id'], 'error': 'PRIVATE_PAYLOAD'} for r in requests]
elif mode == 'error_success':
    responses[0]['error'] = 'PRIVATE_PAYLOAD'
elif mode == 'missing': responses.pop()
elif mode == 'extra': responses.append(dict(responses[0], session_id='extra'))
elif mode == 'duplicate': responses.append(responses[0])
elif mode == 'wrong': responses[0]['session_id'] = 'wrong'
elif mode == 'no_id': del responses[0]['session_id']
elif mode == 'clean_type': responses[0]['clean_text'] = 1
elif mode == 'manifest_type': responses[0]['manifest'] = {}
elif mode == 'manifest_entry': responses[0]['manifest'] = [{}]
elif mode == 'no_manifest': del responses[0]['manifest']
elif mode == 'no_clean': del responses[0]['clean_text']
elif mode == 'reverse': responses.reverse()
elif mode == 'bad_json':
    print('PRIVATE_PAYLOAD')
    sys.exit(0)
elif mode == 'no_lf':
    sys.stdout.write(json.dumps(responses[0]))
    sys.exit(0)
elif mode == 'duplicate_key':
    print('{"session_id":"doc-0","session_id":"doc-0"}')
    sys.exit(0)
elif mode == 'deadline':
    __import__('time').sleep(2)

for response in responses: print(json.dumps(response, ensure_ascii=False), flush=True)
if mode == 'nonzero':
    print('PRIVATE_STDERR', file=sys.stderr)
    sys.exit(7)
'''.replace('MODE', repr(mode)).replace('/opt/homebrew/bin/python3.13', __import__('sys').executable))
        binary.chmod(0o755)
        return str(binary)

    def test_failed_batches_are_closed_failures_for_both_runners(self):
        for run in (iban.run, collision.run):
            for mode in ('error', 'error_success', 'missing', 'extra', 'duplicate', 'wrong',
                         'no_id', 'nonzero', 'clean_type', 'manifest_type', 'manifest_entry',
                         'no_manifest', 'no_clean', 'bad_json', 'no_lf', 'duplicate_key'):
                with self.subTest(runner=run.__module__, mode=mode):
                    with self.assertRaises(ProducerFailure) as caught:
                        run(self.daemon(mode), self.policy, self.docs)
                    self.assertNotIn('PRIVATE', str(caught.exception))
                    self.assertIsNone(caught.exception.__context__)

    def test_out_of_order_success_joins_by_id_and_keeps_unicode_separator(self):
        self.docs[0]['text'] = 'left\u2028right'
        for run in (iban.run, collision.run):
            with self.subTest(runner=run.__module__):
                responses = run(self.daemon('reverse'), self.policy, self.docs)
                self.assertEqual([r['clean_text'] for r in responses], [d['text'] for d in self.docs])
                self.assertEqual([r['session_id'] for r in responses], ['doc-0', 'doc-1'])

    def test_matching_error_arms_cannot_pass_collision_gate(self):
        doc = {'text': 'fixture', 'settled': True, 'country': 'fixture', 'iban_span': (0, 7)}
        binary = self.daemon('error')
        output = self.root / 'report.json'
        with patch.object(collision, 'documents', return_value=[doc]), \
             patch.object(collision, 'POLICIES', {'fixture': ('"core"', '"global"', False, False)}), \
             patch('sys.argv', ['proof', binary, binary, str(output)]), \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(ProducerFailure):
                collision.main()
        self.assertFalse(output.exists())

    def test_matching_error_arms_cannot_pass_family_gate(self):
        doc = {'text': 'fixture', 'iban_span': (0, 7), 'country': 'fixture'}
        binary = self.daemon('error')
        output = self.root / 'report.json'
        with patch.object(iban, 'documents', return_value=[doc]), \
             patch.object(iban, 'binary_identity', side_effect=[{'sha256': 'base'}, {'sha256': 'head'}]), \
             patch.object(family, 'POLICIES', {'fixture': ([], 'tokenize', False)}), \
             patch.object(family, 'LOCALES', {'fixture': ('"core"', '"global"')}), \
             patch('sys.argv', ['proof', binary, binary, str(output)]), \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(ProducerFailure):
                family.main()
        self.assertFalse(output.exists())

    def test_transport_limits_abort_without_payload_diagnostics(self):
        for mode, limits, code in (
            ('reverse', TransportLimits(request_bytes=16, chunk_bytes=16), 'input_limit'),
            ('reverse', TransportLimits(stdout_bytes=16, chunk_bytes=16), 'output_limit'),
            ('nonzero', TransportLimits(stderr_bytes=4, chunk_bytes=4), 'stderr_limit'),
            ('deadline', TransportLimits(exchange_seconds=0.1), 'deadline'),
        ):
            with self.subTest(code=code):
                with self.assertRaises(ProducerFailure) as caught:
                    run_daemon_batch(self.daemon(mode), self.policy, self.docs, limits=limits)
                self.assertEqual(caught.exception.code, code)
                self.assertIsNone(caught.exception.__context__)

    def test_phone_runner_joins_reordered_responses(self):
        rows = [('fixture', 'first', {0}), ('fixture', 'second', {1, 2})]
        self.assertEqual(phone.run(self.daemon('reverse'), rows, self.policy),
                         [({0}, set(), {0}), ({1, 2}, set(), {1, 2})])
        with self.assertRaises(ProducerFailure):
            phone.run(self.daemon('error'), rows, self.policy)
