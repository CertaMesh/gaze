"""Refreshing legacy rows preserves history and refuses unexplained metric changes."""
import copy
import json
from pathlib import Path

import pytest

from test_add_tool import ENTRY, ROW, data, own_presidio, report, write
from refresh_receipts import metric_changes, page, refresh


def legacy():
    state = data()
    entry = state['benchmarks']['presidio-research']
    entry['harness_revision'] = 'a' * 40
    entry['receipt_status'] = {'pii-tracer': 'pre-receipt'}
    entry['rows']['pii-tracer'] = copy.deepcopy(ROW)
    entry['provenance']['pii-tracer'] = {}
    entry['own_metric']['pii-tracer'] = own_presidio()['scored']
    return state


def test_records_old_and_new_timing_and_keeps_other_rows(tmp_path):
    state = legacy()
    body = report()
    body['rows'] = copy.deepcopy(body['rows'])
    body['rows']['pii-tracer']['test']['latency']['p50_ms'] = 2
    candidate, history = refresh(state, {'rows': []}, *write(tmp_path, body), 'pii-tracer', None)
    change = history['rows'][0]['changes'][0]
    assert (change['metric'], change['published'], change['regenerated']) == ('row.latency.p50_ms', 1, 2)
    assert candidate['benchmarks']['presidio-research']['rows']['opf'] == state['benchmarks']['presidio-research']['rows']['opf']
    assert candidate['benchmarks']['presidio-research']['receipt_status'] == {}
    assert state['benchmarks']['presidio-research']['receipt_status'] == {'pii-tracer': 'pre-receipt'}


def test_requires_a_reason_for_changed_detection_metric_without_mutation(tmp_path):
    state = legacy()
    body = report()
    body['rows'] = copy.deepcopy(body['rows'])
    body['rows']['pii-tracer']['test']['product_coverage']['leaked_bytes'] = 6
    old = copy.deepcopy(state)
    rp, op = write(tmp_path, body)
    with pytest.raises(ValueError, match='explain changed metric'):
        refresh(state, {'rows': []}, rp, op, 'pii-tracer', None)
    assert state == old
    _, history = refresh(state, {'rows': []}, rp, op, 'pii-tracer', 'Documented configuration change.')
    assert history['rows'][0]['changes'][0]['reason'] == 'Documented configuration change.'


def test_invalid_receipt_preserves_both_inputs(tmp_path):
    state, history = legacy(), {'rows': []}
    old = copy.deepcopy(state)
    own = own_presidio()
    own['input']['prediction_sha256'] = '0' * 64
    with pytest.raises(ValueError):
        refresh(state, history, *write(tmp_path, report(), own), 'pii-tracer', None)
    assert state == old and history == {'rows': []}


def test_does_not_replace_an_already_verified_row(tmp_path):
    state = legacy()
    state['benchmarks']['presidio-research']['receipt_status'].clear()
    with pytest.raises(ValueError, match='pre-receipt'):
        refresh(state, {'rows': []}, *write(tmp_path, report()), 'pii-tracer', None)


def test_numeric_history_includes_removed_metrics_but_not_new_telemetry():
    assert metric_changes({'a': 1, 'b': 2}, {'a': 1, 'new': 3}) == [
        {'metric': 'b', 'published': 2, 'regenerated': None}]


def test_long_ratio_display_keeps_full_precision_in_numeric_history(tmp_path):
    state = legacy()
    body = report()
    body['rows'] = copy.deepcopy(body['rows'])
    body['rows']['pii-tracer']['test']['latency']['p50_ms'] = 0.05218500317447932
    _, history = refresh(state, {'rows': [], 'published_commit': 'a' * 40, 'expected_rows': 1},
                         *write(tmp_path, body), 'pii-tracer', None)
    change = next(item for item in history['rows'][0]['changes'] if item['metric'] == 'row.latency.p50_ms')
    assert json.loads(json.dumps(change))['regenerated'] == 0.05218500317447932
    rendered = page(history)
    assert '5.218500e-02' in rendered
    assert 'seven significant digits' in rendered and 'JSON history' in rendered


def test_new_sample_backed_timing_is_not_described_as_missing_samples(tmp_path):
    state = legacy()
    body = copy.deepcopy(report())
    body['rows']['pii-tracer']['test']['latency']['p50_ms'] = 2
    body['provenance']['pii-tracer']['timing_samples'] = {'test': {'sha256': 'a' * 64}}
    _, history = refresh(state, {'rows': []}, *write(tmp_path, body), 'pii-tracer', None)
    reason = history['rows'][0]['changes'][0]['reason']
    assert 'samples are retained' in reason
    assert 'not retained' not in reason
