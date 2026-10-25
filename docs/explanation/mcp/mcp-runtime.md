# gaze-mcp runtime architecture

## Scope

`gaze-mcp-core` protects source data passing through agent-tier MCP tools.
Every call uses `PiiEnvelope::dispatch`. Authorized operator tools may bypass
response protection for restore/export; raw responses must stay owner-side.

Chat paste, uploads, and screenshots bypass MCP and reach the model without
this protection. Use a host preprocessor or the API-key
[proxy](../proxy/proxy-runtime.md) on that path.

## The chokepoint

The chokepoint is `PiiEnvelope::dispatch` in
[`crates/gaze-mcp-core/src/dispatch.rs`](../../../crates/gaze-mcp-core/src/dispatch.rs).
Every tool call traverses this sequence in order:

| Step | Action | Failure mode |
|---|---|---|
| 1 | Validate transport-supplied session id via `SessionIdPolicy` | `DispatchError::SessionId`; no manifest row |
| 2 | Look up tool in `ToolRegistry` | `DispatchError::UnknownTool`; no manifest row |
| 3 | Authorize via `AuthHook::authorize_agent` or `_operator` (driven by `ToolDescriptor::tier`) | `DispatchError::Auth`; no manifest row |
| 4 | Preflight argument carriers, then protect raw args via `gaze::Pipeline::protect_text_transaction` (the staged args transaction commits after `begin_call`) | `DispatchError::Carrier` / `DispatchError::Protection` / `DispatchError::ResourceLimit` for preflight and protect — no manifest row; `DispatchError::Transaction` for the post-begin commit — fail_call written first |
| 5 | `ManifestStore::begin_call(BeginCallContext)` | `DispatchError::Manifest`; no manifest row written |
| 6 | Build the sealed `ToolCtx` (only construction site in the crate) | — |
| 7 | `Tool::invoke(&ctx).await` | `DispatchError::ToolError`; fail_call written first |
| 8 | For `ResponseRedaction::Apply`, preflight response carriers, then stage response protection via `gaze::Pipeline::protect_text_transaction`. Operator-tier `BypassByOperator` uses the raw payload with no preflight, protection, or response transaction; agent-tier bypass is rejected | `DispatchError::Carrier` / `DispatchError::Protection` / `DispatchError::ResourceLimit`, or `DispatchError::Redaction` for the agent-bypass rejection; fail_call written first |
| 9 | Compute out-of-row `SnapshotRef` over the response payload (protected for `Apply`, raw for operator bypass) | `DispatchError::ResponseSerialization`; fail_call written first |
| 10 | Commit the staged response transaction, if present, after snapshot computation | `DispatchError::Transaction`; fail_call written first |
| 11 | `ManifestStore::finish_call(handle, snapshot)` | `DispatchError::Manifest` |
| — | Return the response payload (protected for `Apply`, raw for operator bypass) | — |

Validation, lookup, auth, and argument preflight happen before the manifest.
After `begin_call` succeeds, dispatch runs either `finish_call` or `fail_call`
before returning. `tests/chokepoint_ordering.rs` pins the order.

## Operation resource budgets

Arguments and protected responses are separate operations. Full expansion of owned
tokens has an aggregate budget of 16 times the existing 2 MiB sizing constant,
currently 32 MiB across all string leaves, including literal input bytes. There
is no separate decoded-input cap.
The runtime expands the complete text and applies the real Rule matcher before
commit or release. It does not truncate text or skip coverage to fit the budget.

Exceeding the expansion budget returns the distinct `DispatchError::ResourceLimit`.
This error contains no input values and discards the operation's staged state.
An argument limit failure occurs before `begin_call` or tool invocation. A protected
response limit failure writes `fail_call` before returning and releases no response.
Token provenance and restore failures retain `DispatchError::Protection`.
The rmcp adapter maps this budget refusal to the stable `resource_limit` text
class with `isError: true`. Whitespace-only interior gaps between owned spans
count as covered in projection and final proof without minting new mappings.
Empty owned values keep their wire positions and identities. Projection splits
a replacement at an interior empty marker and leaves boundary markers outside
the replacement. The independent coverage proof still checks the full expansion.

## Type-level seal

| Seal | Effect |
|---|---|
| `pub(crate) ToolCtx::new` | No external constructor |
| Private fields + `#[non_exhaustive]` | No external struct literal |
| `ToolCtx<'a>` borrows dispatch-owned context | Cannot retain context beyond the call |
| `ToolRegistry::register<T: Tool + 'static>` | No raw closure registration; tools implement `descriptor` and `invoke` |

Compile-fail fixtures in `crates/gaze-mcp-core/tests/ui` verify construction
is unavailable. Tools receive `&ToolCtx<'_>`.

## Agent vs operator tier

`ToolDescriptor::tier` chooses `AuthHook::authorize_agent` or
`authorize_operator`. Default `DenyAllAuthHook` denies missing authorization.

The default-off `operator-tier` feature gates `RestoreTool`,
`RestoreStrictTool`, `ExportManifestTool`, `tools::{export, restore,
restore_strict}`, and `operator_tools`. Default agent builds cannot link them.
`mcp-tier-isolation` runs four core feature graphs with external `trybuild`
fixtures, and requires named tests so zero tests cannot pass the gate.
The [Dylint protected paths](../../../lint/dylint) also reject forbidden
`gaze_audit::*` imports in the core.

## rmcp transport sink

`gaze-mcp-rmcp::RmcpFrontend` maps descriptors to rmcp tools, call requests to
`(tool_name, raw_args, external_session_id, HostContextSelection)`, and responses to rmcp results.
It sees only `Arc<dyn DispatchHost>`; with the core wrapper every call traverses
`PiiEnvelope::dispatch`. A failed `ManifestStore::finish_call` returns an error,
never the tool payload.

| Feature | Transport |
|---|---|
| `transport-stdio` (default) | Process stdio |
| `transport-http` | Axum streamable HTTP at `/mcp` |

Adopters supply `PrincipalResolver`; `FixedPrincipalResolver` serves local
stdio/tests. Listing hides operator tools from non-operator principals, but
authorization still happens in dispatch.

rmcp 2.x removes the legacy `_session_id` argument without using it. Host middleware
can supply `HostCallContext` through server-local request extensions. Its optional
session id labels the audit trail; only the host selects the actual Session.
Isolation tests cover stdio, stdio+HTTP, and no-default-feature graphs.

### Private record context

An embedder passes `HostContextSelection::Absent` or `Present` directly into
dispatch. The envelope parses the preserved JSON with `gaze::Context::from_json_str`
and assembles a request-local response pipeline against the resident policy.
NER and safety-net handles remain resident. No default implementation discards
`Present`; unsupported hosts reject it before invoking a tool.

The resident pipeline protects model-authored arguments. Private context applies
only to responses, with any value guessed in the original arguments excluded.
The exclusion uses conservative Unicode and whitespace matching without requiring
cues. A tool that adds a cue to an echoed guess must not reveal record membership.
This trades context recall for resistance to guessing attacks. Ordinary recognizers
and existing session mappings continue to protect those values.

Only matched response values persist as Rule evidence. Later turns protect those
values without retaining the record. Unused values and raw context do not enter the
session, `ToolCtx`, manifest metadata, or protocol output. Custom context identifiers
use opaque names in detection metadata. The host retains restore authority.

Embedded HTTP middleware sets `HostCallContext` in request extensions. Raw context
in `_meta`, arguments, headers, or provider bodies is never trusted input. Stock
stdio hosts need an in-process integration to use private context. No sidecar or
stdio file-descriptor protocol is supported in this release.

## Manifest contract

`ManifestStore` (in [`crates/gaze-mcp-core/src/manifest.rs`](../../../crates/gaze-mcp-core/src/manifest.rs))
is async + Send + Sync and has three methods:

- `begin_call(ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError>`
- `finish_call(handle, snapshot: SnapshotRef) -> Result<(), ManifestError>`
- `fail_call(handle, reason: FailureReason) -> Result<(), ManifestError>`

`SnapshotRef` stores only locator, SHA-256 hex, and byte length, not response
bytes. Adopters needing payload persistence must wrap their store and persist
before `finish_call`, using owner-controlled storage. External session binding
belongs in the `ManifestStore` implementation constructor.

### Audit row fields the dispatcher provides

| Field | Source | Purpose |
|---|---|---|
| `call_id: Ulid` | dispatcher (one per call) | Stable handle reused across begin/finish |
| `external_session_id: Option<&str>` | transport | Adopter-supplied; validated by `SessionIdPolicy` first |
| `principal_id: &str` | `Principal::id` after auth | Audit attribution |
| `tool_name: &str` | `ToolDescriptor::name` | Routing + audit |
| `redacted_args: &serde_json::Value` | post-redaction args | Safe to persist |
| `started_at: SystemTime` | dispatcher | Schema field for `started_at` ordering |

On the success path, the adopter additionally records the
`SnapshotRef` (locator + sha256 + byte_len). On failure, a
`FailureReason` enum:

- `ToolError { class, message }` — `class` is one of the stable strings
  from `ToolError::class()` (`"invalid-args"`, `"not-found"`, `"internal"`).
- `AuthDenied { reason }`.
- `RedactionFailed { message }`.
- `Other { message }` — escape hatch for adopter-defined cases.

## Threat model

| Threat | Boundary |
|---|---|
| Raw source result | Protect response and persist completion before return |
| Missing auth | `DenyAllAuthHook`; no authorized tool invocation |
| Fabricated/reused context | Constructor, fields, and lifetime seal |
| Raw closure bypass | Trait-only registration |
| Restore accidentally exposed | Default-off feature plus operator auth |
| Audit dependency drift | Metadata isolation gate plus Dylint |
| Chat paste/uploads/screenshots | Outside MCP; protect before provider receives input |

## Out of scope

MCP tools run after user input reaches the model service; receipt already
constitutes processing. Every MCP README and architecture page must state this
source-tool versus user-input boundary. A proxy, host preprocessor, or workflow
that filters before submission is needed for user input.
