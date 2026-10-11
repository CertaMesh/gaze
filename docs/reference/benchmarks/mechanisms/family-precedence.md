# Effective family precedence arm

This opt-in arm exercises adopter-supplied collision metadata. A variant's
effective precedence is the minimum declared by its members. Comparing two
recognizers and deciding whether they tie must use that same minimum.

The arm adds 60 layer A documents and 80 layer D documents to each partition.
It covers a false raw tie, a missed effective tie, and no-tie, same-variant and
absent-membership controls on key-value and JSON surfaces. The calibration
recognizer compiles but does not match these documents. It lowers its variant's
precedence without adding another candidate. All references are invented.

The default corpus, generated layers and published headline stay unchanged.
The wrapper delegates transport, scoring, observation recording and gain gates
to the canonical full runner. Apply the committed delta to each runtime's own
setup policy before measurement:

```sh
cargo run -p gaze-cli -- setup --non-interactive --force --policy-out target/gate/setup.toml
uv run --project scripts/bench python scripts/bench/family_precedence_arm.py policy \
  --input target/gate/setup.toml --output target/gate/policy.toml
uv run --project scripts/bench python scripts/bench/family_precedence_arm.py full \
  --seed 20260710 --no-download --release --policy target/gate/policy.toml \
  --scored-labels docs/reference/benchmarks/scored-labels-v2.json \
  --output-dir target/bench-data/effective-family-v2
```

Repeat for each gated scored-label contract. An unextended policy or disabled
generated layers is rejected, so a blind run cannot become evidence.

## Tagged release captures

The current harness measured the added cells on each displayed release's own
locked detection code and setup policy, using rustc 1.96.0. These figures cover
only the new arm. They do not replace the default benchmark headline.

| Release | A leaked bytes | D false-positive bytes | Refused | Exact restores | Valid manifests |
| --- | ---: | ---: | ---: | ---: | ---: |
| v0.14.0 | 1,100 | 580 | 0 | 140/140 | 140/140 |
| v0.15.0 | 1,100 | 580 | 0 | 140/140 | 140/140 |
| v0.15.1 | 1,100 | 580 | 0 | 140/140 | 140/140 |
| v0.16.0 | 1,100 | 580 | 0 | 140/140 | 140/140 |

All four scored-label contracts give these totals. Layer A has no false-positive
bytes; layer D has no leaked bytes. No-tie, same-variant and absent-membership
controls have zero leaked and false-positive bytes. There are no restore or
manifest failures.

The v0.14.0 benchmark example predates policy-file input. Its committed
transport bridge loads that release's policy through its existing assembly API;
it changes no detector or resolver code. Later releases need no source changes.

`family_precedence_history.py build` creates a receipt pinning the tag,
Cargo.lock, CLI, benchmark example source and binary, and any transport bridge.
`measure` verifies that receipt and captures the added cells through the
canonical scorer. Callers must serialize measurement with the benchmark-machine
lease. Supply `--release`, `--worktree` and `--build-receipt` to both commands;
`measure` additionally takes the current validator probe via `--validator-probe`
and an output directory via `--output`.

The committed captures and compressed observations are in
[`family-precedence-history/`](family-precedence-history/). Replay every release:

```sh
uv run --project scripts/bench python scripts/bench/family_precedence_history.py check
```

Replay checks tag inputs, generator and policy pins, document inventory and gold
bounds, observation hashes, and canonical metrics under all four contracts.
Docs CI runs the same check.
