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

ROOT = Path(__file__).resolve().parents[2]
SOURCE = 'scripts/bench/evidence_eval.py'
CONTRACT = 'docs/reference/benchmarks/class-commitments-v1.json'
DEPENDENCIES = (
    'scripts/bench/evidence_protocol.py',
    'scripts/bench/gaze_bench_score.py',
    'scripts/bench/bench_subprocess.py',
)
RULEPACK_DIR = 'crates/gaze-recognizers/embedded'
MODEL_DECLARATION = 'scripts/bench/no_opf_models.toml'


def emit(record):
    print(json.dumps(record, sort_keys=True), flush=True)


def git_bytes(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT, stderr=subprocess.PIPE)


def compare(baseline, candidate, mode):
    import benchmark_evidence_eval as benchmark

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
    try:
        revision = git_bytes('rev-parse', '--verify', args.baseline_revision + '^{commit}').decode().strip()
    except subprocess.CalledProcessError:
        parser.error('baseline commit is unavailable locally')
    if revision != args.baseline_revision:
        parser.error('--baseline-revision must identify the commit itself')
    contract = (ROOT / CONTRACT).read_bytes()
    try:
        source = git_bytes('show', f'{revision}:{SOURCE}')
        baseline_contract = git_bytes('show', f'{revision}:{CONTRACT}')
        baseline_dependencies = {path: git_bytes('show', f'{revision}:{path}') for path in DEPENDENCIES}
    except subprocess.CalledProcessError:
        parser.error('baseline evaluator, contract, or dependency is unavailable locally')
    if baseline_contract != contract:
        parser.error('baseline and candidate class contracts differ')
    dependency_hashes = {}
    for path, baseline_bytes in baseline_dependencies.items():
        current_bytes = (ROOT / path).read_bytes()
        if baseline_bytes != current_bytes:
            parser.error(f'baseline and candidate dependency differ: {path}')
        dependency_hashes[path] = dict(baseline_sha256=hashlib.sha256(baseline_bytes).hexdigest(),
                                      candidate_sha256=hashlib.sha256(current_bytes).hexdigest())
    # gaze_bench_score initializes its vocabulary from these files at import time.
    baseline_rulepacks = {path for path in git_bytes(
        'ls-tree', '-r', '--name-only', revision, '--', RULEPACK_DIR).decode().splitlines()
        if str(Path(path).parent) == RULEPACK_DIR and path.endswith('.toml')}
    current_rulepacks = {str(path.relative_to(ROOT)) for path in (ROOT / RULEPACK_DIR).glob('*.toml')}
    if baseline_rulepacks != current_rulepacks:
        parser.error('baseline and candidate rulepack inventories differ')
    initialization_data = {}
    for path in sorted(baseline_rulepacks | {MODEL_DECLARATION}):
        try:
            baseline_bytes = git_bytes('show', f'{revision}:{path}')
            current_bytes = (ROOT / path).read_bytes()
        except (subprocess.CalledProcessError, OSError):
            parser.error('baseline or candidate initialization data is unavailable locally')
        if baseline_bytes != current_bytes:
            parser.error(f'baseline and candidate initialization data differ: {path}')
        initialization_data[path] = dict(baseline_sha256=hashlib.sha256(baseline_bytes).hexdigest(),
                                        candidate_sha256=hashlib.sha256(current_bytes).hexdigest())
    candidate_source = (ROOT / SOURCE).read_bytes()
    if source == candidate_source:
        parser.error('baseline and candidate sources are identical; comparison would be vacuous')
    emit(dict(scope='generated_counts_only', baseline_revision=revision,
              baseline_source_sha256=hashlib.sha256(source).hexdigest(),
              candidate_revision=git_bytes('rev-parse', 'HEAD').decode().strip(),
              candidate_source_sha256=hashlib.sha256(candidate_source).hexdigest(),
              candidate_source_dirty=bool(git_bytes('status', '--porcelain', '--', SOURCE)),
              dependencies=dependency_hashes,
              initialization_data=initialization_data,
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
    import evidence_eval as candidate

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
            compare(baseline, candidate, args.mode)
        finally:
            del sys.modules[spec.name]


if __name__ == '__main__':
    main()
