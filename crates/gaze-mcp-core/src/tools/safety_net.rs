//! `safety_net_check` agent-tier tool. Reports whether the redacted args
//! contain any residual PII the gaze safety-net pass-3 detected.
//!
//! ## v0.1 scope
//!
//! The body is a stub that returns `{ "leaks": [], "ok": true }`. Wiring
//! the real `gaze::Pipeline::clean_with_safety_net` path through a tool body
//! requires deciding how the gaze pipeline's safety-net configuration is
//! threaded through the dispatcher (currently the dispatcher only calls
//! `Pipeline::redact`, not the safety-net variant). That decision is
//! deferred to a follow-up PR; the tool's descriptor + tier wiring is in
//! place so adopters can register it today and pick up the upgraded body
//! when it lands.

use async_trait::async_trait;
use serde_json::json;

use crate::ctx::ToolCtx;
use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResponse};

/// `safety_net_check` agent-tier tool. See module docs.
#[derive(Debug)]
pub struct SafetyNetCheckTool {
    descriptor: ToolDescriptor,
}

impl SafetyNetCheckTool {
    /// Construct a `SafetyNetCheckTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::agent(
                "safety_net_check",
                json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Text to scan." }
                    },
                    "required": ["text"]
                }),
            )
            .with_description("Run the gaze safety-net pass against text and report residual PII."),
        }
    }
}

impl Default for SafetyNetCheckTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SafetyNetCheckTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, _ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        // v0.1 stub — see module docs.
        Ok(ToolResponse::json(json!({
            "ok": true,
            "leaks": [],
            "note": "v0.1 stub; full safety-net wiring lands before v0.7 release."
        })))
    }
}
