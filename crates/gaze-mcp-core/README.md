# gaze-mcp-core

### Scope

`gaze-mcp` enforces the chokepoint on the **data-source ↔ model** path. Any data flowing **from a source through an MCP tool to the model** passes through `PiiEnvelope::dispatch` and is redacted before the model sees it.

`gaze-mcp` **does not** cover the **user ↔ model** path. Pasted text, uploaded files, and screenshots in the agent host's chat UI reach the model unredacted. For that axis, see `gaze-proxy` (planned for v0.8 — multi-vendor reverse proxy supporting Anthropic, OpenAI, Gemini).

---

`gaze-mcp-core` is the transport-free runtime for Gaze's MCP chokepoint. It
exposes a `Tool` trait, a sealed `ToolCtx`, a `ToolRegistry`, the
`PiiEnvelope::dispatch` chokepoint, the `Frontend` / `DispatchHost` plug-in
points, the `ManifestStore` contract (lifted byte-identical from
`gaze::session::manifest`), and the `AuthHook` + `SessionIdPolicy` policy
surfaces. Transports (rmcp stdio/http, custom) live in sink crates that depend
on this one.

The architectural rationale and per-axis tradeoffs are locked in scratchpad
1453 (`brainstorm-gaze-mcp-crate-2026-05-08`); the implementation plan is
scratchpad 1468.

> **DO NOT MERGE.** This crate is held until the v0.7 release window.
> Adopter quickstart and the full API guide land in the docs commit before the
> PR moves to ready-for-review.
