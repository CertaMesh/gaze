"""Compare generated intervals against an immutable evaluator revision, without models."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import platform
import re
from statistics import median
import subprocess
import sys
import tempfile

import benchmark_evidence_eval as benchmark
import evidence_eval as candidate


ROOT = Path(__file__).resolve().parents[2]
SOURCE = 'scripts/bench/evidence_eval.py'
CONTRACT = 'docs/reference/benchmarks/class-commitments-v1.json'


def emit(record):
    print(json.dumps(record, sort_keys=True), flush=True)


def git_bytes(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)


def compare(baseline, mode):
    if mode != 'timing':
        cases = 0
        for groups in (2, 7, 25):
            for seed in range(10):
                declaration = dict(
                    confidence_level=.95, resample_count=16, seed=seed,
                    strata=['synthetic_en', 'synthetic_de'], weighting='inventory_group',
                    multiplicity_treatment='synthetic_none', coverage_target=.8,
                    acceptance_limit=.1,
                )
                intervals = [benchmark.synthetic_evaluator(groups, module).paired_interval(
                    benchmark.METRIC, declaration) for module in (baseline, candidate)]
                if not isinstance(intervals[0], dict) or intervals[0] != intervals[1]:
                    raise RuntimeError(f'interval_parity_failed: groups={groups} seed={seed}')
                cases += 1
        emit(dict(parity_cases=cases, exact=True, groups=[2, 7, 25],
                  seeds=list(range(10)), resamples=16))
    if mode == 'parity':
        return
    samples = {'base': [], 'candidate': []}
    expected = None
    for repetition in range(4):
        order = [('base', baseline), ('candidate', candidate)]
        if repetition % 2:
            order.reverse()
        for arm, module in order:
            result = benchmark.measure(groups=500, resamples=64, repetitions=1, module=module)
            if expected is None:
                expected = result['interval']
            if result['interval'] != expected:
                raise RuntimeError('timed_interval_parity_failed')
            samples[arm].append(result['median_seconds'])
            emit(dict(arm=arm, round=repetition, result=result))
    base_seconds = median(samples['base'])
    candidate_seconds = median(samples['candidate'])
    emit(dict(exact_interval_parity=True, base_median_seconds=base_seconds,
              candidate_median_seconds=candidate_seconds, speedup=base_seconds / candidate_seconds))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline-revision', required=True,
                        help='full immutable 40-character commit hash; branches and HEAD refused')
    parser.add_argument('--mode', choices=('parity', 'timing', 'both'), default='both')
    parser.add_argument('--machine-lease', help='active exclusive lease reference for timing')
    args = parser.parse_args()
    if not re.fullmatch(r'[0-9a-f]{40}', args.baseline_revision):
        parser.error('--baseline-revision must be a full lowercase commit hash')
    if args.mode != 'parity' and not args.machine_lease:
        parser.error('timing requires --machine-lease and no concurrent owned jobs')
    revision = git_bytes('rev-parse', '--verify', args.baseline_revision + '^{commit}').decode().strip()
    if revision != args.baseline_revision:
        parser.error('--baseline-revision must identify the commit itself')
    source = git_bytes('show', f'{revision}:{SOURCE}')
    contract = (ROOT / CONTRACT).read_bytes()
    if git_bytes('show', f'{revision}:{CONTRACT}') != contract:
        parser.error('baseline and candidate class contracts differ')
    candidate_source = Path(candidate.__file__).read_bytes()
    if source == candidate_source:
        parser.error('baseline and candidate sources are identical; comparison would be vacuous')
    emit(dict(scope='generated_counts_only', baseline_revision=revision,
              baseline_source_sha256=hashlib.sha256(source).hexdigest(),
              candidate_revision=git_bytes('rev-parse', 'HEAD').decode().strip(),
              candidate_source_sha256=hashlib.sha256(candidate_source).hexdigest(),
              class_contract_sha256=hashlib.sha256(contract).hexdigest(),
              python=sys.version, platform=platform.platform(),
              command=[sys.executable, *sys.argv], mode=args.mode,
              config=dict(parity_groups=[2, 7, 25], parity_seeds=list(range(10)),
                          parity_resamples=16, timing_groups=500, timing_resamples=64,
                          timing_seed=20260910, rounds=4, repetitions_per_arm=1,
                          warmups_per_arm=1, alternating_order=True),
              machine_lease=args.machine_lease,
              timing_requires_no_concurrent_owned_jobs=args.mode != 'parity'))
    # Preserve the module's real relative contract lookup, with byte-identical data.
    with tempfile.TemporaryDirectory(prefix='gaze-evaluator-comparison-') as directory:
        root = Path(directory)
        path = root / SOURCE
        path.parent.mkdir(parents=True)
        path.write_bytes(source)
        contract_path = root / CONTRACT
        contract_path.parent.mkdir(parents=True)
        contract_path.write_bytes(contract)
        spec = importlib.util.spec_from_file_location('gaze_baseline_evidence_eval', path)
        baseline = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = baseline
        try:
            spec.loader.exec_module(baseline)
            compare(baseline, args.mode)
        finally:
            del sys.modules[spec.name]


if __name__ == '__main__':
    main()
