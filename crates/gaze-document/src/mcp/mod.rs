//! MCP tool adapters for `gaze-document`.
//!
//! Tools in this module are opt-in via the `mcp` feature and must be
//! registered explicitly by adopters. Invocation still happens through
//! `gaze_mcp_core::PiiEnvelope::dispatch`; this module only provides the
//! tool bodies and catalog metadata.
