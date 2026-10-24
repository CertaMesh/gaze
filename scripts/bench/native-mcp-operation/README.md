# MCP operation ordering arm

This arm measures each runtime checkout's real `PiiEnvelope::dispatch`, using
its own `gaze setup` policy and the complete primary + Nym configuration.
Arguments captured inside the tool and protected responses released to the host
are distinct operations. It does not flatten JSON into the text benchmark.

The committed fixture `../fixtures/mcp_operation_order.json` is specification
`mcp-operation-order-v5`. It contains 100 synthetic operations: declared sources
in A, ordinary text, code, and number counterweights in D, and source-first and source-last
repeats and owned-token overlaps in R. The specification includes bare-filename and other-namespace token
counterweights, plus repeats beside both. It also measures public whitespace beside newly protected name parts, including Unicode whitespace, both directions, and three-part chains. Empty owned mapping admission is measured at both prefix and suffix boundaries in each direction. The host seeds those mappings through the public tokenize API, which permits an empty value. Both sides must be captured on this
version; earlier specifications are superseded. Current-session manifest values
restore through the runtime assessment API. Unowned spellings remain literal,
as required by the existing MCP contract; their other owner is not validated
by this arm. The real token-bridge integration test covers that owner contract. Payloads use fixed producer declarations
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
# Run in each selected runtime checkout. These commands execute inference.
# Use the same worker-owned copy paths for the two serial arms.
mcp_model_root=/path/to/approved-private-model-root
export GAZE_NER_MODEL_DIR="$mcp_model_root/gaze/models/davlan-mbert-ner-hrl"
export GAZE_NYM_MODEL_DIR="$mcp_model_root/gaze/models/nym-small-int8"
XDG_DATA_HOME="$mcp_model_root" cargo run -p gaze-cli -- setup \
  --non-interactive --force --policy-out target/gate/policy.toml
python3 /path/to/committed/scripts/bench/build_native_mcp_operation.py \
  --runtime-root "$PWD" --output target/mcp-host
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

The composed gate in `../mcp_operation_gate.py` adds the native operation counts
to A, D, and R while retaining the original text component and C. It first admits
the text scorecards through `agentic_layers.gate` and replays the native byte
oracle. Exact runtime revisions, setup-policy hashes, NER thresholds, and model
bundle digests must match within each side. A regression in either component
fails even when the combined totals would hide it. Original capture scorecards
and their execution receipts remain unchanged. The derived scorecards retain
both input hashes and the composed totals used by the canonical byte-gain rule.

```sh
uv run --project scripts/bench --group comparison-test python \
  scripts/bench/mcp_operation_gate.py \
  --base-v1 /path/to/base-v1/scorecard.json \
  --candidate-v1 /path/to/candidate-v1/scorecard.json \
  --base-v2 /path/to/base-v2/scorecard.json \
  --candidate-v2 /path/to/candidate-v2/scorecard.json \
  --base-native /path/to/base-native.json \
  --candidate-native /path/to/candidate-native.json \
  --output-dir target/mcp-composed-gate
```

The command writes four composed scorecards and `gate.json`. It exits with 1
unless both contracts pass. A native-only comparison does not establish the
full gain gate. Missing or mismatched text captures block composition. Run the
required history measurements or report unsupported releases before claiming
the benchmark's release evidence is complete.

Model-free oracle tests:

```sh
uv run --project scripts/bench --group comparison-test python -m pytest \
  -q scripts/bench/test_mcp_operation_order.py
```

An explicit source-identity reuse ruling can replace fresh text capture when
all C/A/D/R inputs are unchanged. In that mode, pass `--text-path-identity`
instead of text scorecards. The gate verifies every recorded file hash and
checks that the published runtime matches the measured candidate. Derived
scorecards mark text layers `unchanged_by_construction`; they contain no
invented text counts. See the committed
[operation-order evidence](../../../docs/reference/benchmarks/gates/mcp-operation-order/README.md)
for the approved capture, identity record, cleanup receipt, and model-free replay.
