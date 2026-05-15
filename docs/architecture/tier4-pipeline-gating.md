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
  and inputs without a capital letter at a non-sentence-start position. This is
  valid only for configured English/German capital-case locales; unsupported
  locales fail closed with `UnsupportedCapitalHeuristicLocale`.
- `prefix_cache`: stores recently tokenized raw prefixes inside the owning
  `Session`. Cache state is not exported and is not shared across sessions.
  Every cached token emission writes an audit row with
  `provenance_stage = "prefix_cache"`.
- `length_bucketing`: reserves an opt-in config flag for batching callers that
  group same-length model inputs to reduce padding waste. The current core path
  does not batch Pass-3 calls, so this flag is a compatibility hook.

## Invariants

- Existing adopters see no behavior change unless a flag is explicitly enabled.
- Gates only reduce observer-only Pass-3 calls. They never suppress a
  resolve/redact SafetyNet pass.
- Prefix cache entries are session-scoped and dropped with the session.
- Cache hits are auditable and metadata-only; audit rows never include source
  bytes or token strings.

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

Prefix cache keystroke-style bench:

| config | detector bytes processed | elapsed ms | reduction |
| --- | ---: | ---: | ---: |
| baseline | 2190 | 28.169 | - |
| prefix_cache | 1035 | 13.848 | 52.7% bytes, 50.8% latency |

The bench asserts zero SafetyNet suspects for every config, preserving the
observer-mode recall baseline for the synthetic fixture set.
