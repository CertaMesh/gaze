# Third-party receipt refresh

These rows originally had no prediction digest or scorer receipt. Each refreshed row passes `check_own_result` before its receipt marker is removed. The dataset, model and configuration remain pinned. Raw text and predictions stay outside the repository.

The tables keep every replaced numeric value from the published measurement. New per-label byte telemetry has no historical value and appears only in the aggregate. Unlisted metrics retain their published values. Timing is recorded for provenance and does not establish a speed gain.

Published aggregate before this campaign: `e7e018ce8c4fa590a5c28db7031f030712a83103`.

Refreshed rows: 6 of 31.

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
