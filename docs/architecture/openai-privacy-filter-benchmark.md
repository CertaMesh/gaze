# OpenAI Privacy Filter Benchmark

The tracked benchmark snapshot lives at
`crates/gaze-recognizers/benches/openai_privacy_filter_direct_vs_observer_snapshot.json`.
`cargo bench -p gaze-recognizers --features safety-net-openai --bench openai_privacy_filter_direct_vs_observer`
validates that the snapshot pins match the runtime OPF constants and prints
the JSON for CI logs.

Current status: `not_run_requires_local_opf_binary`.

| Pin | Value |
|---|---|
| Source repo | `openai/privacy-filter` |
| Source commit | `f7f00ca7fb869683eb732c010299d901457f19c3` |
| Checkpoint bundle SHA256 | `null` |

OPF currently publishes a source repository and an `opf` Python CLI that
downloads its checkpoint into `~/.opf/privacy_filter` by default, or into the
directory selected by `OPF_CHECKPOINT` / `--checkpoint`. It does not publish a
GitHub release binary, so the source commit and checkpoint bundle are the trust
anchors. The source commit is pinned above; the checkpoint bundle hash remains
`null` until a clean local download is captured, reviewed, and recorded in
`OPF_CHECKPOINT_BUNDLE_SHA256` with the matching `REQUIRED_OPF_ARTIFACTS` list.

The result fields for direct-detector precision, recall, F1, per-class
breakdown, and observer-residual recall are present in the snapshot but remain
`null` until a pinned local OPF command and checkpoint directory are available.
Publishing numeric OPF claims without those pins would violate the Axis 4 trust
contract.

This snapshot lands at `schema_version = 1`; the v0.9 #33c benchmark-matrix
work will fold OPF into the backend/mode/locale matrix and bump the schema.
