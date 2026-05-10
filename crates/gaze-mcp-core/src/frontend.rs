//! Transport-facing traits.
//!
//! Adopters or sink crates (e.g. `gaze-mcp-rmcp`) implement [`Frontend`] to
//! adapt a wire protocol (rmcp stdio/http, custom JSON-RPC, gRPC, …) to the
//! gaze-mcp-core runtime. Transports never see [`crate::dispatch::PiiEnvelope`]
//! directly — they receive a [`DispatchHost`] reference whose only public
//! surface is `dispatch` + `list_tools`.
//!
//! This split keeps the chokepoint internals (the gaze pipeline, the gaze
//! session, the manifest store) inaccessible from the transport layer, so a
//! buggy or hostile transport cannot reach past the redaction step.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use crate::auth::Principal;
use crate::dispatch::DispatchError;
use crate::tool::{ToolDescriptor, ToolResponse};

/// Object-safe view of [`crate::dispatch::PiiEnvelope`] handed to [`Frontend`]
/// implementations. Transports get exactly two operations: dispatch a tool
/// call (which runs through the chokepoint), and list registered tool
/// descriptors for `tools/list`-style endpoints.
#[async_trait]
pub trait DispatchHost: Send + Sync {
    /// Dispatch a tool call. Same contract as
    /// [`crate::dispatch::PiiEnvelope::dispatch`].
    async fn dispatch(
        &self,
        principal: &Principal,
        tool_name: &str,
        raw_args: serde_json::Value,
        external_session_id: Option<&str>,
    ) -> Result<ToolResponse, DispatchError>;

    /// Snapshot of registered tool descriptors. Transports cache this once
    /// at startup or call per request — implementations should make this
    /// cheap (it is internally an [`crate::registry::ToolRegistry::list`]
    /// call).
    fn list_tools(&self) -> Vec<ToolDescriptor>;
}

/// Cooperative shutdown signal handed to [`Frontend::serve`]. The transport
/// observes [`Self::is_cancelled`] (or wraps it in its runtime's primitive
/// for an awaitable signal) and tears down listeners + in-flight dispatches.
///
/// Runtime-agnostic: the token is built on `Arc<AtomicBool>` rather than a
/// tokio `CancellationToken`, so the gaze-mcp-core public surface does not
/// pull tokio into adopter dependency graphs (and the safety-net
/// dependency-isolation gate stays clean).
#[derive(Debug, Clone, Default)]
pub struct ShutdownToken {
    inner: Arc<AtomicBool>,
}

impl ShutdownToken {
    /// Construct a fresh, un-cancelled shutdown token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Trigger the shutdown signal. Idempotent.
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    /// True if [`Self::cancel`] has been called.
    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }
}

/// Errors a [`Frontend`] implementation may surface from [`Frontend::serve`].
///
/// The trait stays generic over backend errors via the `Backend` variant so
/// rmcp / custom impls can wrap their own error type without depending on
/// gaze-mcp-core's internal taxonomy.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FrontendError {
    /// Transport bind / accept / IO failure.
    #[error("transport IO error: {0}")]
    Io(#[source] std::io::Error),
    /// Catch-all for adopter / transport backend failures.
    #[error("frontend backend error: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl FrontendError {
    /// Convenience constructor for wrapping arbitrary adopter errors.
    pub fn backend<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Backend(Box::new(err))
    }
}

/// Trait implemented by transports (rmcp stdio/http, custom JSON-RPC, …).
///
/// One [`Self::serve`] call drives the entire transport lifetime: it accepts
/// connections, dispatches `tools/call` requests through the host, and
/// returns when [`ShutdownToken::cancel`] fires (or earlier on a backend
/// error).
///
/// Frontends do NOT register tools — tool registration happens once, on the
/// `ToolRegistry` the host wraps. The plan body suggested a `register_tool`
/// method on `Frontend`; we dropped it because a separate registration
/// surface on the transport invites dual-bookkeeping (registry vs frontend
/// out of sync) and is unnecessary: `DispatchHost::list_tools` already gives
/// the transport everything it needs to advertise the tool catalog.
#[async_trait]
pub trait Frontend: Send {
    /// Drive the transport until shutdown or backend failure.
    async fn serve(
        self,
        host: Arc<dyn DispatchHost>,
        shutdown: ShutdownToken,
    ) -> Result<(), FrontendError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_token_round_trip() {
        let tok = ShutdownToken::new();
        assert!(!tok.is_cancelled());
        let clone = tok.clone();
        clone.cancel();
        assert!(tok.is_cancelled());
        // Idempotent.
        clone.cancel();
        assert!(tok.is_cancelled());
    }
}
