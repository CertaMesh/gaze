#!/usr/bin/env python3
"""Run one capture phase under Anvil's build and benchmark locks."""
from __future__ import annotations

import json
import os
import shutil
import re
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def run(args, cwd, *, allowed=(0,), release_environment=False):
    if shutil.disk_usage("/System/Volumes/Data").free < 105 * 1024**3:
        raise SystemExit("disk headroom below 105 GiB; retry after cleanup")
    free = re.search(r"System-wide memory free percentage: (\d+)%", subprocess.check_output(["memory_pressure"], text=True))
    if free is None or int(free[1]) < 30 or (Path.home()/".cache/root-coordinator/teyos-turn.active").exists():
        raise SystemExit("shared resource preflight failed; retry when memory/marker guard clears")
    environment = {key: value for key, value in os.environ.items() if not key.startswith('GAZE_KR_')}
    if release_environment:
        for key in ('CARGO_INCREMENTAL', 'CARGO_PROFILE_DEV_DEBUG', 'CARGO_PROFILE_TEST_DEBUG'):
            environment.pop(key, None)
    result = subprocess.run(args, cwd=cwd, env=environment)
    if result.returncode not in allowed:
        raise SystemExit(result.returncode)
    return result


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
            if subject != ROOT:
                raise SystemExit('canonical full capture must run from its subject harness; reuse the shared base record')
            sys.path.insert(0, str(ROOT/'scripts/bench'))
            import dataiku_en_de_gaze_bench as dataiku
            source = Path(os.environ['GAZE_KR_DATASET']).resolve()
            dataset = ROOT/'target/bench-data/dataiku-en-de/test.parquet'
            if not dataset.exists():
                dataiku.verify_dataset(source)
                dataset.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, dataset)
            dataiku.verify_dataset(dataset)
            output = ROOT/'target/bench-data/gate-candidate/full'
            if (output/'observations-v1.jsonl.gz').exists():
                raise SystemExit('candidate record already exists; verify/reuse instead of recapturing')
            # Exact canonical invocation used by the shared main-corpus base.
            # Exit 4 means the captured product is not release-ready; keep it.
            run([*python, 'scripts/bench/run_no_opf_benchmark.py', 'full', '--seed', '20260710',
                 '--no-download', '--release', '--policy', 'target/gate/policy.toml',
                 '--output-dir', 'target/bench-data/gate-candidate'], ROOT,
                allowed=(0, 4), release_environment=True)
            for version in (1, 2, 4):
                contract = ROOT/f'docs/reference/benchmarks/scored-labels-v{version}.json'
                args = [*python, 'scripts/bench/rescore.py', str(output/'observations-v1.jsonl.gz'),
                        '--output', str(output/f'scorecard-scored-labels-v{version}.json')]
                if version != 1:
                    args += ['--scored-labels', str(contract)]
                run(args, ROOT)
        else:
            run([*python, 'scripts/ci/known_record_normalization_capture.py', '--subject', str(subject),
                 '--policy', str(policy), '--model-dir', str(model), '--output-dir', str(output)], ROOT)
    print(json.dumps({'phase': kind, 'subject_revision': revision, 'output': str(output)}))


if __name__ == '__main__':
    main()
