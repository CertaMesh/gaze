# Published row receipt schema

`published-rows.json` is a schema-version-1 snapshot of the capture leaves used
by every receipt-backed public row in `their-benchmarks.json`. It contains no
text, document identifiers, or predictions. The exporter is
[`row_receipts.py`](../../../../scripts/bench/compare/theirbench/row_receipts.py).

`benchmarks.BENCHMARK.TOOL` contains these fields:

| Field | Meaning |
| --- | --- |
| `row` | Complete measured row, including latency, null values, and counts. |
| `own_metric` | Complete published vendor-evaluator metric tree. |
| `own_scorer_input` | Dataset and prediction digests and independent scorer counts. |
| `prediction_sha256` | Prediction stream measured by the capture. |
| `source` | SHA-256 of the retained capture report and vendor-evaluator result, or the historical Git blob for unchanged Gaze and tuned rows. |
| `latency_evidence` | `aggregate-only` with the missing-samples limit, or `samples-retained` with sample file, sample SHA-256, and prediction SHA-256. |

Receipt-verified covers quality/count metrics only. All historical entries here
have single-run, machine-dependent timing aggregates. Per-document samples were
not retained, so p50/p95 cannot be reproduced from committed data. This includes
both refreshed OPF rows and all three PII-Tracer rows. Comparing these values
with a capture receipt verifies transcription, not the timing measurement.
No samples have been backfilled.

For future competitor captures, the runner retains one JSONL sample per document,
with exactly `index` and `ns`. The index starts at zero and follows prediction
order. The duration is a nonnegative integer in nanoseconds. The receipt binds
these samples to the prediction digest. Export and rescore recompute p50/p95 and
the sample count before accepting them. The receipt test rejects any changed
published leaf, including latency, and any changed retained sample byte.

The compressed historical aggregate is the exact original Git blob cited by the
panel manifest. It preserves the source of unchanged Gaze and tuned rows. Fresh
competitor snapshots were extracted from retained reports and evaluator results,
with their source digests. Raw predictions stay outside the repository.
