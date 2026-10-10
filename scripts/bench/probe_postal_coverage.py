#!/usr/bin/env python3
"""Record value-free postal source attribution for a setup policy and its DE twin."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib

import agentic_layers


def measure(binary: Path, policy: Path) -> dict:
    records = [r for r in agentic_layers._coverage_records('test')
               if r.family == 'postal_de']
    if len(records) != 20:
        raise ValueError('expected ten postal positives and ten benign twins')
    requests = ''.join(json.dumps({'fixture_id': r.uid, 'text': r.text,
                                   'locale_chain': r.to_document().locale_chain}) + '\n'
                       for r in records)
    result = subprocess.run([str(binary.resolve()), '--config', 'policy-file'],
                            input=requests, capture_output=True, text=True, check=True, timeout=300,
                            env=dict(os.environ, GAZE_BENCH_POLICY=str(policy.resolve())))
    responses = [json.loads(line) for line in result.stdout.splitlines()]
    if len(responses) != len(records):
        raise ValueError('producer response count differs from the corpus')
    documents = []
    for record, response in zip(records, responses):
        if response.get('fixture_id') != record.uid:
            raise ValueError('producer returned a different document')
        if 'final_protection_trace' not in response:
            raise ValueError('producer refused or omitted the protection trace')
        documents.append({'uid': record.uid, 'layer': record.layer,
                          'trace': [{'raw_start': t['raw_start'], 'raw_end': t['raw_end'],
                                     'class': t['class'],
                                     'source_ids': t['provenance']['source_ids']}
                                    for t in response['final_protection_trace']]})
    return {'locale_chain': tomllib.loads(policy.read_text())['locale']['active'],
            'policy_sha256': hashlib.sha256(policy.read_bytes()).hexdigest(),
            'documents': documents}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--policy', type=Path, required=True)
    parser.add_argument('--de-policy', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    original = tomllib.loads(args.policy.read_text())
    german = tomllib.loads(args.de_policy.read_text())
    expected = [original['locale']['active'].copy(), german['locale']['active'].copy()]
    if expected[1] != ['de-DE'] + [locale for locale in expected[0] if locale != 'de-DE']:
        raise ValueError('DE policy must move only de-DE to the front')
    original.pop('locale'); german.pop('locale')
    if original != german:
        raise ValueError('policies differ beyond locale order')
    result = {'schema_version': 1, 'generator_version': agentic_layers.GENERATOR_VERSION,
              'harness': agentic_layers.score.git_metadata(Path(__file__).resolve().parents[2]),
              'corpus_sha256': hashlib.sha256(agentic_layers.corpus_bytes(
                  agentic_layers.generate('test'))).hexdigest(),
              'binary_sha256': hashlib.sha256(args.binary.read_bytes()).hexdigest(),
              'runs': [measure(args.binary, p) for p in (args.policy, args.de_policy)]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
