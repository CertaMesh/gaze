//! # gaze-mcp-core
//!
//! ### Scope
//!
//! `gaze-mcp` enforces the chokepoint on the **data-source ↔ model** path. Any data
//! flowing **from a source through an MCP tool to the model** passes through
//! `PiiEnvelope::dispatch` and is redacted before the model sees it.
//!
//! `gaze-mcp` **does not** cover the **user ↔ model** path. Pasted text, uploaded
//! files, and screenshots in the agent host's chat UI reach the model unredacted.
//! For that axis, see `gaze-proxy` (planned for v0.8 — multi-vendor reverse proxy
//! supporting Anthropic, OpenAI, Gemini).
//!
//! ---
//!
//! Transport-free MCP-shaped runtime that enforces the Gaze chokepoint contract:
//! every tool dispatch flows redact → manifest → invoke → redact → persist → return.
//! The ordering is hard-coded inside [`PiiEnvelope::dispatch`] (the only construction
//! site for [`ToolCtx`]) so a tool implementation cannot fabricate a context, escape
//! the manifest, or return a response without a persisted audit trail.
//!
//! Transports plug in via the [`Frontend`] trait. The companion crate
//! `gaze-mcp-rmcp` ships an [`rmcp`](https://docs.rs/rmcp) implementation;
//! adopters who want a different transport implement [`Frontend`] themselves.
//!
//! See `docs/architecture/mcp-runtime.md` and the verdict in scratchpad 1453
//! (`brainstorm-gaze-mcp-crate-2026-05-08`) for the architectural rationale.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod auth;
pub mod ctx;
pub mod dispatch;
pub mod frontend;
pub mod manifest;
pub mod registry;
pub mod session_id;
pub mod tool;

pub use crate::auth::{AuthError, AuthHook, DenyAllAuthHook, Principal};
pub use crate::ctx::{SessionHandle, ToolCtx};
pub use crate::dispatch::{DispatchError, PiiEnvelope};
pub use crate::frontend::{DispatchHost, Frontend, FrontendError, ShutdownToken};
pub use crate::manifest::{
    BeginCallContext, CallHandle, FailureReason, ManifestError, ManifestStore, SnapshotRef,
};
pub use crate::registry::{ToolRegistry, ToolRegistryError};
pub use crate::session_id::{SessionIdError, SessionIdFormat, SessionIdPolicy};
pub use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResponse, ToolTier};
