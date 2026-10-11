"""Every publication route binds the displayed score to independently measured predictions."""
import copy
import json
import unittest
import tempfile
from pathlib import Path

import render_theirbench as render
from test_add_tool import report, own_presidio, trace_report, own_pii_trace, data
from test_theirbench import TaggedRowTest, VendorTunedRowTest, RESOLVE


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
                        state, _, body, own = TaggedRowTest().entry_and_report()
                    elif route == 'add_tuned':
                        state, _, body, own = VendorTunedRowTest().entry_and_report()
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
                        elif route == 'add_tagged': render.add_tagged(state, body, own, RESOLVE)
                        elif route == 'add_tuned': render.add_tuned(state, body, own, VendorTunedRowTest().declaration())
                        else: getattr(render, route)(state, rp, op, tool)
                    self.assertEqual(state, before)

    def test_batch_and_incremental_publication_are_byte_identical(self):
        with tempfile.TemporaryDirectory() as root:
            body, own = trace_report(), own_pii_trace()
            rp, op = (Path(root) / n for n in ('report.json', 'own.json'))
            rp.write_text(json.dumps(body)); op.write_text(json.dumps(own))
            batch = render.assemble([rp], [f'pii-trace={op}'], [f'pii-trace={op}'])
            incremental = {'schema_version': 1, 'report_only': 'never used to design or tune Gaze rules',
                           'not_run': render.NOT_RUN, 'benchmarks': {}}
            render.add_benchmark(incremental, rp, op, 'pii-tracer')
            self.assertEqual(json.dumps(batch, sort_keys=True), json.dumps(incremental, sort_keys=True))
