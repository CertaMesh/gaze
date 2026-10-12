# Tier 4 pipeline gating

Tier 4 gates are pipeline-level performance controls. They do not weaken the
recognizer floor, do not change token shapes, and are default-off through
`PipelineOptimizationConfig`.

## Gates

- `skip_class_gating`: skips Pass-3 SafetyNet only when `SafetyNetMode` is
  observer-only (`Strict` or `Tolerant`) and the rule floor has already emitted
  token spans with no residual gold-shape signals. It never applies to
  `Resolve` or `Redact`.
- `capitals_heuristic_gate`: skips observer-only Pass-3 for numeric-heavy inputs
  and inputs without a capital letter at a non-sentence-start position. The
  heuristic is valid only for configured English/German capital-case locales;
  unsupported locales fail closed with `UnsupportedCapitalHeuristicLocale`.
- `prefix_cache`: compatibility flag only. The pipeline always rescans each
  complete input, including repeated or extended text in live and transactional
  calls. It stores no raw prefixes and emits the current recognizer/rule audit
  rows, never synthetic `prefix_cache` rows. `enable_prefix_cache()` and
  `with_prefix_cache(true)` remain accepted but provide no scan shortcut.
- `length_bucketing`: reserves an opt-in config flag for batching callers that
  group same-length model inputs to reduce padding waste. The current core path
  does not batch Pass-3 calls, so this flag is a compatibility hook.

## Invariants

Observer gates never suppress `Resolve` or `Redact`. Prefix reuse is disabled
under both `PrefixCacheWriteMode::Allow` and `Suppress`: every call scans all
bytes and stores no raw prefix. This trades opted-in throughput for correctness.

Configuration identity cannot certify stateful custom detectors. An appended
suffix can complete `alice@` into an email, so no cached decision crosses
fields, pipelines, locales, dictionaries, or calls. Token reuse, manifest
offsets, transaction commit/drop, and logger errors retain full-scan behavior.

## Bench snapshot

Command:

```bash
cargo bench -p gaze-pii --bench tier4_pipeline_gating --all-features
```

Local result on May 15, 2026:

| config | SafetyNet calls | elapsed ms | call reduction |
| --- | ---: | ---: | ---: |
| baseline | 300 | 41.713 | 0.0% |
| skip_class_gating | 200 | 28.221 | 33.3% |
| capitals_heuristic_gate | 100 | 15.061 | 66.7% |
| combined_skip_and_capitals | 0 | 1.904 | 100.0% |

Historical prefix cache keystroke-style bench (unsafe reuse, now disabled):

| config | detector bytes processed | elapsed ms | reduction |
| --- | ---: | ---: | ---: |
| baseline | 2190 | 28.169 | - |
| prefix_cache | 1035 | 13.848 | 52.7% bytes, 50.8% latency |

The historical prefix savings above do not apply to the current runtime. The
current benchmark requires equal detector bytes with the flag on and off.

The timed corpus is benign and has zero SafetyNet suspects. Before timing,
the bench separately requires a deterministic positive: a synthetic residual
must be protected under `Redact` and `Resolve`, with the same suspect count and
protection outcome in baseline and gated configurations. `Resolve` must restore
exactly. Benign observer fixtures separately prove the intended skips retain
protection, restoration and report semantics. A separate capitalized observer
positive must retain one suspect and the strict caller-boundary refusal signal
in every configuration.

These proofs also run as `tier4_gating_preserves_required_positive` and
`tier4_observer_skips_preserve_benign_semantics` in the `safety_net` integration
test binary. CI executes that binary in the runtime workspace shard
([roster](../../../scripts/ci/test-shards.json),
[workflow](../../../.github/workflows/test.yml)). The timing target has
`harness = false`, so ordinary workspace tests rely on these shared proofs.
