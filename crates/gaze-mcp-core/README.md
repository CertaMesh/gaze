# gaze-mcp-core

[![Crates.io](https://img.shields.io/crates/v/gaze-mcp-core.svg)](https://crates.io/crates/gaze-mcp-core)
[![docs.rs](https://docs.rs/gaze-mcp-core/badge.svg)](https://docs.rs/gaze-mcp-core)
[![License](https://img.shields.io/crates/l/gaze-mcp-core.svg)](https://github.com/CertaMesh/gaze#license)

Transport-free MCP runtime: `Tool`, sealed `ToolCtx`, `ToolRegistry`, `PiiEnvelope::dispatch`, `ManifestStore`, `AuthHook`, and `SessionIdPolicy`. Use `gaze-mcp-rmcp` for stdio/HTTP or implement `Frontend` / `DispatchHost`.

## Scope

Agent-tier source data passes through `PiiEnvelope::dispatch` before reaching the model. Authorized operator tools may bypass response protection for restore/export; raw results must stay on the operator surface.

Chat pastes, uploads, and screenshots bypass MCP. Use `gaze-proxy` for the user-to-model path (OpenAI, Anthropic, Gemini).

## Adopter quickstart

Add `gaze-assembly` as a direct dependency alongside `gaze-mcp-core` and
`gaze` (package `gaze-pii`). It builds the bundled primary recognizers and
matching locale chain used below.

```rust
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use gaze_mcp_core::{
    AuthHook, AuthError, BeginCallContext, CallHandle, FailureReason,
    ManifestError, ManifestStore, PiiEnvelope, Principal,
    SessionIdPolicy, SnapshotRef, Tool, ToolCtx, ToolDescriptor,
    ToolError, ToolRegistry, ToolResponse,
};

// 1. Implement `ManifestStore` against your backing store.
struct MyManifest { /* … */ }

#[async_trait]
impl ManifestStore for MyManifest {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        // Persist `ctx.call_id`, `ctx.principal_id`, `ctx.tool_name`,
        // `ctx.redacted_args`, `ctx.started_at`, optionally bind to
        // `ctx.external_session_id`.
        Ok(CallHandle::new(ctx.call_id))
    }

    async fn finish_call(
        &self,
        _handle: CallHandle,
        _snapshot: SnapshotRef,
    ) -> Result<(), ManifestError> {
        // Record the redacted-response snapshot reference.
        Ok(())
    }

    async fn fail_call(
        &self,
        _handle: CallHandle,
        _reason: FailureReason,
    ) -> Result<(), ManifestError> {
        // Record the failure reason. Must always succeed in chokepoint
        // ordering — return Err only when the backing store is genuinely
        // unavailable.
        Ok(())
    }
}

// 2. Implement `AuthHook` to gate dispatch.
struct MyAuth;

#[async_trait]
impl AuthHook for MyAuth {
    async fn authorize_agent(&self, _p: &Principal, _tool: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn authorize_operator(&self, _p: &Principal, _tool: &str) -> Result<(), AuthError> {
        Err(AuthError::Denied("operators must use the admin path".into()))
    }
}

// 3. Build the gaze pipeline + session per conversation.
let core = gaze_assembly::CorePipelineConfig::new().build().expect("core pipeline");
let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");

// 4. Register tools.
let mut registry = ToolRegistry::new();
# #[cfg(feature = "core-tools")]
registry.register(gaze_mcp_core::core_tools::CleanTool::new()).unwrap();

// 5. Build the envelope; pass to the transport via `Frontend::serve`.
let manifest = MyManifest {};
let auth = MyAuth;
let policy = SessionIdPolicy::default_strict();
let _envelope = PiiEnvelope::new(
    &registry, &auth, &manifest, core.pipeline(), &session,
    core.locale_chain().as_slice(), &policy,
);
```

The transport sink (e.g. `gaze-mcp-rmcp::RmcpFrontend`) wraps the envelope
behind the `DispatchHost` trait and calls `Frontend::serve` from the
adopter's tokio runtime.

## What gets enforced

```mermaid
flowchart LR
    Args[Protect arguments] --> Begin[manifest.begin]
    Begin --> Invoke[Invoke tool]
    Invoke --> Response[Protect response]
    Response --> Finish[manifest.finish or fail]
    Finish --> Return[Return]
```

Only the dispatcher constructs `ToolCtx`: its constructor and fields are `pub(crate)`, its shape is `#[non_exhaustive]`, and its lifetime binds it to the call. The registry accepts only `Tool` implementations. Compile-fail fixtures in `tests/ui/` and `tests/chokepoint_ordering.rs` check these boundaries.

Snapshot refs use `sha256(audit_session_id || 0x00 || call_id || 0x00 || payload_bytes)`. Audit readers can verify guessed payloads offline. Protect snapshot storage; if audit readers become less trusted, use keyed HMAC owned by `ManifestStore`.

## Session ownership boundary

Use one `gaze::Session` per authorization domain. With private record context,
use one `Session` and one `DispatchHost` per conversation. Sharing either across
conversations shares Rule-evidence token mappings, HMAC labels, and restore
authority. An audit session id labels calls but does not partition this state.

`export_session_tokens` exposes the session's entire token/raw map without per-call
filtering. Sharing a session across users or agents lets one authorized operator
read another's PII. Hosts must create and pass separate sessions to `PiiEnvelope::new`.

## Operator-tier tools and audit storage

Operator-tier tools (`restore`, `restore_strict`, `export_session_tokens`)
are privileged by design. Their descriptors set
`ResponseRedaction::BypassByOperator`, so the dispatcher returns their raw
payloads to an operator principal after `AuthHook::authorize_operator`
passes. Agent-tier tools cannot opt into this posture; registry validation
rejects it and the dispatcher fails closed if that invariant is ever broken.

Successful operator-tier responses are still committed through
`ManifestStore::finish_call` before returning. For these tools, the response
snapshot can contain raw PII: restored values or the complete session token
inventory. `ManifestStore` implementations that persist snapshots must protect
those bytes with encryption at rest and operator-only read access. Audit rows
record snapshot locators plus the salted SHA-256 integrity marker described
above; audit readers are trusted to see manifest metadata and to verify guessed
payloads offline under the v0.7.x threat model.

## Strict protection boundary and migration

`PiiEnvelope` now protects both arguments and agent responses through
`Pipeline::protect_text_transaction`. Supply a configured primary pipeline,
for example `gaze_assembly::CorePipelineConfig::new().build()`, and pass its
locale chain to the envelope. An empty primary registry is rejected even for
calls containing no strings. The CLI MCP host uses this assembly path.

Arguments and protected responses are separate operations. Each operation discovers
validated deterministic Rule sources across all original text leaves before protecting
those leaves. An earlier repeat is therefore covered even when its source is later
in the same operation. Learned and safety-net-only detections do not become Rule
sources. Existing owned tokens keep their identities; overlapping Rule matches cover
adjacent raw bytes without rewriting those tokens. An independent final Rule coverage
proof runs before commit or release. Empty owned values remain wire-position
markers, including at Rule-hit boundaries and inside matches. Projection never
consumes these markers. Failed protection discards staged mappings.

Each operation has a 32 MiB full owned-token expansion budget, derived as
16 times the existing 2 MiB sizing constant. It counts all restored string leaves
together, including literal input bytes. Decoded input has no separate size cap.
Exceeding the expansion budget returns `DispatchError::ResourceLimit`, with no
input bytes in the error. Whitespace-only interior gaps between owned spans count
as covered without new mappings; other uncovered bytes still require protection.
Unknown or forged tokens retain their existing protection errors. Full expansion
and the real Rule matcher still run within the budget. Transports must also bound
wire input and JSON structure. Separate dispatches remain separate operations,
and later declarations cannot retract earlier releases.

The compatible `PiiEnvelope::new` constructor uses an empty dictionary bundle.
Use `.with_dictionaries(&bundle)` to supply tenant terms. Primary recognizers,
mandatory safety nets, and `ToolResources::protection_context()` receive the
same locale and bundle. Observer tools use the new
`scan_safety_nets_with_dictionaries` and structured counterpart; the old
observer signatures retain an empty bundle. Model interfaces that do not
consume dictionaries are unchanged.

Every installed custom safety net must support the supplied locale chain.
A configured model registry must resolve coverage. All selected safety-net
backends run on every complete final string leaf, even when primary detection
emits nothing or the input consists entirely of existing tokens. Observer
skip optimizations cannot disable this boundary check. Invalid spans, backend
failures, and residual suspects outside verified token coverage reject the
operation. Zero installed safety nets is a primary-only floor, not a
claim that all PII was detected. Global residual policies are unchanged.

Custom producers must declare their JSON carriers at trusted registration:

```rust
use gaze_mcp_core::{CarrierDeclaration, ToolDescriptor};
use serde_json::json;

let descriptor = ToolDescriptor::agent("lookup", json!({"type":"object"}))
    .with_carriers(
        CarrierDeclaration::text_fields(&["query"]),
        CarrierDeclaration::text_fields(&["result"]),
    );
```

Use `CarrierDeclaration::new` for nested objects or explicitly non-sensitive
numbers. Each path is a vector of `CarrierSegment::Member(exact_name)` and
`CarrierSegment::AnyIndex` (one array edge only). Declare every encountered
object edge and every numeric leaf separately. Full paths must match exactly;
parent declarations do not authorize descendants. Numbers retain their
original `serde_json::Number` representation. Numeric declarations mean the
producer asserts those fields are non-sensitive; detectors do not certify
their contents. Booleans and null remain non-text values.

Schemas are catalog metadata and grant no authority. Declarations are omitted
from serialization; deserializing a descriptor cannot recreate trusted
permissions. Unconfigured descriptors accept root strings or arrays of strings
but reject object members and numbers. Built-in text, tokenize, and document
tools carry explicit declarations. For structured `SafetyNetCheckTool` input,
use `with_argument_carriers` with the complete supported document shape.
Arbitrary dynamic keys and cross-field concatenation are unsupported.

Known tokens are matched with the session's actual restoration semantics,
including non-angle format-preserving tokens. Only owned input ranges and
verified newly emitted ranges are protected coverage. Literal collisions,
including across JSON leaves, reject rather than silently reinterpret input.
Primary detection still runs independently on gaps between owned tokens;
this does not extend its cross-token detection domain. Full-leaf safety nets
see the entire final text. One-way replacements fail reversibility checks.

Arguments stage as one operation: preflight, protect, begin manifest, commit,
then invoke. A fresh response transaction starts after invocation. Response
preflight, protection, and snapshot construction precede its commit; successful
`finish_call` then permits egress. A later leaf failure discards that operation's
staging. Argument mappings and legitimate tool-side live-session mutations
are outside response rollback. Concurrent generation conflicts publish none
of the losing transaction's state.

A terminal manifest attempt consumes the handle even when persistence fails.
Never call `fail_call` after attempting `finish_call`. Response mappings are
already committed when finish is attempted; a failed finish retains those
mappings but returns no payload. This is not whole-call rollback.

Authorized operator response bypass remains a separate exception: its raw
keys, strings, and numbers are intentional, require operator authorization,
and return only after successful manifest finish. Operator arguments still
use the strict declared-carrier boundary. Agent bypass registration rejects.
All transport errors remain class-only; detailed backend errors and manifest
records belong exclusively to trusted-side diagnostics.

Direct users of the core leaf API must discard their transaction after any
error: staged mappings may remain, and previously returned strings are not
immutable operation proofs. The envelope performs this discard automatically.
Safety-net reconstructed `raw_span` offsets address expanded input, with owned
tokens replaced by their stored raw values; they cannot index the literal
input containing those tokens. Observer `nets_run` remains a configured count
(a nonempty registry counts as one), not an executed-model count.

## Explicit untrusted request mode

The default `dispatch` contract is unchanged. A tool must register
`RequestMode::UntrustedInvocation` to use `dispatch_request`; both entry points
reject a descriptor written for the other mode before authorization or audit.
The new mode also rejects operator response bypass.

`ToolCtx::invocation_args()` provides explicitly untrusted execution data. The
host validates bounds and authorization and may restore existing session tokens
locally. It must not log these arguments. The wrapper's Debug omits its payload;
`redacted_args()` is Null in this mode, never an audit marker or raw arguments.

`BeginCallContext::args_audit` contains only the versioned metadata-only omission
record. Legacy `redacted_args` is Null. Stores must record the explicit audit
format and report original arguments as omitted during replay, not restored.
No request detection or new request mappings occur in the envelope. Response
protection, transaction commit and durable finish use the same shared path as
legacy dispatch. A failed finish retains response mappings but returns no output.
Snapshots can restore separately captured tokenized output; they are not an
archive of the response body. Detection is not a guarantee that every PII value
will be recognized.

## Implementing your own `Frontend`

Adopters who do not want rmcp implement [`Frontend`] themselves. The
contract is one method (`serve(self, host, shutdown) -> Result<(), FrontendError>`)
that drives the transport's accept-and-dispatch loop until
[`ShutdownToken::cancel`] fires. The host (`Arc<dyn DispatchHost>`)
wraps the envelope behind a narrow surface (`dispatch` + `list_tools`)
so the transport never sees the gaze pipeline, the gaze session, or the
manifest store directly.

A reference adapter for rmcp's `tools/list` + `tools/call` shape lives in
`crates/gaze-mcp-rmcp`.

## Cargo features

| Feature | Default | Adds |
|---|---|---|
| `core-tools` | yes | `core_tools::{CleanTool, TokenizeFieldTool, SafetyNetCheckTool}` registrations. |
| `operator-tier` | no | `operator_tools::{RestoreTool, RestoreStrictTool, ExportSessionTokensTool}`. Tools route through `AuthHook::authorize_operator`. |

The `operator-tier` feature is opt-in. Default builds expose only the
agent surface, so an adopter who skips wiring auth hits
`AuthError::MissingHook` from `DenyAllAuthHook` instead of accidentally
exposing restore.

The exclusion is a `#[cfg(feature = "operator-tier")]` gate on each operator
module, so rustc keeps the surface out of an agent-tier build entirely. That it
stays that way is verified by the trybuild compile-fail fixtures in
`tests/ui/tier/`, one per gated surface, each compiled as an external crate
against an agent-tier feature graph and required to fail resolution, driven by
the `mcp-tier-isolation` xtask gate. `scripts/gate/mcp-tier-isolation-mutation-probe.sh`
re-proves that the gate goes red when a gate is removed.

## Related crates and documents

Transport: `gaze-mcp-rmcp`. User-input protection: `gaze-proxy`.

Full dispatch, audit, and threat contracts: [MCP runtime](../../docs/explanation/mcp/mcp-runtime.md). Correlation fields, manifest lifecycle, failure variants, and authorization audit: [Metrics](../../docs/reference/metrics.md#7-mcp-chokepoint-observability-gaze-mcp-core).

Only current-session token mappings expand for operation discovery and coverage.
Other namespaces, including token-bridge owner tokens, retain their base behavior
as literal text. They are not expanded or newly rejected by this operation proof.
Their adopter-specific validation and error stage remain unchanged. A separate
opaque-carrier and foreign-namespace contract is a follow-up. Ordinary filenames
such as `scan_1.png` remain literal text too.

## Per-call host record context

`PiiEnvelope::dispatch`, `dispatch_request`, and `DispatchHost::dispatch` take an
explicit final `HostContextSelection` argument. Pass `Absent` for ordinary calls.
For a private owner record, pass `HostContextSelection::present(raw_json)` and
configure the envelope with `with_context_policy(policy, rulepacks)`, using the
exact resident assembly inputs. `CorePipeline::policy` and `rulepacks` expose
those inputs for the bundled configuration. Unsupported hosts reject `Present`.

The shared Context parser preserves CLI and daemon record limits. Context affects
only tool responses. Private terms present in model-authored arguments are excluded
for the call. Ordinary detection still protects arguments and responses.

Calls with `Present` context have a per-call guess budget, independent of the
record. `HostContextPolicy::default()` allows at most 4,096 argument bytes and
32 scalar values in each original and recursively decoded JSON view. Keys count
toward bytes. Both raw and normalized bytes must fit. Every visited key, scalar
value and container also counts against an entry cap equal to
`max_argument_bytes` (4,096 entries by default), including empty keys and
containers. Dispatch rejects oversized arguments with a value-free error before parsing the record or changing session
or audit state. Set `PiiEnvelope::with_host_context_policy(HostContextPolicy {
max_argument_bytes, max_argument_scalars })` to configure these host-owned limits.
Raising either limit increases the guess budget. `Absent` calls use the ordinary
MCP operation limits. Batch lookups above 32 IDs or tool bodies above 4 KB of
argument text are rejected at the defaults when context is attached.

Exclusion uses the detection pipeline's normalization and the response matcher's
token-boundary rule. It follows each recognizer's compilation mode and allowed
match kinds: Unicode case folding applies to Name records, while exact-only
fields retain their case and whitespace requirements. Flexible whitespace is
considered only when that match kind is allowed. Folded matches map back to the same normalized, pre-casefold text for boundary checks. A superstring
such as `Joanna Lim` does not exclude `Anna Li`. Coverage includes single fields
and all scalar fields joined in lexicographic key order with empty or space
separators. Arrays retain index order. It also checks every scalar pair in both
orders with empty, space, `@`, `-`, and `/` separators. This covers given/family
fields whose key order differs from schema order, an unrelated interleaved field,
and common two-field email or identifier templates. Original strings and decoded
JSON leaves are checked separately. Arbitrary transformations, reordered joins of
three or more fields, and other separators can still evade exclusion.

A record-returning lookup tool can still confirm a guess. A correct guess
excludes the returned record value from private-context detection. A wrong guess
leaves that value eligible for a token. In addition to guesses contained in the
admitted argument text and the two full scalar joins, pair joins add up to
`max_argument_scalars × (max_argument_scalars − 1)` candidates per separator.
At the defaults, that is 992 pairs per separator and 4,960 pair candidates across
the five separators in each view. The original and decoded views can differ,
adding up to 9,920 pair candidates per call. Short numeric fields such as PINs and
house numbers can therefore be enumerated in a few calls. Lower `max_argument_scalars` for tools that look up
short numeric records. Enumeration that fits within the budget remains possible.
Repeated calls are the host's rate-limit responsibility. Exclusion does not
provide general resistance to guessing attacks. Ordinary recognizers and existing session mappings
continue to protect their known values.

Only actual response matches enter the session as Rule evidence. Unused record
values do not persist; matched values restore exactly and remain protected on
later turns. `Present` cannot accompany an operator response bypass.

Keep context out of tool args, schemas, `_meta`, `ToolCtx`, audit payloads, and
logs. The raw wrapper has redacting Debug and clears its owned buffer on final
drop. This does not guarantee erasure of allocator or parser copies. The HTTP
adapter accepts host middleware extensions; ordinary stdio has no private channel.

Private class and dictionary identifiers use domain-separated HMAC labels under
a random per-session key. They are stable across turns and trusted snapshot
imports, while independent sessions use unrelated labels. The key is stored only
in the live session and its signed version 8 sensitive snapshot. Keyless legacy
opaque classes remain restorable but cannot dispatch tools. See
[UPGRADE.md](../../UPGRADE.md) for the migration boundary.
