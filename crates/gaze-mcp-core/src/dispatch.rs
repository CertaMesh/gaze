//! `PiiEnvelope::dispatch` — the chokepoint runtime.
//!
//! The dispatcher is the single point that:
//! 1. Validates the transport-supplied session id (via [`SessionIdPolicy`]).
//! 2. Authorizes the principal against the tool's tier (via [`AuthHook`]).
//! 3. Looks the tool up in the [`ToolRegistry`].
//! 4. Redacts raw args through the gaze pipeline.
//! 5. Opens a manifest entry via [`ManifestStore::begin_call`].
//! 6. Builds the sealed [`ToolCtx`] (only construction site).
//! 7. Drives [`crate::tool::Tool::invoke`] to completion.
//! 8. Redacts the tool response.
//! 9. Finalizes the manifest entry via `finish_call` or `fail_call`.
//! 10. Returns the redacted response.
//!
//! Steps 5 and 9 form the chokepoint contract: a tool response cannot escape
//! the dispatcher without a corresponding `finish_call` or `fail_call` row,
//! verified by the golden test in `tests/chokepoint_ordering.rs`.

use std::time::SystemTime;

use sha2::{Digest, Sha256};
use ulid::Ulid;

use crate::auth::{AuthError, AuthHook, Principal};
use crate::ctx::{SessionHandle, ToolCtx, ToolResources};
use crate::manifest::{
    BeginCallContext, CallHandle, FailureReason, ManifestError, ManifestStore, SnapshotRef,
};
use crate::registry::ToolRegistry;
use crate::session_id::{SessionIdError, SessionIdPolicy};
use crate::tool::{ResponseRedaction, ToolError, ToolResponse, ToolTier};

/// Errors returned by [`PiiEnvelope::dispatch`]. Each variant maps onto a
/// distinct manifest [`FailureReason`] (or, on the success path, no failure
/// reason at all). The dispatcher always finalizes the manifest entry before
/// returning a `Dispatch_Error` — there is no path that records a failure
/// without persisting the row.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DispatchError {
    /// Transport supplied an invalid session id (format or entropy).
    #[error("session id rejected: {0}")]
    SessionId(#[from] SessionIdError),
    /// `AuthHook` denied (or could not evaluate) the call.
    #[error("authorization rejected: {0}")]
    Auth(#[from] AuthError),
    /// No tool registered under this name.
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    /// Manifest store rejected one of `begin_call` / `finish_call` / `fail_call`.
    #[error("manifest persistence failure: {0}")]
    Manifest(#[from] ManifestError),
    /// Tool body returned an error. Manifest entry has been finalized via
    /// `fail_call` before this variant is observed.
    #[error("tool returned error: {0}")]
    ToolError(#[from] ToolError),
    /// Redaction of the args or response returned an error from the gaze
    /// pipeline. The manifest entry, if opened, is finalized via `fail_call`
    /// before this is observed.
    #[error("redaction failed: {0}")]
    Redaction(String),
    /// Response could not be serialized for snapshot hashing. Should be rare
    /// (`serde_json::Value` round-trips cleanly).
    #[error("response serialization failure: {0}")]
    ResponseSerialization(#[source] serde_json::Error),
}

/// The chokepoint runtime. Borrows references to every collaborator so the
/// dispatcher itself is free of allocation per dispatch.
///
/// The plan calls these out as the "five wires" of the chokepoint:
/// registry (what to call), auth (who may call), manifest (audit sink),
/// pipeline+session (redaction substrate), session id policy (transport input
/// validation).
#[non_exhaustive]
pub struct PiiEnvelope<'a> {
    /// Registered tools — the only invocable surface.
    pub registry: &'a ToolRegistry,
    /// Authorization gate.
    pub auth: &'a dyn AuthHook,
    /// Manifest persistence sink. Receives `begin_call` BEFORE invoke and
    /// `finish_call`/`fail_call` BEFORE the dispatcher returns.
    pub manifest: &'a dyn ManifestStore,
    /// Gaze pipeline used to redact args + responses.
    pub pipeline: &'a gaze::Pipeline,
    /// Gaze session that holds the per-conversation token manifest.
    pub session: &'a gaze::Session,
    /// Locale chain available to tool bodies that need observer-only checks.
    pub locale_chain: &'a [gaze::LocaleTag],
    /// Transport-supplied session id validation policy.
    pub session_id_policy: &'a SessionIdPolicy,
}

impl<'a> PiiEnvelope<'a> {
    /// Construct a new envelope. All references are borrowed — adopters
    /// build the collaborators once and hand them to the envelope per
    /// dispatch (or per host lifetime).
    pub fn new(
        registry: &'a ToolRegistry,
        auth: &'a dyn AuthHook,
        manifest: &'a dyn ManifestStore,
        pipeline: &'a gaze::Pipeline,
        session: &'a gaze::Session,
        locale_chain: &'a [gaze::LocaleTag],
        session_id_policy: &'a SessionIdPolicy,
    ) -> Self {
        Self {
            registry,
            auth,
            manifest,
            pipeline,
            session,
            locale_chain,
            session_id_policy,
        }
    }

    /// Dispatch a tool call. The contract is:
    ///
    /// - Either a redacted [`ToolResponse`] is returned AND a manifest
    ///   `finish_call` row was persisted, OR
    /// - A [`DispatchError`] is returned AND a manifest `fail_call` row was
    ///   persisted (when a manifest entry was opened — variants returned
    ///   before `begin_call` cannot persist a row).
    ///
    /// There is no third path. Verified by `tests/chokepoint_ordering.rs`.
    pub async fn dispatch(
        &self,
        principal: &Principal,
        tool_name: &str,
        raw_args: serde_json::Value,
        external_session_id: Option<&str>,
    ) -> Result<ToolResponse, DispatchError> {
        // 1. Validate the session id (cheapest fail-closed check; do this
        //    before any auth call to avoid leaking which session ids exist).
        if let Some(sid) = external_session_id {
            self.session_id_policy.validate(sid)?;
        }

        // 2. Look up the tool; without a registered tool we cannot decide
        //    whether to gate on agent or operator auth.
        let tool = self
            .registry
            .get(tool_name)
            .ok_or_else(|| DispatchError::UnknownTool(tool_name.to_string()))?;
        let descriptor = tool.descriptor();
        let tier = descriptor.tier();

        // 3. Authorize. Errors here MUST NOT have written a manifest row —
        //    auth failures are pre-manifest by design (matches the plan's
        //    fail-closed-without-audit-noise tier).
        match tier {
            ToolTier::Agent => self.auth.authorize_agent(principal, tool_name).await?,
            ToolTier::Operator => self.auth.authorize_operator(principal, tool_name).await?,
        };

        // 4. Redact raw args. Errors before begin_call also stay pre-manifest.
        let redacted_args = redact_json(self.pipeline, self.session, &raw_args)
            .map_err(|e| DispatchError::Redaction(e.to_string()))?;

        // Generate the call id once and reuse it as the manifest handle.
        let call_id = Ulid::new();
        let started_at = SystemTime::now();

        // 5. Begin manifest entry. Past this point we MUST finalize via
        //    finish_call or fail_call before returning.
        let begin_ctx = BeginCallContext {
            call_id,
            external_session_id,
            principal_id: principal.id.as_str(),
            tool_name,
            redacted_args: &redacted_args,
            started_at,
        };
        let handle = self.manifest.begin_call(begin_ctx).await?;

        // 6. Build the sealed ToolCtx — pub(crate) constructor; this is the
        //    only call site in the entire crate.
        let audit_session_id_owned = match external_session_id {
            Some(sid) => sid.to_string(),
            // Mint a stable id from the call id when transport supplies none.
            // Adopters who want stronger correlation supply their own.
            None => call_id.to_string(),
        };
        let session_handle = SessionHandle::new(&audit_session_id_owned);
        let resources = ToolResources::new(
            self.pipeline,
            self.session,
            self.manifest,
            self.locale_chain,
        );
        let ctx = ToolCtx::new_with_resources(
            session_handle,
            resources,
            redacted_args.clone(),
            call_id,
            tool_name,
            principal.id.as_str(),
        );

        // 7. Invoke the tool. On error, fail_call MUST run before we return.
        let raw_response = match tool.invoke(&ctx).await {
            Ok(resp) => resp,
            Err(tool_err) => {
                let reason = FailureReason::ToolError {
                    class: tool_err.class().to_string(),
                    message: tool_err.to_string(),
                };
                // Manifest fail_call errors are propagated; they win over the
                // tool error because manifest persistence is the chokepoint
                // guarantee. The original tool error stays in the audit row.
                self.manifest.fail_call(handle, reason).await?;
                return Err(DispatchError::ToolError(tool_err));
            }
        };

        // 8. Redact the response payload unless an operator-tier descriptor
        //    explicitly opted out for restore/export semantics.
        let response_payload = match (tier, descriptor.response_redaction()) {
            (ToolTier::Agent, ResponseRedaction::BypassByOperator) => {
                let reason = FailureReason::Other {
                    message: "agent tool with BypassByOperator reached dispatch".to_string(),
                };
                self.manifest.fail_call(handle, reason).await?;
                return Err(DispatchError::Redaction(
                    "agent tool cannot bypass response redaction".to_string(),
                ));
            }
            (_, ResponseRedaction::Apply) => {
                match redact_json(self.pipeline, self.session, &raw_response.payload) {
                    Ok(value) => value,
                    Err(e) => {
                        let reason = FailureReason::RedactionFailed {
                            message: e.to_string(),
                        };
                        self.manifest.fail_call(handle, reason).await?;
                        return Err(DispatchError::Redaction(e.to_string()));
                    }
                }
            }
            (ToolTier::Operator, ResponseRedaction::BypassByOperator) => raw_response.payload,
        };

        // 9. Compute SnapshotRef on the redacted bytes (out-of-row metadata
        //    only — adopters who want byte-level persistence wrap their
        //    ManifestStore impl with their own snapshot store).
        let snapshot = match build_snapshot_ref(&audit_session_id_owned, call_id, &response_payload)
        {
            Ok(snap) => snap,
            Err(e) => {
                let reason = FailureReason::Other {
                    message: format!("response serialization failure: {e}"),
                };
                // Best effort fail_call; if even that fails, the manifest
                // error trumps the serialization error in the propagated
                // DispatchError chain.
                self.manifest.fail_call(handle, reason).await?;
                return Err(DispatchError::ResponseSerialization(e));
            }
        };

        // 10. Finalize the manifest entry. If finish_call fails, the redacted
        //     response is NOT returned — the chokepoint contract demands the
        //     response only escape after the manifest row is durable.
        self.manifest.finish_call(handle, snapshot).await?;

        Ok(ToolResponse::json(response_payload))
    }
}

/// Walk a JSON value and run the gaze pipeline on every string leaf. Numbers,
/// booleans, and nulls pass through unchanged. Object keys are preserved
/// verbatim (keys aren't redacted — the chokepoint contract redacts values,
/// not field names).
fn redact_json(
    pipeline: &gaze::Pipeline,
    session: &gaze::Session,
    value: &serde_json::Value,
) -> Result<serde_json::Value, gaze::Error> {
    use serde_json::Value as JsonValue;
    match value {
        JsonValue::String(s) => Ok(JsonValue::String(redact_json_string(pipeline, session, s)?)),
        JsonValue::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(redact_json(pipeline, session, item)?);
            }
            Ok(JsonValue::Array(out))
        }
        JsonValue::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), redact_json(pipeline, session, v)?);
            }
            Ok(JsonValue::Object(out))
        }
        // Null / Bool / Number — no string content to redact.
        other => Ok(other.clone()),
    }
}

fn redact_json_string(
    pipeline: &gaze::Pipeline,
    session: &gaze::Session,
    value: &str,
) -> Result<String, gaze::Error> {
    let mut out = String::with_capacity(value.len());
    let mut cursor = 0usize;
    for token in gaze::token_shape::pattern().find_iter(value) {
        if !session.contains_token(token.as_str()) {
            continue;
        }
        out.push_str(&redact_json_string_segment(
            pipeline,
            session,
            &value[cursor..token.start()],
        )?);
        out.push_str(token.as_str());
        cursor = token.end();
    }
    out.push_str(&redact_json_string_segment(
        pipeline,
        session,
        &value[cursor..],
    )?);
    Ok(out)
}

fn redact_json_string_segment(
    pipeline: &gaze::Pipeline,
    session: &gaze::Session,
    segment: &str,
) -> Result<String, gaze::Error> {
    if segment.is_empty() {
        return Ok(String::new());
    }
    let clean = pipeline.redact(session, gaze::RawDocument::Text(segment.to_string()))?;
    match clean {
        gaze::CleanDocument::Text(text) => Ok(text),
        _ => Err(gaze::Error::UnsupportedRawDocumentVariant),
    }
}

/// Compute a [`SnapshotRef`] over the canonical JSON bytes of `payload`.
///
/// The locator is `"inline-sha256:<hex>"` — we explicitly do NOT write the
/// bytes to a side store from the dispatcher (out-of-row guarantee per
/// scratchpad 1453). Adopters who want byte-level persistence wrap their
/// `ManifestStore` impl and persist before calling `finish_call`.
fn build_snapshot_ref(
    audit_session_id: &str,
    call_id: Ulid,
    payload: &serde_json::Value,
) -> Result<SnapshotRef, serde_json::Error> {
    let bytes = serde_json::to_vec(payload)?;
    let mut hasher = Sha256::new();
    hasher.update(audit_session_id.as_bytes());
    hasher.update([0u8]);
    hasher.update(call_id.to_bytes());
    hasher.update([0u8]);
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let sha256_hex = hex_lower(&digest);
    let locator = format!("inline-sha256:{call_id}");
    Ok(SnapshotRef::new(locator, sha256_hex, bytes.len() as u64))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Helper consumed by tests to inspect a [`CallHandle`] without exporting
/// internal types more broadly. Adopters never need this.
#[doc(hidden)]
pub fn _debug_call_handle(handle: CallHandle) -> Ulid {
    handle.id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze::{Action, ClassRule, DefaultRule, Detection, Detector, PiiClass, RawDocument};
    use serde_json::json;

    #[derive(Clone)]
    struct FixedDetector;

    impl Detector for FixedDetector {
        fn detect(&self, input: &str) -> Vec<Detection> {
            input
                .find("alice@example.invalid")
                .map(|start| {
                    Detection::new(
                        start..start + "alice@example.invalid".len(),
                        PiiClass::Email,
                        "fixed",
                    )
                })
                .into_iter()
                .collect()
        }
    }

    fn tokenizing_pipeline() -> gaze::Pipeline {
        gaze::Pipeline::builder()
            .detector(FixedDetector)
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .build()
            .expect("pipeline")
    }

    #[test]
    fn snapshot_ref_preimage_byte_sequence_exact() {
        let call_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("ulid");
        let payload = json!({"email": "alice@example.invalid"});

        let snapshot =
            build_snapshot_ref("audit-session", call_id, &payload).expect("snapshot ref");

        let payload_bytes = serde_json::to_vec(&payload).expect("payload bytes");
        let mut hasher = Sha256::new();
        hasher.update(b"audit-session");
        hasher.update([0u8]);
        hasher.update(call_id.to_bytes());
        hasher.update([0u8]);
        hasher.update(&payload_bytes);
        let expected = hex_lower(&hasher.finalize());

        assert_eq!(snapshot.sha256_hex, expected);
        assert_eq!(snapshot.byte_len, payload_bytes.len() as u64);
    }

    #[test]
    fn redact_json_preserves_session_owned_token_shapes() {
        let pipeline = tokenizing_pipeline();
        let session = gaze::Session::new(gaze::Scope::Ephemeral).expect("session");
        let tokenized = pipeline
            .redact(
                &session,
                RawDocument::Text("alice@example.invalid".to_string()),
            )
            .expect("redact");
        let token = match tokenized {
            gaze::CleanDocument::Text(text) => text,
            _ => panic!("expected text"),
        };

        let payload = json!(format!("{token}alice@example.invalid"));
        let redacted = redact_json(&pipeline, &session, &payload).expect("redact json");

        assert_eq!(redacted, json!(format!("{token}{token}")));
    }
}
