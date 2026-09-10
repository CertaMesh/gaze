"""Time paired intervals on generated counts only; no corpus or model access."""
from __future__ import annotations

import argparse
import json
from statistics import median
from time import perf_counter

import evidence_eval as ee


METRIC = 'gold_bytes_surviving_egress'


def synthetic_evaluator(groups):
    cases = []
    for group in range(groups):
        for member in range(1 + group % 3):
            cases.append(ee.PlannedCase(
                f'case_{group}_{member}', f'group_{group}',
                ('synthetic_en', 'synthetic_de')[group % 2],
                float(1 + group % 5), gold_bytes=100,
            ))
    evaluator = ee.PrivateEvaluator(ee.PlannedInventory(cases))
    for index, case in enumerate(cases):
        for arm, count in [('base', index % 31), ('candidate', (index * 7) % 29)]:
            evaluator.add(arm, case.key, ee.DocRecord(
                observed_metrics=frozenset((METRIC,)), outcome='COMPLETED',
                gold_bytes_surviving=count,
            ))
    return evaluator.finalize()


def measure(groups=500, resamples=64, repetitions=5):
    evaluator = synthetic_evaluator(groups)
    declaration = dict(
        confidence_level=.95, resample_count=resamples, seed=20260910,
        strata=['synthetic_en', 'synthetic_de'], weighting='inventory_group',
        multiplicity_treatment='synthetic_none', coverage_target=.8,
        acceptance_limit=.1,
    )
    # Discard one full warmup. Construction is outside the measured interval.
    expected = evaluator.paired_interval(METRIC, declaration)
    if not isinstance(expected, dict):
        raise RuntimeError('synthetic_interval_unavailable')
    elapsed = []
    for _ in range(repetitions):
        start = perf_counter()
        result = evaluator.paired_interval(METRIC, declaration)
        elapsed.append(perf_counter() - start)
        if result != expected:
            raise RuntimeError('synthetic_interval_nondeterministic')
    return dict(
        scope='generated_counts_only', groups=groups, cases=len(evaluator.inventory),
        resamples=resamples, repetitions=repetitions, seed=declaration['seed'],
        seconds=elapsed, median_seconds=median(elapsed), interval=expected,
    )


def positive(value):
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError('must be positive')
    return parsed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--groups', type=positive, default=500)
    parser.add_argument('--resamples', type=positive, default=64)
    parser.add_argument('--repetitions', type=positive, default=5)
    args = parser.parse_args()
    if args.groups < 2:
        parser.error('--groups must cover both synthetic strata (at least 2)')
    print(json.dumps(measure(args.groups, args.resamples, args.repetitions), sort_keys=True))


if __name__ == '__main__':
    main()
