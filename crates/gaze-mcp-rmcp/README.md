# gaze-mcp-rmcp

### Scope

`gaze-mcp` enforces the chokepoint on the **data-source ↔ model** path. Any data flowing **from a source through an MCP tool to the model** passes through `PiiEnvelope::dispatch` and is redacted before the model sees it.

`gaze-mcp` **does not** cover the **user ↔ model** path. Pasted text, uploaded files, and screenshots in the agent host's chat UI reach the model unredacted. For that axis, see `gaze-proxy` (planned for v0.8 — multi-vendor reverse proxy supporting Anthropic, OpenAI, Gemini).

---

`gaze-mcp-rmcp` is the [rmcp](https://crates.io/crates/rmcp) (MCP Rust SDK) transport sink for [`gaze-mcp-core`]. It exposes an `RmcpFrontend` that implements `gaze_mcp_core::Frontend`, wiring rmcp's `tools/list` and `tools/call` flow through the chokepoint runtime.

## Status

**v0.7 — held.** This crate is being authored against the v0.7 release window and is not yet published. See [verdict scratchpad 1453](https://github.com/EmpireTwo/gaze) and the per-phase implementation plan for the contract and rollout sequence.

## Transports

- `transport-stdio` (default) — MCP over stdio, the standard agent-host integration path.
- `transport-http` (opt-in feature) — MCP over HTTP, for hosts that prefer a network transport.

## Stability disclaimer

This crate re-exports types from `rmcp 0.2`. **No SemVer guarantee** is offered on rmcp re-exports — major rmcp bumps (e.g. 0.2 → 0.3) may force a breaking release of `gaze-mcp-rmcp` even within otherwise SemVer-stable Gaze cycles.

## Pairing with `gaze-mcp-core`

Adopters integrate `gaze-mcp-rmcp` together with `gaze-mcp-core`: build a `PiiEnvelope` (chokepoint runtime) from core, register tools via core's `ToolRegistry`, then hand the `DispatchHost` to `RmcpFrontend::serve` for transport. The Frontend never sees envelope internals — it only sees the `DispatchHost` trait.

[`gaze-mcp-core`]: https://crates.io/crates/gaze-mcp-core
