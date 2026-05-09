//! `export_manifest` operator-tier tool. Returns a serializable snapshot of
//! the current manifest entries for the calling session.
//!
//! v0.1 scope: stub body. The real export path needs a `ManifestStore`
//! method to enumerate entries (the trait shipped in Phase 2 only writes
//! entries — it does not read). Adding `enumerate(...)` to the trait is a
//! breaking change that's deferred to the follow-up that lands the
//! production restore body.

use async_trait::async_trait;
use serde_json::json;

use crate::ctx::ToolCtx;
use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResponse};

/// `export_manifest` operator-tier tool. See module docs.
#[derive(Debug)]
pub struct ExportManifestTool {
    descriptor: ToolDescriptor,
}

impl ExportManifestTool {
    /// Construct an `ExportManifestTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::operator(
                "export_manifest",
                json!({
                    "type": "object",
                    "properties": {}
                }),
            )
            .with_description("Operator-only: dump the current session's manifest entries."),
        }
    }
}

impl Default for ExportManifestTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ExportManifestTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, _ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        Err(ToolError::Internal(Box::new(NotYetImplementedError(
            "export_manifest tool body lands before v0.7 release",
        ))))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct NotYetImplementedError(&'static str);
