#!/usr/bin/env python3
"""Run one capture phase under Anvil's build and benchmark locks."""
from __future__ import annotations

import json
import os
import shutil
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def run(args, cwd):
    if shutil.disk_usage("/System/Volumes/Data").free < 105 * 1024**3:
        raise SystemExit("disk headroom below 105 GiB; retry after cleanup")
    subprocess.run(args, cwd=cwd, check=True)


def main():
    subject = Path(os.environ['GAZE_KR_SUBJECT']).resolve()
    revision = os.environ['GAZE_KR_REVISION']
    kind = os.environ['GAZE_KR_CAPTURE_KIND']
    if kind not in {'setup', 'full', 'normalization'}:
        raise SystemExit('invalid capture stage')
    actual = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=subject, text=True).strip()
    if actual != revision or subprocess.check_output(['git', 'status', '--porcelain'], cwd=subject).strip():
        raise SystemExit('capture subject must be clean at the declared revision')
    policy = subject/'target/gate/policy.toml'
    output = subject/'target/bench-data/known-record-normalization'/kind
    if kind == 'setup':
        policy.parent.mkdir(parents=True, exist_ok=True)
        run(['anvil', 'run', str(subject), '--', 'nice', '-n', '10', 'cargo', 'run', '-p', 'gaze-cli', '--',
             'setup', '--non-interactive', '--force', '--policy-out', str(policy)], subject)
    else:
        if not policy.is_file():
            raise SystemExit('run setup on this subject first')
        if (output/'observations-v1.jsonl.gz').exists():
            raise SystemExit('observation already exists; verify and reuse it instead of recapturing')
        python = [sys.executable]
        model = Path(os.environ.get('GAZE_NER_MODEL_DIR', str(Path.home()/'.local/share/gaze/models/davlan-mbert-ner-hrl')))
        if kind == 'full':
            dataset = Path(os.environ['GAZE_KR_DATASET']).resolve()
            run([*python, 'scripts/bench/rescore_past_release.py', '--release-root', str(subject), '--build',
                 '--binary-profile', 'release', '--configs', 'policy-file', '--policy', str(policy),
                 '--agentic-layers', '--seed', '20260710', '--dataset', str(dataset),
                 '--model-dir', str(model), '--no-cache', '--output', str(output/'capture.json')], ROOT)
            for version in (1, 2, 4):
                contract = ROOT/f'docs/reference/benchmarks/scored-labels-v{version}.json'
                args = [*python, 'scripts/bench/rescore.py', str(output/'observations-v1.jsonl.gz'),
                        '--output', str(output/f'scorecard-v{version}.json')]
                if version != 1:
                    args += ['--scored-labels', str(contract)]
                run(args, ROOT)
        else:
            run([*python, 'scripts/ci/known_record_normalization_capture.py', '--subject', str(subject),
                 '--policy', str(policy), '--model-dir', str(model), '--output-dir', str(output)], ROOT)
    print(json.dumps({'phase': kind, 'subject_revision': revision, 'output': str(output)}))


if __name__ == '__main__':
    main()
