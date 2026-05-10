//! Default tools shipped with gaze-mcp-core. Opt-in via Cargo features.
//!
//! ## Feature gating
//!
//! - `core-tools` (default-on) exposes [`clean::CleanTool`],
//!   [`tokenize::TokenizeFieldTool`], [`safety_net::SafetyNetCheckTool`].
//!   These are agent-tier tools that demonstrate the chokepoint pattern
//!   against existing gaze pipeline functionality.
//! - `operator-tier` (opt-in) exposes [`restore::RestoreTool`],
//!   [`restore_strict::RestoreStrictTool`], [`export::ExportSessionTokensTool`].
//!   These are operator-tier tools gated by
//!   [`crate::auth::AuthHook::authorize_operator`]; default builds intentionally
//!   do NOT link them so an adopter who skips wiring auth cannot accidentally
//!   expose restore.
//!
//! ## Shipped defaults
//!
//! The default tool bodies for both tiers are implemented. Agent-tier tools
//! clean, tokenize, and safety-net-check payloads through the pipeline
//! chokepoint. Operator-tier tools restore text, strictly restore text, and
//! export manifest inventory after [`crate::auth::AuthHook`] approval. These
//! defaults remain convenience implementations: adopters can still implement
//! [`crate::tool::Tool`] themselves and register against
//! [`crate::registry::ToolRegistry`] for host-specific behavior.

#[cfg(feature = "core-tools")]
pub mod clean;
#[cfg(feature = "core-tools")]
pub mod safety_net;
#[cfg(feature = "core-tools")]
pub mod tokenize;

#[cfg(feature = "operator-tier")]
pub mod export;
#[cfg(feature = "operator-tier")]
pub mod restore;
#[cfg(feature = "operator-tier")]
pub mod restore_strict;
