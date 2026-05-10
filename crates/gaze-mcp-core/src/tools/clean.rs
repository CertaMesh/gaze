//! `clean` agent-tier tool. Echoes back the redacted args as the canonical
//! "redact this and give me the tokens" tool a model can call.
//!
//! ## Why this is mostly an echo
//!
//! `PiiEnvelope::dispatch` redacts args BEFORE the tool body sees them, so
//! by the time `CleanTool::invoke` runs the args are already tokenized.
//! `clean` therefore simply mirrors the redacted args back as its response;
//! the dispatcher will then redact the response again (a second pass over
//! already-tokenized text is a no-op for axis-1 / axis-2 correctness, and
//! gaze's tokenization is idempotent on already-tokenized strings).
//!
//! Adopters who want a non-echo `clean` (e.g., one that reads bytes from
//! disk and returns tokenized output) implement [`crate::tool::Tool`]
//! themselves; the chokepoint contract is the same either way.

use async_trait::async_trait;
use serde_json::json;

use crate::ctx::ToolCtx;
use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResponse};

/// `clean` agent-tier tool. See module docs.
#[derive(Debug)]
#[non_exhaustive]
pub struct CleanTool {
    descriptor: ToolDescriptor,
}

impl CleanTool {
    /// Construct a `CleanTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::agent(
                "clean",
                json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Text to redact." }
                    },
                    "required": ["text"]
                }),
            )
            .with_description("Redact PII in the supplied text via the gaze pipeline."),
        }
    }
}

impl Default for CleanTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CleanTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        // Args are already redacted by the dispatcher. Mirror them back.
        Ok(ToolResponse::json(ctx.redacted_args().clone()))
    }
}
