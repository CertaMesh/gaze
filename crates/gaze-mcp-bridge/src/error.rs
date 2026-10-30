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
    #[error("protection failed: {0}")]
    Protection(#[from] gaze::ProtectionError),
    #[error("redaction failed: {0}")]
    Redaction(String),
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),
}

impl BridgeError {
    pub(crate) fn from_redaction(error: gaze::Error) -> Self {
        match error {
            gaze::Error::Protection(error) => Self::Protection(error),
            other => Self::Redaction(other.to_string()),
        }
    }

    pub fn into_dispatch(self) -> DispatchError {
        match self {
            Self::Protection(error) => DispatchError::Protection(error),
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
            Self::SessionStore(_) => DispatchError::ToolError(ToolError::BackendFailure(
                "bridge session store failure".to_string(),
            )),
            Self::Downstream(_) => DispatchError::ToolError(ToolError::BackendFailure(
                "downstream bridge failure".to_string(),
            )),
            Self::Redaction(_) => DispatchError::ToolError(ToolError::BackendFailure(
                "bridge redaction failure".to_string(),
            )),
            Self::ApprovalRequired => {
                DispatchError::ToolError(ToolError::BackendFailure("approval required".to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_preserves_closed_untransformable_reason() {
        let error = gaze::Error::Protection(gaze::ProtectionError::Untransformable {
            reason: gaze_types::UntransformableReason::EncodedCarrier,
            field_path: gaze_types::FieldPath::default()
                .child("synthetic@example.invalid".to_string()),
        });
        let dispatch = BridgeError::from_redaction(error).into_dispatch();
        assert_eq!(dispatch.untransformable_reason(), Some("encoded_carrier"));
        assert!(!format!("{dispatch:?}").contains("synthetic@example.invalid"));
    }
}
