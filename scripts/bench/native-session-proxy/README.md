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
longer name. Contained and chained overlap cases reuse a full name token whose
value overlaps another declared name. Their lowercase and URL counterweights
remain outside the authored name spans. Fixture gold describes the expanded owner text. The scorer uses
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

The v3 observation record uses successful Name/Email owner echoes as mapping
constraints, then requires those constraints to reconstruct every authored leaf.
Cross-leaf reused tokens resolve literal-gap ambiguity. Remaining ambiguity fails
capture. Owned-token identity is checked at every original expanded position;
extra copies elsewhere do not hide a replaced owned token.

The default comparison requires proven admission FP controls. An explicitly
reviewed paired gate uses `--gate-policy paired`: only targets forwarded by
both arms contribute byte or restore gain. Newly forwarded targets, their full
FP cost, and missing baseline controls remain separate evidence without gate
credit. A base request refusal followed by a candidate owner-response refusal
is reported as "refused in both arms; stage moved". An owner-response failure
for a case that already forwarded in base still fails this gate. The response
validator and its authorized-range contract are unchanged.

When adjacent new tokens remain ambiguous after a failed measured owner response,
the scorer requests independent owner proofs in the same session. Each proof
sends exactly one existing Name or Email token and requires the identical token
upstream and a successful owner restore in isolation. Any refusal, changed token,
raw forwarded bytes, or remaining ambiguity fails the capture. The returned
values constrain the original source-byte alignment. Proof requests are recorded
separately and receive no target byte, restore, availability, or gate credit.
The measured response's refusal and restoration result remain unchanged. This
avoids joining unrelated owner echoes during the auxiliary proof and changes no
response validator or target response format.

Unsupported-carrier fixtures send a nested string under `unsupported_carrier`,
separately from mutable message content. The observation includes that exact
upstream leaf in its real wire position; authored gold and all benign bytes
are scored there too. Plain repeats, existing-token remainders and contained
overlaps have code, URL, public-word and lowercase-part counterweights.
Anthropic's codec already rejects unknown fields; its rows remain fail-closed
controls for this legacy OpenAI/Gemini repair.

The opt-in `--gate-policy leak-to-refusal` applies the reviewed immutable-carrier
ruling. Each newly refused target must have positive raw target leaked bytes
in the fresh base capture. Seed leaks cannot justify a target refusal. Every
accepted transition is listed as `leak -> refused`, with credit limited to the
base target bytes stopped. Refusals remain a separate count. Refusing a target
that did not leak in base fails unless it is the separately typed resource
limit described below, as does any other byte, seed, restore or
ownership regression outside those justified transitions. Refused targets
supply no false-positive reduction credit. Increases in a refused target's seed
false positives still count as cost. Successful rows retain their exact wire
leaves and proven mappings so reviewers can re-score each stopped leak.
The default strict comparison and the both-forwarded paired comparison remain
available unchanged in policy.

Immutable-carrier Rule coverage permits an uncovered gap only when it contains
whitespace between two covered spans of the same occurrence. Leading or trailing
gaps refuse. Every uncovered non-whitespace byte in a Rule occurrence, including punctuation,
digits, and zero-width characters, refuses. The native cases include a fully owned
name repeated across a public separator and a private interior name between
owned copies. The mutable protector keeps its existing, stronger byte coverage.

Large-carrier fixtures use the committed `large_owned_carriers` specifications.
The loader expands each specification into owner text, wire text, and authored
gold before measurement or comparison. A 2,048-byte synthetic name repeated
1,025 times occupies 2,100,225 owner-text bytes but less than 20,000 wire bytes.
The safe case has only owned values and public spaces. Its companion inserts
a raw private `S` between two owned values and declares the longer header name
in mutable content. Their target leak counts must be zero and one byte,
respectively, when the carrier forwards unchanged.

Residual Rule checking fully expands owned tokens, then runs the real Rule
matcher against complete text. This preserves distant URL exclusions and
compound-name growth. Adjacent-token controls append public punctuation, digits,
a URL, or Unicode whitespace; their raw target leak counts remain zero.

All immutable leaves in one request share an expansion resource budget of 16 times
`ProxyConfig::body_limit_bytes` (32 MiB with the default 2 MiB body cap).
`ProxyConfig::request_residual_expansion_limit_bytes` overrides that budget. The
proxy preflights the combined expanded size before allocating any expanded text
or ownership mask, normalizing, or matching. Larger totals return HTTP 413 `ResidualExpansionLimitExceeded`, independently of
coverage. The approved `leak-to-refusal` policy records these transitions in
`resource_limit_transitions` and `resource_limit_refusals`, with no byte or restore
gain credit. Seed false-positive increases still count. This is the sole resource
exception to the positive-base-leak rule; coverage refusals still require proof
of a base leak. Strict and ordinary paired policies retain their refusal gates.
The cap bounds expanded input, not peak process memory or concurrency.

Rule coverage uses the captured request transaction; the independent detector/net
probe uses a fresh discarded session to retain baseline neural context. Owned
classes must not turn public suffixes into new neural findings. The digit-suffix
counterweight exercises this distinction under the full policy. Probes preserve
mappings and session state. They do not suppress audit attempts: `control_would_mutate` retains one audit attempt on
both base and candidate. This pre-existing audit behavior is outside the repair.

The large-carrier cases apply to OpenAI and Gemini, which use the repaired
legacy residual path. Anthropic's existing `ResponseBodyLimitExceeded` rejects
the 2,048-byte name seed before the unsupported-field target can run. The
committed fixture records that exclusion. Anthropic's existing unknown-field
controls remain in the comparison. No response limit or validator changes for
this counterweight.
