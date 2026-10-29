# Caller context admission evidence

The [count-only receipt](caller-context-floor-v1.json) covers 54,327 benchmark
contexts. Main's real `TypedContext::load` reports zero `UnsafeShortMatch`
values in every arm. A separate known-short positive control reports exactly one.
These are admission counts, not detection scores or performance measurements.

The enumeration includes both scored-label contracts, oracle layers C/A/D/R/K
with shipping and probe match kinds, direct kind-cell contexts, current and pinned
dictionary generators on both partitions, and the perf context cases. Dictionary
contexts contain terms rather than record values. Perf contexts are empty objects.
The oracle generators filter unsafe values before constructing contexts.
The receipt pins the generated stream, generator inputs, dataset, loader binary,
and main parser source by SHA-256.

To reproduce, use a prepared review worktree at the receipt's loader commit.
Copy `crates/gaze/examples/caller_context_probe.rs` into that worktree and build
the example there with `--no-default-features`. The probe calls `TypedContext::load`
for each emitted context and prints counts only. From the candidate checkout:

```sh
uv run --project scripts/bench --python 3.13 python scripts/verification/enumerate_caller_contexts.py \
  --dataset "$DATASET" --output-dir target/caller-context-floor
"$BASE_WORKTREE/target/debug/examples/caller_context_probe" \
  < target/caller-context-floor/contexts.jsonl > target/caller-context-floor/result.json
```

The enumeration writes private JSONL inputs under `target/`; keep these out of
public reports. No benchmark capture or model execution is needed. Because no
benchmark context reaches the new refusal branch, benchmark outputs are unchanged
by construction. This does not measure recall or precision on short caller values.
