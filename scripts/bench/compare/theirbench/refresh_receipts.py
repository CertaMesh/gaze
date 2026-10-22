#!/usr/bin/env python3
"""Refresh pre-receipt competitor rows without losing published numeric history."""
from __future__ import annotations

import argparse
import copy
import json
from pathlib import Path

import render_theirbench as render

ROOT = Path(__file__).resolve().parents[4]
HISTORY = ROOT / 'docs/reference/benchmarks/third-party-receipt-history.json'
PAGE = HISTORY.with_suffix('.md')


def numbers(value: object, prefix: str = '') -> dict[str, int | float]:
    if isinstance(value, dict):
        return {key: number for name, child in value.items()
                for key, number in numbers(child, f'{prefix}.{name}' if prefix else name).items()}
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return {prefix: value}
    return {}


def metric_changes(before: dict, after: dict) -> list[dict]:
    previous, current = numbers(before), numbers(after)
    return [{'metric': key, 'published': previous[key], 'regenerated': current.get(key)}
            for key in sorted(previous) if previous[key] != current.get(key)]


def refresh(data: dict, history: dict, report: Path, own: Path, tool: str,
            metric_reason: str | None) -> tuple[dict, dict]:
    measured = json.loads(report.read_text(encoding='utf-8'))
    benchmark = measured['benchmark']
    entry = data['benchmarks'][benchmark]
    if entry.get('receipt_status', {}).get(tool) != 'pre-receipt':
        raise ValueError(f'{benchmark}/{tool}: expected an explicit pre-receipt row')
    candidate = copy.deepcopy(data)
    render.add_tool(candidate, report, own, tool)
    fresh = candidate['benchmarks'][benchmark]
    before = {'row': entry['rows'][tool], 'own_metric': entry['own_metric'][tool]}
    after = {'row': fresh['rows'][tool], 'own_metric': fresh['own_metric'][tool]}
    changes = metric_changes(before, after)
    for change in changes:
        key = change['metric']
        timing = key.startswith('row.latency.') or key == 'own_metric.predict_seconds'
        if not timing and not metric_reason:
            raise ValueError(f'{benchmark}/{tool}: explain changed metric {key} before publication')
        change['reason'] = ('Fresh capture of the pinned configuration. Timing varies with runtime startup '
                            'and machine load; no speed comparison is claimed.' if timing else metric_reason)
    provenance = fresh['provenance'][tool]
    updated = copy.deepcopy(history)
    updated['rows'].append({
        'benchmark': benchmark, 'tool': tool,
        'published_harness_revision': entry.get('rows_measured_separately', {}).get(tool, {}).get(
            'harness_revision', entry['harness_revision']),
        'regenerated_harness_revision': measured['harness_revision'],
        'rescored_with': measured['rescored_with'],
        'prediction_sha256': provenance['prediction_sha256'],
        'dataset_sha256': provenance['own_scorer_input']['dataset_sha256'],
        'changes': changes,
    })
    return candidate, updated


def page(history: dict) -> str:
    lines = ['# Third-party receipt refresh', '',
             'These rows originally had no prediction digest or scorer receipt. Each refreshed row passes '
             '`check_own_result` before its receipt marker is removed. The dataset, model and configuration '
             'remain pinned. Raw text and predictions stay outside the repository.', '',
             'The tables keep every replaced numeric value from the published measurement. New per-label '
             'byte telemetry has no historical value and appears only in the aggregate. Unlisted metrics '
             'retain their published values. Timing is recorded for provenance and does not establish a speed gain.', '',
             f"Published aggregate before this campaign: `{history['published_commit']}`.", '',
             f"Refreshed rows: {len(history['rows'])} of {history['expected_rows']}.", '']
    if history.get('order_diagnostic'):
        lines += ['## Presidio overlap order', '',
                  'This campaign records PYTHONHASHSEED=0. Presidio 2.2.364 deduplicates candidates '
                  'through a Python set, then resolves equal-score overlaps in that order. '
                  'Changing only the hash seed to 1 changed types in 31 PIIBench documents, '
                  'with identical span coordinates. The pinned analyzer confirmed that every '
                  'changed pair had equal scores.', '',
                  'The [aggregate order diagnostic](presidio-order-diagnostic.json) pins both prediction '
                  'streams and the dataset. It demonstrates order sensitivity in retained captures. '
                  'The original seed and predictions were not recorded, so its exact historical '
                  'type choices cannot be recovered. Product-coverage leaked and false-positive byte '
                  'totals reproduce for the three plain Presidio configurations. Typed and '
                  'common-intersection metrics can differ because a tie assigns different types '
                  'to the same protected bytes.', '']
    for item in history['rows']:
        lines += [f"## {item['benchmark']}: {item['tool']}", '',
                  f"Prediction SHA-256: `{item['prediction_sha256']}`. Dataset SHA-256: `{item['dataset_sha256']}`.", '',
                  f"Capture commit: `{item['regenerated_harness_revision']}`. "
                  f"Previous capture commit: `{item['published_harness_revision']}`.", '']
        if not item['changes']:
            lines += ['All existing numeric metrics reproduce exactly.', '']
        else:
            lines += ['| Metric | Published (superseded) | Regenerated | Why |',
                      '| --- | ---: | ---: | --- |']
            lines += [f"| `{v['metric']}` | {v['published']} | {v['regenerated']} | {v['reason']} |"
                      for v in item['changes']]
            lines += ['']
    for item in history.get('unavailable', []):
        lines += [f"## {item['tool']} remains unverified", '', item['reason'], '']
    return '\n'.join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data', type=Path, default=render.DATA)
    parser.add_argument('--history', type=Path, default=HISTORY)
    parser.add_argument('--page', type=Path, default=PAGE)
    parser.add_argument('--report', type=Path)
    parser.add_argument('--own', type=Path)
    parser.add_argument('--tool')
    parser.add_argument('--metric-reason')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    history = json.loads(args.history.read_text(encoding='utf-8'))
    if args.report:
        if not args.own or not args.tool:
            parser.error('--report needs --own and --tool')
        data = json.loads(args.data.read_text(encoding='utf-8'))
        data, history = refresh(data, history, args.report, args.own, args.tool, args.metric_reason)
        args.data.write_text(json.dumps(data, indent=2, sort_keys=True) + '\n', encoding='utf-8')
        args.history.write_text(json.dumps(history, indent=2, sort_keys=True) + '\n', encoding='utf-8')
    expected = page(history)
    if args.check:
        if args.page.read_text(encoding='utf-8') != expected:
            raise SystemExit('receipt history page is stale')
    else:
        args.page.write_text(expected, encoding='utf-8')


if __name__ == '__main__':
    main()
