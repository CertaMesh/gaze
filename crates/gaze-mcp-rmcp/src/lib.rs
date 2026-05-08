//! # gaze-mcp-rmcp
//!
//! ### Scope
//!
//! `gaze-mcp` enforces the chokepoint on the **data-source ↔ model** path. Any data flowing
//! **from a source through an MCP tool to the model** passes through `PiiEnvelope::dispatch`
//! and is redacted before the model sees it.
//!
//! `gaze-mcp` **does not** cover the **user ↔ model** path. Pasted text, uploaded files, and
//! screenshots in the agent host's chat UI reach the model unredacted. For that axis, see
//! `gaze-proxy` (planned for v0.8 — multi-vendor reverse proxy supporting Anthropic, OpenAI,
//! Gemini).
//!
//! ---
//!
//! `gaze-mcp-rmcp` is the rmcp (MCP Rust SDK) transport sink for [`gaze-mcp-core`]. It
//! exposes a [`RmcpFrontend`] that implements `gaze_mcp_core::Frontend`, wiring rmcp's
//! `tools/list` and `tools/call` flow through the chokepoint runtime.
//!
//! ## Stability
//!
//! This crate re-exports types from `rmcp 0.2`. There is **no SemVer guarantee** on rmcp
//! re-exports — major rmcp bumps may force breaking releases of `gaze-mcp-rmcp`.
//!
//! [`gaze-mcp-core`]: https://crates.io/crates/gaze-mcp-core
//! [`RmcpFrontend`]: crate::RmcpFrontend

// Phase 1 scaffold: public surface is added in Phase 2+ once the branch is rebased
// onto feature/gaze-mcp-core. See scratchpad 1468 for the per-phase plan.
