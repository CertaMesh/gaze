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

mod rule_coverage;

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

/// Typed dispatch failures. Errors before begin have no manifest handle.
/// Open handles receive one terminal attempt; persistence failure prevents
/// response egress and must not trigger another terminal call.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DispatchError {
    /// Host-only context admission failed before invoking or mutating the session.
    #[error("{0}")]
    HostContext(#[from] crate::HostContextError),
    /// The entry point does not match the registered request contract.
    #[error("request mode rejected")]
    RequestMode,
    /// Aggregate restored expansion exceeded the MCP operation budget.
    #[error("MCP operation resource limit exceeded")]
    ResourceLimit,
    /// Strict boundary failure, containing class-only diagnostics.
    #[error("protection failed: {0}")]
    Protection(#[from] gaze::ProtectionError),
    /// Producer carrier contract violation, containing no input paths or values.
    #[error("carrier rejected: {0}")]
    Carrier(#[from] crate::CarrierError),
    /// Concurrent live mutation invalidated operation staging.
    #[error("session transaction conflict")]
    Transaction(#[from] gaze::SessionTransactionError),
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
    /// Default-empty unless explicitly supplied by the host.
    pub dictionaries: &'a gaze::DictionaryBundle,
    /// Transport-supplied session id validation policy.
    pub session_id_policy: &'a SessionIdPolicy,
    host_context_policy: crate::HostContextPolicy,
    context_policy: Option<(&'a gaze::Policy, &'a [gaze::Rulepack])>,
}

enum RequestState {
    Protected(serde_json::Value),
    Untrusted(crate::UntrustedInvocationArgs),
}

impl RequestState {
    fn mode(&self) -> crate::RequestMode {
        match self {
            Self::Protected(_) => crate::RequestMode::Protected,
            Self::Untrusted(_) => crate::RequestMode::UntrustedInvocation,
        }
    }
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
            context_policy: None,
            host_context_policy: crate::HostContextPolicy::default(),
            dictionaries: default_dictionaries(),
            registry,
            auth,
            manifest,
            pipeline,
            session,
            locale_chain,
            session_id_policy,
        }
    }

    /// Supply dictionaries without changing the compatible empty-bundle constructor.
    pub fn with_dictionaries(mut self, dictionaries: &'a gaze::DictionaryBundle) -> Self {
        self.dictionaries = dictionaries;
        self
    }

    /// Supply the exact policy and rulepacks used to assemble the resident pipeline.
    /// Present context is rejected when these inputs are absent.
    pub fn with_context_policy(
        mut self,
        policy: &'a gaze::Policy,
        rulepacks: &'a [gaze::Rulepack],
    ) -> Self {
        self.context_policy = Some((policy, rulepacks));
        self
    }

    /// Set the record-independent argument budget for calls with Present context.
    /// Raising limits increases the per-call guess budget; hosts must rate-limit calls.
    pub fn with_host_context_policy(mut self, policy: crate::HostContextPolicy) -> Self {
        self.host_context_policy = policy;
        self
    }

    /// Protect one declared JSON argument operation, invoke with live resources,
    /// then protect a fresh response operation. Egress requires response commit
    /// and successful manifest finish. A failed terminal attempt is never retried;
    /// failed finish retains committed mappings but returns no response.
    pub async fn dispatch(
        &self,
        principal: &Principal,
        tool_name: &str,
        raw_args: serde_json::Value,
        external_session_id: Option<&str>,
        host_context: crate::HostContextSelection,
    ) -> Result<ToolResponse, DispatchError> {
        self.dispatch_with(
            principal,
            tool_name,
            RequestState::Protected(raw_args),
            external_session_id,
            host_context,
        )
        .await
    }

    /// Invoke an opted-in tool without detecting or transforming request data.
    /// Only a constant metadata record is audited. Tools must validate locally;
    /// response protection, commit and durable finish remain mandatory.
    pub async fn dispatch_request(
        &self,
        principal: &Principal,
        tool_name: &str,
        args: crate::UntrustedInvocationArgs,
        external_session_id: Option<&str>,
        host_context: crate::HostContextSelection,
    ) -> Result<ToolResponse, DispatchError> {
        self.dispatch_with(
            principal,
            tool_name,
            RequestState::Untrusted(args),
            external_session_id,
            host_context,
        )
        .await
    }

    async fn dispatch_with(
        &self,
        principal: &Principal,
        tool_name: &str,
        request: RequestState,
        external_session_id: Option<&str>,
        host_context: crate::HostContextSelection,
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
        if descriptor.request_mode() != request.mode()
            || (request.mode() == crate::RequestMode::UntrustedInvocation
                && descriptor.response_redaction() != ResponseRedaction::Apply)
        {
            return Err(DispatchError::RequestMode);
        }

        // 3. Authorize. Errors here MUST NOT have written a manifest row —
        //    auth failures are pre-manifest by design (matches the plan's
        //    fail-closed-without-audit-noise tier).
        match tier {
            ToolTier::Agent => self.auth.authorize_agent(principal, tool_name).await?,
            ToolTier::Operator => self.auth.authorize_operator(principal, tool_name).await?,
        };

        // Admit argument size before parsing a record, inspecting session mappings,
        // or allocating private labels. Rejection cannot depend on record content.
        let argument_text = if matches!(host_context, crate::HostContextSelection::Present(_)) {
            let args = match &request {
                RequestState::Protected(args) => args,
                RequestState::Untrusted(args) => args.as_value(),
            };
            argument_guess_material(args, self.host_context_policy)?
        } else {
            ArgumentGuessMaterial::default()
        };

        // Static policy aliases let matched private custom classes remain protected
        // on later calls without retaining a record or context class map.
        crate::host_context::validate_session(self.session)?;
        let mut host_context = crate::host_context::parse(host_context)?;
        if host_context.is_some() && descriptor.response_redaction() != ResponseRedaction::Apply {
            return Err(crate::HostContextError::Unsupported.into());
        }
        let aliased_pipeline = self
            .context_policy
            .filter(|_| host_context.is_some() || self.session.has_opaque_identifier_key())
            .map(|(policy, _)| {
                let aliases = crate::host_context::policy_aliases(self.session, policy)?;
                Ok::<_, crate::HostContextError>(self.pipeline.clone().with_class_aliases(&aliases))
            })
            .transpose()?;
        let resident_pipeline = aliased_pipeline.as_ref().unwrap_or(self.pipeline);
        let response_inputs = if let Some(context) = host_context.as_mut() {
            let (policy, rulepacks) = self
                .context_policy
                .ok_or(crate::HostContextError::Unsupported)?;
            let policy = crate::host_context::opaque_context(self.session, context, policy)?;
            if context
                .class_map
                .values()
                .any(|class| !gaze_assembly::class_has_reversible_action(&policy.rules, class))
                || context
                    .dictionaries
                    .keys()
                    .any(|name| self.dictionaries.get(name).is_some())
            {
                return Err(crate::HostContextError::Policy.into());
            }
            // Validate the complete record before argument-dependent exclusions, so
            // a guessed term cannot change whether the policy admits the call.
            let pipeline = gaze_assembly::pipeline_with_private_context(
                resident_pipeline,
                &policy,
                context,
                rulepacks,
                &gaze::LocaleChain::from(self.locale_chain),
            )
            .map_err(|_| crate::HostContextError::Policy)?;
            exclude_argument_guesses(context, &argument_text)?;
            let dictionaries = gaze::DictionaryBundle::merge(
                self.dictionaries.clone(),
                gaze::dictionary_bundle_from_context(context),
            );
            Some((pipeline, dictionaries))
        } else {
            None
        };

        // 4. Prepare the registered request contract before opening audit.
        let context = gaze::ProtectionContext::strict(self.locale_chain, self.dictionaries);
        let (redacted_args, args_transaction, invocation_args, args_audit) = match request {
            RequestState::Protected(raw_args) => {
                descriptor.argument_carriers().preflight(&raw_args)?;
                let mut transaction = self.session.begin_transaction();
                let pre_commit_token_count = transaction.tokens().len();
                let protected =
                    protect_json(resident_pipeline, &mut transaction, context, &raw_args)?;
                (
                    protected,
                    Some((transaction, pre_commit_token_count)),
                    None,
                    None,
                )
            }
            RequestState::Untrusted(args) => (
                serde_json::Value::Null,
                None,
                Some(args),
                Some(
                    serde_json::json!({"gaze_request_audit": 1, "mode": "metadata_only", "arguments": "omitted"}),
                ),
            ),
        };

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
            args_audit: args_audit.as_ref(),
            started_at,
        };
        let handle = self.manifest.begin_call(begin_ctx).await?;
        if let Some((transaction, pre_commit_token_count)) = args_transaction {
            if transaction.tokens().len() != pre_commit_token_count {
                if let Err(error) = transaction.commit() {
                    self.manifest
                        .fail_call(
                            handle,
                            FailureReason::RedactionFailed {
                                message: "session transaction conflict".into(),
                            },
                        )
                        .await?;
                    return Err(error.into());
                }
            } else {
                std::mem::drop(transaction);
            }
        }

        // 6. Build the sealed ToolCtx — pub(crate) constructor; this is the
        //    only call site in the entire crate.
        let audit_session_id_owned = match external_session_id {
            Some(sid) => sid.to_string(),
            // Mint a stable id from the call id when transport supplies none.
            // Adopters who want stronger correlation supply their own.
            None => call_id.to_string(),
        };
        let session_handle = SessionHandle::new(&audit_session_id_owned);
        let resources = ToolResources::new_with_dictionaries(
            resident_pipeline,
            self.session,
            self.manifest,
            self.locale_chain,
            self.dictionaries,
        );
        let ctx = ToolCtx::new_with_resources(
            session_handle,
            resources,
            redacted_args.clone(),
            call_id,
            tool_name,
            principal.id.as_str(),
        )
        .with_invocation_args(invocation_args);

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

        // Response staging starts after invoke, preserving legitimate live tool changes.
        let mut response_transaction = None;
        let response_result: Result<_, DispatchError> =
            match (tier, descriptor.response_redaction()) {
                (ToolTier::Agent, ResponseRedaction::BypassByOperator) => {
                    Err(DispatchError::Redaction("agent bypass rejected".into()))
                }
                (_, ResponseRedaction::Apply) => {
                    match descriptor
                        .response_carriers()
                        .preflight(&raw_response.payload)
                    {
                        Err(error) => Err(error.into()),
                        Ok(()) => {
                            let mut transaction = self.session.begin_transaction();
                            let result = protect_json(
                                response_inputs
                                    .as_ref()
                                    .map_or(resident_pipeline, |(pipeline, _)| pipeline),
                                &mut transaction,
                                gaze::ProtectionContext::strict(
                                    self.locale_chain,
                                    response_inputs
                                        .as_ref()
                                        .map_or(self.dictionaries, |(_, dictionaries)| {
                                            dictionaries
                                        }),
                                ),
                                &raw_response.payload,
                            );
                            response_transaction = Some(transaction);
                            result
                        }
                    }
                }
                (ToolTier::Operator, ResponseRedaction::BypassByOperator) => {
                    Ok(raw_response.payload)
                }
            };
        let response_payload = match response_result {
            Ok(payload) => payload,
            Err(error) => {
                self.manifest
                    .fail_call(
                        handle,
                        FailureReason::RedactionFailed {
                            message: error.to_string(),
                        },
                    )
                    .await?;
                return Err(error);
            }
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
        if let Some(transaction) = response_transaction {
            if let Err(error) = transaction.commit() {
                self.manifest
                    .fail_call(
                        handle,
                        FailureReason::RedactionFailed {
                            message: "session transaction conflict".into(),
                        },
                    )
                    .await?;
                return Err(error.into());
            }
        }
        // A terminal attempt consumes the handle even when persistence fails.
        // A failed finish retains committed response mappings but returns no payload.
        self.manifest.finish_call(handle, snapshot).await?;

        Ok(ToolResponse::json(response_payload))
    }
}

#[cfg(test)]
thread_local! { static PAIR_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[derive(Default)]
struct ArgumentGuessMaterial {
    texts: Vec<gaze_recognizers::RecordGuessText>,
    scalars: Vec<Vec<usize>>,
}

fn argument_guess_material(
    args: &serde_json::Value,
    policy: crate::HostContextPolicy,
) -> Result<ArgumentGuessMaterial, crate::HostContextError> {
    let mut material = ArgumentGuessMaterial::default();
    let mut interned = std::collections::HashMap::<String, usize>::new();
    for decode_json in [false, true] {
        let mut scalars = Vec::new();
        let mut raw_bytes = 0usize;
        let mut normalized_bytes = 0usize;
        let mut entries = 0usize;
        visit_argument_text(args, decode_json, &mut |entry| {
            entries = entries
                .checked_add(1)
                .filter(|size| *size <= policy.max_argument_bytes)
                .ok_or(crate::HostContextError::ArgumentLimit)?;
            let Some((text, is_scalar)) = entry else {
                return Ok(());
            };
            raw_bytes = raw_bytes
                .checked_add(text.len())
                .filter(|size| *size <= policy.max_argument_bytes)
                .ok_or(crate::HostContextError::ArgumentLimit)?;
            let index = *interned.entry(text.to_owned()).or_insert_with(|| {
                let index = material.texts.len();
                material
                    .texts
                    .push(gaze_recognizers::RecordGuessText::new(text));
                index
            });
            normalized_bytes = normalized_bytes
                .checked_add(material.texts[index].budget_bytes())
                .filter(|size| *size <= policy.max_argument_bytes)
                .ok_or(crate::HostContextError::ArgumentLimit)?;
            if is_scalar {
                if scalars.len() >= policy.max_argument_scalars {
                    return Err(crate::HostContextError::ArgumentLimit);
                }
                scalars.push(index);
            }
            Ok(())
        })?;
        material.scalars.push(scalars);
    }
    Ok(material)
}

fn exclude_argument_guesses(
    context: &mut gaze::Context,
    material: &ArgumentGuessMaterial,
) -> Result<(), crate::HostContextError> {
    use gaze_recognizers::{RecordGuessMatcher, RecordGuessMode, RecordGuessTerm, RecordGuessText};
    let mut keys = Vec::new();
    let mut terms = Vec::new();
    for (name, dictionary) in &context.dictionaries {
        for term in &dictionary.terms {
            let mode = if name.starts_with(gaze::RECORD_DICTIONARY_PREFIX) {
                let class = context
                    .class_map
                    .get(name)
                    .ok_or(crate::HostContextError::Policy)?;
                RecordGuessMode::Record {
                    class: class.clone(),
                    allowed_kinds: context.record_allowed_match_kinds(class, term),
                }
            } else {
                RecordGuessMode::Dictionary {
                    case_sensitive: dictionary.case_sensitive,
                }
            };
            keys.push((name, term));
            terms.push(RecordGuessTerm { value: term, mode });
        }
    }
    let mut matcher =
        RecordGuessMatcher::new(terms).map_err(|_| crate::HostContextError::Policy)?;
    for text in &material.texts {
        matcher.check(text);
    }
    let separators = ["", " ", "@", "-", "/"].map(RecordGuessText::new);
    for scalars in &material.scalars {
        let texts = scalars
            .iter()
            .map(|index| &material.texts[*index])
            .collect::<Vec<_>>();
        for separator in &separators[..2] {
            matcher.check_join(&texts, separator);
        }
        for (left_index, left) in texts.iter().enumerate() {
            for (right_index, right) in texts.iter().enumerate() {
                if left_index != right_index {
                    for separator in &separators {
                        #[cfg(test)]
                        PAIR_SCANS.with(|count| count.set(count.get() + 1));
                        matcher.check_pair(left, separator, right);
                    }
                }
            }
        }
    }
    let excluded = keys
        .into_iter()
        .zip(matcher.matches())
        .filter(|(_, matched)| **matched)
        .map(|((name, term), _)| ((name.clone(), term.clone()), ()))
        .collect::<std::collections::HashMap<_, _>>();
    drop(matcher);
    for (name, dictionary) in &mut context.dictionaries {
        dictionary
            .terms
            .retain(|term| !excluded.contains_key(&(name.clone(), term.clone())));
    }
    context
        .dictionaries
        .retain(|_, dictionary| !dictionary.terms.is_empty());
    Ok(())
}

fn visit_argument_text(
    value: &serde_json::Value,
    decode_json: bool,
    visit: &mut impl FnMut(Option<(&str, bool)>) -> Result<(), crate::HostContextError>,
) -> Result<(), crate::HostContextError> {
    fn text(
        value: &str,
        is_scalar: bool,
        remaining: &mut usize,
        visit: &mut impl FnMut(Option<(&str, bool)>) -> Result<(), crate::HostContextError>,
    ) -> Result<(), crate::HostContextError> {
        *remaining = remaining
            .checked_sub(value.len())
            .ok_or(crate::HostContextError::Invalid)?;
        visit(Some((value, is_scalar)))
    }
    fn walk(
        value: &serde_json::Value,
        depth: usize,
        decode_json: bool,
        remaining: &mut usize,
        visit: &mut impl FnMut(Option<(&str, bool)>) -> Result<(), crate::HostContextError>,
    ) -> Result<(), crate::HostContextError> {
        if depth > 128 {
            return Err(crate::HostContextError::Invalid);
        }
        match value {
            serde_json::Value::String(value) => {
                let nested = decode_json
                    .then(|| serde_json::from_str::<serde_json::Value>(value).ok())
                    .flatten();
                text(value, nested.is_none(), remaining, visit)?;
                if let Some(nested) = nested {
                    walk(&nested, depth + 1, decode_json, remaining, visit)?;
                }
            }
            serde_json::Value::Array(values) => {
                visit(None)?;
                for value in values {
                    walk(value, depth + 1, decode_json, remaining, visit)?;
                }
            }
            serde_json::Value::Object(values) => {
                visit(None)?;
                for (key, value) in values {
                    text(key, false, remaining, visit)?;
                    walk(value, depth + 1, decode_json, remaining, visit)?;
                }
            }
            serde_json::Value::Number(value) => text(&value.to_string(), true, remaining, visit)?,
            serde_json::Value::Bool(value) => text(
                if *value { "true" } else { "false" },
                true,
                remaining,
                visit,
            )?,
            serde_json::Value::Null => text("null", true, remaining, visit)?,
        }
        Ok(())
    }
    let mut remaining = rule_coverage::MAX_OPERATION_EXPANDED_BYTES;
    walk(value, 0, decode_json, &mut remaining, visit)
}

pub(crate) fn default_dictionaries() -> &'static gaze::DictionaryBundle {
    static DEFAULT: std::sync::LazyLock<gaze::DictionaryBundle> =
        std::sync::LazyLock::new(gaze::DictionaryBundle::default);
    &DEFAULT
}

fn protect_json(
    pipeline: &gaze::Pipeline,
    transaction: &mut gaze::SessionTransaction<'_>,
    context: gaze::ProtectionContext<'_>,
    value: &serde_json::Value,
) -> Result<serde_json::Value, DispatchError> {
    pipeline.validate_protection_context(context)?;
    // Freeze interpretation for the whole operation, not just the current leaf.
    let original_tokens = transaction.tokens();
    // Expand every original before discovery can mint any token spelling. A later
    // literal must never acquire ownership from an earlier discovery leaf.
    let mut expanded = Vec::new();
    let mut remaining = rule_coverage::MAX_OPERATION_EXPANDED_BYTES;
    visit_text(value, &mut |text| {
        let view = rule_coverage::expand_owned_bounded(transaction, text, remaining)?;
        remaining -= view.len();
        if !view.is_empty() {
            expanded.push(view);
        }
        Ok(())
    })?;
    for view in expanded {
        pipeline
            .discover_staged_rule_sources(
                transaction,
                &view,
                context.locale_chain(),
                context.dictionaries(),
            )
            .map_err(|refusal| refusal.error)?;
    }
    let output = protect_json_leaves(pipeline, transaction, context, value)?;
    verify_json_rule_coverage(pipeline, transaction, &output)?;
    for token in transaction
        .tokens()
        .iter()
        .filter(|token| !original_tokens.contains(token))
    {
        if json_contains_literal(value, token) {
            return Err(gaze::ProtectionError::Provenance.into());
        }
    }
    Ok(output)
}

fn visit_text(
    value: &serde_json::Value,
    visit: &mut impl FnMut(&str) -> Result<(), DispatchError>,
) -> Result<(), DispatchError> {
    match value {
        serde_json::Value::String(text) => visit(text),
        serde_json::Value::Array(values) => {
            values.iter().try_for_each(|value| visit_text(value, visit))
        }
        serde_json::Value::Object(values) => values
            .values()
            .try_for_each(|value| visit_text(value, visit)),
        _ => Ok(()),
    }
}

fn verify_json_rule_coverage(
    pipeline: &gaze::Pipeline,
    transaction: &gaze::SessionTransaction<'_>,
    value: &serde_json::Value,
) -> Result<(), DispatchError> {
    visit_text(value, &mut |text| {
        rule_coverage::verify_rule_coverage(pipeline, transaction, text)
    })
}

fn json_contains_literal(value: &serde_json::Value, token: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text.contains(token),
        serde_json::Value::Array(values) => values.iter().any(|v| json_contains_literal(v, token)),
        serde_json::Value::Object(values) => {
            values.values().any(|v| json_contains_literal(v, token))
        }
        _ => false,
    }
}

fn protect_json_leaves(
    pipeline: &gaze::Pipeline,
    transaction: &mut gaze::SessionTransaction<'_>,
    context: gaze::ProtectionContext<'_>,
    value: &serde_json::Value,
) -> Result<serde_json::Value, DispatchError> {
    use serde_json::Value as JsonValue;
    match value {
        JsonValue::String(text) => {
            let expected = rule_coverage::expand_owned_bounded(
                transaction,
                text,
                rule_coverage::MAX_OPERATION_EXPANDED_BYTES,
            )?;
            let primary = pipeline.protect_text_transaction(transaction, text, context)?;
            let protected =
                rule_coverage::cover_rule_repeats(pipeline, transaction, text, &primary)?;
            if rule_coverage::expand_owned_bounded(
                transaction,
                &protected,
                rule_coverage::MAX_OPERATION_EXPANDED_BYTES,
            )? != expected
            {
                return Err(gaze::ProtectionError::Provenance.into());
            }
            if protected != primary {
                pipeline.admit_safety_nets_transaction(
                    transaction,
                    &protected,
                    context.locale_chain(),
                    context.dictionaries(),
                )?;
            }
            Ok(JsonValue::String(protected))
        }
        JsonValue::Array(values) => values
            .iter()
            .map(|value| protect_json_leaves(pipeline, transaction, context, value))
            .collect::<Result<Vec<_>, _>>()
            .map(JsonValue::Array),
        JsonValue::Object(values) => values
            .iter()
            .map(|(key, value)| {
                Ok((
                    key.clone(),
                    protect_json_leaves(pipeline, transaction, context, value)?,
                ))
            })
            .collect::<Result<serde_json::Map<_, _>, _>>()
            .map(JsonValue::Object),
        other => Ok(other.clone()),
    }
}

/// Compute a [`SnapshotRef`] over the canonical JSON bytes of `payload`.
///
/// The locator is `"inline-sha256:<hex>"` — we explicitly do NOT write the
/// bytes to a side store from the dispatcher (out-of-row guarantee per
/// (internal design note). Adopters who want byte-level persistence wrap their
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

    #[test]
    fn host_argument_identical_views_scan_pairs_once() {
        let policy = crate::HostContextPolicy::default();
        let plain = json!((0..32).map(|i| format!("Synthetic{i}")).collect::<Vec<_>>());
        let encoded = json!((0..32)
            .map(|i| format!("\"Synthetic{i}\""))
            .collect::<Vec<_>>());
        for (args, views) in [(plain, 1), (encoded, 2)] {
            let material = argument_guess_material(&args, policy).unwrap();
            let mut context =
                gaze::Context::from_json_str(r#"{"record":{"name":"Synthetic Qorvex"}}"#).unwrap();
            PAIR_SCANS.with(|count| count.set(0));
            exclude_argument_guesses(&mut context, &material).unwrap();
            assert_eq!(PAIR_SCANS.with(|count| count.get()), views * 32 * 31 * 5);
        }
    }

    #[test]
    fn host_argument_entry_budget_counts_every_structural_entry_and_empty_key() {
        let policy = crate::HostContextPolicy {
            max_argument_bytes: 8,
            max_argument_scalars: 2,
        };
        for (args, admitted) in [
            (json!([[], [], [], [], [], [], []]), true),
            (json!([[], [], [], [], [], [], [], []]), false),
            (json!([{"":[]}, {"":[]}]), true),
            (json!([{"":[]}, {"":[]}, {"":[]}]), false),
        ] {
            assert_eq!(
                super::argument_guess_material(&args, policy).is_ok(),
                admitted,
                "{args}"
            );
        }
    }

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
    fn final_rule_coverage_rejects_raw_output_independently_of_projection() {
        let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
        let pipeline = core.pipeline();
        let session = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
        let mut transaction = session.begin_transaction();
        pipeline
            .discover_staged_rule_sources(
                &mut transaction,
                "alice@example.invalid",
                core.locale_chain().as_slice(),
                default_dictionaries(),
            )
            .unwrap();
        assert!(
            matches!(
                verify_json_rule_coverage(
                    pipeline,
                    &transaction,
                    &json!(["alice@example.invalid"])
                ),
                Err(DispatchError::Protection(gaze::ProtectionError::Residual)),
            ),
            "raw Rule repeat accepted by final proof",
        );
        let protected = pipeline
            .protect_text_transaction(
                &mut transaction,
                "alice@example.invalid",
                gaze::ProtectionContext::strict(
                    core.locale_chain().as_slice(),
                    default_dictionaries(),
                ),
            )
            .unwrap();
        verify_json_rule_coverage(pipeline, &transaction, &json!([protected])).unwrap();
        assert!(session.tokens().is_empty());
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
        let mut transaction = session.begin_transaction();
        let redacted = protect_json(
            &pipeline,
            &mut transaction,
            gaze::ProtectionContext::strict(&[gaze::LocaleTag::Global], default_dictionaries()),
            &payload,
        )
        .expect("protect json");

        assert_eq!(redacted, json!(format!("{token}{token}")));
    }
}
