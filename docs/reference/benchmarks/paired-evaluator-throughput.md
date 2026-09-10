# Paired evaluator throughput, synthetic measurement

Measured 10 September 2026 on the same Apple Silicon host with CPython 3.13.13
and the locked benchmark environment. Base: `ea3f59872c732025ba911c663e551386c5700c14`.
Candidate changes only resampled multiplicity counting in `evidence_eval.py`,
replacing repeated list scans with one `collections.Counter` per estimate.

This is evaluator throughput, not model inference or Gaze request latency.
There is no detector quality, release readiness or private-corpus claim.

## Method and results

The committed `benchmark_evidence_eval.py` generated 999 cases in 500 groups,
with group sizes 1–3, weights 1–5, two strata, seed 20260910, 64 resamples and a
95% percentile interval. Construction was outside timing. Each measurement
followed a discarded full warmup. Four rounds alternated base/candidate order;
no test suite ran concurrently with these measurements. The baseline module
came from `git show` of the base revision and resolved the same unchanged
committed class contract as the candidate.

| Arm | Four samples, seconds | Median, seconds |
| --- | --- | --- |
| Base | 1.228080, 1.218373, 1.217468, 1.216230 | 1.217921 |
| Candidate | 0.551614, 0.553273, 0.552237, 0.555790 | 0.552755 |

Observed speedup: **2.20×** for this workload. No timing threshold
was added to CI. Host scheduling and workload size affect this ratio.

Every measured interval was exactly equal. Thirty additional comparisons
covered seeds 0–9 at 2, 7 and 25 groups, each with 16 resamples. They also
matched exactly. The existing exhaustive oracle, refusal, outcome and receipt
checks plus the added partial-group and repeated-group checks passed:
129 tests via `python -m unittest discover -s scripts/bench -p 'test_evidence*.py'`.

## Structure review

- Verdict: implement.
- Opportunity: one local multiplicity map per estimate.
- Why: removes quadratic rescanning without changing arithmetic order or the
  invariant that all records of a sampled group occur equally often.
- Scope: paired estimator only; its interval and receipt callers retain their
  existing behavior. No persistent cache or mirrored lifecycle state.
- Validation: exact before/after parity, independent existing arithmetic oracle,
  malformed group rejection, 129 affected tests and the timing samples above.

Reproduce a current-revision measurement using the command in
[`scripts/bench/README.md`](../../../scripts/bench/README.md#paired-evaluator-throughput-synthetic-only).
