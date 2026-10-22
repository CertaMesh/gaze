#!/usr/bin/env python3
"""Report ground-truth byte exposure changes from two value-free captures."""

from __future__ import annotations

import argparse
from collections import defaultdict
import json
from pathlib import Path
import sys

import agentic_layers as agentic
import gaze_bench_score as score
import scorecard_record as records


SCHEMA_VERSION = 1


def _inventory(header):
    inventory = {}
    for row in header['documents']:
        records._document_from_row(row)
        inventory[row['id']] = (row['layer'], row['utf8_bytes'], sorted(row['gold']))
    return inventory


def _observations(header, rows, config):
    documents = {row['id']: row for row in header['documents']}
    card = header['scorecard']
    runs = [('C', run) for run in card.get('runs', [])]
    runs += [(layer, run) for layer in ('A', 'D', 'R')
             for run in card.get('layers', {}).get(layer, {}).get('runs', [])]
    selected = [(layer, run) for layer, run in runs if run['config'] == config]
    if not selected or len({layer for layer, _ in selected}) != len(selected):
        raise records.RecordError('missing or duplicate layer/config in record')
    expected = {}
    for layer, run in selected:
        scored = run['scored_population']['document_ids']
        refused = run['failed_closed_population']['document_ids']
        if len(set(scored + refused)) != len(scored + refused):
            raise records.RecordError('duplicate document in scored/refused population')
        expected.update({(layer, uid): uid in refused for uid in scored + refused})
    result = {}
    for row in rows:
        uid = row['document_id']
        if uid not in documents or documents[uid]['layer'] != row['layer']:
            raise records.RecordError('observation disagrees with document inventory')
        if row['config'] != config:
            continue
        key = (row['layer'], uid)
        response = row['response']
        if (key not in expected or key in result or response['fixture_id'] != uid
                or type(response.get('refused')) is not bool
                or response['refused'] != expected[key]
                or response['refused'] != ('pipeline_error_code' in response)):
            raise records.RecordError('invalid or duplicate observation/population')
        length = documents[uid]['utf8_bytes']
        # A refused document returns no content. Keep refusal counts alongside bytes.
        protected = [(0, length)] if response['refused'] else []
        if not response['refused']:
            for item in response['final_protection_trace']:
                if (not records._span(item['raw_start'], item['raw_end'], length)
                        or not isinstance(item['class'], str)
                        or item['action'] not in {'tokenize', 'redact'}):
                    raise records.RecordError('invalid protection span or action')
                protected.append((item['raw_start'], item['raw_end']))
        result[key] = (score.merge_intervals(protected), response['refused'])
    if set(result) != set(expected):
        raise records.RecordError('incomplete observation population for selected config')
    return result


def _counts(exposed, protected):
    return dict(new_exposed_bytes=exposed, newly_protected_bytes=protected,
                net_leaked_bytes=exposed - protected)


def report(base_record: Path, candidate_record: Path, *, config: str | None = None,
           span_limit: int = 20) -> dict:
    """Count all gold independently of contracts; positive net means more leakage.

    Totals union overlapping gold across classes. Class rows union within a class,
    so class rows can overlap and must not be summed to recover layer totals.
    """
    if type(span_limit) is not int or span_limit < 0:
        raise records.RecordError('span_limit must be a nonnegative integer')
    try:
        base, base_rows = records._read(base_record)
        candidate, candidate_rows = records._read(candidate_record)
        if base['corpus_sha256'] != candidate['corpus_sha256']:
            raise records.RecordError('different corpus digests')
        inventory = _inventory(base)
        if inventory != _inventory(candidate) or base.get('layer_identity') != candidate.get('layer_identity'):
            raise records.RecordError('different document/gold inventories or layer identities')
        config = config or agentic.production_config(candidate['scorecard'])
        left = _observations(base, base_rows, config)
        right = _observations(candidate, candidate_rows, config)
        if left.keys() != right.keys():
            raise records.RecordError('different observed document populations')
        layers = {}
        samples = []
        span_count = 0
        for layer, uid in sorted(left):
            before, base_refused = left[(layer, uid)]
            after, candidate_refused = right[(layer, uid)]
            row = layers.setdefault(layer, {**_counts(0, 0), 'documents': 0,
                                           'refusals': dict(base=0, candidate=0), 'classes': {}})
            row['documents'] += 1
            row['refusals']['base'] += int(base_refused)
            row['refusals']['candidate'] += int(candidate_refused)
            classes = defaultdict(list)
            for start, end, label in inventory[uid][2]:
                classes[label].append((start, end))
            # Each subtraction intersects protection changes with the gold union.
            gold = score.merge_intervals([span for spans in classes.values() for span in spans])
            exposed = score.subtract_intervals(score.subtract_intervals(gold, after),
                                               score.subtract_intervals(gold, before))
            protected = score.subtract_intervals(score.subtract_intervals(gold, before),
                                                 score.subtract_intervals(gold, after))
            for field, count in _counts(score.interval_length(exposed), score.interval_length(protected)).items():
                row[field] += count
            for label, spans in sorted(classes.items()):
                gold = score.merge_intervals(spans)
                new = score.subtract_intervals(score.subtract_intervals(gold, after),
                                               score.subtract_intervals(gold, before))
                gain = score.subtract_intervals(score.subtract_intervals(gold, before),
                                                score.subtract_intervals(gold, after))
                counts = row['classes'].setdefault(label, _counts(0, 0))
                for field, count in _counts(score.interval_length(new), score.interval_length(gain)).items():
                    counts[field] += count
                span_count += len(new)
                for start, end in new:
                    if len(samples) < span_limit:
                        samples.append(dict(layer=layer, document_id=uid, start=start, end=end,
                                            **{'class': label}))
        return dict(schema_version=SCHEMA_VERSION, report_only=True, scope='all-ground-truth',
                    config=config, record_sha256=dict(base=score.sha256_file(base_record),
                                                     candidate=score.sha256_file(candidate_record)),
                    layers=layers, new_exposure_span_count=span_count,
                    new_exposure_spans=samples, span_limit=span_limit)
    except (KeyError, TypeError, ValueError, OSError) as error:
        if isinstance(error, records.RecordError):
            raise
        # Do not echo submitted records, trace values, or private filesystem paths.
        raise records.RecordError('invalid or unreadable differential record input') from error


def markdown(result: dict) -> str:
    lines = ['New exposure report (all ground-truth bytes; report only).', '',
             'Net leaked bytes = new exposure minus newly protected bytes. Positive means more leakage. '
             'Layer totals deduplicate gold across classes; class rows can overlap.', '',
             '| Layer | Class | New exposure | Newly protected | Net leaked bytes |',
             '| --- | --- | ---: | ---: | ---: |']
    for layer, row in result['layers'].items():
        for label, counts in [('all classes', row), *sorted(row['classes'].items())]:
            label = label.replace('|', '&#124;').replace('\n', ' ')
            lines.append(f"| {layer} | {label} | {counts['new_exposed_bytes']} | "
                         f"{counts['newly_protected_bytes']} | {counts['net_leaked_bytes']} |")
    lines += ['', f"New-exposure spans: {result['new_exposure_span_count']}; "
              f"showing {len(result['new_exposure_spans'])} in the JSON receipt. "
              'Offsets are half-open UTF-8 byte offsets. Refusals release no bytes and remain subject to the existing gate.']
    return '\n'.join(lines) + '\n'


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-record', type=Path, required=True)
    parser.add_argument('--candidate-record', type=Path, required=True)
    parser.add_argument('--config')
    parser.add_argument('--span-limit', type=int, default=20)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args(argv)
    try:
        result = report(args.base_record, args.candidate_record, config=args.config, span_limit=args.span_limit)
    except records.RecordError as error:
        print(f'ERROR: {error}', file=sys.stderr)
        return 2
    encoded = json.dumps(result, indent=2, sort_keys=True, ensure_ascii=True) + '\n'
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding='utf-8')
    else:
        print(encoded, end='')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
