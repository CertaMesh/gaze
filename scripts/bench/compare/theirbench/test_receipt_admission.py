"""Every publication route binds the displayed score to independently measured predictions."""
import copy
import json
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
                            render.assemble([rp], [f'presidio-research={op}'], [f'presidio-research={repro}'])
                        elif route == 'add_tagged': render.add_tagged(state, body, own, fixtures.RESOLVE)
                        elif route == 'add_tuned': render.add_tuned(state, body, own, fixtures.VendorTunedRowTest().declaration())
                        else: getattr(render, route)(state, rp, op, tool)
                    self.assertEqual(state, before)

    def test_batch_and_incremental_publication_are_byte_identical(self):
        with tempfile.TemporaryDirectory() as root:
            body, own = trace_report(chart_configs={"pii-tracer": "pii-tracer"}), own_pii_trace()
            rp, op = (Path(root) / n for n in ('report.json', 'own.json'))
            rp.write_text(json.dumps(body)); op.write_text(json.dumps(own))
            batch = render.assemble([rp], [f'pii-trace={op}'], [f'pii-trace={op}'])
            incremental = {'schema_version': 1, 'report_only': 'never used to design or tune Gaze rules',
                           'not_run': render.NOT_RUN, 'benchmarks': {}}
            render.add_benchmark(incremental, rp, op, 'pii-tracer')
            self.assertEqual(json.dumps(batch, sort_keys=True), json.dumps(incremental, sort_keys=True))

    def test_committed_receipt_backed_rows_reassemble_identically(self):
        committed = json.loads(render.DATA.read_text())
        verified = 0
        for name, entry in committed['benchmarks'].items():
            for tool in render.public_rows(entry['rows']):
                provenance = entry['provenance'][tool]
                if not provenance.get('own_scorer_input'):
                    continue
                report = {'rows': {tool: {'test': entry['rows'][tool]}}, 'provenance': {tool: provenance}}
                own = {'system': tool, 'input': provenance['own_scorer_input'],
                       'scored': entry['own_metric'][tool], 'overall': entry['own_metric'][tool]}
                bundle = render.checked_row_bundle(name, entry, report, tool, own)
                after = copy.deepcopy(entry)
                render.install_row_bundle(after, tool, bundle)
                self.assertEqual(json.dumps(entry, sort_keys=True), json.dumps(after, sort_keys=True))
                verified += 1
        self.assertEqual(verified, 5)

    def test_receipt_status_marks_only_explicit_legacy_rows(self):
        entry = fixtures.synthetic()['benchmarks']['presidio-research']
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
