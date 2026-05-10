//! Sealed tool-invocation context.
//!
//! [`ToolCtx`] is the only handle a [`crate::tool::Tool`] implementation
//! receives during dispatch. Its construction site is `pub(crate)` so the
//! only thing that can build a `ToolCtx<'_>` is [`crate::dispatch::PiiEnvelope`]
//! — which means a tool can never observe a tool context outside the
//! redact → manifest.begin → invoke → redact-response → manifest.finish/fail
//! chokepoint ordering.
//!
//! ## Why this seal exists
//!
//! gaze-mcp-core's primary safety axis is **never leak** (CLAUDE.md axis 1).
//! The threat model assumes tool implementations are partially-trusted code
//! (third-party adopters, plugins, future contributors). Without the seal a
//! tool could:
//!
//! 1. Construct its own `ToolCtx` and call peer tools directly, bypassing
//!    redaction, manifest persistence, and auth.
//! 2. Stash a context value on the heap to reuse it after the dispatch frame
//!    has unwound (skipping the redaction-before-return invariant).
//!
//! The combination of `pub(crate) fn new`, private fields, `#[non_exhaustive]`,
//! and the borrowed lifetime `'a` makes both attacks unrepresentable: external
//! crates have no path to construct a `ToolCtx`, and the borrow checker
//! prevents storing the borrowed context past the dispatch frame.

use std::marker::PhantomData;

use ulid::Ulid;

use crate::ManifestStore;

/// Audit-correlation handle exposed to a [`crate::tool::Tool`] implementation.
///
/// The handle deliberately exposes only the external session id. The actual
/// `gaze::Session` (which holds the signing key + token manifest) stays inside
/// the dispatcher; tools that need to redact text must do so via
/// [`crate::dispatch::PiiEnvelope`]'s redaction surface, not by reaching into
/// the session.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SessionHandle<'a> {
    audit_session_id: &'a str,
    _life: PhantomData<&'a ()>,
}

impl<'a> SessionHandle<'a> {
    /// Construct a handle. The dispatcher is the only public caller; adopters
    /// never build a `SessionHandle` directly.
    pub(crate) fn new(audit_session_id: &'a str) -> Self {
        Self {
            audit_session_id,
            _life: PhantomData,
        }
    }

    /// Audit-correlation id for this dispatch (the value the
    /// [`crate::manifest::ManifestStore`] receives via
    /// [`crate::manifest::BeginCallContext::external_session_id`], if one was
    /// supplied by the transport).
    pub fn audit_session_id(&self) -> &'a str {
        self.audit_session_id
    }
}

/// Borrowed backend handles available to tool bodies during one dispatch.
#[non_exhaustive]
pub struct ToolResources<'a> {
    pipeline: &'a gaze::Pipeline,
    session: &'a gaze::Session,
    manifest: &'a dyn ManifestStore,
    locale_chain: &'a [gaze::LocaleTag],
    _life: PhantomData<&'a ()>,
}

impl<'a> ToolResources<'a> {
    pub(crate) fn new(
        pipeline: &'a gaze::Pipeline,
        session: &'a gaze::Session,
        manifest: &'a dyn ManifestStore,
        locale_chain: &'a [gaze::LocaleTag],
    ) -> Self {
        Self {
            pipeline,
            session,
            manifest,
            locale_chain,
            _life: PhantomData,
        }
    }

    /// Gaze pipeline backing this dispatch.
    pub fn pipeline(&self) -> &'a gaze::Pipeline {
        self.pipeline
    }

    /// Session backing this dispatch.
    pub fn session(&self) -> &'a gaze::Session {
        self.session
    }

    /// Manifest store backing this dispatch.
    pub fn manifest(&self) -> &'a dyn ManifestStore {
        self.manifest
    }

    /// Locale chain configured for observer-only tool operations.
    pub fn locale_chain(&self) -> &'a [gaze::LocaleTag] {
        self.locale_chain
    }
}

impl std::fmt::Debug for ToolResources<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolResources")
            .field("pipeline", &"<gaze::Pipeline>")
            .field("session", &"<gaze::Session>")
            .field("manifest", &"<dyn ManifestStore>")
            .field("locale_chain", &self.locale_chain)
            .finish()
    }
}

/// Sealed tool-invocation context. See module docs for the full invariant.
///
/// The `'a` lifetime binds the context to the dispatcher's stack frame so a
/// tool implementation cannot store the context past the call. Fields are
/// `pub(crate)` and `#[non_exhaustive]` so external crates cannot construct
/// a `ToolCtx` via struct literal or `..Default::default()`.
#[non_exhaustive]
pub struct ToolCtx<'a> {
    pub(crate) session: SessionHandle<'a>,
    pub(crate) resources: ToolResources<'a>,
    pub(crate) redacted_args: serde_json::Value,
    pub(crate) call_id: Ulid,
    pub(crate) tool_name: &'a str,
    pub(crate) principal_id: &'a str,
    _life: PhantomData<&'a ()>,
}

impl<'a> ToolCtx<'a> {
    /// Construct a `ToolCtx`. `pub(crate)` — the dispatcher in
    /// [`crate::dispatch`] is the only call site. Verified at compile time by
    /// the trybuild fixtures in `tests/tool_ctx_seal.rs`.
    pub(crate) fn new_with_resources(
        session: SessionHandle<'a>,
        resources: ToolResources<'a>,
        redacted_args: serde_json::Value,
        call_id: Ulid,
        tool_name: &'a str,
        principal_id: &'a str,
    ) -> Self {
        Self {
            session,
            resources,
            redacted_args,
            call_id,
            tool_name,
            principal_id,
            _life: PhantomData,
        }
    }

    /// Redacted JSON arguments the dispatcher received from the transport.
    /// These are post-redaction, safe to inspect, and safe to re-emit in
    /// the tool response without breaking the never-leak invariant.
    pub fn redacted_args(&self) -> &serde_json::Value {
        &self.redacted_args
    }

    /// Audit-correlation handle (see [`SessionHandle`]).
    pub fn session(&self) -> &SessionHandle<'a> {
        &self.session
    }

    /// Borrowed backend handles for tool bodies that need Gaze internals.
    pub fn resources(&self) -> &ToolResources<'a> {
        &self.resources
    }

    /// Stable identifier for this tool call. Same value the dispatcher passes
    /// to the manifest store via [`crate::manifest::BeginCallContext::call_id`].
    pub fn call_id(&self) -> Ulid {
        self.call_id
    }

    /// Name of the tool currently being invoked. Useful for tool
    /// implementations that share an `impl Tool` body across multiple
    /// descriptors (e.g. parameterized tools).
    pub fn tool_name(&self) -> &'a str {
        self.tool_name
    }

    /// Stable id of the principal whose request triggered this dispatch.
    /// Already authorized by [`crate::auth::AuthHook`] before the tool body
    /// runs; tools should treat this as informational, not as a permission
    /// re-check.
    pub fn principal_id(&self) -> &'a str {
        self.principal_id
    }
}

impl std::fmt::Debug for ToolCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolCtx")
            .field("session", &self.session)
            .field("resources", &self.resources)
            .field("redacted_args", &self.redacted_args)
            .field("call_id", &self.call_id)
            .field("tool_name", &self.tool_name)
            .field("principal_id", &self.principal_id)
            .finish()
    }
}
