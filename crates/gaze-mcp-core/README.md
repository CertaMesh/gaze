# gaze-mcp-core

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

The architectural rationale + per-axis tradeoffs are locked in scratchpad
1453 (`brainstorm-gaze-mcp-crate-2026-05-08`); the user-input vs
data-source axis correction is in scratchpad 1471 (`v0.7 + v0.8 scope
decision`); the implementation plan is scratchpad 1468.

> **DO NOT MERGE.** This crate is held until the v0.7 release window. The
> rmcp transport sink ships in the companion PR
> `feature/gaze-mcp-rmcp`.

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
let _envelope = PiiEnvelope::new(&registry, &auth, &manifest, &pipeline, &session, &policy);
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
| `operator-tier` | no | `operator_tools::{RestoreTool, RestoreStrictTool, ExportManifestTool}`. Tools route through `AuthHook::authorize_operator`. |

The `operator-tier` feature is opt-in. Default builds expose only the
agent surface, so an adopter who skips wiring auth hits
`AuthError::MissingHook` from `DenyAllAuthHook` instead of accidentally
exposing restore.

## Open items

- The default operator-tier tool bodies (`restore`, `restore_strict`,
  `export_manifest`) are v0.1 stubs that return `ToolError::Internal`
  with class `"not-yet-implemented"`. The dispatcher path through them
  (auth gating, fail-closed manifest persistence) is fully wired and
  tested; the bodies land in a follow-up before v0.7 release. Adopters
  who need a production restore path TODAY implement `Tool` themselves.
- `SafetyNetCheckTool` is a v0.1 stub for the same reason — wiring
  `gaze::Pipeline::clean_with_safety_net` requires a dispatcher-level
  decision about safety-net configuration that is deferred to a
  follow-up.
- `gaze-mcp-rmcp` ships in a companion PR (`feature/gaze-mcp-rmcp`).
  Both are held for v0.7.
- `gaze-proxy` (the user-input axis sibling, multi-vendor LLM API
  reverse proxy) is planned for v0.8 — see the `## Scope` boundary
  statement at the top of this README.

See [`docs/architecture/mcp-runtime.md`](../../docs/architecture/mcp-runtime.md)
for the full chokepoint contract + audit-row schema + threat model.
