# Native proxy session benchmark host

`main.rs` is compiled against each measured checkout's own libraries. It loads
that checkout's `gaze setup` policy through `gaze-assembly`, including NER and
Nym, and starts the supported provider adapters. Anthropic explicitly enables
`OpaqueHeaderContinuity`; the public CLI uses ephemeral Anthropic sessions.
The adapters, protection, admission, serialization, and restore code belong to
the measured revision.

From the current benchmark checkout, build the host for each runtime:

```sh
python3 scripts/bench/build_native_session_proxy.py \
    --runtime-root <runtime-checkout> --output <runtime-checkout>/target/session-host
python3 scripts/bench/proxy_request_order.py \
    --runtime-root <runtime-checkout> \
    --binary <runtime-checkout>/target/release/gaze \
    --session-binary <runtime-checkout>/target/release/gaze-native-session-proxy \
    --policy <runtime-checkout>/target/gate/policy.toml --output <capture.json>
```

The builder copies the runtime lockfile and rejects any registry dependency
outside its pinned package versions and checksums. Its evidence file records
source, binary, and lockfile hashes. The helper remains outside the measured
source tree in `target`; no runtime code is patched.

Two-turn cases first send a real header and capture its owned name token. The
second request reuses that token in the same session alongside a request-known
longer name. Fixture gold describes the expanded owner text. The scorer uses
the independently captured seed mapping to align adjacent tokens uniquely.
Unseeded ambiguous token alignments still refuse scoring.

Seed requests must forward and restore exactly. Both seed and second-request
bytes contribute to leak and false-positive counts. Target-request refusals
remain separate from seed counts. The owner echo checks selected Name and
Email tokens; it does not validate an exported manifest. Every forwarded byte
is scored even when owner response processing fails.

Captures are untimed and run sequentially under the benchmark lease with
private model and ORT copies. Historical revisions use this same committed
host and fixture. A revision that cannot build or supply its own full policy
is reported as unavailable.

Admission controls use the same authored owner text, gold spans, and seed request.
Their main copy is sent as expanded text while the public counterweight retains
its owned token. A baseline source-first control must already forward and have
exactly the candidate admission's false-positive bytes under both contracts.
These admissions receive no paired leak credit. The driver verifies that bound
wire text restores to the authored owner text before scoring.
