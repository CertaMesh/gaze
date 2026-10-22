"""Every publication route binds the displayed score to independently measured predictions."""
import copy
import json
import sys
import unittest
import tempfile
from pathlib import Path

import render_theirbench as render
from test_add_tool import report, own_presidio, trace_report, own_pii_trace, data
import test_theirbench as fixtures


def complete_report():
    body = report(hardware='hw', comparison_sha256={}, chart_configs={'pii-tracer': 'pii-tracer'})
    return body


class ReceiptAdmissionTest(unittest.TestCase):
    def test_bad_receipts_are_rejected_without_mutation_on_every_route(self):
        cases = ('missing input', 'wrong system', 'prediction', 'dataset', 'count', 'metric', 'missing digest')
        for route in ('assemble', 'add_tool', 'add_benchmark', 'add_tagged', 'add_tuned'):
            for case in cases:
                with self.subTest(route=route, case=case), tempfile.TemporaryDirectory() as root:
                    state = data()
                    body, own = complete_report(), own_presidio()
                    if route == 'add_benchmark':
                        body, own = trace_report(), own_pii_trace()
                    elif route == 'add_tagged':
                        state, _, body, own = fixtures.TaggedRowTest().entry_and_report()
                    elif route == 'add_tuned':
                        state, _, body, own = fixtures.VendorTunedRowTest().entry_and_report()
                    tool = next(iter(body['rows']))
                    if case == 'missing input': del own['input']
                    elif case == 'wrong system': own['system'] = 'other'
                    elif case == 'prediction': own['input']['prediction_sha256'] = '0' * 64
                    elif case == 'dataset': own['input']['dataset_sha256'] = '0' * 64
                    elif case == 'count':
                        own['input']['messages' if route == 'add_benchmark' else 'documents'] = 1
                    elif case == 'metric': own['scored']['char_f1' if route == 'add_benchmark' else 'f2'] = .99
                    else:
                        prov = body['provenance'][tool]
                        prov.get('release', prov.get('vendor_tuned', prov)).pop('prediction_sha256', None)
                    before = copy.deepcopy(state)
                    rp, op, repro = (Path(root) / n for n in ('report.json', 'own.json', 'repro.json'))
                    rp.write_text(json.dumps(body)); op.write_text(json.dumps(own))
                    repro.write_text(json.dumps({'published': {}, 'reproduced': {}, 'versions': {}}))
                    with self.assertRaises(ValueError):
                        if route == 'assemble':
                            own_paths = [f'presidio-research={op}']
                            if case == 'wrong system':
                                correct = Path(root) / 'correct.json'
                                correct.write_text(json.dumps(own_presidio()))
                                own_paths.append(f'presidio-research={correct}')
                            render.assemble([rp], own_paths, [f'presidio-research={repro}'])
                        elif route == 'add_tagged': render.add_tagged(state, body, own, fixtures.RESOLVE)
                        elif route == 'add_tuned': render.add_tuned(state, body, own, fixtures.VendorTunedRowTest().declaration())
                        else: getattr(render, route)(state, rp, op, tool)
                    self.assertEqual(state, before)

    def test_batch_and_incremental_publication_are_byte_identical(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            body = trace_report(chart_configs={"pii-tracer": "pii-tracer"})
            body['rows']['opf'] = copy.deepcopy(body['rows']['pii-tracer'])
            body['provenance']['opf'] = copy.deepcopy(body['provenance']['pii-tracer'])
            body['provenance']['opf']['prediction_sha256'] = '8' * 64
            own, opf_own = own_pii_trace(), own_pii_trace('opf')
            opf_own['input']['prediction_sha256'] = '8' * 64
            rp, op, opf_op = (root / n for n in ('report.json', 'own.json', 'opf-own.json'))
            rp.write_text(json.dumps(body))
            op.write_text(json.dumps(own)); opf_op.write_text(json.dumps(opf_own))
            batch = render.assemble([rp], [f'pii-trace={op}', f'pii-trace={opf_op}'], [f'pii-trace={op}'])
            incremental = {'schema_version': 1, 'report_only': 'never used to design or tune Gaze rules',
                           'not_run': render.NOT_RUN, 'benchmarks': {}}
            for tool, own_path in [('pii-tracer', op), ('opf', opf_op)]:
                single = copy.deepcopy(body)
                single['rows'] = {tool: single['rows'][tool]}
                single['provenance'] = {tool: single['provenance'][tool]}
                rp.write_text(json.dumps(single))
                route = render.add_benchmark if tool == 'pii-tracer' else render.add_tool
                route(incremental, rp, own_path, tool)
            self.assertEqual(json.dumps(batch, sort_keys=True), json.dumps(incremental, sort_keys=True))
            self.assertNotIn('rows_measured_separately', incremental['benchmarks']['pii-trace'])

    def test_incremental_publication_preserves_different_harness_provenance(self):
        changes = {
            'harness_revision': 'different-measurement', 'gaze_crates_tree': 'different-tree',
            'comparison_revision': 'different-comparison', 'label_maps_sha256': 'different-labels',
            'rescored_with': {'harness_dirty': False, 'harness_revision': 'different-rescore'},
        }
        for key, value in changes.items():
            with self.subTest(key=key), tempfile.TemporaryDirectory() as root:
                rp, op = (Path(root) / n for n in ('report.json', 'own.json'))
                body = trace_report()
                rp.write_text(json.dumps(body)); op.write_text(json.dumps(own_pii_trace()))
                state = {'not_run': {}, 'benchmarks': {}}
                render.add_benchmark(state, rp, op, 'pii-tracer')
                second = copy.deepcopy(body)
                second['rows'] = {'opf': second['rows']['pii-tracer']}
                second['provenance'] = {'opf': second['provenance']['pii-tracer']}
                second[key] = value
                rp.write_text(json.dumps(second)); op.write_text(json.dumps(own_pii_trace('opf')))
                render.add_tool(state, rp, op, 'opf')
                entry = state['benchmarks']['pii-trace']
                self.assertEqual(entry['rows_measured_separately']['opf'][key], value)
                self.assertEqual(entry[key], body[key])
                # Replacing that row with equivalent evidence removes the route-only override.
                second[key] = body[key]
                rp.write_text(json.dumps(second))
                render.add_tool(state, rp, op, 'opf')
                self.assertNotIn('rows_measured_separately', entry)

    def test_incomplete_tuned_declarations_are_rejected_without_mutation(self):
        for field in ('row', 'family', 'bar_name', 'caption', 'setup', 'source', 'commit', 'model'):
            with self.subTest(field=field):
                state, _, body, own = fixtures.VendorTunedRowTest().entry_and_report()
                declaration = fixtures.VendorTunedRowTest().declaration()
                del declaration[field]
                before = copy.deepcopy(state)
                try:
                    with self.assertRaisesRegex(ValueError, field):
                        render.add_tuned(state, body, own, declaration)
                finally:
                    self.assertEqual(state, before)

    def test_missing_route_metadata_is_rejected_before_installation(self):
        for route in ('add_tool', 'add_benchmark', 'add_tagged', 'add_tuned'):
            with self.subTest(route=route), tempfile.TemporaryDirectory() as root:
                state, body, own = data(), complete_report(), own_presidio()
                if route == 'add_tool':
                    del body['typed_hold']
                elif route == 'add_benchmark':
                    body, own = trace_report(), own_pii_trace()
                    del own['versions']
                elif route == 'add_tagged':
                    state, _, body, own = fixtures.TaggedRowTest().entry_and_report()
                    del body['generated_at']
                else:
                    state, _, body, own = fixtures.VendorTunedRowTest().entry_and_report()
                    del own['evaluator_commit']
                before = copy.deepcopy(state)
                rp, op = (Path(root) / n for n in ('report.json', 'own.json'))
                rp.write_text(json.dumps(body)); op.write_text(json.dumps(own))
                with self.assertRaises((ValueError, KeyError)):
                    if route == 'add_tagged': render.add_tagged(state, body, own, fixtures.RESOLVE)
                    elif route == 'add_tuned': render.add_tuned(state, body, own, fixtures.VendorTunedRowTest().declaration())
                    else: getattr(render, route)(state, rp, op, next(iter(body['rows'])))
                self.assertEqual(state, before)

    def test_committed_receipt_backed_rows_reassemble_identically(self):
        committed = json.loads(render.DATA.read_text())
        verified = 0
        for name, entry in committed['benchmarks'].items():
            for tool in render.public_rows(entry['rows']):
                provenance = entry['provenance'][tool]
                if not provenance.get('own_scorer_input'):
                    self.assertEqual(entry.get('receipt_status', {}).get(tool), 'pre-receipt')
                    continue
                report = {'rows': {tool: {'test': entry['rows'][tool]}}, 'provenance': {tool: provenance}}
                own = {'system': tool, 'input': provenance['own_scorer_input'],
                       'scored': entry['own_metric'][tool], 'overall': entry['own_metric'][tool]}
                if name == 'pii-trace':
                    own['overall'], _ = render.pii_trace_repro.published(
                        render.pii_trace_repro.scores_from_counts(own['input']['counts']))
                bundle = render.checked_row_bundle(name, entry, report, tool, own)
                after = copy.deepcopy(entry)
                render.install_row_bundle(after, tool, bundle)
                self.assertEqual(json.dumps(entry, sort_keys=True), json.dumps(after, sort_keys=True))
                verified += 1
        expected = sum(1 for entry in committed['benchmarks'].values()
                       for tool in render.public_rows(entry['rows'])
                       if entry.get('receipt_status', {}).get(tool) != 'pre-receipt')
        self.assertGreater(verified, 0)
        self.assertEqual(verified, expected)

    def test_receipt_status_marks_only_explicit_legacy_rows(self):
        baseline = render.render(fixtures.synthetic())
        state = fixtures.synthetic()
        state['benchmarks']['presidio-research']['receipt_status'] = {'presidio-en': 'pre-receipt'}
        marked = render.render(state)
        self.assertIn('| presidio-en† |', marked)
        self.assertIn('not receipt-verified; regeneration pending', marked)
        self.assertEqual([line for line in baseline.splitlines() if line.startswith('| gaze-')],
                         [line for line in marked.splitlines() if line.startswith('| gaze-')])

    def test_failed_tuned_chart_admission_preserves_all_maps(self):
        state, entry, report, own = fixtures.VendorTunedRowTest().entry_and_report()
        entry['chart_rows'] = ['gaze-full']
        before = copy.deepcopy(state)
        with self.assertRaises(ValueError):
            render.add_tuned(state, report, own, fixtures.VendorTunedRowTest().declaration())
        self.assertEqual(state, before)

    def test_untagged_gaze_measurement_records_its_prediction_file_digest(self):
        from unittest import mock
        import theirbench

        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            output, predictions, policy = root / 'report.json', root / 'predictions', root / 'policy.toml'
            policy.write_text('[policy]\n')

            def empty_measurement(name, args, policy, splits, mapping, common, directory, diagnostics):
                (directory / f'{name}.test.jsonl').write_bytes(b'')
                return {'test': fixtures.row(0)}

            argv = ['theirbench.py', '--benchmark', 'presidio-research', '--tool', 'gaze-full',
                    '--gaze-binary', str(root / 'binary'), '--gaze-model-dir', str(root / 'models'),
                    '--gaze-policy', str(policy), '--predictions-dir', str(predictions), '--output', str(output)]
            with mock.patch.object(sys, 'argv', argv), \
                 mock.patch.object(theirbench, 'load_benchmark', return_value=({'test': []}, {'sha256': 'd' * 64})), \
                 mock.patch.object(theirbench, 'measure_gaze', side_effect=empty_measurement):
                self.assertEqual(theirbench.main(), 0)
            provenance = json.loads(output.read_text())['provenance']['gaze-full']
            self.assertEqual(provenance['prediction_sha256'],
                             'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855')
