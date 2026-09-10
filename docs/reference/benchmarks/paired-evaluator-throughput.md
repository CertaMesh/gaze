# Paired evaluator throughput, synthetic measurement

Measured 10 September 2026 on the same Apple Silicon host with CPython 3.13.13
and the offline, locked benchmark environment. Base: `ea3f59872c732025ba911c663e551386c5700c14`.
Comparison revision: `f4d7cc37c3b5f23d33b1e1df9eac3263347f11c3`.
Candidate changes only resampled multiplicity counting in `evidence_eval.py`,
replacing repeated list scans with one `collections.Counter` per estimate.

This is evaluator throughput, not model inference or Gaze request latency.
There is no detector quality, release readiness or private-corpus claim.

## Method and results

The committed `compare_evidence_eval.py` used `benchmark_evidence_eval.py` to
generate 999 cases in 500 groups,
with group sizes 1–3, weights 1–5, two strata, seed 20260910, 64 resamples and a
95% percentile interval. Construction was outside timing. Each measurement
followed a discarded full warmup. Four rounds alternated base/candidate order;
an exclusive machine lease and a fresh process check preceded timing, and no
owned test or build job ran concurrently. The baseline module came from `git show`
of the full immutable base revision. The command verified byte equality of the
class contract, all three local dependency modules, the exact embedded rulepack
file set and contents, and the model-ID declaration TOML. No model weights were read.

| Arm | Four samples, seconds | Median, seconds |
| --- | --- | --- |
| Base | 1.201377, 1.202327, 1.197410, 1.189974 | 1.199393 |
| Candidate | 0.537414, 0.585611, 0.538795, 0.539505 | 0.539150 |

Observed speedup: **2.22×** for this workload. These samples replace the earlier
2.20× measurement made with an uncommitted comparison script. No timing threshold
was added to CI. Host scheduling and workload size affect this ratio.

Every measured interval was exactly equal. Thirty additional comparisons
covered seeds 0–9 at 2, 7 and 25 groups, each with 16 resamples. They also
matched exactly. The existing exhaustive oracle, refusal, outcome and receipt
checks plus the added partial-group and repeated-group checks passed:
129 tests via `python -m unittest discover -s scripts/bench -p 'test_evidence*.py'`.
Seven subprocess refusal tests also passed, covering mutable/unknown baselines,
identical sources, missing timing lease, contract and dependency drift, and
added, changed or deleted initialization data. Initial refusal tests exposed
imports running before provenance checks; the final command checks provenance
before importing evaluator code.

[Sanitized measurement records](paired-evaluator-throughput.jsonl) contain source
and dependency SHA256s, initialization-data hashes, Python/platform, configuration,
30-case parity and all eight samples. Raw commands, local paths and lease
identifiers remain in local evidence. Serialization is an operator assertion
backed by those local records, not a property the comparison tool verifies.

## Structure review

- Verdict: implement.
- Opportunity: one local multiplicity map per estimate.
- Why: removes quadratic rescanning without changing arithmetic order or the
  invariant that all records of a sampled group occur equally often.
- Scope: paired estimator only; its interval and receipt callers retain their
  existing behavior. No persistent cache or mirrored lifecycle state.
- Validation: exact before/after parity, independent existing arithmetic oracle,
  malformed group rejection, 129 evidence tests, seven provenance refusal tests
  and the timing samples above.

Reproduce the fixed-baseline comparison using the command in
[`scripts/bench/README.md`](../../../scripts/bench/README.md#paired-evaluator-throughput-synthetic-only).
