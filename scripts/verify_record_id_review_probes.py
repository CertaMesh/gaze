#!/usr/bin/env python3
"""Replay the committed review inputs with the real CLI, recording every verdict and restore."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'crates/gaze/tests/fixtures/record-id-review-probes.json'

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    cases = json.loads(FIXTURE.read_text())
    assert {r: sum(c['round'] == r for c in cases) for r in ('r8', 'r9')} == {'r8': 83, 'r9': 137}
    env = dict(os.environ, GAZE_DISABLE_OS_KEYSTORE='1')
    def invoke(command, value):
        result = subprocess.run([str(args.binary.resolve()), command], input=value, capture_output=True, env=env, check=True)
        return json.loads(result.stdout)
    rows = []
    for case in cases:
        clean = invoke('clean', case['text'].encode())
        restored = invoke('restore', json.dumps({'session_blob': clean['session_blob'], 'text': clean['clean_text']}).encode())
        assert restored['text'] == case['text'], case['name']
        original_oracle = (all(v not in clean['clean_text'] for v in case['values']) if case['kind'] == 'L' else clean['clean_text'] == case['text'])
        rows.append(dict(case, original_oracle=original_oracle, restore_exact=True, clean=clean['clean_text'], raws=[e['raw'] for e in clean.get('entries', [])]))
    summary = {r: dict(count=sum(c['round'] == r for c in rows), original_oracle_failures=[c['name'] for c in rows if c['round'] == r and not c['original_oracle']], exact_restores=sum(c['round'] == r and c['restore_exact'] for c in rows)) for r in ('r8', 'r9')}
    artifact = dict(head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(), fixture_sha256=hashlib.sha256(FIXTURE.read_bytes()).hexdigest(), summary=summary, observations=rows)
    args.output.write_text(json.dumps(artifact, indent=2, ensure_ascii=False)+'\n')
    print(json.dumps(summary))

if __name__ == '__main__':
    main()
