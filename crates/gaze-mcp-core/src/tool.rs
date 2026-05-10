//! `Tool` trait + descriptor + response/error types.
//!
//! Tools register against a [`crate::registry::ToolRegistry`] and are invoked
//! exclusively by [`crate::dispatch::PiiEnvelope::dispatch`]. The trait
//! surface intentionally hands out a sealed [`crate::ctx::ToolCtx`] rather
//! than raw redaction or manifest handles, so a tool body never gets a chance
//! to bypass the chokepoint ordering.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ctx::ToolCtx;

/// Operator surface vs. agent surface — picks which [`crate::auth::AuthHook`]
/// method gates the dispatch and which feature graph the tool ships under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ToolTier {
    /// Default-tier tool callable by agents (clean, tokenize, safety_net,
    /// custom adopter tools). Gated by [`crate::auth::AuthHook::authorize_agent`].
    Agent,
    /// Operator-tier tool (restore, restore_strict, export). Gated by
    /// [`crate::auth::AuthHook::authorize_operator`]. Only available behind
    /// the `operator-tier` Cargo feature.
    Operator,
}

/// Dispatcher policy for redacting tool response payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ResponseRedaction {
    /// Run response JSON through the same redaction chokepoint as arguments.
    #[default]
    Apply,
    /// Operator-tier-only escape hatch for restore/export tools returning raw bytes.
    BypassByOperator,
}

/// Static metadata describing a tool. Surfaced to transports via
/// [`crate::registry::ToolRegistry::list`] for `tools/list`-style endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolDescriptor {
    /// Stable wire name (`"clean"`, `"query"`, …). Must be unique per registry.
    name: String,
    /// Tier — drives both auth-hook routing and feature-flag visibility.
    tier: ToolTier,
    /// JSON-schema document describing the tool's input arguments. The
    /// dispatcher does not re-validate against this — adopter tools are
    /// expected to validate inside the body and return
    /// [`ToolError::InvalidArgs`] on shape errors. Surface-only metadata.
    schema: serde_json::Value,
    /// Optional human-readable description for catalog UIs.
    description: Option<String>,
    #[serde(skip)]
    response_redaction: ResponseRedaction,
}

impl ToolDescriptor {
    /// Convenience constructor for an Agent-tier descriptor with no description.
    pub fn agent(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            tier: ToolTier::Agent,
            schema,
            description: None,
            response_redaction: ResponseRedaction::Apply,
        }
    }

    /// Convenience constructor for an Operator-tier descriptor with no description.
    pub fn operator(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            tier: ToolTier::Operator,
            schema,
            description: None,
            response_redaction: ResponseRedaction::Apply,
        }
    }

    /// Builder-style description override.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Override response-redaction posture. Registry rejects agent-tier bypasses.
    pub fn with_response_redaction(mut self, response_redaction: ResponseRedaction) -> Self {
        self.response_redaction = response_redaction;
        self
    }

    /// Stable wire name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Tool auth/feature tier.
    pub fn tier(&self) -> ToolTier {
        self.tier
    }

    /// JSON-schema metadata for input arguments.
    pub fn schema(&self) -> &serde_json::Value {
        &self.schema
    }

    /// Optional catalog description.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Dispatcher response-redaction posture.
    pub fn response_redaction(&self) -> ResponseRedaction {
        self.response_redaction
    }
}

/// Successful tool output. The dispatcher always re-runs `payload` through
/// gaze's redaction pipeline before returning it to the transport — there is
/// no opt-out for individual tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolResponse {
    /// JSON payload the tool produced. May contain raw bytes from the data
    /// source; the dispatcher redacts before persistence and before egress.
    pub payload: serde_json::Value,
}

impl ToolResponse {
    /// Wrap a JSON value as a tool response.
    pub fn json(payload: serde_json::Value) -> Self {
        Self { payload }
    }

    /// Wrap a plain string as a JSON-string tool response.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            payload: serde_json::Value::String(text.into()),
        }
    }
}

/// Error returned by a [`Tool::invoke`] body. The dispatcher classifies these
/// into a [`crate::manifest::FailureReason::ToolError`] manifest row and a
/// transport-level error response.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ToolError {
    /// The tool received arguments that did not match its schema.
    #[error("invalid args: {0}")]
    InvalidArgs(String),
    /// A resource referenced by the call (table, file, identity) was not found.
    #[error("not found: {0}")]
    NotFound(String),
    /// Adopter / backend failure unrelated to argument validation.
    #[error("tool internal error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl ToolError {
    /// Classify the error into the wire-stable class string the manifest
    /// records (`"invalid-args"`, `"not-found"`, `"internal"`).
    pub fn class(&self) -> &'static str {
        match self {
            Self::InvalidArgs(_) => "invalid-args",
            Self::NotFound(_) => "not-found",
            Self::Internal(_) => "internal",
        }
    }

    /// Convenience constructor for wrapping arbitrary adopter errors.
    pub fn internal<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Internal(Box::new(err))
    }
}

/// Tool implementation trait. The single integration point adopters use to
/// register custom tools against a [`crate::registry::ToolRegistry`].
///
/// Implementations MUST be `Send + Sync` so the same tool instance can be
/// shared across concurrent dispatches (the registry stores them behind
/// `Arc<dyn Tool>`). The dispatcher drives one [`Self::invoke`] per
/// `tools/call`; tool implementations should not retain state beyond a call
/// in the tool struct itself — anything stateful belongs in the adopter's
/// `ManifestStore` or external store.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Static metadata describing this tool. The registry inspects this once
    /// at registration time; the dispatcher reads `tier` per call to pick the
    /// auth surface.
    fn descriptor(&self) -> &ToolDescriptor;

    /// Run the tool body. The context exposes redacted args, the audit
    /// session handle, the call id, and the principal id; everything else
    /// (raw args, signing key, manifest store) is intentionally not reachable.
    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError>;
}
