//! Authorization contract for the chokepoint dispatcher.
//!
//! Adopters supply an [`AuthHook`] implementation that maps an authenticated
//! [`Principal`] to a yes/no decision per tool. The dispatcher gates every
//! tool call on this hook BEFORE redaction or manifest persistence so denied
//! calls leave no side effects beyond a manifest fail row.
//!
//! The agent surface (`authorize_agent`) and the operator surface
//! (`authorize_operator`) are intentionally separate methods: operator-tier
//! tools (restore, export) MUST go through `authorize_operator`, and the
//! default [`DenyAllAuthHook`] returns [`AuthError::MissingHook`] for both,
//! so an adopter that forgets to wire auth fails closed.

use async_trait::async_trait;

/// Authenticated caller of a tool, as resolved by the transport layer.
///
/// Adopters carry their own identity model in `id` (subject claim, OIDC sub,
/// internal account ulid, …) and surface coarse-grained role labels in `roles`
/// so [`AuthHook`] implementations can branch on roles without re-parsing the
/// underlying token.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Principal {
    /// Stable identifier for the calling principal. Logged into the manifest
    /// row and reused for downstream audit correlation.
    pub id: String,
    /// Coarse-grained role labels (e.g. `"agent"`, `"operator"`, `"oncall"`).
    /// Optional — adopters who use claim-based auth may leave this empty and
    /// drive decisions off `id` plus an external policy store.
    pub roles: Vec<String>,
}

impl Principal {
    /// Construct a principal with no roles.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            roles: Vec::new(),
        }
    }

    /// Construct a principal with explicit roles.
    pub fn with_roles(id: impl Into<String>, roles: Vec<String>) -> Self {
        Self {
            id: id.into(),
            roles,
        }
    }

    /// True if the principal carries the named role label.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

/// Errors returned by [`AuthHook`] implementations.
///
/// The dispatcher never inspects the inner detail — it converts an `AuthError`
/// into a manifest [`crate::manifest::FailureReason::AuthDenied`] row and a
/// transport-level error response. The string payloads should therefore be
/// safe to persist post-redaction.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// The principal was authenticated but is not authorized for this tool.
    /// The string is a short reason safe to persist (no PII).
    #[error("authorization denied: {0}")]
    Denied(String),
    /// The hook itself is absent or unconfigured. Returned by
    /// [`DenyAllAuthHook`] so an adopter that forgets to wire auth fails
    /// closed instead of accepting the call.
    #[error("authorization hook is missing")]
    MissingHook,
    /// Catch-all for adopter backend failures (DB unavailable, identity
    /// provider unreachable, etc.). The dispatcher fails closed on any
    /// `Internal` variant — there is no retry.
    #[error("authorization hook internal error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl AuthError {
    /// Convenience constructor for wrapping arbitrary adopter errors.
    pub fn internal<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Internal(Box::new(err))
    }
}

/// Authorization decision point for the chokepoint dispatcher.
///
/// Implementations are responsible for gating tool calls per principal. The
/// agent surface (`authorize_agent`) is invoked for default-tier tools
/// (`clean`, `tokenize`, `safety_net_check`, custom registrations); the
/// operator surface (`authorize_operator`) is invoked for the operator-tier
/// tools (`restore`, `restore_strict`, `export_session_tokens`). The dispatcher
/// decides which to call based on the tool's [`crate::tool::ToolDescriptor`]
/// metadata — adopters do not opt in to operator-tier semantics by accident.
#[async_trait]
pub trait AuthHook: Send + Sync {
    /// Decide whether `principal` may invoke the agent-tier tool named
    /// `tool_name`. Return `Ok(())` to allow, [`AuthError::Denied`] to reject
    /// with a recorded reason, or [`AuthError::Internal`] on backend failure.
    async fn authorize_agent(
        &self,
        principal: &Principal,
        tool_name: &str,
    ) -> Result<(), AuthError>;

    /// Decide whether `principal` may invoke the operator-tier tool named
    /// `tool_name`. The dispatcher fails closed if the call should have gone
    /// through `authorize_agent` instead.
    async fn authorize_operator(
        &self,
        principal: &Principal,
        tool_name: &str,
    ) -> Result<(), AuthError>;
}

/// Default fail-closed [`AuthHook`] that rejects every call with
/// [`AuthError::MissingHook`].
///
/// Adopters who haven't wired up authentication get this hook by default —
/// every dispatch lands in a manifest fail row tagged `MissingHook`, so the
/// adopter learns immediately that auth is unconfigured. There is no way to
/// turn this off short of providing a real `AuthHook` impl.
#[derive(Debug, Default, Clone, Copy)]
pub struct DenyAllAuthHook;

#[async_trait]
impl AuthHook for DenyAllAuthHook {
    async fn authorize_agent(
        &self,
        _principal: &Principal,
        _tool_name: &str,
    ) -> Result<(), AuthError> {
        Err(AuthError::MissingHook)
    }

    async fn authorize_operator(
        &self,
        _principal: &Principal,
        _tool_name: &str,
    ) -> Result<(), AuthError> {
        Err(AuthError::MissingHook)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_role_membership() {
        let p = Principal::with_roles("user-1", vec!["agent".into(), "oncall".into()]);
        assert!(p.has_role("agent"));
        assert!(p.has_role("oncall"));
        assert!(!p.has_role("operator"));
    }

    #[tokio::test]
    async fn deny_all_hook_returns_missing_for_both_surfaces() {
        let hook = DenyAllAuthHook;
        let p = Principal::new("anyone");
        assert!(matches!(
            hook.authorize_agent(&p, "clean").await,
            Err(AuthError::MissingHook)
        ));
        assert!(matches!(
            hook.authorize_operator(&p, "restore").await,
            Err(AuthError::MissingHook)
        ));
    }
}
