//! Manifest persistence contract for the gaze-mcp-core chokepoint.
//!
//! Adopters (`gaze-cli`, `gaze-lens`, custom hosts) implement [`ManifestStore`]
//! against their own backing store — `gaze-cli` writes to the canonical
//! `gaze-audit` SQLite, `gaze-lens` keeps its `~/.gaze-lens/manifest.sqlite`
//! schema, custom hosts can persist to whatever durable store satisfies their
//! axis-1 (never-leak) + axis-2 (reversible) requirements.
//!
//! The dispatcher in [`crate::dispatch`] enforces the ordering invariant: a
//! tool response MUST NOT escape the chokepoint until the corresponding
//! manifest call has been finalized via [`ManifestStore::finish_call`] (success
//! path) or [`ManifestStore::fail_call`] (error path).

use std::time::SystemTime;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Opaque identifier for an in-flight tool call as tracked by the manifest
/// store. Issued by [`ManifestStore::begin_call`] and consumed by exactly one
/// of [`ManifestStore::finish_call`] / [`ManifestStore::fail_call`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CallHandle(pub ulid::Ulid);

impl CallHandle {
    /// Construct a handle from a pre-generated ULID. Adopters generally do not
    /// build handles directly — the chokepoint dispatcher provides the ULID
    /// via [`BeginCallContext::call_id`].
    pub fn new(id: ulid::Ulid) -> Self {
        Self(id)
    }

    /// Underlying ULID.
    pub fn id(&self) -> ulid::Ulid {
        self.0
    }
}

/// Context passed to [`ManifestStore::begin_call`] when the dispatcher opens
/// a new manifest entry. Borrowed for the duration of the call so adopters can
/// avoid copying the redacted args or audit metadata.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct BeginCallContext<'a> {
    /// Stable identifier for this tool call. The dispatcher generates a fresh
    /// ULID per call and reuses it as the [`CallHandle`] so adopters do not
    /// have to mint their own.
    pub call_id: ulid::Ulid,
    /// Optional external session id supplied by the transport. Adopters bind
    /// their own session schema via the `ManifestStore` constructor; the
    /// dispatcher only enforces format validation via
    /// [`crate::session_id::SessionIdPolicy`] before calling `begin_call`.
    pub external_session_id: Option<&'a str>,
    /// Stable identifier of the principal invoking the tool, as resolved by
    /// the [`crate::auth::AuthHook`] for this call.
    pub principal_id: &'a str,
    /// Name of the tool being dispatched (e.g. `"clean"`, `"query"`).
    pub tool_name: &'a str,
    /// Redacted JSON arguments passed to the tool. The dispatcher guarantees
    /// these are post-redaction; adopters should treat them as safe to persist.
    pub redacted_args: &'a serde_json::Value,
    /// Wall-clock instant the dispatcher accepted the call. Adopters use this
    /// for ordering and schema fields like `started_at`.
    pub started_at: SystemTime,
}

/// Reason a manifest call did not complete successfully. The dispatcher always
/// supplies one of these on the failure path so the manifest row carries
/// enough context to drive operator review later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum FailureReason {
    /// The tool implementation returned a [`crate::tool::ToolError`].
    ToolError {
        /// Stable error class string, e.g. `"invalid-args"`.
        class: String,
        /// Human-readable error message; safe to persist (post-redaction).
        message: String,
    },
    /// Authorization denied by the [`crate::auth::AuthHook`] before the tool
    /// was invoked. Recorded for compliance / audit.
    AuthDenied {
        /// Reason string returned by the auth hook.
        reason: String,
    },
    /// Redaction of the tool's response itself returned an error (defense in
    /// depth — should be rare).
    RedactionFailed {
        /// Diagnostic message safe to persist.
        message: String,
    },
    /// Catch-all for adopter-supplied or unforeseen failure modes. Use sparingly.
    Other {
        /// Diagnostic message safe to persist.
        message: String,
    },
}

/// Out-of-row reference to the redacted response snapshot for a tool call.
///
/// Inline blobs would defeat the threat models of adopters whose underlying
/// store sits on encrypted disk volumes (lens-eye round 1 TOP_RISK in
/// scratchpad 1453). Adopters write the response bytes to their own snapshot
/// store and pass back this reference — the manifest row records the path +
/// integrity hash but not the bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct SnapshotRef {
    /// Adopter-defined locator for the snapshot (path, blob key, etc.).
    pub locator: String,
    /// Hex-encoded SHA-256 of the snapshot bytes; verified on reverse lookup.
    pub sha256_hex: String,
    /// Size of the snapshot in bytes, recorded for sanity / DoS budget checks.
    pub byte_len: u64,
}

impl SnapshotRef {
    /// Construct a `SnapshotRef` from its components. The dispatcher does not
    /// build these — adopters return one from their snapshot persistence path
    /// and hand it to [`ManifestStore::finish_call`].
    pub fn new(locator: impl Into<String>, sha256_hex: impl Into<String>, byte_len: u64) -> Self {
        Self {
            locator: locator.into(),
            sha256_hex: sha256_hex.into(),
            byte_len,
        }
    }
}

/// Errors returned by [`ManifestStore`] implementations.
///
/// Generic by design: gaze-mcp-core does not couple to any one backend's
/// error type (e.g. `LensError`, `rusqlite::Error`). Adopters wrap their
/// backend error in [`ManifestError::Backend`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// Adopter-supplied backend failure (DB write error, IO error, etc.).
    #[error("manifest backend error: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// `finish_call` / `fail_call` was called with a handle the store does not
    /// know about. Indicates a programming error in the dispatcher or a hostile
    /// caller — fail closed.
    #[error("manifest handle not found: {0:?}")]
    UnknownHandle(CallHandle),
    /// `begin_call` was rejected because an entry with the same call id already
    /// exists. Indicates a ULID collision (vanishingly rare) or replay attempt.
    #[error("duplicate manifest call id: {0:?}")]
    DuplicateCallId(CallHandle),
    /// Validation failure on the begin payload (bad session id format, missing
    /// required adopter binding, etc.). Adopter-defined message.
    #[error("manifest validation rejected the call: {0}")]
    Validation(String),
}

impl ManifestError {
    /// Convenience constructor for wrapping arbitrary adopter errors.
    pub fn backend<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Backend(Box::new(err))
    }
}

/// Persistence sink for the chokepoint manifest.
///
/// Implementations are responsible for binding the call to whatever external
/// session schema the adopter has chosen (lens binds to its
/// `lens_session_id`/`gaze_audit_session_id` pair, gaze-cli binds to
/// `gaze-audit`'s session ulid). gaze-mcp-core never reads back manifest
/// rows — it only writes them — so the trait surface is narrow on purpose.
#[async_trait]
pub trait ManifestStore: Send + Sync {
    /// Open a manifest entry for a tool call. The dispatcher invokes this
    /// AFTER auth + redaction of inputs and BEFORE the tool body runs, so the
    /// store always observes the call before any side effects.
    ///
    /// Implementations MUST be idempotent on `ctx.call_id` collisions (return
    /// [`ManifestError::DuplicateCallId`] rather than overwriting).
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError>;

    /// Finalize a manifest entry on the success path. The dispatcher calls
    /// this AFTER the tool returned and AFTER its response was redacted,
    /// passing an out-of-row [`SnapshotRef`] to the redacted response bytes.
    ///
    /// The chokepoint contract requires this call to complete (or
    /// [`fail_call`](Self::fail_call) to be called) before the dispatcher
    /// returns the response to the transport.
    async fn finish_call(
        &self,
        handle: CallHandle,
        snapshot: SnapshotRef,
    ) -> Result<(), ManifestError>;

    /// Finalize a manifest entry on the failure path. The dispatcher calls
    /// this when auth, the tool body, or response redaction returned an error.
    /// The manifest entry is closed with a [`FailureReason`] so operators can
    /// review the call later.
    async fn fail_call(
        &self,
        handle: CallHandle,
        reason: FailureReason,
    ) -> Result<(), ManifestError>;
}
