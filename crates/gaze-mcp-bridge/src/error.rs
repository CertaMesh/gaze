use gaze_mcp_core::{AuthError, DispatchError, ManifestError, ToolError};

pub type BridgeResult<T> = Result<T, BridgeError>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BridgeError {
    #[error("configuration rejected: {0}")]
    Config(String),
    #[error("policy rejected: {0}")]
    Policy(String),
    #[error("session id rejected: {0}")]
    SessionId(#[from] gaze_mcp_core::SessionIdError),
    #[error("authorization rejected: {0}")]
    Auth(#[from] AuthError),
    #[error("unknown bridge tool: {0}")]
    UnknownTool(String),
    #[error("session store failure: {0}")]
    SessionStore(String),
    #[error("approval required")]
    ApprovalRequired,
    #[error("audit persistence failure: {0}")]
    Audit(String),
    #[error("downstream MCP failure: {0}")]
    Downstream(String),
    #[error("redaction failed: {0}")]
    Redaction(String),
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),
}

impl BridgeError {
    pub fn into_dispatch(self) -> DispatchError {
        match self {
            Self::SessionId(err) => DispatchError::SessionId(err),
            Self::Auth(err) => DispatchError::Auth(err),
            Self::UnknownTool(tool) => DispatchError::UnknownTool(tool),
            Self::Audit(message) => {
                DispatchError::Manifest(ManifestError::backend(std::io::Error::other(message)))
            }
            Self::LimitExceeded(message) => {
                DispatchError::ToolError(ToolError::LimitExceeded(message))
            }
            Self::Policy(message) | Self::Config(message) => {
                DispatchError::ToolError(ToolError::InvalidArgs(message))
            }
            Self::SessionStore(message) | Self::Downstream(message) | Self::Redaction(message) => {
                DispatchError::ToolError(ToolError::BackendFailure(message))
            }
            Self::ApprovalRequired => {
                DispatchError::ToolError(ToolError::BackendFailure("approval required".to_string()))
            }
        }
    }
}
