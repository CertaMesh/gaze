//! Error types surfaced by the rmcp frontend.

use thiserror::Error;

/// Errors emitted by the rmcp transport sink.
///
/// `RmcpFrontendError` covers wiring problems (transport bind, malformed
/// request payloads, registration conflicts) — not chokepoint outcomes.
/// Per-call PII / auth / dispatch errors are surfaced through
/// `gaze_mcp_core::ToolError` and translated into MCP `CallToolResult { is_error: true }`
/// payloads by [`crate::adapter::error_to_rmcp_call_tool_result`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RmcpFrontendError {
    /// The configured transport (stdio / http) failed to bind or to serve.
    #[error("rmcp transport error: {0}")]
    Transport(String),

    /// A `tools/call` request arrived with a payload the adapter cannot
    /// translate into a `DispatchHost::dispatch` invocation. The dispatcher
    /// itself is never invoked in this case; nothing is persisted.
    #[error("malformed tools/call request: {0}")]
    MalformedRequest(String),

    /// A tool was registered with a name that conflicts with an existing
    /// registration on the same `Frontend` instance.
    #[error("duplicate tool registration: {0}")]
    DuplicateTool(String),

    /// An unexpected transport-internal failure not classifiable as the
    /// above. Use sparingly — most failures fit one of the named variants.
    #[error("internal frontend error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl RmcpFrontendError {
    /// Stable string class for telemetry and audit-log labels.
    pub fn class(&self) -> &'static str {
        match self {
            Self::Transport(_) => "transport",
            Self::MalformedRequest(_) => "malformed-request",
            Self::DuplicateTool(_) => "duplicate-tool",
            Self::Internal(_) => "internal",
        }
    }
}
