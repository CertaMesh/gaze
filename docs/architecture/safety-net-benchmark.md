# Safety-Net Benchmark

The tracked benchmark snapshot lives at
`crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json`.
`cargo bench -p gaze-recognizers --features safety-net-kiji,safety-net-openai --bench safety_net_matrix`
validates that the snapshot pins match runtime constants and prints the JSON
for CI logs.

Current status: `not_run_requires_local_backends`.

## Backend Pins

### Kiji DistilBERT

| Pin | Value |
|---|---|
| Source repo | `onnx-community/distilbert-NER-ONNX` |
| Source commit | `3a19fe9404a4469d91aa3d551558a97f68872f67` |
| Bundle SHA256 | `c129e135d86698e67c4836456212666f94a56ceaf995acd60532f557b3120d2f` |
| Model SHA256 | `b5f77096d0d9f425d34a2e263f8a2dfb845cdc757dc00c7a1e69e9cbb93115d5` |
| Tokenizer SHA256 | `cb26b43c98e8266ae3e99c2a583cf8315d73b33a17e6b20b4df7ff1f22392d34` |
| Label-map SHA256 | `d3753ce580a9d43b113d779c712494bd61341285317beec49cc1e848b86f9a97` |

## OpenAI Privacy Filter Pins

| Pin | Value |
|---|---|
| Source repo | `openai/privacy-filter` |
| Source commit | `f7f00ca7fb869683eb732c010299d901457f19c3` |
| Checkpoint bundle SHA256 | `4680158333621f3f344f58366f59612d52eff67ce6f46cff7becede5be1853ae` |
| Required checkpoint artifacts | `["config.json", "dtypes.json", "model.safetensors", "viterbi_calibration.json"]` |

OPF publishes a source repository and an `opf` Python CLI that downloads its
checkpoint into `~/.opf/privacy_filter` by default, or into the directory
selected by `OPF_CHECKPOINT` / `--checkpoint`. It does not publish a GitHub
release binary, so Gaze does not pin a binary checksum. The source commit and
checkpoint bundle are the trust anchors.

The source commit is pinned above. The checkpoint bundle hash was captured from
a clean local `opf download` on 2026-05-15. The bundle hash is SHA256 over the
Kiji-style line-per-file SHA256SUMS manifest for the required artifact list in
declaration order. Publishing numeric OPF claims without populated benchmark
cells would violate the Axis 4 trust contract.

## Matrix Shape

The snapshot schema is version 2 and is keyed by backend, locale, and mode.
Initial cells cover:

| Dimension | Values |
|---|---|
| Backends | `kiji_distilbert`, `openai_privacy_filter` |
| Locales | `Global`, `EnUs`, `DeDe` |
| Modes | `direct_detector`, `observer_residual` |

That produces 12 cells. Each `direct_detector` cell carries nullable
precision, recall, F1, and per-class metrics. Each `observer_residual` cell
also carries nullable `observer_residual_recall`,
`agreement_with_rule_floor`, `expansion_fraction`,
`contradiction_fraction`, and `novel_tp_over_rule_floor`.

The top-level `strict_span_leak_rate` block is mode-independent and records
one nullable headline field for each backend-locale pair. It measures
end-to-end fail-closed behavior rather than detector precision/recall.

The result fields remain `null` until pinned local backend commands and model
directories are available. Publishing numeric Kiji or OPF claims without those
pins would violate the Axis 4 trust contract.

The methodology and rule-floor baseline are documented in
[`docs/research/v0.8-kiji-benchmark.md`](../research/v0.8-kiji-benchmark.md);
the v0.9 snapshot extends that two-mode methodology into the
backend × locale × mode matrix used for routing decisions.
