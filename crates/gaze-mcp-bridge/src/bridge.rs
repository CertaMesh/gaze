use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use gaze::{CleanDocument, RawDocument};
use gaze_assembly::CorePipelineConfig;
use gaze_mcp_core::{
    AuthHook, DenyAllAuthHook, DispatchError, DispatchHost, Principal, SessionIdError,
    SessionIdPolicy, ToolDescriptor, ToolResponse,
};
use rmcp::model::{CallToolResult, RawContent};
use ulid::Ulid;

use crate::approval::{ApprovalDecision, ApprovalHook, ApprovalRequest, DenyApprovalHook};
use crate::audit::{ArgPath, BridgeAuditEvent, BridgeAuditSink, ResultPath};
use crate::client::{DownstreamClient, DownstreamError, RmcpChildClient};
use crate::config::BridgeConfig;
use crate::error::{BridgeError, BridgeResult};
use crate::policy::{DecisionOutcome, ResolvedToolPolicy, ResultMode};
use crate::registry::{BridgeRegistry, NamespacedTool};
use crate::session_store::BridgeSessionStore;

pub struct BridgeHost {
    registry: Arc<BridgeRegistry>,
    auth: Arc<dyn AuthHook>,
    approval: Arc<dyn ApprovalHook>,
    audit: Arc<dyn BridgeAuditSink>,
    session_store: Arc<BridgeSessionStore>,
    pipeline: gaze::Pipeline,
    locale_chain: Vec<gaze::LocaleTag>,
    session_id_policy: SessionIdPolicy,
    policy: crate::policy::PolicyConfig,
    limits: crate::config::LimitCfg,
}

impl BridgeHost {
    pub async fn from_config(
        config: BridgeConfig,
        audit: Arc<dyn BridgeAuditSink>,
    ) -> BridgeResult<Self> {
        let mut clients = BTreeMap::<String, Arc<dyn DownstreamClient>>::new();
        for (name, server) in &config.servers {
            clients.insert(name.clone(), RmcpChildClient::spawn(server).await?);
        }
        let registry =
            Arc::new(BridgeRegistry::discover(clients, config.limits.call_timeout()).await?);
        let session_store = Arc::new(BridgeSessionStore::from_config(&config.session)?);
        let core = CorePipelineConfig::new()
            .build()
            .map_err(|err| BridgeError::Config(format!("core pipeline build failed: {err}")))?;
        let locale_chain = core.locale_chain().as_slice().to_vec();
        Ok(BridgeHostBuilder::new(registry, session_store, audit)
            .pipeline(core.into_pipeline(), locale_chain)
            .policy(config.policy)
            .limits(config.limits)
            .build())
    }

    pub fn registry(&self) -> &BridgeRegistry {
        &self.registry
    }

    async fn dispatch_inner(
        &self,
        principal: &Principal,
        tool_name: &str,
        raw_args: serde_json::Value,
        external_session_id: Option<&str>,
    ) -> BridgeResult<ToolResponse> {
        let session_id = external_session_id.ok_or(SessionIdError::Empty)?;
        self.session_id_policy.validate(session_id)?;
        self.auth.authorize_agent(principal, tool_name).await?;

        let tool = self
            .registry
            .get(tool_name)
            .ok_or_else(|| BridgeError::UnknownTool(tool_name.to_string()))?;
        let resolved_policy =
            self.policy
                .resolve(&tool.server, &tool.namespaced_name, &tool.sanitized_tool);

        let call_id = Ulid::new().to_string();
        let shared_session = self.session_store.get(session_id).await?;
        let session_guard = shared_session.lock().await;

        let egress = process_egress(
            &self.pipeline,
            &self.locale_chain,
            &session_guard,
            &resolved_policy,
            &raw_args,
        )?;
        if let Some(blocked) = egress.blocked_response.clone() {
            self.record_audit(AuditRecord {
                call_id: &call_id,
                session_id,
                tool,
                arg_paths: egress.arg_paths.clone(),
                result_paths: Vec::new(),
                outcome: DecisionOutcome::Blocked,
                deciding_rule: egress.deciding_rule.clone(),
            })
            .await?;
            return Ok(blocked);
        }

        if egress.requires_approval {
            let approval_request = ApprovalRequest::new(
                call_id.clone(),
                session_id.to_string(),
                &raw_args,
                egress.token_classes.clone(),
                &egress.tokens,
                tool.namespaced_name.clone(),
                tool.discovery_hash.clone(),
                "sensitive token restore requires approval",
                egress.arg_paths.clone(),
            );
            match self.approval.decide(&approval_request).await {
                ApprovalDecision::Granted => {}
                ApprovalDecision::Denied | ApprovalDecision::Pending => {
                    self.record_audit(AuditRecord {
                        call_id: &call_id,
                        session_id,
                        tool,
                        arg_paths: egress.arg_paths.clone(),
                        result_paths: Vec::new(),
                        outcome: DecisionOutcome::RequiresApproval,
                        deciding_rule: "approval.required".to_string(),
                    })
                    .await?;
                    return Ok(approval_required_response(&approval_request));
                }
            }
        }

        self.record_audit(AuditRecord {
            call_id: &call_id,
            session_id,
            tool,
            arg_paths: egress.arg_paths.clone(),
            result_paths: Vec::new(),
            outcome: DecisionOutcome::Allowed,
            deciding_rule: egress.deciding_rule.clone(),
        })
        .await?;

        let downstream_result = tool
            .client
            .call_tool(
                &tool.raw_tool,
                egress.restored_args,
                self.limits.call_timeout(),
            )
            .await;
        let processed = match downstream_result {
            Ok(result) => process_ingress(
                &self.pipeline,
                &self.locale_chain,
                &session_guard,
                &resolved_policy,
                &self.limits,
                result,
            )?,
            Err(DownstreamError::Mcp { message, data }) => process_downstream_error(
                &self.pipeline,
                &self.locale_chain,
                &session_guard,
                message,
                data,
            )?,
            Err(DownstreamError::Service(_)) => process_downstream_service_error()?,
        };

        self.session_store
            .persist(session_id, &session_guard)
            .await?;
        self.record_audit(AuditRecord {
            call_id: &call_id,
            session_id,
            tool,
            arg_paths: Vec::new(),
            result_paths: processed.result_paths,
            outcome: DecisionOutcome::Allowed,
            deciding_rule: "ingress.processed".to_string(),
        })
        .await?;
        Ok(processed.response)
    }

    async fn record_audit(&self, record: AuditRecord<'_>) -> BridgeResult<()> {
        let mut event = BridgeAuditEvent::new(
            record.call_id.to_string(),
            record.session_id,
            record.tool.server.clone(),
            record.tool.sanitized_tool.clone(),
            record.outcome,
            record.deciding_rule,
        );
        event.arg_paths_affected = record.arg_paths;
        event.result_paths_affected = record.result_paths;
        self.audit.record(&event).await
    }
}

struct AuditRecord<'a> {
    call_id: &'a str,
    session_id: &'a str,
    tool: &'a NamespacedTool,
    arg_paths: Vec<ArgPath>,
    result_paths: Vec<ResultPath>,
    outcome: DecisionOutcome,
    deciding_rule: String,
}

#[async_trait]
impl DispatchHost for BridgeHost {
    async fn dispatch(
        &self,
        principal: &Principal,
        tool_name: &str,
        raw_args: serde_json::Value,
        external_session_id: Option<&str>,
    ) -> Result<ToolResponse, DispatchError> {
        self.dispatch_inner(principal, tool_name, raw_args, external_session_id)
            .await
            .map_err(BridgeError::into_dispatch)
    }

    fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.registry.list_descriptors()
    }
}

pub struct BridgeHostBuilder {
    registry: Arc<BridgeRegistry>,
    session_store: Arc<BridgeSessionStore>,
    audit: Arc<dyn BridgeAuditSink>,
    auth: Arc<dyn AuthHook>,
    approval: Arc<dyn ApprovalHook>,
    pipeline: Option<(gaze::Pipeline, Vec<gaze::LocaleTag>)>,
    session_id_policy: SessionIdPolicy,
    policy: crate::policy::PolicyConfig,
    limits: crate::config::LimitCfg,
}

impl BridgeHostBuilder {
    pub fn new(
        registry: Arc<BridgeRegistry>,
        session_store: Arc<BridgeSessionStore>,
        audit: Arc<dyn BridgeAuditSink>,
    ) -> Self {
        Self {
            registry,
            session_store,
            audit,
            auth: Arc::new(DenyAllAuthHook),
            approval: Arc::new(DenyApprovalHook),
            pipeline: None,
            session_id_policy: SessionIdPolicy::default_strict(),
            policy: crate::policy::PolicyConfig::default(),
            limits: crate::config::LimitCfg::default(),
        }
    }

    pub fn auth(mut self, auth: Arc<dyn AuthHook>) -> Self {
        self.auth = auth;
        self
    }

    pub fn approval(mut self, approval: Arc<dyn ApprovalHook>) -> Self {
        self.approval = approval;
        self
    }

    pub fn pipeline(
        mut self,
        pipeline: gaze::Pipeline,
        locale_chain: Vec<gaze::LocaleTag>,
    ) -> Self {
        self.pipeline = Some((pipeline, locale_chain));
        self
    }

    pub fn session_id_policy(mut self, policy: SessionIdPolicy) -> Self {
        self.session_id_policy = policy;
        self
    }

    pub fn policy(mut self, policy: crate::policy::PolicyConfig) -> Self {
        self.policy = policy;
        self
    }

    pub fn limits(mut self, limits: crate::config::LimitCfg) -> Self {
        self.limits = limits;
        self
    }

    pub fn build(self) -> BridgeHost {
        let (pipeline, locale_chain) = self.pipeline.unwrap_or_else(default_pipeline);
        BridgeHost {
            registry: self.registry,
            auth: self.auth,
            approval: self.approval,
            audit: self.audit,
            session_store: self.session_store,
            pipeline,
            locale_chain,
            session_id_policy: self.session_id_policy,
            policy: self.policy,
            limits: self.limits,
        }
    }
}

fn default_pipeline() -> (gaze::Pipeline, Vec<gaze::LocaleTag>) {
    let core = CorePipelineConfig::new()
        .build()
        .expect("bundled core pipeline should build");
    let locale_chain = core.locale_chain().as_slice().to_vec();
    (core.into_pipeline(), locale_chain)
}

#[derive(Clone)]
struct EgressResult {
    restored_args: serde_json::Value,
    arg_paths: Vec<ArgPath>,
    tokens: Vec<String>,
    token_classes: BTreeSet<String>,
    requires_approval: bool,
    blocked_response: Option<ToolResponse>,
    deciding_rule: String,
}

fn process_egress(
    pipeline: &gaze::Pipeline,
    locale_chain: &[gaze::LocaleTag],
    session: &gaze::Session,
    policy: &ResolvedToolPolicy,
    raw_args: &serde_json::Value,
) -> BridgeResult<EgressResult> {
    let mut state = EgressState {
        pipeline,
        locale_chain,
        session,
        policy,
        arg_paths: Vec::new(),
        tokens: Vec::new(),
        token_classes: BTreeSet::new(),
        requires_approval: false,
        blocked_response: None,
        deciding_rule: "egress.allowed".to_string(),
    };
    let restored_args = state.walk(raw_args, &ArgPath::root())?;
    Ok(EgressResult {
        restored_args,
        arg_paths: state.arg_paths,
        tokens: state.tokens,
        token_classes: state.token_classes,
        requires_approval: state.requires_approval,
        blocked_response: state.blocked_response,
        deciding_rule: state.deciding_rule,
    })
}

struct EgressState<'a> {
    pipeline: &'a gaze::Pipeline,
    locale_chain: &'a [gaze::LocaleTag],
    session: &'a gaze::Session,
    policy: &'a ResolvedToolPolicy,
    arg_paths: Vec<ArgPath>,
    tokens: Vec<String>,
    token_classes: BTreeSet<String>,
    requires_approval: bool,
    blocked_response: Option<ToolResponse>,
    deciding_rule: String,
}

impl EgressState<'_> {
    fn walk(
        &mut self,
        value: &serde_json::Value,
        path: &ArgPath,
    ) -> BridgeResult<serde_json::Value> {
        if self.blocked_response.is_some() {
            return Ok(value.clone());
        }
        match value {
            serde_json::Value::String(text) => self.process_string(text, path),
            serde_json::Value::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (idx, item) in items.iter().enumerate() {
                    out.push(self.walk(item, &path.child(format!("[{idx}]"))?)?);
                }
                Ok(serde_json::Value::Array(out))
            }
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::with_capacity(map.len());
                for (key, item) in map {
                    out.insert(key.clone(), self.walk(item, &path.child(key)?)?);
                }
                Ok(serde_json::Value::Object(out))
            }
            other => Ok(other.clone()),
        }
    }

    fn process_string(&mut self, text: &str, path: &ArgPath) -> BridgeResult<serde_json::Value> {
        let field_policy = self.policy.argument_policy(path.as_str());
        if contains_raw_pii(self.pipeline, self.locale_chain, text)? {
            self.arg_paths.push(path.clone());
            self.deciding_rule = "egress.raw_pii".to_string();
            self.blocked_response = Some(blocked_response("raw_pii_in_args", "egress.raw_pii"));
            return Ok(serde_json::Value::String(text.to_string()));
        }

        let matches = gaze::token_shape::find_tokens(text)
            .map(str::to_string)
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return Ok(serde_json::Value::String(text.to_string()));
        }
        self.arg_paths.push(path.clone());
        if !field_policy.allow_sensitive_fields {
            self.deciding_rule = "egress.token_not_allowed".to_string();
            self.blocked_response = Some(blocked_response(
                "sensitive_field_not_allowed",
                "egress.token_not_allowed",
            ));
            return Ok(serde_json::Value::String(text.to_string()));
        }
        if field_policy.requires_approval {
            self.requires_approval = true;
        }
        for token in &matches {
            if self.session.restore(token).is_none() {
                self.deciding_rule = "egress.unknown_token".to_string();
                self.blocked_response =
                    Some(blocked_response("unknown_token", "egress.unknown_token"));
                return Ok(serde_json::Value::String(text.to_string()));
            }
            self.token_classes.insert(token_class(token));
        }
        self.tokens.extend(matches);
        let restored = self
            .session
            .restore_strict_text(text)
            .map_err(|err| BridgeError::Policy(format!("unknown token rejected: {err}")))?;
        Ok(serde_json::Value::String(restored))
    }
}

struct IngressResult {
    response: ToolResponse,
    result_paths: Vec<ResultPath>,
}

fn process_ingress(
    pipeline: &gaze::Pipeline,
    locale_chain: &[gaze::LocaleTag],
    session: &gaze::Session,
    policy: &ResolvedToolPolicy,
    limits: &crate::config::LimitCfg,
    result: CallToolResult,
) -> BridgeResult<IngressResult> {
    let raw_len = serde_json::to_vec(&result)
        .map_err(|err| BridgeError::Downstream(format!("serialize result failed: {err}")))?
        .len();
    if raw_len > limits.response_bytes {
        return Ok(IngressResult {
            response: blocked_response("response_too_large", "ingress.limit.bytes"),
            result_paths: vec![ResultPath::root()],
        });
    }
    if result.content.len() > limits.content_blocks {
        return Ok(IngressResult {
            response: blocked_response("too_many_content_blocks", "ingress.limit.blocks"),
            result_paths: vec![ResultPath::root()],
        });
    }

    match policy.result.mode {
        ResultMode::Deny => {
            return Ok(IngressResult {
                response: blocked_response("result_denied", "ingress.result.deny"),
                result_paths: vec![ResultPath::root()],
            });
        }
        ResultMode::Allow => {
            return Ok(IngressResult {
                response: ToolResponse::json(
                    serde_json::to_value(result)
                        .map_err(|err| BridgeError::Downstream(err.to_string()))?,
                ),
                result_paths: Vec::new(),
            });
        }
        ResultMode::Process => {}
    }

    let mut result_paths = Vec::new();
    let mut content_values = Vec::with_capacity(result.content.len());
    for (idx, mut content) in result.content.into_iter().enumerate() {
        match &mut content.raw {
            RawContent::Text(text) => {
                let text_path = ResultPath::root()
                    .child("content")?
                    .child(format!("[{idx}]"))?
                    .child("text")?;
                let redacted = redact_text(pipeline, locale_chain, session, &text.text)?;
                if redacted != text.text {
                    result_paths.push(text_path);
                }
                text.text = redacted;
                if let Some(meta) = &mut text.meta {
                    let meta_value = serde_json::Value::Object(meta.0.clone());
                    let redacted_meta =
                        redact_json_value(pipeline, locale_chain, session, &meta_value)?;
                    if redacted_meta != meta_value {
                        result_paths.push(
                            ResultPath::root()
                                .child("content")?
                                .child(format!("[{idx}]"))?
                                .child("_meta")?,
                        );
                    }
                    if let serde_json::Value::Object(map) = redacted_meta {
                        meta.0 = map;
                    }
                }
            }
            RawContent::Image(_)
            | RawContent::Audio(_)
            | RawContent::Resource(_)
            | RawContent::ResourceLink(_) => {
                return Ok(IngressResult {
                    response: blocked_response("unsupported_content_block", "ingress.kind.deny"),
                    result_paths: vec![ResultPath::root()
                        .child("content")?
                        .child(format!("[{idx}]"))?],
                });
            }
        }
        content_values.push(
            serde_json::to_value(content).map_err(|err| {
                BridgeError::Downstream(format!("serialize content failed: {err}"))
            })?,
        );
    }

    let structured_content = match result.structured_content {
        Some(value) => {
            let redacted = redact_json_value(pipeline, locale_chain, session, &value)?;
            if redacted != value {
                result_paths.push(ResultPath::root().child("structured_content")?);
            }
            Some(redacted)
        }
        None => None,
    };
    let meta = match result.meta {
        Some(meta) => {
            let value = serde_json::Value::Object(meta.0);
            let redacted = redact_json_value(pipeline, locale_chain, session, &value)?;
            if redacted != value {
                result_paths.push(ResultPath::root().child("_meta")?);
            }
            Some(redacted)
        }
        None => None,
    };

    Ok(IngressResult {
        response: ToolResponse::json(serde_json::json!({
            "content": content_values,
            "structuredContent": structured_content,
            "isError": result.is_error.unwrap_or(false),
            "_meta": meta,
        })),
        result_paths,
    })
}

fn process_downstream_error(
    pipeline: &gaze::Pipeline,
    locale_chain: &[gaze::LocaleTag],
    session: &gaze::Session,
    message: String,
    data: Option<serde_json::Value>,
) -> BridgeResult<IngressResult> {
    let redacted_message = redact_text(pipeline, locale_chain, session, &message)?;
    let mut result_paths = Vec::new();
    if redacted_message != message {
        result_paths.push(ResultPath::root().child("error")?.child("message")?);
    }
    let redacted_data = match data {
        Some(value) => {
            let redacted = redact_json_value(pipeline, locale_chain, session, &value)?;
            if redacted != value {
                result_paths.push(ResultPath::root().child("error")?.child("data")?);
            }
            Some(redacted)
        }
        None => None,
    };
    Ok(IngressResult {
        response: ToolResponse::json(serde_json::json!({
            "isError": true,
            "error": {
                "message": redacted_message,
                "data": redacted_data,
            }
        })),
        result_paths,
    })
}

fn process_downstream_service_error() -> BridgeResult<IngressResult> {
    Ok(IngressResult {
        response: ToolResponse::json(serde_json::json!({
            "isError": true,
            "error": {
                "message": "downstream service error",
                "data": null,
            }
        })),
        result_paths: vec![ResultPath::root().child("error")?.child("message")?],
    })
}

fn contains_raw_pii(
    pipeline: &gaze::Pipeline,
    locale_chain: &[gaze::LocaleTag],
    text: &str,
) -> BridgeResult<bool> {
    if text.is_empty() {
        return Ok(false);
    }
    let scan_session = gaze::Session::new(gaze::Scope::Ephemeral)
        .map_err(|err| BridgeError::Redaction(err.to_string()))?;
    let clean = pipeline
        .pseudonymize_with_context(
            &scan_session,
            RawDocument::Text(text.to_string()),
            locale_chain,
        )
        .map_err(|err| BridgeError::Redaction(err.to_string()))?;
    match clean {
        CleanDocument::Text(redacted) => Ok(redacted != text),
        _ => Err(BridgeError::Redaction(
            "unexpected non-text redaction result".to_string(),
        )),
    }
}

fn redact_json_value(
    pipeline: &gaze::Pipeline,
    locale_chain: &[gaze::LocaleTag],
    session: &gaze::Session,
    value: &serde_json::Value,
) -> BridgeResult<serde_json::Value> {
    match value {
        serde_json::Value::String(text) => Ok(serde_json::Value::String(redact_text(
            pipeline,
            locale_chain,
            session,
            text,
        )?)),
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(redact_json_value(pipeline, locale_chain, session, item)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, item) in map {
                out.insert(
                    key.clone(),
                    redact_json_value(pipeline, locale_chain, session, item)?,
                );
            }
            Ok(serde_json::Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

fn redact_text(
    pipeline: &gaze::Pipeline,
    locale_chain: &[gaze::LocaleTag],
    session: &gaze::Session,
    text: &str,
) -> BridgeResult<String> {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for token in gaze::token_shape::pattern().find_iter(text) {
        if !session.contains_token(token.as_str()) {
            continue;
        }
        out.push_str(&redact_segment(
            pipeline,
            locale_chain,
            session,
            &text[cursor..token.start()],
        )?);
        out.push_str(token.as_str());
        cursor = token.end();
    }
    out.push_str(&redact_segment(
        pipeline,
        locale_chain,
        session,
        &text[cursor..],
    )?);
    Ok(out)
}

fn redact_segment(
    pipeline: &gaze::Pipeline,
    locale_chain: &[gaze::LocaleTag],
    session: &gaze::Session,
    segment: &str,
) -> BridgeResult<String> {
    if segment.is_empty() {
        return Ok(String::new());
    }
    match pipeline
        .pseudonymize_with_context(
            session,
            RawDocument::Text(segment.to_string()),
            locale_chain,
        )
        .map_err(|err| BridgeError::Redaction(err.to_string()))?
    {
        CleanDocument::Text(text) => Ok(text),
        _ => Err(BridgeError::Redaction(
            "unexpected non-text redaction result".to_string(),
        )),
    }
}

fn token_class(token: &str) -> String {
    let trimmed = token.trim_matches(|c| c == '<' || c == '>');
    let after_prefix = trimmed
        .split_once(':')
        .map(|(_, tail)| tail)
        .unwrap_or(trimmed);
    after_prefix
        .rsplit_once('_')
        .map(|(class, _)| class.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn blocked_response(reason: &str, rule: &str) -> ToolResponse {
    ToolResponse::json(serde_json::json!({
        "gaze_bridge": {
            "outcome": "blocked",
            "reason": reason,
            "rule": rule,
        }
    }))
}

fn approval_required_response(request: &ApprovalRequest) -> ToolResponse {
    ToolResponse::json(serde_json::json!({
        "gaze_bridge": {
            "outcome": "approval_required",
            "call_id": request.call_id,
            "redacted_arg_sha256": request.redacted_arg_sha256,
            "token_classes": request.token_classes,
            "reason": request.reason,
        }
    }))
}
