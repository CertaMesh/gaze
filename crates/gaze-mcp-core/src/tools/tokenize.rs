//! `tokenize_field` agent-tier tool. Takes a `{value: string}` arg and
//! returns the redacted value under the `token` key.

use async_trait::async_trait;
use serde_json::json;

use crate::ctx::ToolCtx;
use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResponse};

/// `tokenize_field` agent-tier tool. See module docs.
#[derive(Debug)]
pub struct TokenizeFieldTool {
    descriptor: ToolDescriptor,
}

impl TokenizeFieldTool {
    /// Construct a `TokenizeFieldTool` with its canonical descriptor.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::agent(
                "tokenize_field",
                json!({
                    "type": "object",
                    "properties": {
                        "value": { "type": "string", "description": "Value to tokenize." }
                    },
                    "required": ["value"]
                }),
            )
            .with_description("Tokenize a single field value via the gaze pipeline."),
        }
    }
}

impl Default for TokenizeFieldTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TokenizeFieldTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        let token = ctx
            .redacted_args()
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing required field `value`".into()))?
            .to_string();
        Ok(ToolResponse::json(json!({ "token": token })))
    }
}
