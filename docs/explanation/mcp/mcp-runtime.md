# gaze-mcp runtime architecture

### Scope

`gaze-mcp` enforces the chokepoint on the **data-source ↔ model** path. Any data flowing **from a source through an MCP tool to the model** passes through `PiiEnvelope::dispatch` and is redacted before the model sees it.

`gaze-mcp` **does not** cover the **user ↔ model** path. Pasted text, uploaded files, and screenshots in the agent host's chat UI reach the model unredacted. For that axis, see `gaze-proxy` (planned for v0.8 — multi-vendor reverse proxy supporting Anthropic, OpenAI, Gemini).

---

This document specifies the runtime contract `gaze-mcp-core` v0.7 ships and
the threat model adopters can rely on. It is the source of truth for the
type-level chokepoint guarantees. The architectural rationale, per-axis
trade-offs, and scope split between `gaze-mcp` (model↔source) and `gaze-proxy`
(user↔model) are summarized in the boundary statement above and in the v0.7.0
CHANGELOG entry.

## The chokepoint

The chokepoint is `PiiEnvelope::dispatch` in
[`crates/gaze-mcp-core/src/dispatch.rs`](../../../crates/gaze-mcp-core/src/dispatch.rs).
Every tool call traverses this sequence in order:

| Step | Action | Failure mode |
|---|---|---|
| 1 | Validate transport-supplied session id via `SessionIdPolicy` | `DispatchError::SessionId`; **no manifest row** |
| 2 | Look up tool in `ToolRegistry` | `DispatchError::UnknownTool`; **no manifest row** |
| 3 | Authorize via `AuthHook::authorize_agent` or `_operator` (driven by `ToolDescriptor::tier`) | `DispatchError::Auth`; **no manifest row** |
| 4 | Redact raw args via `gaze::Pipeline::redact` (string leaves) | `DispatchError::Redaction`; **no manifest row** |
| 5 | `ManifestStore::begin_call(BeginCallContext)` | `DispatchError::Manifest`; **no manifest row written** |
| 6 | Build the sealed `ToolCtx` (only construction site in the crate) | — |
| 7 | `Tool::invoke(&ctx).await` | `DispatchError::ToolError`; **fail_call written first** |
| 8 | Redact response payload | `DispatchError::Redaction`; **fail_call written first** |
| 9 | Compute out-of-row `SnapshotRef` over redacted bytes | `DispatchError::ResponseSerialization`; **fail_call written first** |
| 10 | `ManifestStore::finish_call(handle, snapshot)` | `DispatchError::Manifest` |
| — | Return redacted response | — |

The first three steps are pre-manifest by design: a denied request leaves
no audit-log noise. Once `begin_call` returns Ok, the dispatcher
guarantees one of `finish_call` or `fail_call` runs before the function
returns. There is no third path. The `tests/chokepoint_ordering.rs`
golden tests assert the contract.

## Type-level seal

The chokepoint guarantee depends on tools being unable to fabricate or
reuse a `ToolCtx` outside `dispatch`. The seal has four layers:

1. **`pub(crate) fn ToolCtx::new`.** External crates cannot call the
   constructor. Verified at compile time by
   `crates/gaze-mcp-core/tests/ui/tool_ctx_no_external_constructor.rs`.
2. **`pub(crate)` fields + `#[non_exhaustive]`.** External crates cannot
   build one via struct literal either. Verified by
   `tests/ui/tool_ctx_no_struct_literal.rs`.
3. **Lifetime binding `ToolCtx<'a>`.** The dispatcher's stack frame owns
   the borrowed pieces (`&'a str` for tool name, principal id, audit
   session id; `serde_json::Value` for redacted args). Tools cannot
   stash the context across the call boundary.
4. **`ToolRegistry::register<T: Tool + 'static>(t)`.** Closures cannot
   masquerade as tools — registration only accepts types that implement
   the `Tool` trait. The trait's only methods (`descriptor`, `invoke`)
   take a `&ToolCtx<'_>` they can't recreate.

## Agent vs operator tier

Tools carry a `ToolDescriptor::tier` enum (`Agent` | `Operator`). The
dispatcher reads this per call:

- `ToolTier::Agent` → `AuthHook::authorize_agent(principal, tool_name)`
- `ToolTier::Operator` → `AuthHook::authorize_operator(principal, tool_name)`

The `operator-tier` Cargo feature controls whether the built-in
operator-tier tools (`RestoreTool`, `RestoreStrictTool`,
`ExportManifestTool`) are linked at all. Default builds expose only the
agent surface. The `mcp-tier-isolation` xtask gate
([`crates/xtask/src/mcp_tier_isolation.rs`](../../../crates/xtask/src/mcp_tier_isolation.rs))
runs the `tier_isolation` integration test under four feature graphs to
verify the partitioning holds.

The `gaze_dylint` protected-path lint
([`lint/dylint`](../../../lint/dylint)) lists `crates/gaze-mcp-core/src` so
any future change attempting to pull `gaze_audit::*` (or other
`forbidden_items`) into the chokepoint runtime is rejected at build time.

## rmcp transport sink

`gaze-mcp-rmcp` is the rmcp-backed transport adapter for this runtime. It
implements `Frontend` as `RmcpFrontend` and translates only wire-level objects:

- `ToolDescriptor` -> rmcp `Tool` for `tools/list`.
- rmcp `CallToolRequestParam` -> `(tool_name, raw_args, external_session_id)`.
- `ToolResponse` -> rmcp `CallToolResult`.

The adapter never receives `PiiEnvelope` internals. It sees only
`Arc<dyn DispatchHost>`, so every `tools/call` request still returns through
`PiiEnvelope::dispatch` when adopters use the core host wrapper. The rmcp smoke
tests exercise this via an in-process rmcp client/server transport, and the
manifest-persistence test routes through a real `PiiEnvelope` with a failing
`ManifestStore::finish_call`; the client receives an error result instead of
the tool payload.

Transports:

- `transport-stdio` (default): process stdio, standard for local agent hosts.
- `transport-http`: rmcp streamable HTTP served via axum at `/mcp`.

`PrincipalResolver` is adopter-supplied and maps rmcp request context to
`Principal`. `FixedPrincipalResolver` exists for local stdio servers and tests.
The adapter filters operator-tier tool descriptors from `tools/list` unless the
resolved principal has the `operator` role, but this is not the authorization
boundary. Authorization still happens inside `PiiEnvelope::dispatch` through
`AuthHook`.

rmcp 0.2 has no dedicated Gaze session-id carrier, so the adapter reserves a
top-level `_session_id` argument key. It removes that key before dispatch and
passes the value as `external_session_id`; `SessionIdPolicy` validates it before
the manifest opens.

The `mcp-tier-isolation` xtask gate covers `gaze-mcp-rmcp` under
`transport-stdio`, `transport-stdio,transport-http`, and `--no-default-features`
graphs so transport feature changes do not accidentally pull in an operator
surface by default.

## Manifest contract

`ManifestStore` (in [`crates/gaze-mcp-core/src/manifest.rs`](../../../crates/gaze-mcp-core/src/manifest.rs))
is async + Send + Sync and has three methods:

- `begin_call(ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError>`
- `finish_call(handle, snapshot: SnapshotRef) -> Result<(), ManifestError>`
- `fail_call(handle, reason: FailureReason) -> Result<(), ManifestError>`

`SnapshotRef` is intentionally **out-of-row metadata only**: locator
string, sha256 hex, byte length. The dispatcher never writes response
bytes to a side store. Adopters who want byte-level persistence wrap
their `ManifestStore` impl with their own snapshot store and persist
before calling `finish_call`. Inline blobs were rejected during design
review because they would defeat encrypted-volume threat models that
keep response payloads on adopter-controlled storage.

External session id binding (lens's
`lens_session_id`/`gaze_audit_session_id` pair, gaze-cli's audit ulid)
lives in the `ManifestStore` impl constructor — the trait is generic.

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

| Threat | Mitigation |
|---|---|
| Tool reads PII from data source and returns it raw | Step 8 redacts response before `finish_call`; response cannot escape until persisted |
| Adopter forgets to wire auth | `DenyAllAuthHook` is the default fail-closed policy; `MissingHook` lands in the audit log |
| Tool fabricates a `ToolCtx` to bypass the redaction step | Type-level seal (4 layers above) makes construction unrepresentable |
| Tool stashes a `ToolCtx` across calls | Borrow checker rejects: lifetime `'a` is the dispatcher frame |
| Closure registers as a tool to skip the trait | `ToolRegistry::register<T: Tool>` only — no `register_raw` |
| Restore exposed by default | `operator-tier` Cargo feature is opt-in; default builds don't link the symbols |
| Operator surface lit up without auth | `authorize_operator` is the only path; `DenyAllAuthHook` returns `MissingHook` |
| Audit-sink coupling drift in chokepoint | `cargo-metadata-audit-isolation` xtask + dylint protected-path lint both reject `gaze_audit::*` from gaze-mcp-core |
| **User pastes PII into chat UI** | **Out of scope** — `gaze-proxy` (v0.8) covers user→model axis. See top-of-doc boundary statement. |

## Out of scope

The boundary statement at the top of this document is mandatory in every
README + architecture doc the gaze-mcp project ships. The user-input axis
(paste-into-chat, multimodal uploads, screenshots) is **not** covered by
gaze-mcp because:

- MCP tools are **model-callable**, not pre-input filters. The user's
  bytes reach the LLM service before the model decides to call any tool.
- Under GDPR Art. 4(2), receipt by the LLM service is processing.
- Pre-input filtering needs a different mechanism (host-side
  preprocessor, vendor-agnostic API reverse proxy, or workflow
  discipline) — `gaze-proxy` v0.8.

The v0.7.0 CHANGELOG entry captures the model↔source vs user↔model
split and the v0.8 follow-up.
