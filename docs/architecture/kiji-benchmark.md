# Kiji Benchmark

The tracked benchmark snapshot lives at
`crates/gaze-recognizers/benches/kiji_direct_vs_observer_snapshot.json`.
`cargo bench -p gaze-recognizers --features safety-net-kiji --bench kiji_direct_vs_observer`
validates that the snapshot pins match the runtime Kiji constants and prints
the JSON for CI logs.

Current status: `not_run_requires_local_kiji_command`.

| Pin | Value |
|---|---|
| Source repo | `onnx-community/distilbert-NER-ONNX` |
| Source commit | `3a19fe9404a4469d91aa3d551558a97f68872f67` |
| Bundle SHA256 | `c129e135d86698e67c4836456212666f94a56ceaf995acd60532f557b3120d2f` |
| Model SHA256 | `b5f77096d0d9f425d34a2e263f8a2dfb845cdc757dc00c7a1e69e9cbb93115d5` |
| Tokenizer SHA256 | `cb26b43c98e8266ae3e99c2a583cf8315d73b33a17e6b20b4df7ff1f22392d34` |
| Label-map SHA256 | `d3753ce580a9d43b113d779c712494bd61341285317beec49cc1e848b86f9a97` |

The result fields for direct-detector precision, recall, F1, per-class
breakdown, and observer-residual recall are present in the snapshot but remain
`null` until a pinned local Kiji command and model directory are available.
Publishing numeric Kiji claims without those pins would violate the Axis 4
trust contract.

The methodology and rule-floor baseline are documented in
[`docs/research/v0.8-kiji-benchmark.md`](../research/v0.8-kiji-benchmark.md).
