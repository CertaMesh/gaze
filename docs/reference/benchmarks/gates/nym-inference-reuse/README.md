# Nym inference reuse proof

Nym retains successful window probabilities for one synchronous protection request.
The key includes the immutable model instance, every threshold, the safety view,
exact window text bytes, and token IDs. The input tensors, window plan, model,
threshold decisions, and span decoding keep their existing behavior.

The cache has an 8 MiB budget for keys and probability rows. A full cache causes
fresh inference. Each nested pipeline has an isolated scope. Return and unwind
release the request's cache. Another thread starts with an empty cache.

## Bounded equivalence

The committed driver selects 24 Nym-heavy documents from each canonical layer:
C, A, D, and R. It fixes session prefixes and compares clean bytes, manifest spans,
audit rows, candidate events, protection traces, safety-net statistics, and restore
outcomes. Timing fields are excluded. Each binary runs once per selected document.
Both scored-label contracts replay those observations. Generated layers use their
committed agentic contract, as in the full benchmark runner.

The raw-window verifier compares every probability bit on a cache hit against a
fresh ORT call, then compares decoded raw findings. Inputs include the selected
raw documents, reconstructed stable and neutral output views, and the synthetic
short, PII, history, 2k-token, and 10k-token driver texts.

Run the proof with the pinned local holdout and both release producers:

```sh
uv run --project scripts/bench --python 3.13 python scripts/bench/inference_reuse_proof.py equivalence \
  --base ../inference-reuse-base/target/release/examples/clean_for_bench \
  --candidate target/release/examples/clean_for_bench \
  --policy scripts/bench/fixtures/inference_reuse.toml \
  --model-root "$HOME/.local/share/gaze/models" \
  --dataset target/bench-data/dataiku-en-de/test.parquet \
  --verifier target/release/examples/verify_nym_reuse --output target/inference-reuse-proof
```

Run it again with `scripts/bench/fixtures/policy_equivalence.toml` and omit
`--verifier` to compare the model-free policy. Build the verifier with
`cargo build --release -p gaze-recognizers --features safety-net-nym,test-support --example verify_nym_reuse`.

## Key mutation probes

Commit the implementation, then run:

```sh
uv run --project scripts/bench --python 3.13 python scripts/bench/mutate_inference_reuse_keys.py \
  --output target/inference-reuse-mutations
```

The probes remove each key input separately. Each removal must cause the existing
isolation test to fail at runtime. The script restores the original source after
every probe, including errors.

## Limits

This bounded proof does not certify main. The full-corpus gate remains required
before merge. The diagnostic timing driver alternates binary order, records load,
and requires at least five runs per side. It holds no `bench-machine` lease and
supports no publishable speedup claim.
