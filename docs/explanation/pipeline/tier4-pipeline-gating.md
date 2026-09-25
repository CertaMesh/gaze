# Tier 4 Pipeline Gating

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

- Prefix reuse is disabled even when explicitly enabled; the other flags retain
  their existing behavior.
- Gates only reduce observer-only Pass-3 calls. They never suppress a
  resolve/redact SafetyNet pass.
- No cached decision is trusted across fields, pipelines, locales, dictionaries,
  or calls. Immutable configuration identity cannot certify stateful custom
  recognizers/rules, and an appended suffix can complete an entity across a
  cached boundary (for example, `alice@` followed by `example.invalid`).
- Both `PrefixCacheWriteMode::Allow` and `Suppress` perform full scans without
  prefix storage. Token mappings, manifest offsets, transaction commit/drop and
  logger error propagation retain the normal full-scan behavior.
- Disabling prefix reuse intentionally trades opted-in prefix-cache throughput
  for detection correctness. Repeated growing inputs scan all bytes each time,
  as with the default configuration; token mappings remain reusable and
  restorable.

## Bench Snapshot

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

The bench asserts zero SafetyNet suspects for every config, preserving the
observer-mode recall baseline for the synthetic fixture set.
