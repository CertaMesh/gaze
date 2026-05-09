//! `restore` operator-tier tool. Inverts the manifest: takes a redacted
//! string and returns the original PII for an authenticated operator.
//!
//! ## v0.1 scope
//!
//! Body is a stub returning `ToolError::Internal` with class
//! `"not-yet-implemented"`. The descriptor + tier + auth-hook routing is
//! wired and exercised by the Phase 9 fixtures, so adopters can register
//! the tool today and the upgraded body lands in a follow-up before v0.7
//! release.
//!
//! ## Why restore is hard inside the chokepoint
//!
//! `PiiEnvelope::dispatch` redacts the tool's args BEFORE invoke. A naive
//! restore tool would receive `{ "token": "<re-redacted>" }` — pointless,
//! because the original token has been mapped to a fresh token by the
//! incoming pass. Production restore therefore requires either (a) a
//! dispatcher path that opts out of arg-redaction for operator-tier tools,
//! or (b) an out-of-band channel for the token-to-restore. Locking that
//! design is the deferred work; everything else (tier gating, auth-hook
//! routing, fail-closed manifest) is solid and tested.

use async_trait::async_trait;
use serde_json::json;

use crate::ctx::ToolCtx;
use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResponse};

/// `restore` operator-tier tool. See module docs.
#[derive(Debug)]
pub struct RestoreTool {
    descriptor: ToolDescriptor,
}

impl RestoreTool {
    /// Construct a `RestoreTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::operator(
                "restore",
                json!({
                    "type": "object",
                    "properties": {
                        "token": { "type": "string", "description": "Token to restore." }
                    },
                    "required": ["token"]
                }),
            )
            .with_description("Operator-only: restore PII from a manifest token."),
        }
    }
}

impl Default for RestoreTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RestoreTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, _ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        Err(ToolError::Internal(Box::new(NotYetImplementedError(
            "restore tool body lands before v0.7 release",
        ))))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct NotYetImplementedError(&'static str);
