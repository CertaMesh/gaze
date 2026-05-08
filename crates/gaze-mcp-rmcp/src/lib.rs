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
//! `gaze-mcp-rmcp` is the rmcp (MCP Rust SDK) transport sink for [`gaze-mcp-core`]. It will
//! expose a `RmcpFrontend` implementing `gaze_mcp_core::Frontend`, wiring rmcp's
//! `tools/list` and `tools/call` flow through the chokepoint runtime.
//!
//! ## Phase status
//!
//! - **Phase 1 — scaffold + workspace wiring.** Done.
//! - **Phase 2 — adapter layer.** [`adapter`] translates between rmcp wire types and
//!   `gaze_mcp_core` contracts (descriptors, args, responses, errors).
//! - **Phase 3 — `RmcpFrontend` impl.** Pending. Requires `gaze_mcp_core::Frontend` +
//!   `DispatchHost` traits (core's Phase 7) before it can compile.
//!
//! ## Stability
//!
//! This crate re-exports types from `rmcp 0.2`. There is **no SemVer guarantee** on rmcp
//! re-exports — major rmcp bumps may force breaking releases of `gaze-mcp-rmcp`.
//!
//! [`gaze-mcp-core`]: https://crates.io/crates/gaze-mcp-core

pub mod adapter;
pub mod error;

pub use crate::error::RmcpFrontendError;
