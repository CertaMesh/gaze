//! Default tools shipped with gaze-mcp-core. Opt-in via Cargo features.
//!
//! ## Feature gating
//!
//! - `core-tools` (default-on) exposes [`clean::CleanTool`],
//!   [`tokenize::TokenizeFieldTool`], [`safety_net::SafetyNetCheckTool`].
//!   These are agent-tier tools that demonstrate the chokepoint pattern
//!   against existing gaze pipeline functionality.
//! - `operator-tier` (opt-in) exposes [`restore::RestoreTool`],
//!   [`restore_strict::RestoreStrictTool`], [`export::ExportManifestTool`].
//!   These are operator-tier tools gated by
//!   [`crate::auth::AuthHook::authorize_operator`]; default builds intentionally
//!   do NOT link them so an adopter who skips wiring auth cannot accidentally
//!   expose restore.
//!
//! ## v0.1 scope
//!
//! The operator-tier tool bodies in this crate are stubs that return
//! [`crate::tool::ToolError::Internal`] with class `"not-yet-implemented"`.
//! Their public-API surface (descriptors, tier, feature gating, auth-hook
//! routing) is wired and tested by the Phase 9 fixtures; the bodies will
//! land in a follow-up PR before v0.7 release. Adopters who need a
//! production restore path TODAY can implement [`crate::tool::Tool`]
//! themselves and register against [`crate::registry::ToolRegistry`] —
//! these defaults are convenience scaffolding, not the only path.

#[cfg(feature = "core-tools")]
pub mod clean;
#[cfg(feature = "core-tools")]
pub mod safety_net;
#[cfg(feature = "core-tools")]
pub mod tokenize;

// operator-tier modules land in commit 8b.
