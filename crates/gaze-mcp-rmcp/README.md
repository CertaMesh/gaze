# gaze-mcp-rmcp

[![Crates.io](https://img.shields.io/crates/v/gaze-mcp-rmcp.svg)](https://crates.io/crates/gaze-mcp-rmcp)
[![docs.rs](https://docs.rs/gaze-mcp-rmcp/badge.svg)](https://docs.rs/gaze-mcp-rmcp)
[![License](https://img.shields.io/crates/l/gaze-mcp-rmcp.svg)](https://github.com/EmpireTwo/gaze#license)

### Scope

`gaze-mcp` enforces the chokepoint on the **data-source ↔ model** path. Any data flowing **from a source through an MCP tool to the model** passes through `PiiEnvelope::dispatch` and is redacted before the model sees it.

`gaze-mcp` **does not** cover the **user ↔ model** path. Pasted text, uploaded files, and screenshots in the agent host's chat UI reach the model unredacted. For that axis, use `gaze-proxy`, the v0.8+ multi-vendor reverse proxy supporting OpenAI, Anthropic, and Gemini SDK base-URL swaps.

`gaze-mcp-rmcp` is the [rmcp](https://crates.io/crates/rmcp) transport sink for [`gaze-mcp-core`]. It exposes `RmcpFrontend`, a `gaze_mcp_core::Frontend` implementation that wires rmcp `tools/list` and `tools/call` requests to a `DispatchHost`.

## Feature Flags

- `transport-stdio` (default): MCP over process stdio, the standard agent-host integration path.
- `transport-http`: MCP streamable HTTP via rmcp + axum at `/mcp`.

`transport-http` is opt-in because it pulls HTTP server dependencies. `transport-stdio` remains the default path for local agent hosts.

## Quickstart

```toml
[dependencies]
gaze-mcp-core = "0.11.0"
gaze-mcp-rmcp = "0.11.0"
```

```rust
use std::sync::Arc;

use gaze_mcp_core::{Frontend, Principal};
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};

# async fn run(host: Arc<dyn gaze_mcp_core::DispatchHost>) -> Result<(), gaze_mcp_core::FrontendError> {
let frontend = RmcpFrontend::stdio(Arc::new(FixedPrincipalResolver::new(
    Principal::new("local-agent"),
)));

frontend
    .serve(host, gaze_mcp_core::ShutdownToken::new())
    .await?;
# Ok(())
# }
```

Most adopters build `host` by wrapping `gaze_mcp_core::PiiEnvelope`: configure the Gaze pipeline/session, register tools in `ToolRegistry`, implement `ManifestStore`, then pass that dispatch host to `RmcpFrontend::serve`.

## Principal Resolution

`PrincipalResolver` maps rmcp request context to `gaze_mcp_core::Principal`. For local stdio servers, `FixedPrincipalResolver` is enough. HTTP adopters should usually supply their own resolver that reads authenticated request context and emits stable principal ids plus roles.

`RmcpFrontend` treats the `operator` role specially for `tools/list`: operator-tier descriptors are only advertised to principals with `principal.has_role("operator")`. Invocation authorization still happens inside `PiiEnvelope::dispatch` through `AuthHook`; transport-side filtering is only a catalog convenience.

## Session Ids

rmcp 0.2's `tools/call` payload has no native Gaze session-id field. This adapter reserves a top-level `_session_id` argument key:

```json
{
  "text": "hello",
  "_session_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"
}
```

The adapter removes `_session_id` before dispatching tool args and passes it as `external_session_id`. `gaze-mcp-core` validates it with `SessionIdPolicy` before any manifest row opens.

## rmcp Version Policy

This crate builds on `rmcp 1.x`. There is **no SemVer guarantee** on rmcp re-exports or transport internals; major rmcp bumps may force breaking releases of `gaze-mcp-rmcp` even within otherwise stable Gaze cycles.

[`gaze-mcp-core`]: https://crates.io/crates/gaze-mcp-core
