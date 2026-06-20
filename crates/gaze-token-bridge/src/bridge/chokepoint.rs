//! The `search_documents` MCP chokepoint tool.
//!
//! [`SearchDocumentsTool`] implements [`gaze_mcp_core::Tool`] so the bridge can be
//! driven from a gaze-mcp-core dispatch without touching that crate's published,
//! trybuild-sealed surface (`Tool` / `ToolCtx` / `ToolResources` / dispatch).
//!
//! # Owner-side session model (the chokepoint invariant)
//! The sealed [`gaze_mcp_core::ToolCtx`] only exposes the *external* session id; its
//! inner `gaze::Session` is unreachable, and the frozen [`RedactionSession`] has no
//! constructor that wraps a foreign session. So this tool owns the conversation's
//! redaction namespace **owner-side**: a per-principal [`RedactionSession`] registry.
//! The owner tokenizes PII via [`SearchDocumentsTool::tokenize_for`]; the agent only
//! ever holds the resulting token and passes it back as `source_token`; the tool
//! resolves it owner-side against the *same* session before driving the bridge. Raw
//! PII, the manifest, and index aliases never cross to the agent path.
//!
//! Unifying this owner-side session with the dispatch's own `gaze::Session` (one
//! shared manifest spanning both) is deliberately out of scope — it needs a frozen
//! `RedactionSession` constructor or a gaze-mcp-core wiring change, and is deferred
//! as a future contract refinement (R3).
//!
//! # No-oracle deny
//! Every failure — unknown principal, malformed args, or any typed [`DenyReason`]
//! from the bridge — collapses to one uniform `{authorized: false, results: []}`
//! payload. The typed reason lives only in the bridge audit; the agent-visible
//! surface never reveals which path fired.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use gaze::PiiClass;
use gaze_mcp_core::{Tool, ToolCtx, ToolDescriptor, ToolError, ToolResponse};
use serde_json::{json, Value};

use crate::bridge::TokenBridge;
use crate::error::BridgeError;
use crate::model::{
    BridgeRequest, BridgeSearchOutcome, BridgeSearchResponse, Principal, RequestedScope,
};
use crate::session::RedactionSession;

/// Wire name of this agent-tier tool **and** the policy `tool_name` the bridge
/// authorizes it under. Adopters MUST author an allow-rule (and list the tool in
/// each domain's `allowed_tools`) under `"search_documents"`. The agent never
/// supplies the policy tool_name — that would hand authorization to the LLM
/// (the LLM never decides authorization).
const TOOL_NAME: &str = "search_documents";

/// Bridge action this tool performs. Target domains must list it in
/// `allowed_actions`.
const ACTION: &str = "search_documents";

/// Owner-side, agent-tier `search_documents` tool.
///
/// One instance is shared (behind `Arc<dyn Tool>`) across concurrent dispatches,
/// so the mutable bridge and the session registry live behind mutexes.
pub struct SearchDocumentsTool {
    bridge: Mutex<TokenBridge>,
    principals: HashMap<String, Principal>,
    sessions: Mutex<HashMap<String, RedactionSession>>,
    descriptor: ToolDescriptor,
}

impl SearchDocumentsTool {
    /// Build the tool over an owner-side bridge and a principal registry. The
    /// principal registry is the trusted runtime's resolution of `principal_id`
    /// to a [`Principal`]; the agent's `principal_id` is looked up here and any
    /// unknown id fails closed.
    pub fn new(bridge: TokenBridge, principals: HashMap<String, Principal>) -> Self {
        Self {
            bridge: Mutex::new(bridge),
            principals,
            sessions: Mutex::new(HashMap::new()),
            descriptor: ToolDescriptor::agent(TOOL_NAME, input_schema()).with_description(
                "Search an owner-registered index domain by a session token minted earlier in \
                 this conversation. Results carry only current-session tokens — never raw PII.",
            ),
        }
    }

    /// Owner-side helper: mint (or reuse) a conversation token for `raw` of `class`
    /// in `principal_id`'s owner-side redaction namespace.
    ///
    /// This is how the owner establishes the namespace the agent's `source_token`
    /// later resolves against: the owner tokenizes PII here, hands the agent the
    /// returned token, and the agent passes it back into [`Self::run`]. Tests use
    /// it to seed the happy path.
    pub fn tokenize_for(
        &self,
        principal_id: &str,
        class: &PiiClass,
        raw: &str,
    ) -> Result<String, BridgeError> {
        let mut sessions = lock(&self.sessions);
        get_or_create_session(&mut sessions, principal_id)?.tokenize(class, raw)
    }

    /// Synchronous tool core, driven by [`Tool::invoke`] and by tests directly.
    ///
    /// The sealed [`ToolCtx`] cannot be constructed outside `gaze-mcp-core`, so the
    /// body lives here where it is unit-testable without fabricating a context.
    /// Always fails closed to a uniform, no-oracle deny.
    pub fn run(&self, principal_id: &str, args: &Value) -> Value {
        // Parsed first so every deny path can echo the caller's own target_domain
        // (echoing the caller's input reveals nothing about owner-side state).
        let target_domain = args
            .get("target_domain")
            .and_then(Value::as_str)
            .unwrap_or_default();

        // Fail-closed pre-checks. Each returns the SAME uniform deny so the agent
        // cannot distinguish unknown-principal from bad-args from policy-deny.
        let Some(principal) = self.principals.get(principal_id) else {
            return deny_response(target_domain);
        };
        let Some(source_token) = args.get("source_token").and_then(Value::as_str) else {
            return deny_response(target_domain);
        };
        if target_domain.is_empty() {
            return deny_response(target_domain);
        }

        let mut sessions = lock(&self.sessions);
        let session = match get_or_create_session(&mut sessions, principal_id) {
            Ok(session) => session,
            Err(_) => return deny_response(target_domain),
        };

        let request = BridgeRequest {
            principal: principal.clone(),
            tenant_id: principal.tenant_id.clone(),
            workspace_id: principal.workspace_id.clone(),
            agent_run_id: args
                .get("agent_run_id")
                .and_then(Value::as_str)
                .unwrap_or("agent-run")
                .to_string(),
            conversation_session_id: args
                .get("conversation_session_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| session.session_id().to_string()),
            tool_name: TOOL_NAME.to_string(),
            action: ACTION.to_string(),
            // Request-stated purpose only; the gate ignores it and uses the
            // owner-bound purpose resolved from policy config.
            purpose: args
                .get("purpose")
                .and_then(Value::as_str)
                .unwrap_or("agent_search")
                .to_string(),
            source_token: source_token.to_string(),
            target_domain: target_domain.to_string(),
            requested_scope: RequestedScope::SameDomain,
        };

        let mut bridge = lock(&self.bridge);
        match bridge.search(session, &request) {
            BridgeSearchOutcome::Allowed(response) => allowed_response(&response),
            // The typed DenyReason stays in the bridge audit ONLY — never emitted.
            BridgeSearchOutcome::Denied(_) => deny_response(target_domain),
        }
    }
}

#[async_trait]
impl Tool for SearchDocumentsTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        // Thin wrapper: the dispatcher already redacted args and authorized the
        // principal. Delegate to the sync, testable core. The dispatcher re-redacts
        // the response anyway (defense in depth); our output is already clean.
        let output = self.run(ctx.principal_id(), ctx.redacted_args());
        Ok(ToolResponse::json(output))
    }
}

/// Get the principal's owner-side session, creating an ephemeral one on first use.
fn get_or_create_session<'a>(
    sessions: &'a mut HashMap<String, RedactionSession>,
    principal_id: &str,
) -> Result<&'a RedactionSession, BridgeError> {
    if !sessions.contains_key(principal_id) {
        sessions.insert(
            principal_id.to_string(),
            RedactionSession::ephemeral_for(principal_id)?,
        );
    }
    Ok(sessions
        .get(principal_id)
        .expect("session inserted above is present"))
}

/// JSON-schema metadata for the tool's input arguments. Surface-only; the
/// dispatcher does not validate against it, and [`SearchDocumentsTool::run`]
/// validates the shape itself and fails closed on mismatch.
fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "source_token": {
                "type": "string",
                "description": "A session token minted earlier in this conversation for the entity to search by."
            },
            "target_domain": {
                "type": "string",
                "description": "The owner-registered index domain to search (e.g. tenant_demo/customer_docs/v1)."
            },
            "filters": {
                "type": "array",
                "description": "Reserved. The bounded same-entity search path does not accept agent-supplied filters.",
                "items": { "type": "object" }
            }
        },
        "required": ["source_token", "target_domain"]
    })
}

/// Uniform, no-oracle deny. Byte-identical across unknown-principal, malformed-args,
/// and every typed [`crate::error::DenyReason`] for a given `target_domain` echo.
fn deny_response(target_domain: &str) -> Value {
    json!({
        "target_domain": target_domain,
        "results": [],
        "authorized": false,
    })
}

/// Allow payload: the agent-visible [`BridgeSearchResponse`] (session-token-only
/// results) plus the binary `authorized` flag.
fn allowed_response(response: &BridgeSearchResponse) -> Value {
    match serde_json::to_value(response) {
        Ok(Value::Object(mut map)) => {
            map.insert("authorized".to_string(), Value::Bool(true));
            Value::Object(map)
        }
        // Serializing a frozen-contract struct cannot realistically fail; if it
        // ever did, fail closed rather than emit a half-formed allow.
        _ => deny_response(&response.target_domain),
    }
}

/// Lock a mutex, recovering the guard on poison so a prior panic elsewhere cannot
/// turn into a hard failure here (fail-closed: the wrapped data stays valid).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
