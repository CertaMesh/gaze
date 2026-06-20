//! FROZEN CONTRACT — error + deny taxonomy. Master-owned.
//!
//! `DenyReason` is `#[non_exhaustive]`: adding a variant is a master decision so
//! all match sites stay exhaustive-with-wildcard. Every deny is typed and audited;
//! the agent-facing surface must never reveal which variant fired (no oracle).

use serde::{Deserialize, Serialize};

/// Owner-side construction/configuration error (policy load, session, key material).
/// Distinct from [`DenyReason`], which is the per-request authorization outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    /// Policy/registry/key configuration problem.
    Policy(String),
    /// Underlying gaze session error.
    Session(String),
}

/// Why a bridge request or handle execution was denied. All paths fail closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DenyReason {
    /// Source token is not a well-formed gaze token.
    MalformedToken,
    /// Token is well-formed but absent from the active session manifest.
    UnknownToken,
    /// Principal tenant/workspace does not match the request or target domain.
    TenantOrWorkspaceMismatch,
    /// Target domain is not registered.
    DomainNotFound,
    /// Principal lacks a role the domain allows.
    PrincipalNotAllowed,
    /// Entity class is not bridgeable into the target domain.
    ClassNotAllowed,
    /// No allow rule matched (default-deny).
    NoMatchingAllowRule,
    /// Cross-domain search attempted without elevated, mutually-allowing policy.
    CrossDomainDenied,
    /// SearchHandle is past its expiry tick.
    ExpiredHandle,
    /// SearchHandle nonce was already consumed (replay).
    ReplayedHandle,
    /// Handle's entity_ref domain does not match its target domain.
    HandleDomainMismatch,
    /// Requested entity_ref does not match the entity_ref the handle was minted for.
    EntityRefMismatch,
    /// Session is bound to a different principal than the requester.
    SessionPrincipalMismatch,
    /// Domain forbids returning snippets.
    SnippetsDisabled,
    /// Response translation could not map all index aliases to session tokens.
    TranslatorFailed,
}
