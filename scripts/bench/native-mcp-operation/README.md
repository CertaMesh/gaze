# MCP operation ordering arm

This arm measures each runtime checkout's real `PiiEnvelope::dispatch`, using
its own `gaze setup` policy and the complete primary + Nym configuration.
Arguments captured inside the tool and protected responses released to the host
are distinct operations. It does not flatten JSON into the text benchmark.

The committed fixture `../fixtures/mcp_operation_order.json` is specification
`mcp-operation-order-v1`. It contains 38 synthetic operations: declared sources
in A, ordinary text, code, and number counterweights in D, and source-first and source-last
repeats and owned-token overlaps in R. Payloads use fixed producer declarations
for `items` and a public `count`; fixtures do not confer carrier authority.
The ordinary agentic generator is unchanged; this arm has its own frozen identity.

Each case has authoritative expanded originals and UTF-8 gold extents. The Python
oracle independently verifies the exported session manifest's Ed25519 signature,
reconstructs each emitted token's original bytes, checks JSON shape and types and
non-text values, and counts raw private bytes and protected non-gold bytes.
It also verifies the manifest call and the response snapshot hash. Session export and import
runs in the runtime host too. Neither a runtime safety flag nor re-detection of
protected output establishes coverage. Seed failures count as refusals. If arguments reached the tool before a later
response failure, their disclosed bytes still count. The comparison also rejects
an increase in dispatch failures.

Both scored-label contracts are reported. All current gold is Name or Email,
scored under v1 and v2; v2 neutral predictions are excluded from false positives.
The comparison rejects any per-layer leak/refusal/restore/manifest regression,
any newly refused previously successful case, or an input, model, or host identity
mismatch. Removed leaks must exceed added false positives. D false positives
are also reported by text, code, and number category. This finite fixture is not a
claim of complete detection, generic fragmentation support, or worst-case cost.

Obtain benchmark-machine/load-coordinator approval before capture. Use separate
Anvil base/candidate worktrees, isolated verified model copies and one Cargo job,
with Rust 1.96.0 and explicit RUSTC and RUSTDOC paths. The committed candidate driver builds the
same host source against each runtime's exact libraries and lockfile. Measure
base first. Host compilation/API failure is unsupported, never zero-leak success.

```sh
# Run in each selected runtime checkout; these commands execute inference.
cargo run -p gaze-cli -- setup --non-interactive --force --policy-out target/gate/policy.toml
python3 /path/to/committed/scripts/bench/build_native_mcp_operation.py \
  --runtime-root "$PWD" --output target/mcp-host
# Set GAZE_NER_MODEL_DIR and GAZE_NYM_MODEL_DIR to coordinator-approved isolated copies.
uv run --project scripts/bench --group comparison-test python \
  /path/to/committed/scripts/bench/mcp_operation_order.py \
  --runtime-root "$PWD" --binary target/release/gaze-native-mcp-operation \
  --build-evidence target/mcp-host/build-evidence.json \
  --policy target/gate/policy.toml --output target/mcp-operation-capture.json

python3 scripts/bench/mcp_operation_order.py \
  --compare /path/to/base-capture.json /path/to/candidate-capture.json \
  --output target/mcp-operation-comparison.json
```

Captures retain exact synthetic protected leaves and signed manifests, plus
hashes for the fixture, driver, host, builder, runtime lock, binary, policy, and models. Preserve
setup/build/capture logs alongside them. The host emits no timing claim.

This arm supplements the required full v2 and v1 gain gate for layers C, A, D, and R. Its A, D, and R totals
must not be silently substituted for the text layers. Coordinate gate integration
and required history measurements before claiming a detection-changing merge pass.
An implementation with only unit-test gains is not ready to merge.

Model-free oracle tests:

```sh
uv run --project scripts/bench --group comparison-test python -m pytest \
  -q scripts/bench/test_mcp_operation_order.py
```
