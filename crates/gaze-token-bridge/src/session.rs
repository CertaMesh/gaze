//! The RedactionSession wrapper.
//!
//! A `RedactionSession` is the short-lived, conversation-local pseudonym namespace.
//! It wraps a `gaze::Session` and is **bound to exactly one principal** at creation:
//! a request from a different principal is denied before token resolution. This blocks
//! shared-session correlation while projection stays `(tenant,domain)`-keyed.

use gaze::{PiiClass, Scope, Session};

use crate::error::{BridgeError, DenyReason};
use crate::util::parse_token_class;

/// Short-lived conversation namespace, bound to one principal, owning the restore map.
pub struct RedactionSession {
    session_id: String,
    principal_id: String,
    inner: Session,
}

impl RedactionSession {
    /// Create an ephemeral session bound to `principal_id`. Ephemeral scope cannot be
    /// exported, so the restore map is structurally owner-side only.
    pub fn ephemeral_for(principal_id: &str) -> Result<Self, BridgeError> {
        Ok(Self {
            session_id: format!("conversation:{principal_id}"),
            principal_id: principal_id.to_string(),
            inner: Session::new(Scope::Ephemeral)
                .map_err(|err| BridgeError::Session(format!("{err:?}")))?,
        })
    }

    /// Mint (or reuse) a session token for a raw value of `class`.
    pub fn tokenize(&self, class: &PiiClass, raw: &str) -> Result<String, BridgeError> {
        self.inner
            .tokenize(class, raw)
            .map_err(|err| BridgeError::Session(format!("{err:?}")))
    }

    /// All tokens currently minted in this session.
    pub fn tokens(&self) -> Vec<String> {
        self.inner.tokens()
    }

    /// The conversation/session id this session is keyed by.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// True if this session belongs to `principal_id`.
    pub fn is_bound_to(&self, principal_id: &str) -> bool {
        self.principal_id == principal_id
    }

    /// Resolve a token to its class + raw value, owner-side. Fails closed:
    /// `MalformedToken` for bad shape, `UnknownToken` if not in this session's manifest.
    pub fn resolve_token(&self, token: &str) -> Result<(PiiClass, String), DenyReason> {
        let class = parse_token_class(token).ok_or(DenyReason::MalformedToken)?;
        let raw = self
            .inner
            .restore_strict(token)
            .map_err(|_| DenyReason::UnknownToken)?;
        Ok((class, raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A safety-net redaction marker must never resolve to anything owner-side.
    ///
    /// The bridge resolves an agent-supplied token to its raw value. A marker stands in the clean
    /// text exactly where a token would, so an agent can hand one back; if the bridge ever
    /// resolved it, the redaction would be reversible through the bridge even though restore
    /// itself refuses it. It is refused as malformed -- it is not in the token grammar at all --
    /// before the session is consulted.
    #[test]
    fn a_redaction_marker_never_resolves_owner_side() {
        let session = RedactionSession::ephemeral_for("principal-1").expect("session");
        // Mint a real token so the session is not trivially empty: a marker must be refused
        // on its shape, not merely because nothing had been tokenized.
        session
            .tokenize(&PiiClass::Name, "Schmidt")
            .expect("tokenize");
        for class in [
            PiiClass::Name,
            PiiClass::Email,
            PiiClass::custom("address_2").expect("valid custom class"),
        ] {
            let marker = gaze::redaction_marker(&class);
            assert_eq!(
                session.resolve_token(&marker),
                Err(DenyReason::MalformedToken),
                "{marker} must never resolve"
            );
        }
    }
}
