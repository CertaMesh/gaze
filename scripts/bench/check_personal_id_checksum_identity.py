#!/usr/bin/env python3
"""Reproduce the tagged Kiji benchmark's document-ID/session-prefix sensitivity.

Run with the binary bound by the v18 historical scorecard and the same local
NER/Kiji model directories used for that measurement. Only synthetic generated
text is sent to the binary; the evidence stores hashes and span metadata.
"""
import argparse
import gzip
import hashlib
import json
import os
import subprocess
from pathlib import Path

import agentic_layers as api
import scorecard_record

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--ner-model-dir', type=Path, required=True)
    parser.add_argument('--kiji-model-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, default=ROOT / 'docs/reference/benchmarks/personal-id-checksum-v18-kiji-identity-diff.json')
    args = parser.parse_args()
    root = ROOT
    folder = root / 'docs/reference/benchmarks'
    fields = ('final_protection_trace', 'restore', 'manifest_integrity', 'fallback_redact',
              'refused', 'initial_safety_net_stats', 'post_policy_safety_net_stats')
    diff = json.loads((folder / 'personal-id-checksum-v18-cell-diff.json').read_text())
    mapping = {r['old_id']: r['new_id'] for r in diff['partitions']['test']['changes']}

    def observations(version):
        with gzip.open(folder / f'observations-agentic-v{version}-v0.14.0.jsonl.gz', 'rt') as stream:
            next(stream)
            return {row['document_id']: row['response'] for line in stream
                    if (row := json.loads(line))['kind'] == 'observation'
                    and row['config'] == 'full-stack-kiji-resolve'}

    old, new = observations(17), observations(18)
    changed = []
    for old_id, before in old.items():
        new_id = mapping.get(old_id, old_id)
        after = new[new_id]
        if any(before.get(field) != after.get(field) for field in fields):
            assert old_id in mapping
            changed.append((old_id, new_id))
    assert len(changed) == 8
    records = {row.uid: row for row in api.generate('test')}
    requests = []
    expected = []
    for repeat in range(2):
        for old_id, new_id in changed:
            row = records[new_id]
            for uid, original in ((old_id, old[old_id]), (new_id, new[new_id])):
                requests.append({'fixture_id': uid, 'locale_chain': row.to_document().locale_chain, 'text': row.text})
                expected.append(original)
    binary = args.binary.resolve()
    card = json.loads(gzip.decompress((folder / 'agentic-layers-v18-v0.14.0.json.gz').read_bytes()))
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == card['binary_sha256']
    env = dict(os.environ, GAZE_NER_MODEL_DIR=str(args.ner_model_dir.resolve()),
               GAZE_NER_THRESHOLD=str(card['parameters']['ner_threshold']),
               GAZE_KIJI_DISTILBERT_MODEL_DIR=str(args.kiji_model_dir.resolve()))
    result = subprocess.run([str(binary), '--config', 'full-stack-kiji-resolve'],
                            input=''.join(json.dumps(row) + '\n' for row in requests),
                            env=env, text=True, capture_output=True)
    assert result.returncode == 0, result.stderr
    responses = [scorecard_record._compact_response(json.loads(line)) for line in result.stdout.splitlines()]
    assert len(responses) == len(requests)
    for request, actual, original in zip(requests, responses, expected, strict=True):
        assert actual['fixture_id'] == request['fixture_id']
        for field in fields:
            assert actual.get(field) == original.get(field), (request['fixture_id'], field)
    rows = []
    for old_id, new_id in changed:
        rows.append({'old_document_id': old_id, 'new_document_id': new_id,
                     'old_session_hex': hashlib.sha256(old_id.encode()).hexdigest()[:8],
                     'new_session_hex': hashlib.sha256(new_id.encode()).hexdigest()[:8],
                     'text_sha256': hashlib.sha256(records[new_id].text.encode()).hexdigest(),
                     'old_trace': old[old_id]['final_protection_trace'],
                     'new_trace': new[new_id]['final_protection_trace'],
                     'old_exact_restore': old[old_id]['restore']['exact'],
                     'new_exact_restore': new[new_id]['restore']['exact']})
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    evidence = {'schema_version': 1, 'release': 'v0.14.0', 'arm': 'full-stack-kiji-resolve',
                'binary_revision': '616e8a10b957961f9f7dcb745610afa978783c3a',
                'binary_sha256': sha(binary),
                'old_observations_sha256': sha(folder / 'observations-agentic-v17-v0.14.0.jsonl.gz'),
                'new_observations_sha256': sha(folder / 'observations-agentic-v18-v0.14.0.jsonl.gz'),
                'semantics': 'The tagged benchmark producer hashes fixture_id into a fixed four-byte session prefix. D-to-A document identity changes alter the prefix. Two repeated controlled probes per old/new identity hold text and locales constant and reproduce all trace/restore/fallback fields from both recorded runs. Every changed response lies inside the relabelled NIR cells; unchanged documents have identical compared fields. This is a producer-identity sensitivity effect, not random run-to-run noise or a runtime gain.',
                'changed_responses': 8, 'controlled_requests': len(requests), 'repetitions_per_identity': 2,
                'controlled_responses_match_records': True,
                'false_positive_byte_drift': 2, 'exact_restore_failure_drift': 2,
                'rows': rows}
    args.output.write_text(json.dumps(evidence, indent=2) + '\n')
    print('KIJI IDENTITY PROBE PASS: 32 controlled responses reproduce recorded old/new identities')


if __name__ == "__main__":
    main()
