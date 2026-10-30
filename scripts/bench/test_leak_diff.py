"""New exposures must remain visible even when total protection improves."""

import contextlib
import copy
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import agentic_layers as agentic
import gaze_bench_score as score
import scorecard_record as record
import leak_diff
from test_agentic_layers import _scorecard


class LeakDiffTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def capture(self, name, spans, *, gold=None, refused=False, prediction_class='Email'):
        gold = gold if gold is not None else [[0, 10, 'EMAIL']]
        document = dict(id='synthetic-document', layer='C', utf8_bytes=20, gold=gold,
                        language='en', region='US', source_dataset='synthetic',
                        negative_category=None, cell=None, validator=None)
        population = dict(document_ids=['synthetic-document'])
        card = dict(dataset=dict(integrity=dict(sha256='a' * 64)),
                    runs=[dict(config='production', scored_population=population if not refused else dict(document_ids=[]),
                               failed_closed_population=population if refused else dict(document_ids=[]))])
        header = dict(kind='header', schema_version=1, corpus_sha256='a' * 64,
                      documents=[document], scorecard=card, layer_identity=None)
        response = dict(fixture_id=document['id'], refused=refused,
                        final_protection_trace=[dict(raw_start=a, raw_end=b, action='tokenize', provenance={},
                                                     **{'class': prediction_class}) for a, b in spans])
        if refused:
            response['pipeline_error_code'] = 'policy'
            response['pipeline_error_stage'] = 'clean'
            response.pop('final_protection_trace')
        observation = dict(kind='observation', layer='C', config='production',
                           document_id=document['id'], response=response)
        path = self.root / (name + '.jsonl.gz')
        record._write_rows(path, [header, observation])
        return path

    def diff(self, base_spans, candidate_spans, **kwargs):
        return leak_diff.report(self.capture('base', base_spans, **kwargs),
                                self.capture('candidate', candidate_spans, **kwargs), config='production')

    def counts(self, report):
        row = report['layers']['C']
        return row['new_exposed_bytes'], row['newly_protected_bytes'], row['net_leaked_bytes']

    def test_pure_gain(self):
        report = self.diff([], [(0, 10)])
        self.assertEqual(self.counts(report), (0, 10, -10))
        self.assertEqual(report['new_exposure_spans'], [])

    def test_pure_loss_and_value_free_offsets(self):
        report = self.diff([(0, 10)], [(0, 3)])
        self.assertEqual(self.counts(report), (7, 0, 7))
        self.assertEqual(report['new_exposure_spans'], [dict(layer='C', document_id='synthetic-document',
                                                            start=3, end=10, **{'class': 'EMAIL'})])
        self.assertEqual(report['layers']['C']['classes']['EMAIL']['new_exposed_bytes'], 7)
        self.assertTrue(report['report_only'])

    def test_mixed_net_gain_does_not_hide_new_exposure(self):
        report = self.diff([(0, 3)], [(3, 10)])
        self.assertEqual(self.counts(report), (3, 7, -4))
        self.assertEqual(report['layers']['C']['classes']['EMAIL']['new_exposed_bytes'], 3)
        self.assertEqual(report['new_exposure_spans'][0]['end'], 3)

    def test_overlapping_gold_and_protection_count_each_byte_once(self):
        report = self.diff([(0, 5), (2, 7)], [(3, 10)],
                           gold=[[0, 8, 'EMAIL'], [2, 10, 'EMAIL'], [4, 9, 'PERSON']])
        self.assertEqual(self.counts(report), (3, 3, 0))
        self.assertEqual(report['layers']['C']['classes']['EMAIL']['new_exposed_bytes'], 3)
        self.assertEqual(report['layers']['C']['classes']['PERSON']['new_exposed_bytes'], 0)
        self.assertEqual(report['layers']['C']['classes']['PERSON']['newly_protected_bytes'], 2)

    def test_prediction_class_change_on_same_bytes_is_not_exposure(self):
        base = self.capture('base', [(0, 10)], prediction_class='Email')
        candidate = self.capture('candidate', [(0, 10)], prediction_class='Person')
        self.assertEqual(self.counts(leak_diff.report(base, candidate, config='production')), (0, 0, 0))

    def test_refusal_releases_no_bytes_and_is_reported_separately(self):
        base = self.capture('base', [], refused=True)
        candidate = self.capture('candidate', [])
        result = leak_diff.report(base, candidate, config='production')
        self.assertEqual(self.counts(result), (10, 0, 10))
        self.assertEqual(result['layers']['C']['refusals'], dict(base=1, candidate=0))

    def test_mismatched_gold_or_corpus_refused(self):
        base = self.capture('base', [])
        candidate = self.capture('candidate', [], gold=[[0, 10, 'PERSON']])
        with self.assertRaisesRegex(record.RecordError, 'inventor'):
            leak_diff.report(base, candidate, config='production')
        header, rows = record._read(base)
        header['corpus_sha256'] = header['scorecard']['dataset']['integrity']['sha256'] = 'b' * 64
        record._write_rows(candidate, [header, *rows])
        with self.assertRaisesRegex(record.RecordError, 'corpus'):
            leak_diff.report(base, candidate, config='production')

    def test_incomplete_duplicate_wrong_config_or_invalid_trace_refused(self):
        base = self.capture('base', [(0, 5)])
        header, rows = record._read(base)
        variants = [[], rows + rows, [{**rows[0], 'config': 'other'}]]
        for change in [dict(raw_start=-1), dict(raw_end=21), dict(raw_start=True), dict(action='ignore')]:
            altered = copy.deepcopy(rows)
            altered[0]['response']['final_protection_trace'][0].update(change)
            variants.append(altered)
        for observations in variants:
            with self.subTest(observations=observations):
                candidate = self.root / 'invalid.jsonl.gz'
                record._write_rows(candidate, [header, *observations])
                with self.assertRaises(record.RecordError):
                    leak_diff.report(base, candidate, config='production')

    def test_report_bytes_deterministic_and_samples_bounded(self):
        base = self.capture('base', [(0, 10)])
        candidate = self.capture('candidate', [])
        command = [sys.executable, str(Path(leak_diff.__file__)), '--base-record', str(base),
                   '--candidate-record', str(candidate), '--config', 'production', '--span-limit', '0']
        first = subprocess.check_output(command)
        self.assertEqual(first, subprocess.check_output(command))
        result = json.loads(first)
        self.assertEqual(result['new_exposure_span_count'], 1)
        self.assertEqual(result['new_exposure_spans'], [])
        self.assertNotIn(str(self.root), first.decode())

    def test_gate_attaches_report_without_changing_verdict(self):
        base = self.capture('base', [(0, 3)])
        candidate = self.capture('candidate', [(3, 10)])
        cards, paths = [], []
        for name, path in [('base', base), ('candidate', candidate)]:
            header, _ = record._read(path)
            card = header['scorecard']
            card['observation_record'] = dict(file=path.name, sha256=score.sha256_file(path))
            card_path = self.root / (name + '.json')
            card_path.write_text(json.dumps(card))
            cards.append(card)
            paths.append(card_path)
        verdict = dict(verdict='pass', reason='synthetic gain', layers={}, config='production')
        with mock.patch.object(agentic, 'compare_layers', return_value=verdict):
            result = agentic.gate(*cards, scorecard_paths=tuple(paths))
        self.assertEqual(result['verdict'], 'pass')
        self.assertEqual(self.counts(result['leak_diff']), (3, 7, -4))
        markdown = agentic.gate_markdown(result)
        self.assertIn('New exposure', markdown)
        self.assertIn('| C | EMAIL | 3 | 7 | -4 |', markdown)
        cards[1]['observation_record']['sha256'] = '0' * 64
        with mock.patch.object(agentic, 'compare_layers', return_value=verdict):
            unavailable = agentic.gate(*cards, scorecard_paths=tuple(paths))
        self.assertEqual(unavailable['verdict'], verdict['verdict'])
        self.assertNotIn('leak_diff', unavailable)
        self.assertIn('unavailable:', agentic.gate_markdown(unavailable))

    def test_committed_mechanism_pair_matches_report_bytes(self):
        root = Path(__file__).resolve().parents[2]
        pair = root / 'docs/reference/benchmarks/mechanisms/6cc02a9f9828c76883dba10f65925d675f52dd820e05311cf2280f47569b28d0'
        result = leak_diff.report(
            pair / 'base-32cdbede14c02ecd07a41103c9892ba88b79b6c970500e816ef97460e05184f1.jsonl.gz',
            pair / 'candidate-6e0d206cc7d9cd67b32ddb78df9884b50413f662be8086a49fa9dfd419de699d.jsonl.gz')
        encoded = json.dumps(result, indent=2, sort_keys=True, ensure_ascii=True) + '\n'
        self.assertEqual(encoded, (root / 'scripts/bench/fixtures/leak-diff-example.json').read_text())
        self.assertEqual({layer: row['new_exposed_bytes'] for layer, row in result['layers'].items()},
                         dict.fromkeys('ACDR', 0))
        self.assertEqual(result['layers']['A']['newly_protected_bytes'], 20)
        self.assertEqual(result['layers']['C']['newly_protected_bytes'], 81)


class GateReportCliTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def write_card(self, name, card):
        path = self.root / (name + '.json')
        path.write_text(json.dumps(card))
        return path

    def cli(self, base, candidate, *options):
        output = self.root / 'gate.json'
        output.unlink(missing_ok=True)
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            status = agentic.main(['gate', '--base', str(base), '--candidate', str(candidate),
                                   '--output', str(output), *map(str, options)])
        receipt = json.loads(output.read_text()) if output.exists() else None
        return status, receipt, stdout.getvalue(), stderr.getvalue()

    def test_cli_missing_optional_records_preserves_pass_and_fail(self):
        for leaks, expected, status in [(9, 'pass', 0), (11, 'fail', 1)]:
            with self.subTest(verdict=expected):
                paths = []
                for name, count in [('base', 10), ('candidate', leaks)]:
                    card = _scorecard(dict.fromkeys('CADR', count))
                    card['observation_record'] = dict(file='absent.jsonl.gz', sha256='a' * 64)
                    paths.append(self.write_card(name, card))
                actual, receipt, stdout, stderr = self.cli(*paths)
                self.assertEqual(actual, status, stderr)
                self.assertEqual(receipt['verdict'], expected)
                self.assertNotIn('leak_diff', receipt)
                self.assertIn('unavailable:', stdout)
                self.assertNotIn(str(self.root), stdout + stderr + json.dumps(receipt))
                self.assertEqual(stderr, '')

    def test_cli_invalid_optional_records_preserve_gate_without_echoing_values(self):
        broken = self.root / 'broken.jsonl.gz'
        broken.write_bytes(b'not a gzip record')
        for pointer in [None, dict(file=broken.name, sha256='0' * 64),
                        dict(file=broken.name, sha256=score.sha256_file(broken)),
                        dict(file='../private-record.jsonl.gz', sha256='0' * 64),
                        dict(file=broken.name)]:
            with self.subTest(pointer=pointer):
                paths = []
                for name, count in [('base', 10), ('candidate', 9)]:
                    card = _scorecard(dict.fromkeys('CADR', count))
                    if pointer is not None:
                        card['observation_record'] = pointer
                    paths.append(self.write_card(name, card))
                status, receipt, stdout, stderr = self.cli(*paths)
                self.assertEqual(status, 0, stderr)
                self.assertEqual(receipt['verdict'], 'pass')
                self.assertNotIn('leak_diff', receipt)
                self.assertIn('unavailable:', stdout)
                self.assertNotIn('private-record', stdout + stderr + json.dumps(receipt))
                self.assertEqual(stderr, '')

    def test_cli_unexpected_report_error_preserves_gate_without_echoing_values(self):
        source = self.root / 'record.jsonl.gz'
        source.write_bytes(b'synthetic report input')
        paths = []
        for name, count in [('base', 10), ('candidate', 9)]:
            card = _scorecard(dict.fromkeys('CADR', count))
            card['observation_record'] = dict(file=source.name, sha256=score.sha256_file(source))
            paths.append(self.write_card(name, card))
        with mock.patch.object(leak_diff, 'report', side_effect=RuntimeError('private-input-value')):
            status, receipt, stdout, stderr = self.cli(*paths)
        self.assertEqual(status, 0, stderr)
        self.assertEqual(receipt['verdict'], 'pass')
        self.assertNotIn('leak_diff', receipt)
        self.assertIn('unavailable:', stdout)
        self.assertNotIn('private-input-value', stdout + stderr + json.dumps(receipt))

    @staticmethod
    def mechanism_records():
        root = Path(__file__).resolve().parents[2]
        pair = root / 'docs/reference/benchmarks/mechanisms/6cc02a9f9828c76883dba10f65925d675f52dd820e05311cf2280f47569b28d0'
        return (pair / 'base-32cdbede14c02ecd07a41103c9892ba88b79b6c970500e816ef97460e05184f1.jsonl.gz',
                pair / 'candidate-6e0d206cc7d9cd67b32ddb78df9884b50413f662be8086a49fa9dfd419de699d.jsonl.gz')

    def test_cli_frozen_uses_relocated_explicit_records(self):
        source = self.mechanism_records()[0]
        path = self.write_card('relocated', record.rescore(source, score.SCORED_LABEL_CONTRACT_V1,
                                                        max_workers=1))
        status, receipt, stdout, stderr = self.cli(
            path, path, '--base-record', source, '--candidate-record', source,
            '--freeze-gold-verdicts', 'base', '--allow-legacy-policy-inputs')
        self.assertEqual(status, 1, stderr)
        self.assertEqual(receipt['verdict'], 'fail')
        self.assertIn('Candidate-verdict sensitivity view:', stdout)
        self.assertIn('leak_diff', receipt)
        self.assertIn('leak_diff', receipt['frozen_gold_verdicts']['raw_comparison'])
        self.assertNotIn('unavailable:', stdout)
        self.assertNotIn('ERROR:', stderr)

    def test_committed_fixtures_match_report_free_cli_verdicts_and_receipts(self):
        sources = self.mechanism_records()
        cards = [self.write_card(name, record.rescore(source, score.SCORED_LABEL_CONTRACT_V1,
                                                     max_workers=1))
                 for name, source in zip(('base', 'candidate'), sources, strict=True)]

        def without_reports(value):
            if isinstance(value, dict):
                return {key: without_reports(item) for key, item in value.items()
                        if key not in {'leak_diff', 'leak_diff_unavailable'}}
            return value

        for left, right in [(0, 1), (1, 0), (0, 0)]:
            for frozen in (False, True):
                with self.subTest(left=left, right=right, frozen=frozen):
                    options = ['--base-record', sources[left], '--candidate-record', sources[right],
                               '--allow-legacy-policy-inputs']
                    if frozen:
                        options += ['--freeze-gold-verdicts', 'base']
                    # Disable only optional enrichment; admission and comparison still run.
                    with mock.patch.object(agentic, '_with_leak_diff', side_effect=lambda result, *a, **k: result):
                        baseline = self.cli(cards[left], cards[right], *options)
                    reported = self.cli(cards[left], cards[right], *options)
                    self.assertEqual(reported[0], baseline[0], reported[3])
                    self.assertEqual(reported[1]['verdict'], baseline[1]['verdict'])
                    self.assertEqual(without_reports(reported[1]), baseline[1])
                    self.assertEqual(reported[2].splitlines()[0], baseline[2].splitlines()[0])
                    self.assertEqual(reported[3], baseline[3])
                    if reported[1]['verdict'] == 'not_comparable':
                        self.assertNotIn('leak_diff', reported[1])
                    else:
                        self.assertIn('leak_diff', reported[1])
