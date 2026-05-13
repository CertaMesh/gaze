# gaze-mcp-core

[![Crates.io](https://img.shields.io/crates/v/gaze-mcp-core.svg)](https://crates.io/crates/gaze-mcp-core)
[![docs.rs](https://docs.rs/gaze-mcp-core/badge.svg)](https://docs.rs/gaze-mcp-core)
[![License](https://img.shields.io/crates/l/gaze-mcp-core.svg)](https://github.com/EmpireTwo/gaze#license)

> **Adding `gaze-mcp-core` (v0.7.2) to your crate enables:** the transport-free
> MCP chokepoint runtime — `Tool` trait, `PiiEnvelope::dispatch`,
> `ToolRegistry`, `ManifestStore` / `AuthHook` / `SessionIdPolicy` plug-in
> points, and the optional `core-tools` agent-tier tool set.
> **Does NOT bring in:** an MCP transport. No stdio, no streamable HTTP, no
> JSON-RPC framing — this crate is transport-free by design.
> **For the rmcp transport sink, also add `gaze-mcp-rmcp`** (or implement
> `Frontend` yourself; see below).

### Scope

`gaze-mcp` enforces the chokepoint on the **data-source ↔ model** path. Any data flowing **from a source through an MCP tool to the model** passes through `PiiEnvelope::dispatch` and is redacted before the model sees it.

`gaze-mcp` **does not** cover the **user ↔ model** path. Pasted text, uploaded files, and screenshots in the agent host's chat UI reach the model unredacted. For that axis, see `gaze-proxy` (planned for v0.8 — multi-vendor reverse proxy supporting Anthropic, OpenAI, Gemini).

---

`gaze-mcp-core` is the transport-free runtime for Gaze's MCP chokepoint. It
exposes a `Tool` trait, a sealed `ToolCtx`, a `ToolRegistry`, the
`PiiEnvelope::dispatch` chokepoint, the `Frontend` / `DispatchHost`
plug-in points, the `ManifestStore` contract, and the `AuthHook` +
`SessionIdPolicy` policy surfaces. Transports (rmcp stdio/http, custom
JSON-RPC, …) live in sink crates that depend on this one.

The architectural rationale, per-axis tradeoffs, and the data-source vs
user-input scope split are summarized in the boundary statement above and
captured in the v0.7.0 CHANGELOG entry. The transport-free runtime ships
alongside the `gaze-mcp-rmcp` sink as of v0.7.0.

## What gets enforced

`PiiEnvelope::dispatch` runs every tool call through the same sealed
ordering — **redact args → manifest.begin → invoke → redact response →
manifest.finish (or fail) → return**. The ordering is hard-coded inside
the dispatcher; tools never see a path around it because:

1. `ToolCtx::new` is `pub(crate)`. The dispatcher is the only construction
   site for tool contexts. Verified by the trybuild compile-fail fixtures
   in `tests/ui/`.
2. `ToolCtx`'s fields are `pub(crate)` plus `#[non_exhaustive]`. External
   crates cannot construct one via struct literal or `..Default::default()`
   either.
3. The context's lifetime `'a` binds it to the dispatcher's stack frame —
   tools can't stash a reference past the call.
4. `ToolRegistry::register` only accepts types that implement the
   `Tool` trait. There is no `register_raw(Box<dyn Fn(JsonValue) -> JsonValue>)`
   escape hatch.

The combination is the type-level chokepoint guarantee adopters depend on.
The behavioral cousins — that the manifest store actually receives
`begin_call` before `invoke`, and `finish_call` / `fail_call` before the
response escapes — are checked in `tests/chokepoint_ordering.rs`.

Snapshot refs use `sha256(audit_session_id || 0x00 || call_id || 0x00 ||
payload_bytes)`. This is an integrity marker for audit lookup, not a secret
commitment scheme. Threat model: anyone who can read audit rows and guess the
exact response payload can verify that guess offline. Current v0.7.x stable accepts
that risk because audit readers are already trusted with manifest metadata and
operator-tier deployments must protect snapshot storage. If that trust boundary
changes, replace the hash input with keyed HMAC material owned by the
`ManifestStore` implementation.

## Session ownership boundary

> ⚠️ **One `gaze::Session` per authorization boundary.** A `Session`
> tracks the entire token↔raw map for its lifetime in a single
> `DashMap` (`crates/gaze/src/session.rs:299-304`). The operator-tier
> tool `export_session_tokens` (Tool 3) dumps that **entire** map in
> the clear — every token from every conversation that ever shared the
> Session is exposed in one call. There is no per-call provenance
> filter on `Session::tokens()` in v0.7.
>
> If your host process serves multiple agents or users concurrently,
> your code MUST construct a fresh `Session` per authorization domain
> and supply it to `PiiEnvelope::new`. Sharing a single `Session`
> across conversations is a critical leak: agent B's operator-tier
> principal calling `export_session_tokens` reads agent A's PII
> inventory.
>
> Type-level enforcement of this boundary is deferred to v0.8.

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

## Adopter quickstart

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
let pipeline = gaze::Pipeline::builder().build().expect("pipeline");
let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");

// 4. Register tools.
let mut registry = ToolRegistry::new();
# #[cfg(feature = "core-tools")]
registry.register(gaze_mcp_core::core_tools::CleanTool::new()).unwrap();

// 5. Build the envelope; pass to the transport via `Frontend::serve`.
let manifest = MyManifest {};
let auth = MyAuth;
let policy = SessionIdPolicy::default_strict();
let _envelope = PiiEnvelope::new(&registry, &auth, &manifest, &pipeline, &session, &[], &policy);
```

The transport sink (e.g. `gaze-mcp-rmcp::RmcpFrontend`) wraps the envelope
behind the `DispatchHost` trait and calls `Frontend::serve` from the
adopter's tokio runtime.

## Implementing your own `Frontend`

Adopters who do not want rmcp implement [`Frontend`] themselves. The
contract is one method (`serve(self, host, shutdown) -> Result<(), FrontendError>`)
that drives the transport's accept-and-dispatch loop until
[`ShutdownToken::cancel`] fires. The host (`Arc<dyn DispatchHost>`)
wraps the envelope behind a narrow surface (`dispatch` + `list_tools`)
so the transport never sees the gaze pipeline, the gaze session, or the
manifest store directly.

A reference adapter for rmcp's `tools/list` + `tools/call` shape lives in
`crates/gaze-mcp-rmcp` (companion PR).

## Cargo features

| Feature | Default | Adds |
|---|---|---|
| `core-tools` | yes | `core_tools::{CleanTool, TokenizeFieldTool, SafetyNetCheckTool}` registrations. |
| `operator-tier` | no | `operator_tools::{RestoreTool, RestoreStrictTool, ExportSessionTokensTool}`. Tools route through `AuthHook::authorize_operator`. |

The `operator-tier` feature is opt-in. Default builds expose only the
agent surface, so an adopter who skips wiring auth hits
`AuthError::MissingHook` from `DenyAllAuthHook` instead of accidentally
exposing restore.

## Open items

- `gaze-mcp-rmcp` ships alongside this crate as the reference rmcp
  transport sink.
- `gaze-proxy` (the user-input axis sibling, multi-vendor LLM API
  reverse proxy) is planned for v0.8 — see the `## Scope` boundary
  statement at the top of this README.

See [`docs/architecture/mcp-runtime.md`](../../docs/architecture/mcp-runtime.md)
for the full chokepoint contract + audit-row schema + threat model.
