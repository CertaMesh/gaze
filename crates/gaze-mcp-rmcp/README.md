# gaze-mcp-rmcp

[![Crates.io](https://img.shields.io/crates/v/gaze-mcp-rmcp.svg)](https://crates.io/crates/gaze-mcp-rmcp)
[![docs.rs](https://docs.rs/gaze-mcp-rmcp/badge.svg)](https://docs.rs/gaze-mcp-rmcp)
[![License](https://img.shields.io/crates/l/gaze-mcp-rmcp.svg)](https://github.com/CertaMesh/gaze#license)

`gaze-mcp-rmcp` is the [rmcp](https://crates.io/crates/rmcp) transport sink for [`gaze-mcp-core`]. It exposes `RmcpFrontend`, a `gaze_mcp_core::Frontend` implementation that wires rmcp `tools/list` and `tools/call` requests to a `DispatchHost`.

## Scope

Agent-tier source data passes through `PiiEnvelope::dispatch` before reaching the model. Authorized operator tools can return raw restore/export results only on the operator surface. Chat pastes, uploads, and screenshots require `gaze-proxy`. See the [core boundary](../gaze-mcp-core/README.md#scope).

## Quickstart

```toml
[dependencies]
gaze-mcp-core = "0.17.0"
gaze-mcp-rmcp = "0.17.0"
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

## Feature flags

- `transport-stdio` (default): MCP over process stdio, the standard agent-host integration path.
- `transport-http`: MCP streamable HTTP via rmcp + axum at `/mcp`.

## Tool errors

Tool failures return `isError: true` with a single text frame containing only
`ToolError::class()`: `invalid-args`, `not-found`, `limit-exceeded`,
`backend-unavailable`, `backend-failure`, or `internal`. Error payloads and
source messages are never included, including for future error variants.

Operation expansion-budget refusals return `isError: true` with the single text
class `resource_limit`, without input values or internal details.

Detailed typed errors and manifest records remain on the trusted side for
caller control flow and operator diagnostics. They may contain unredacted PII
and must never be serialized into model-facing transport responses. Manifest
persistence still completes before a tool result is returned; persistence
failure takes precedence over the tool error.

## Principal resolution

`PrincipalResolver` maps rmcp request context to `gaze_mcp_core::Principal`. For local stdio servers, `FixedPrincipalResolver` is enough. HTTP adopters should usually supply their own resolver that reads authenticated request context and emits stable principal ids plus roles.

`RmcpFrontend` treats the `operator` role specially for `tools/list`: operator-tier descriptors are only advertised to principals with `principal.has_role("operator")`. Invocation authorization still happens inside `PiiEnvelope::dispatch` through `AuthHook`; transport-side filtering is only a catalog convenience.

## Host context and session ids

`HostContextSelection` is an explicit input to `DispatchHost::dispatch` and
`PiiEnvelope::dispatch`. A trusted embedder selects one owner record after the
model plans a call. `HostContextSelection::present` accepts the original context
JSON. Dispatch uses `gaze::Context::from_json_str`, with the same schema and limits
as `gaze clean --context-json`. The host supplies the resident assembly inputs
through `PiiEnvelope::with_context_policy`. A host without that configuration
rejects `Present`.

For embedded HTTP, host middleware inserts `HostCallContext` into
`axum::http::request::Parts::extensions`. rmcp carries these server-local request
parts into the handler. Build your `StreamableHttpService` with
`RmcpFrontend::into_server_handler` to install the middleware on your own router.
`HostCallContext::selection` supplies the context for that call;
`audit_session_id` supplies optional host-owned audit correlation.

Context protects tool responses. A context value already present in model-authored
arguments is excluded for that call.

Calls with `Present` context have a per-call guess budget, independent of the
record. `HostContextPolicy::default()` allows at most 4,096 argument bytes and
32 scalar values in each original and recursively decoded JSON view. Keys count
toward bytes. Both raw and normalized bytes must fit. Dispatch rejects oversized
arguments with a value-free error before parsing the record or changing session
or audit state. Set `PiiEnvelope::with_host_context_policy(HostContextPolicy {
max_argument_bytes, max_argument_scalars })` to configure these host-owned limits.
Raising either limit increases the guess budget. `Absent` calls use the ordinary
MCP operation limits.

Exclusion uses the detection pipeline's normalization, Unicode case folding,
whitespace collapse, and the response matcher's token-boundary rule. A superstring
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
leaves that value eligible for a token. The residual is bounded by the admitted
argument bytes and scalar count per call, rather than a fixed number of candidate
values. Enumeration that fits within that budget remains possible. Repeated calls
are the host's rate-limit responsibility. Exclusion does not provide general
resistance to guessing attacks. Ordinary recognizers and existing session mappings
continue to protect their known values.

Only values first matched in a response become Rule-evidence session entries for
later turns. Unmatched values are dropped. Restoration still uses the owner's manifest.

Context never enters `ToolCtx`, tool schemas, arguments, `_meta`, or manifest call
metadata. `Present` is rejected for an operator tool that bypasses response
protection. Stock stdio hosts have no private context channel. The CLI stdio server
uses `Absent`; an out-of-process adapter is not supported in this release.

The adapter removes the legacy `_session_id` argument and ignores its value.
It cannot change audit correlation or select a Session. Use host-owned
`HostCallContext::audit_session_id` for correlation. `SessionIdPolicy` validates
that id before dispatch; the id never grants access or selects a Session.

Private context requires one `Session` and one `DispatchHost` per conversation.
Sharing a host across MCP sessions shares Rule-evidence mappings, HMAC labels,
and restore authority. Audit session ids do not partition this state. The built-in
`Frontend::serve(host, ...)` HTTP path reuses the supplied host across MCP sessions.
For private conversations, embed the service and create the host inside its
per-MCP-session factory:

```rust,no_run
use std::sync::Arc;
use gaze_mcp_core::DispatchHost;
use gaze_mcp_rmcp::RmcpFrontend;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};

# #[cfg(feature = "transport-http")]
# fn embed_http(
# 	frontend: RmcpFrontend,
# 	new_conversation_host: impl Fn() -> std::io::Result<Arc<dyn DispatchHost>> + Send + Sync + 'static,
# ) {
let service = StreamableHttpService::new(
	move || {
		// Allocate a fresh Session and DispatchHost on every factory invocation.
		let host = new_conversation_host()?;
		Ok(frontend.clone().into_server_handler(host))
	},
	Arc::new(LocalSessionManager::default()),
	StreamableHttpServerConfig::default(),
);
# }
# fn main() {}
```

Enable `gaze-mcp-rmcp/transport-http` and add `rmcp` 2.x with
`transport-streamable-http-server` to use this example. Implement
`new_conversation_host` so each call constructs a new host with its own `Session`.
Resident policies, pipelines, and model handles can be shared. Install trusted
`HostCallContext` middleware on the router that serves this service. Authenticate
MCP session ownership before reusing a session across requests. On reconnect,
restore only that conversation's trusted snapshot into its host.

## rmcp version policy

This crate builds on `rmcp 2.x`. There is no SemVer guarantee on rmcp re-exports or transport internals; major rmcp bumps may force breaking releases of `gaze-mcp-rmcp` even within otherwise stable Gaze cycles.

[`gaze-mcp-core`]: https://crates.io/crates/gaze-mcp-core
