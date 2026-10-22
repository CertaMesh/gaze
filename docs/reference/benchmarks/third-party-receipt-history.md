# Third-party receipt refresh

These rows originally had no prediction digest or scorer receipt. Each refreshed row passes `check_own_result` before its receipt marker is removed. The dataset, model and configuration remain pinned. Raw text and predictions stay outside the repository.

The tables keep every replaced numeric value from the published measurement. New per-label byte telemetry has no historical value and appears only in the aggregate. Unlisted metrics retain their published values. Timing is recorded for provenance and does not establish a speed gain.

Published aggregate before this campaign: `e7e018ce8c4fa590a5c28db7031f030712a83103`.

Refreshed rows: 16 of 31.

## Presidio overlap order

This campaign records PYTHONHASHSEED=0. Presidio 2.2.364 deduplicates candidates through a Python set, then resolves equal-score overlaps in that order. Changing only the hash seed to 1 changed types in 31 PIIBench documents, with identical span coordinates. The pinned analyzer confirmed that every changed pair had equal scores.

The [aggregate order diagnostic](presidio-order-diagnostic.json) pins both prediction streams and the dataset. It demonstrates order sensitivity in retained captures. The original seed and predictions were not recorded, so its exact historical type choices cannot be recovered. Product-coverage leaked and false-positive byte totals reproduce for the three plain Presidio configurations. Typed and common-intersection metrics can differ because a tie assigns different types to the same protected bytes.

## presidio-research: datafog-core

Prediction SHA-256: `72a0589a7a5ec77131919fc33d79e6caf993ffffd9830718f37da640c3d70a92`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.003 | 0.002 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.009 | 0.007 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## piibench-commercial: datafog-core

Prediction SHA-256: `f525d8faa382540e4536ea3804edc07600cfda60426d2be71950ca5a5cc0a208`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p95_ms` | 0.038 | 0.036 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## presidio-research: datafog-regex

Prediction SHA-256: `79e7c050b6b8dafe5557adc355fc8a88c99b2e3abfd016dab4b11b266c66825e`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.047 | 0.04 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.087 | 0.075 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## piibench-commercial: datafog-regex

Prediction SHA-256: `36c1bf9a89b81740846b854a1a6cc9dbd78e2c7d12827a3256535daa11ef573d`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.076 | 0.07 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.441 | 0.399 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## presidio-research: scrubadub-base

Prediction SHA-256: `fbbdbccf4660e95c1dab43b7e1c6ba73a1610d965678355dfc8fde4ee319c8f5`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.035 | 0.033 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.129 | 0.122 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## piibench-commercial: scrubadub-base

Prediction SHA-256: `e6052bf8b83049f1e6e03e117dee6366b6f2cb094614ca07284545a17bbe7bbb`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `e7e018ce8c4fa590a5c28db7031f030712a83103`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 0.093 | 0.09 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 0.631 | 0.597 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## presidio-research: datafog-spacy

Prediction SHA-256: `3b19ca181f46d6aa904c7132f616e81569d8a254ef63de94d00e8c1df863fad3`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 4.706 | 5.647 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 9.843 | 11.451 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## piibench-commercial: datafog-spacy

Prediction SHA-256: `b4d6276d6a0df03925d11e063049068f5125ecc4992fc8bf211da4a7aac1937a`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 8.949 | 10.488 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 60.048 | 65.978 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## presidio-research: scrubadub-spacy

Prediction SHA-256: `7ac40c1d76e83a20d228b3e8a910e818e883765ea77a90bb00c27c9edb879386`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 4.686 | 6.573 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 9.834 | 14.57 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## piibench-commercial: scrubadub-spacy

Prediction SHA-256: `4f70b71154115f99ee0210c6eaefdec99bffac5aa0dfb3361f4f7565f077d63c`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `row.latency.p50_ms` | 8.999 | 11.101 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 60.37 | 70.967 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## presidio-research: presidio-en

Prediction SHA-256: `3dc806b9b8397aadc9e6d76a6f960e90451bfa2fc7bf95607b69125d2bfbd8a4`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 5.213 | 6.802 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 10.971 | 14.601 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## piibench-commercial: presidio-en

Prediction SHA-256: `bf3e039c78e4232ea260c6a642c16124448654ec077f6d577e9effeac4860ce5`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.f1` | 0.1569 | 0.1568 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.precision` | 0.131 | 0.1309 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.recall` | 0.1955 | 0.1954 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f1` | 0.500296632707336 | 0.500168730634325 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f2` | 0.5486752864472908 | 0.5486137438897725 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f5` | 0.578813648810044 | 0.5788004766269812 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.fp` | 63220 | 63270 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.precision` | 0.43619516458428087 | 0.436000748789902 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.false_positive_bytes` | 63387 | 63437 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.predicted_bytes` | 112360 | 112410 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.redaction_load` | 0.05218500317447932 | 0.05220822540800303 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f1` | 0.2998709307328266 | 0.299763457816644 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f2` | 0.35029819741338875 | 0.3502395229640548 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.fp` | 6555 | 6560 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.precision` | 0.24184594031922277 | 0.24170616113744076 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.latency.p50_ms` | 9.793 | 11.922 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 64.429 | 73.77 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.product_coverage.typed_entities.f1` | 0.20767743717205192 | 0.20763140937126023 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.f2` | 0.23901090169405337 | 0.23895792941974173 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fn` | 12467 | 12468 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fp` | 21961 | 21962 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.precision` | 0.17043780455558494 | 0.1704000302194689 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.recall` | 0.2657400318039932 | 0.26568113552034867 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.tp` | 4512 | 4511 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |

## presidio-research: presidio-en-de

Prediction SHA-256: `3dc806b9b8397aadc9e6d76a6f960e90451bfa2fc7bf95607b69125d2bfbd8a4`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 5.137 | 5.614 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 10.733 | 11.684 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## piibench-commercial: presidio-en-de

Prediction SHA-256: `bf3e039c78e4232ea260c6a642c16124448654ec077f6d577e9effeac4860ce5`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.f1` | 0.1569 | 0.1568 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.precision` | 0.131 | 0.1309 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.recall` | 0.1955 | 0.1954 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f1` | 0.500296632707336 | 0.500168730634325 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f2` | 0.5486752864472908 | 0.5486137438897725 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f5` | 0.578813648810044 | 0.5788004766269812 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.fp` | 63220 | 63270 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.precision` | 0.43619516458428087 | 0.436000748789902 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.false_positive_bytes` | 63387 | 63437 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.predicted_bytes` | 112360 | 112410 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.redaction_load` | 0.05218500317447932 | 0.05220822540800303 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f1` | 0.2998709307328266 | 0.299763457816644 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f2` | 0.35029819741338875 | 0.3502395229640548 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.fp` | 6555 | 6560 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.precision` | 0.24184594031922277 | 0.24170616113744076 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.latency.p50_ms` | 9.833 | 12.883 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 64.836 | 76.992 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.product_coverage.typed_entities.f1` | 0.20767743717205192 | 0.20763140937126023 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.f2` | 0.23901090169405337 | 0.23895792941974173 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fn` | 12467 | 12468 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fp` | 21961 | 21962 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.precision` | 0.17043780455558494 | 0.1704000302194689 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.recall` | 0.2657400318039932 | 0.26568113552034867 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.tp` | 4512 | 4511 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |

## presidio-research: presidio-all

Prediction SHA-256: `3dc806b9b8397aadc9e6d76a6f960e90451bfa2fc7bf95607b69125d2bfbd8a4`. Dataset SHA-256: `ec08a771ba8135314cafb60752b2295212222ba3a4cd75d73811839c699e0012`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.predict_seconds` | 0.1 | 0.0 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p50_ms` | 5.173 | 5.422 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 10.765 | 11.141 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |

## piibench-commercial: presidio-all

Prediction SHA-256: `bf3e039c78e4232ea260c6a642c16124448654ec077f6d577e9effeac4860ce5`. Dataset SHA-256: `631f5f87946a8220d8f1b4ea24e9758e3aee3cd3fd98fbac1bb582112011eaad`.

Capture commit: `9225644b9fb58b20a4b84aedd558e5d3cdcb78fd`. Previous capture commit: `a8293dc0bc3de251063848cfd3f7596e620ecb8b`.

| Metric | Published (superseded) | Regenerated | Why |
| --- | ---: | ---: | --- |
| `own_metric.f1` | 0.1569 | 0.1568 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.precision` | 0.131 | 0.1309 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `own_metric.recall` | 0.1955 | 0.1954 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f1` | 0.500296632707336 | 0.500168730634325 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f2` | 0.5486752864472908 | 0.5486137438897725 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.f5` | 0.578813648810044 | 0.5788004766269812 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.fp` | 63220 | 63270 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.char_level.precision` | 0.43619516458428087 | 0.436000748789902 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.false_positive_bytes` | 63387 | 63437 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.predicted_bytes` | 112360 | 112410 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.redaction_load` | 0.05218500317447932 | 0.05220822540800303 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f1` | 0.2998709307328266 | 0.299763457816644 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.f2` | 0.35029819741338875 | 0.3502395229640548 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.fp` | 6555 | 6560 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.common_intersection.typed_entities.precision` | 0.24184594031922277 | 0.24170616113744076 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.latency.p50_ms` | 9.824 | 10.33 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.latency.p95_ms` | 64.904 | 66.174 | Fresh capture of the pinned configuration. Timing varies with runtime startup and machine load; no speed comparison is claimed. |
| `row.product_coverage.typed_entities.f1` | 0.20767743717205192 | 0.20763140937126023 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.f2` | 0.23901090169405337 | 0.23895792941974173 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fn` | 12467 | 12468 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.fp` | 21961 | 21962 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.precision` | 0.17043780455558494 | 0.1704000302194689 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.recall` | 0.2657400318039932 | 0.26568113552034867 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
| `row.product_coverage.typed_entities.tp` | 4512 | 4511 | Equal-score type ties are order-sensitive. This capture fixes hash seed 0; the historical seed and exact type choices were not retained. |
