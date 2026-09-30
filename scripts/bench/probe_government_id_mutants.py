#!/usr/bin/env python3
"""Model-free runtime preview of the committed ID mutants, never a gain gate."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

import agentic_layers as a
import government_id_cells as g


def probe(binary: Path, policy: Path, rows: list[a.Record]) -> dict:
    request = ''.join(json.dumps({'session_id': r.uid, 'text': r.text}) + '\n' for r in rows)
    result = subprocess.run([str(binary), 'daemon', '--policy', str(policy), '--locale', 'global',
                             '--safety-net', 'none'], input=request, capture_output=True, text=True, timeout=120)
    if result.returncode:
        raise RuntimeError('model-free daemon failed; inspect locally without publishing raw values')
    responses = [json.loads(line) for line in result.stdout.splitlines()]
    if len(responses) != len(rows):
        raise RuntimeError('daemon response population changed')
    by_shape = {}
    for row, response in zip(rows, responses, strict=True):
        if response.get('session_id') != row.uid:
            raise RuntimeError('daemon response ownership changed')
        cell = next(c for c in (*g.CELLS, *g.TWINS) if c.family == row.family)
        total = by_shape.setdefault(row.layer + '/' + cell.shape.value,
                                   {'Refused': 0, 'leaked_on_all_processed': 0, 'false_positive': 0})
        if 'error' in response:
            total['Refused'] += 1
            continue
        covered = set()
        for entry in response['manifest']:
            span = entry['raw_span']
            if not 0 <= span['start'] < span['end'] <= len(row.text.encode()):
                raise RuntimeError('daemon returned invalid manifest offsets')
            covered.update(range(span['start'], span['end']))
        gold = {i for span in row.gold for i in range(span.start, span.end)}
        total['leaked_on_all_processed'] += len(gold - covered)
        total['false_positive'] += len(covered - gold)
    return by_shape


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    rows = g.records(a, 'dev')
    root = Path(__file__).resolve().parent
    arms = {}
    with tempfile.TemporaryDirectory() as directory:
        for arm in ('base', 'broad', 'narrow'):
            policy = Path(directory) / (arm + '.toml')
            text = '[session]\nscope = "ephemeral"\n[policy.rulepacks]\nbundled = ["core"]\n'
            if arm != 'base':
                text += (root / f'fixtures/agentic/mutant-{arm}-government-ids.toml').read_text()
            policy.write_text(text)
            arms[arm] = probe(binary, policy, rows)
    failures = []
    for arm in ('broad', 'narrow'):
        for shape in g.Shape:
            key = 'D/' + shape.value
            if arms[arm][key]['false_positive'] <= arms['base'][key]['false_positive']:
                failures.append(arm + '/' + shape.value + ': no added D cost')
            if arms[arm]['A/' + shape.value]['Refused'] or arms[arm][key]['Refused']:
                failures.append(arm + '/' + shape.value + ': refused')
    report = {'scope': 'model-free dev-only preview, not v2/v1 gate or independent holdout evidence',
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'corpus_sha256': hashlib.sha256(a.corpus_bytes(rows)).hexdigest(),
              'arms': arms, 'failures': failures}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'report': str(args.output), 'mutation_failures': len(failures)}))
    return bool(failures)


if __name__ == '__main__':
    sys.exit(main())
