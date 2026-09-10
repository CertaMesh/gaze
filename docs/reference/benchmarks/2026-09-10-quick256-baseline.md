# Fresh canonical quick256 baseline, 2026-09-10

The run completed all 256 documents in all three cells. Pass2 remains the reversible reference: 2,463 trace-derived leaked labeled UTF-8 bytes, 256 exact restores. Kiji reduces that leakage count by 170 bytes but fails exact restoration on 55 documents and adds 12,737 false-positive bytes. This is completed bad-quality evidence, not a collection failure or a release candidate.

| Cell | Trace-derived leaked bytes | False-positive bytes | Exact restores | Redact actions / documents |
| --- | ---: | ---: | ---: | ---: |
| rule-floor-extended | 8,075 | 439 | 256/256 | 0 / 0 |
| pass2-ner | 2,463 | 2,648 | 256/256 | 0 / 0 |
| full-stack-kiji-resolve | 2,293 | 15,385 | 201/256 | 142 / 55 |

Each cell planned, attempted, completed and scored 256 documents. Actual refusals, runtime/protocol errors and unprocessed rows were zero. All 256 manifests per cell passed validation and all restore decisions reported success; exact byte restoration is a separate measurement. Kiji had 164 **strict-would-reject** documents, an attribute of completed outputs, not actual refusals. Its residual scan found 30 suspects; floor/pass2 did not run that scan.

The sample contains 166 Dataiku positives and 90 A4 negatives, 120 German and 136 English documents, 1,275 entities and 11,316 labeled UTF-8 union bytes. A4 false-positive bytes were 368 / 628 / 3,967; positive-document false-positive bytes were 71 / 2,020 / 11,418, respectively. Per-language, per-label, validator and all eight negative-category aggregates are in the companion JSON.

## Provenance and costs

Accepted source was `6e1460c1292d78adf98f69d57f0f917475385db1`. Measurement used clean signed/DCO commit `7fbb7f62912f5aaa7d432de88910235e5b8acb5e`, changing only four local package version entries in the validator probe's nested Cargo.lock. Production and scorer source are unchanged. Offline lock regeneration's unrelated registry changes were rejected; all existing registry pins were retained.

Canonical route: `pipeline_text/clean_for_bench`. Profile: `quick --quick-documents 256 --seed 20260710 --warmups 0 --measured-repetitions 1`, cached pinned Davlan/Kiji fp32, threshold 0.3, no downloads. Both exact-checkout binaries were built with `--locked` before `--skip-build`. Rust 1.96.0, explicit RUSTC/RUSTDOC/PATH, two Cargo jobs, verified cached ONNX Runtime static library with explicit ORT_LIB_PATH.

Full model and dataset hashes were verified before inference. Selected development IDs were frozen before inference, SHA-256 `3993b14aeded90b056854d29172de148424b6e7b0eb967395e6b78de2fbbce36`; every scored population matched exactly. Source/config/binary/model/data hashes and full selected IDs are in [the sanitized metadata](2026-09-10-quick256-baseline.json). The scorecard remains local and its hash is included there.

Measured build/bootstrap command time totaled **245.087 seconds**, including failures and repeated incremental checks. The initial workspace build failed after 157.272 seconds because offline linking needed ORT_LIB_PATH; the corrected build passed in 6.085 seconds. The first clean producer build passed in 33.158 seconds. The nested lock first failed in 0.062 seconds; its repaired validator build passed in 47.377 seconds. Final post-commit locked checks passed. These are cache-assisted build costs, not an empty-machine build benchmark.

Canonical run wall time was **94.528 seconds**, exit **4**. Per-cell wall seconds were **2.324 / 24.229 / 63.348**. External process start to first validated response was **1.097 / 9.977 / 23.352 seconds**, respectively; this includes model startup and the first request, not pure initialization. Both the 20-minute build budget and 15-minute inference estimate were met. The exclusive lease was released and MACHINE FREE sent before packaging.

## Limits

Leakage is **TRACE-DERIVED**, using gold raw-byte coverage and final protection traces. It is not independently verified surviving PII in actual clean output. Redact output replay remains unverified. Per-success-row outcomes and independent partial/full surviving occurrences are unavailable; missing observations are not zero. Reduced leakage cannot compensate for irreversible redaction or failed exact restoration.

Dataiku has historical development contamination. This frozen sample is development/regression evidence; remaining Dataiku can only be confirmation within this programme, not an independent holdout. CLI defaults, MCP, daemon, proxy and streaming are NOT_MEASURED. No model tuning, evaluator tuning, OPF repair, second inference run, private data or raw dataset payload publication occurred.

Readiness failed on Kiji's restore failures, strict-would-reject outputs, leakage, uncovered entities, residual suspects and redact actions. A quick sample also cannot establish full-profile readiness.

## Five-field self-review

- **Verdict:** implement.
- **Opportunity:** consistency of local package versions in the existing nested lockfile.
- **Why:** removes a broken locked-build prerequisite without changing registry pins, runtime/scoring logic or the five project axes.
- **Scope:** four nested Cargo.lock version lines and sanitized reports. No new abstraction or runtime change.
- **Validation:** locked offline builds passed; full input hashes, frozen IDs and all aggregate counts reconciled. Flow review found zero runtime logic changes. No visual output changed.
