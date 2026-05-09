//! `restore_strict` operator-tier tool. Like `restore` but rejects partial
//! restorations — every token in the input string must be in the manifest
//! or the call fails.
//!
//! v0.1 scope: stub body. See [`crate::tools::restore`] for the full
//! deferred-design rationale.

use async_trait::async_trait;
use serde_json::json;

use crate::ctx::ToolCtx;
use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResponse};

/// `restore_strict` operator-tier tool. See module docs.
#[derive(Debug)]
pub struct RestoreStrictTool {
    descriptor: ToolDescriptor,
}

impl RestoreStrictTool {
    /// Construct a `RestoreStrictTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::operator(
                "restore_strict",
                json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Text containing tokens to restore." }
                    },
                    "required": ["text"]
                }),
            )
            .with_description(
                "Operator-only: strict restore that fails if any token is missing.",
            ),
        }
    }
}

impl Default for RestoreStrictTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RestoreStrictTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, _ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        Err(ToolError::Internal(Box::new(NotYetImplementedError(
            "restore_strict tool body lands before v0.7 release",
        ))))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct NotYetImplementedError(&'static str);
