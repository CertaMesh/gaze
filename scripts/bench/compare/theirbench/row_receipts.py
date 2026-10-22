#!/usr/bin/env python3
"""Export text-free capture leaves and check every published leaf against them."""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
from pathlib import Path

from timing_samples import check_samples

ROOT = Path(__file__).resolve().parents[4]
RECEIPTS = ROOT / 'docs/reference/benchmarks/receipt-sources/published-rows.json'
TIMING_LIMIT = ('Single-run, machine-dependent timing aggregates. Per-document samples were not retained; '
                'p50/p95 cannot be reproduced from committed data. Receipt-verified covers quality/count metrics only.')


def capture(report_path: Path, own_path: Path, tool: str, destination: Path) -> dict:
    report = json.loads(report_path.read_text())
    own = json.loads(own_path.read_text())
    provenance = report['provenance'][tool]
    source = provenance.get('release', provenance.get('vendor_tuned', provenance))
    row = report['rows'][tool]['test']
    timing = provenance.get('timing_samples', {}).get('test')
    if timing:
        filename = f"{report['benchmark']}-{tool}.timings.jsonl"
        source_path = report_path.parent / timing['file']
        check_samples(source_path, timing, row['latency'], source['prediction_sha256'])
        destination.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source_path, destination / filename)
        timing = {**timing, 'file': filename, 'status': 'samples-retained'}
    else:
        timing = {'status': 'aggregate-only', 'limit': TIMING_LIMIT}
    return {'row': row, 'own_metric': own.get('scored') or own['overall'],
            'own_scorer_input': own['input'], 'prediction_sha256': source['prediction_sha256'],
            'source': {'report_sha256': hashlib.sha256(report_path.read_bytes()).hexdigest(),
                       'own_sha256': hashlib.sha256(own_path.read_bytes()).hexdigest()},
            'latency_evidence': timing}


def check_row(entry: dict, tool: str, receipt: dict, directory: Path) -> None:
    # Compare complete trees, including latency, counts, nulls and added/removed fields.
    for field, actual in [('row', entry['rows'][tool]), ('own_metric', entry['own_metric'][tool]),
                          ('own_scorer_input', entry['provenance'][tool]['own_scorer_input'])]:
        if actual != receipt[field]:
            raise ValueError(f'{tool}: published {field} differs from committed capture receipt')
    provenance = entry['provenance'][tool]
    source = provenance.get('release', provenance.get('vendor_tuned', provenance))
    if source['prediction_sha256'] != receipt['prediction_sha256']:
        raise ValueError(f'{tool}: prediction digest differs from committed capture receipt')
    timing = receipt['latency_evidence']
    if timing['status'] == 'samples-retained':
        check_samples(directory / timing['file'], timing, receipt['row']['latency'], receipt['prediction_sha256'])
    elif timing['status'] != 'aggregate-only' or timing['limit'] != TIMING_LIMIT:
        raise ValueError(f'{tool}: timing evidence must disclose missing samples')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', type=Path, default=RECEIPTS)
    parser.add_argument('--report', type=Path)
    parser.add_argument('--own', type=Path)
    parser.add_argument('--tool')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    receipts = json.loads(args.receipts.read_text())
    if args.check:
        import render_theirbench as render
        data = json.loads(render.DATA.read_text())
        expected = {(name, tool) for name, entry in data['benchmarks'].items()
                    for tool in render.public_rows(entry['rows'])
                    if entry.get('receipt_status', {}).get(tool) != 'pre-receipt'}
        actual = {(name, tool) for name, rows in receipts['benchmarks'].items() for tool in rows}
        if expected != actual:
            raise ValueError('published rows and capture receipt roster differ')
        for name, tool in sorted(expected):
            check_row(data['benchmarks'][name], tool, receipts['benchmarks'][name][tool], args.receipts.parent)
    else:
        if not args.report or not args.own or not args.tool:
            parser.error('export needs --report, --own and --tool')
        import render_theirbench as render
        report = json.loads(args.report.read_text())
        own = json.loads(args.own.read_text())
        entry = {'identity': report['identity'], 'splits': report['splits']}
        render.check_own_result(report['benchmark'], entry, args.tool, own,
                                render.measured_predictions(report, args.tool))
        receipts['benchmarks'].setdefault(report['benchmark'], {})[args.tool] = capture(
            args.report, args.own, args.tool, args.receipts.parent)
        args.receipts.write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')


if __name__ == '__main__':
    main()
