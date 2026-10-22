# Third-party receipt refresh

These rows originally had no prediction digest or scorer receipt. Each refreshed row passes `check_own_result` before its receipt marker is removed. The dataset, model and configuration remain pinned. Raw text and predictions stay outside the repository.

The tables keep every replaced numeric value from the published measurement. New per-label byte telemetry has no historical value and appears only in the aggregate. Unlisted metrics retain their published values. Timing is recorded for provenance and does not establish a speed gain.

Long decimal ratios use seven significant digits in this page. The [JSON history](third-party-receipt-history.json) preserves full precision for both values.

See the [receipt refresh guide](../../../scripts/bench/compare/theirbench/refresh-receipts.md) for capture and admission commands.

Published aggregate before this campaign: `e7e018ce8c4fa590a5c28db7031f030712a83103`.

Refreshed rows: 31 of 31.

## Presidio overlap order

This campaign records PYTHONHASHSEED=0. Presidio 2.2.364 deduplicates candidates through a Python set, then resolves equal-score overlaps in that order. Changing only the hash seed to 1 changed types in 31 PIIBench documents, with identical span coordinates. The pinned analyzer confirmed that every changed pair had equal scores.

The [aggregate order diagnostic](presidio-order-diagnostic.json) pins both prediction streams and the dataset. It demonstrates order sensitivity in retained captures. The original seed and predictions were not recorded, so its exact historical type choices cannot be recovered. Product-coverage leaked and false-positive byte totals reproduce for all five Presidio configurations. Typed and common-intersection metrics can differ because a tie assigns different types to the same protected bytes.

## presidio-research: datafog-core

Prediction SHA-256: `72a0589a7a5ec77131919fc33d79e6caf993ffffd9830718f37da640c3d70a92`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.003 | 0.002 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.009 | 0.007 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: datafog-core

Prediction SHA-256: `f525d8faa382540e4536ea3804edc07600cfda60426d2be71950ca5a5cc0a208`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p95_ms` | 0.038 | 0.036 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: datafog-regex

Prediction SHA-256: `79e7c050b6b8dafe5557adc355fc8a88c99b2e3abfd016dab4b11b266c66825e`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.047 | 0.04 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.087 | 0.075 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: datafog-regex

Prediction SHA-256: `36c1bf9a89b81740846b854a1a6cc9dbd78e2c7d12827a3256535daa11ef573d`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.076 | 0.07 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.441 | 0.399 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: scrubadub-base

Prediction SHA-256: `fbbdbccf4660e95c1dab43b7e1c6ba73a1610d965678355dfc8fde4ee319c8f5`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.035 | 0.033 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.129 | 0.122 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: scrubadub-base

Prediction SHA-256: `e6052bf8b83049f1e6e03e117dee6366b6f2cb094614ca07284545a17bbe7bbb`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.093 | 0.09 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.631 | 0.597 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: datafog-spacy

Prediction SHA-256: `3b19ca181f46d6aa904c7132f616e81569d8a254ef63de94d00e8c1df863fad3`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 4.706 | 5.647 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 9.843 | 11.451 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: datafog-spacy

Prediction SHA-256: `b4d6276d6a0df03925d11e063049068f5125ecc4992fc8bf211da4a7aac1937a`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 8.949 | 10.488 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 60.048 | 65.978 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: scrubadub-spacy

Prediction SHA-256: `7ac40c1d76e83a20d228b3e8a910e818e883765ea77a90bb00c27c9edb879386`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 4.686 | 6.573 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 9.834 | 14.57 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: scrubadub-spacy

Prediction SHA-256: `4f70b71154115f99ee0210c6eaefdec99bffac5aa0dfb3361f4f7565f077d63c`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 8.999 | 11.101 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 60.37 | 70.967 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: presidio-en

Prediction SHA-256: `3dc806b9b8397aadc9e6d76a6f960e90451bfa2fc7bf95607b69125d2bfbd8a4`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 5.213 | 6.802 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 10.971 | 14.601 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: presidio-en

Prediction SHA-256: `bf3e039c78e4232ea260c6a642c16124448654ec077f6d577e9effeac4860ce5`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.f1` | 0.1569 | 0.1568 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.precision` | 0.131 | 0.1309 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.recall` | 0.1955 | 0.1954 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f1` | 5.002966e-01 | 5.001687e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f2` | 5.486753e-01 | 5.486137e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f5` | 5.788136e-01 | 5.788005e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.fp` | 63220 | 63270 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.precision` | 4.361952e-01 | 4.360007e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.false_positive_bytes` | 63387 | 63437 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.predicted_bytes` | 112360 | 112410 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.redaction_load` | 5.218500e-02 | 5.220823e-02 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f1` | 2.998709e-01 | 2.997635e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f2` | 3.502982e-01 | 3.502395e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.fp` | 6555 | 6560 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.precision` | 2.418459e-01 | 2.417062e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.latency.p50_ms` | 9.793 | 11.922 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 64.429 | 73.77 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.product_coverage.typed_entities.f1` | 2.076774e-01 | 2.076314e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.f2` | 2.390109e-01 | 2.389579e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fn` | 12467 | 12468 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fp` | 21961 | 21962 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.precision` | 1.704378e-01 | 1.704000e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.recall` | 2.657400e-01 | 2.656811e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.tp` | 4512 | 4511 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |

## presidio-research: presidio-en-de

Prediction SHA-256: `3dc806b9b8397aadc9e6d76a6f960e90451bfa2fc7bf95607b69125d2bfbd8a4`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 5.137 | 5.614 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 10.733 | 11.684 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: presidio-en-de

Prediction SHA-256: `bf3e039c78e4232ea260c6a642c16124448654ec077f6d577e9effeac4860ce5`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.f1` | 0.1569 | 0.1568 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.precision` | 0.131 | 0.1309 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.recall` | 0.1955 | 0.1954 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f1` | 5.002966e-01 | 5.001687e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f2` | 5.486753e-01 | 5.486137e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f5` | 5.788136e-01 | 5.788005e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.fp` | 63220 | 63270 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.precision` | 4.361952e-01 | 4.360007e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.false_positive_bytes` | 63387 | 63437 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.predicted_bytes` | 112360 | 112410 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.redaction_load` | 5.218500e-02 | 5.220823e-02 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f1` | 2.998709e-01 | 2.997635e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f2` | 3.502982e-01 | 3.502395e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.fp` | 6555 | 6560 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.precision` | 2.418459e-01 | 2.417062e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.latency.p50_ms` | 9.833 | 12.883 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 64.836 | 76.992 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.product_coverage.typed_entities.f1` | 2.076774e-01 | 2.076314e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.f2` | 2.390109e-01 | 2.389579e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fn` | 12467 | 12468 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fp` | 21961 | 21962 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.precision` | 1.704378e-01 | 1.704000e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.recall` | 2.657400e-01 | 2.656811e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.tp` | 4512 | 4511 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |

## presidio-research: presidio-all

Prediction SHA-256: `3dc806b9b8397aadc9e6d76a6f960e90451bfa2fc7bf95607b69125d2bfbd8a4`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 5.173 | 5.422 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 10.765 | 11.141 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: presidio-all

Prediction SHA-256: `bf3e039c78e4232ea260c6a642c16124448654ec077f6d577e9effeac4860ce5`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.f1` | 0.1569 | 0.1568 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.precision` | 0.131 | 0.1309 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.recall` | 0.1955 | 0.1954 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f1` | 5.002966e-01 | 5.001687e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f2` | 5.486753e-01 | 5.486137e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f5` | 5.788136e-01 | 5.788005e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.fp` | 63220 | 63270 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.precision` | 4.361952e-01 | 4.360007e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.false_positive_bytes` | 63387 | 63437 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.predicted_bytes` | 112360 | 112410 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.redaction_load` | 5.218500e-02 | 5.220823e-02 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f1` | 2.998709e-01 | 2.997635e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f2` | 3.502982e-01 | 3.502395e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.fp` | 6555 | 6560 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.precision` | 2.418459e-01 | 2.417062e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.latency.p50_ms` | 9.824 | 10.33 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 64.904 | 66.174 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.product_coverage.typed_entities.f1` | 2.076774e-01 | 2.076314e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.f2` | 2.390109e-01 | 2.389579e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fn` | 12467 | 12468 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fp` | 21961 | 21962 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.precision` | 1.704378e-01 | 1.704000e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.recall` | 2.657400e-01 | 2.656811e-01 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.tp` | 4512 | 4511 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |

## presidio-research: presidio-strong

Prediction SHA-256: `e175c98b4f1280f682c8c47e738ef5a60ae6ccb2521bfe8726d249dc9dfdae6f`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `1eaff62e7a45dfe3665e2a337ec1764d3496b145`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.0 | 0.1 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 21.199 | 42.459 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 38.686 | 82.95 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: presidio-strong

Prediction SHA-256: `a5100245f875b9a6586d459330337fe6ba12632aef34ef108407fc2ae43e4dfa`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `1eaff62e7a45dfe3665e2a337ec1764d3496b145`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.common_intersection.char_level.f1` | 6.995500e-01 | 6.990674e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.char_level.f2` | 6.413343e-01 | 6.411720e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.char_level.f5` | 6.138285e-01 | 6.137999e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.char_level.fp` | 10805 | 10905 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.char_level.precision` | 8.242489e-01 | 8.229104e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.false_positive_bytes` | 10864 | 10964 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.predicted_bytes` | 61652 | 61752 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.redaction_load` | 2.863394e-02 | 2.868039e-02 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.typed_entities.f1` | 4.604330e-01 | 4.599923e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.typed_entities.f2` | 4.561850e-01 | 4.560118e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.typed_entities.fp` | 2735 | 2745 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.typed_entities.precision` | 4.676917e-01 | 4.667832e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.latency.p50_ms` | 30.079 | 35.707 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 318.774 | 359.326 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: presidio-strong-high-recall

Prediction SHA-256: `f315dfad1d5c4b678793ef4aa34fd336852eaeabefc6d14bc82391d7d2ba9503`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `1eaff62e7a45dfe3665e2a337ec1764d3496b145`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 20.861 | 24.624 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 36.135 | 49.621 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: presidio-strong-high-recall

Prediction SHA-256: `5e4bf983c4dc203968a0f34030d1dd6b5018ba89578ff6fc10d7f551387dd003`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `1eaff62e7a45dfe3665e2a337ec1764d3496b145`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.precision` | 0.2402 | 0.2401 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `own_metric.recall` | 0.2438 | 0.2437 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.char_level.f1` | 6.997703e-01 | 6.994808e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.char_level.f2` | 6.417689e-01 | 6.416715e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.char_level.f5` | 6.143499e-01 | 6.143327e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.char_level.fp` | 10843 | 10903 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.char_level.precision` | 8.238686e-01 | 8.230664e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.false_positive_bytes` | 10902 | 10962 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.predicted_bytes` | 61735 | 61795 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.redaction_load` | 2.867249e-02 | 2.870036e-02 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.typed_entities.f1` | 4.605968e-01 | 4.603326e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.typed_entities.f2` | 4.568220e-01 | 4.567180e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.typed_entities.fp` | 2748 | 2754 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.common_intersection.typed_entities.precision` | 4.670287e-01 | 4.664859e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.latency.p50_ms` | 29.875 | 43.072 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 317.759 | 382.545 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.product_coverage.typed_entities.f1` | 2.939484e-01 | 2.938961e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.product_coverage.typed_entities.f2` | 3.151107e-01 | 3.150547e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.product_coverage.typed_entities.fn` | 11359 | 11360 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.product_coverage.typed_entities.fp` | 15639 | 15640 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.product_coverage.typed_entities.precision` | 2.643586e-01 | 2.643116e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.product_coverage.typed_entities.recall` | 3.309971e-01 | 3.309382e-01 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |
| `row.product_coverage.typed_entities.tp` | 5620 | 5619 | Fresh fixed-seed Presidio capture. Equal-score type ties are order-sensitive; the historical seed and predictions were not retained, so exact historical choices cannot be recovered. |

## presidio-research: datafog-gliner

Prediction SHA-256: `90b25861a0bef99b855327cd0bf1613b4eb780861d0d0105fc33406fa3cc3f35`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `5a25074ab4256da4be39ed81ff169a69646c4d5a`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 62.62 | 69.603 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 82.448 | 170.238 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: datafog-gliner

Prediction SHA-256: `8b0859c8df67eedb78d0d1783557535e3aa457c1d2e9b258499a32203e66f879`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `5a25074ab4256da4be39ed81ff169a69646c4d5a`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 74.895 | 101.176 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 343.161 | 507.595 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: gliner

Prediction SHA-256: `62cc4e3652b11df5f4e1c8a3a9412a12146d2d1deee0073788e0585fdd44e2d2`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `5a25074ab4256da4be39ed81ff169a69646c4d5a`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 119.853 | 132.947 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 268.137 | 220.431 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: gliner

Prediction SHA-256: `94155e4f35d915f59730db434d75d0bd0ce804dc1c77a88e0affaf4b67633297`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `5a25074ab4256da4be39ed81ff169a69646c4d5a`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 132.258 | 187.91 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 447.331 | 723.676 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: gliner-high-recall

Prediction SHA-256: `f36f41f9b5d28e06176a2292ac874a90693b430aa22b1fe5258bc0e1473d2159`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `5a25074ab4256da4be39ed81ff169a69646c4d5a`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.2 | 0.0 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 132.081 | 134.226 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 267.622 | 229.048 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: gliner-high-recall

Prediction SHA-256: `b870a486d224f9226111db58f355bf115ca034e67ff0c3f6e42437d76344061d`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `5a25074ab4256da4be39ed81ff169a69646c4d5a`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 131.793 | 186.631 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 447.995 | 698.038 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: pii-tracer

Prediction SHA-256: `4c3fdcd3ebde02b5021a80dc64fca55136b03ed573c7621209f2f3ff1c5113ba`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `870acad7a7a733cd2f7203a2358080a5aae85f1d`. Previous capture commit: `7b09e385229fe21b15ef2d9be0400569c43bb1e4`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 199.362 | 204.245 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 678.434 | 672.942 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## pii-trace: pii-tracer

Prediction SHA-256: `c5198c15a78a83a009956d59e102e0e78ff93e4c8774b34821a385e400c802c1`. Dataset SHA-256: `1f37039ca2a9c7a93d7b755e1603d5192424c61e61fcbf2ce9405f26705fd670`.

Capture commit: `870acad7a7a733cd2f7203a2358080a5aae85f1d`. Previous capture commit: `abcfeafc5f62151fc827bb0ce1b84185b718b31a`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 374.949 | 318.063 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 1205.339 | 857.613 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: pii-tracer

Prediction SHA-256: `a5ac13185052ab44aa42fdf0150431852bc5d32fc655c3fef7870ed0b79cc02b`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `870acad7a7a733cd2f7203a2358080a5aae85f1d`. Previous capture commit: `bfd35ce60e23fc7adcdc51aa8623e97901d9169d`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 456.913 | 848.938 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 6439.749 | 11526.411 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## presidio-research: opf

Prediction SHA-256: `bce7fd135b5859b45b59141ebcde4a0c32c2c566a8d2b482bde93b6ef6518c81`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `ed8af33b0652e0e5302d9a1715d9a917c5cc1064`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 183.7 | 390.459 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 550.059 | 1188.997 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |

## piibench-commercial: opf

Prediction SHA-256: `25101e73c73033399d85a8c240c6d4a75bcb47954130eeb1e0a536da02043ee2`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `ed8af33b0652e0e5302d9a1715d9a917c5cc1064`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 396.359 | 628.181 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 3268.224 | 5182.321 | Fresh capture of the pinned configuration. Per-document timing varies with machine load; no speed comparison is claimed. |
