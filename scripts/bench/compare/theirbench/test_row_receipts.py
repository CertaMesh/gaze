"""Published leaves and timer samples must agree with independent retained sources."""
import copy
import hashlib
import json
from unittest.mock import patch

import pytest

import render_theirbench as render
import row_receipts
import theirbench
from timing_samples import check_samples, latency_ns


def test_every_published_leaf_is_checked():
    data = json.loads(render.DATA.read_text())
    receipts = json.loads(row_receipts.RECEIPTS.read_text())['benchmarks']
    expected = {(name, tool) for name, entry in data['benchmarks'].items()
                for tool in render.public_rows(entry['rows'])
                if entry.get('receipt_status', {}).get(tool) != 'pre-receipt'}
    assert expected == {(name, tool) for name, rows in receipts.items() for tool in rows}

    def leaves(value, prefix=()):
        if isinstance(value, dict):
            for key, child in value.items():
                yield from leaves(child, (*prefix, key))
        else:
            yield prefix, value

    for name, tool in sorted(expected):
        receipt = receipts[name][tool]
        entry = data['benchmarks'][name]
        for field, target in [('row', entry['rows']), ('own_metric', entry['own_metric'])]:
            original = target[tool]
            for path, value in leaves(original):
                mutated = copy.deepcopy(original)
                parent = mutated
                for key in path[:-1]:
                    parent = parent[key]
                parent[path[-1]] = value + 1 if type(value) in (int, float) else 'changed'
                target[tool] = mutated
                with pytest.raises(ValueError, match='committed capture receipt'):
                    row_receipts.check_row(entry, tool, receipt, row_receipts.RECEIPTS.parent)
            target[tool] = original


def test_capture_retains_text_free_samples_and_recomputes_quantiles(tmp_path):
    documents = [theirbench.score.Document(str(i), 'synthetic', 'en', '', 'synthetic', ()) for i in range(3)]
    # Each duration is distinct; use the real percentile contract and runner, with no model.
    with patch.object(theirbench.time, 'perf_counter_ns', side_effect=[0, 1_000_000, 2_000_000, 5_000_000,
                                                                    6_000_000, 15_000_000]):
        result = theirbench.measure_tool('opf', lambda doc: [], {'test': documents}, {}, frozenset(), tmp_path)
    path = tmp_path / 'opf.test.timings.jsonl'
    assert [json.loads(line) for line in path.read_text().splitlines()] == [
        {'index': 0, 'ns': 1_000_000}, {'index': 1, 'ns': 3_000_000}, {'index': 2, 'ns': 9_000_000}]
    receipt = theirbench.retain_timing_samples('opf', tmp_path, tmp_path / 'reports/report.json')['test']
    retained = tmp_path / 'reports' / receipt['file']
    assert retained.read_bytes() == path.read_bytes()
    assert result['test']['latency'] == latency_ns([1_000_000, 3_000_000, 9_000_000])
    digest = theirbench.sha256_file(tmp_path / 'opf.test.jsonl')
    check_samples(retained, receipt, result['test']['latency'], digest)
    report = {'benchmark': 'presidio-research', 'rows': {'opf': result},
              'provenance': {'opf': {'prediction_sha256': digest, 'timing_samples': {'test': receipt}}}}
    report_path = tmp_path / 'reports/report.json'
    report_path.write_text(json.dumps(report))
    own_path = tmp_path / 'own.json'
    own_path.write_text(json.dumps({'scored': {'documents': 3}, 'input': {'prediction_sha256': digest}}))
    exported = row_receipts.capture(report_path, own_path, 'opf', tmp_path / 'committed')
    assert exported['latency_evidence']['status'] == 'samples-retained'
    assert (tmp_path / 'committed' / exported['latency_evidence']['file']).read_bytes() == path.read_bytes()
    theirbench.rescore(report, {'test': documents}, {'opf': {}}, frozenset(), tmp_path)
    report['rows']['opf']['test']['latency']['p95_ms'] -= .001
    with pytest.raises(ValueError, match='quantiles'):
        theirbench.rescore(report, {'test': documents}, {'opf': {}}, frozenset(), tmp_path)
    report_path.write_text(json.dumps(report))
    with pytest.raises(ValueError, match='quantiles'):
        row_receipts.capture(report_path, own_path, 'opf', tmp_path / 'rejected')
    report['rows']['opf']['test']['latency'] = latency_ns([1_000_000, 3_000_000, 9_000_000])
    changed = copy.deepcopy(result['test']['latency'])
    changed['p50_ms'] -= .001
    with pytest.raises(ValueError, match='quantiles'):
        check_samples(retained, receipt, changed, digest)
    with pytest.raises(ValueError, match='prediction digest'):
        check_samples(retained, receipt, result['test']['latency'], '0' * 64)
    retained.write_bytes(retained.read_bytes() + b'\n')
    with pytest.raises(ValueError, match='sample digest'):
        check_samples(retained, receipt, result['test']['latency'], digest)


@pytest.mark.parametrize('sample', [{'index': 1, 'ns': 10}, {'index': 0, 'ns': -1},
                                   {'index': 0, 'ns': 1.5}, {'index': 0, 'ns': 1, 'text': 'synthetic'}])
def test_sample_contract_rejects_unordered_invalid_or_extra_fields(tmp_path, sample):
    path = tmp_path / 'timings.jsonl'
    path.write_text(json.dumps(sample) + '\n')
    receipt = {'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'prediction_sha256': 'a' * 64}
    with pytest.raises(ValueError, match='indices'):
        check_samples(path, receipt, {}, 'a' * 64)


def test_every_published_history_number_matches_its_old_and_new_source():
    import gzip
    from refresh_receipts import HISTORY, PAGE, metric_changes, page

    history = json.loads(HISTORY.read_text())
    archived = json.loads(gzip.decompress(
        (row_receipts.RECEIPTS.parent / 'their-benchmarks-e7e018ce.json.gz').read_bytes()))
    receipts = json.loads(row_receipts.RECEIPTS.read_text())['benchmarks']
    expected = {(name, tool) for name, entry in archived['benchmarks'].items()
                for tool, status in entry.get('receipt_status', {}).items() if status == 'pre-receipt'}
    assert len(history['rows']) == len(expected)
    assert {(item['benchmark'], item['tool']) for item in history['rows']} == expected
    for item in history['rows']:
        name, tool = item['benchmark'], item['tool']
        old, new = archived['benchmarks'][name], receipts[name][tool]
        changes = metric_changes({'row': old['rows'][tool], 'own_metric': old['own_metric'][tool]},
                                 {'row': new['row'], 'own_metric': new['own_metric']})
        assert changes == [{key: value for key, value in change.items() if key != 'reason'}
                           for change in item['changes']]
        assert all(change['reason'] for change in item['changes'])
    assert PAGE.read_text() == page(history)
