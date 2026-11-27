//! rmcp-backed [`gaze_mcp_core::Frontend`] implementation.
//!
//! The frontend owns transport details only. Tool catalog and invocation stay
//! behind `DispatchHost`, preserving `PiiEnvelope::dispatch` as the only
//! response-returning path.

#[cfg(feature = "transport-http")]
use std::net::SocketAddr;
use std::sync::Arc;
#[cfg(any(feature = "transport-stdio", feature = "transport-http"))]
use std::time::Duration;

use async_trait::async_trait;
use gaze_mcp_core::{
    DispatchError, DispatchHost, Frontend, FrontendError, Principal, ShutdownToken, ToolTier,
};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
    Implementation, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
#[cfg(feature = "transport-stdio")]
use tokio_util::sync::CancellationToken;

use crate::adapter::{
    descriptor_to_rmcp_tool, descriptor_visible_to, error_to_rmcp_call_tool_result,
    response_to_rmcp_call_tool_result, rmcp_args_to_dispatch_args,
};

/// Host middleware input for one call. Never deserialize this from tool args or `_meta`.
/// The session id is audit correlation only; the host selects the actual Session.
#[derive(Clone, Default)]
pub struct HostCallContext {
    /// Private response detection input.
    pub selection: gaze_mcp_core::HostContextSelection,
    /// Host-owned audit correlation, validated by SessionIdPolicy.
    pub audit_session_id: Option<String>,
    /// Host-authenticated conversation, used by
    /// [`RmcpFrontend::into_conversation_handler`] to choose the Session.
    pub conversation: Option<gaze_mcp_core::ConversationId>,
}

impl std::fmt::Debug for HostCallContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HostCallContext([redacted])")
    }
}

fn host_call_context(extensions: &rmcp::model::Extensions) -> HostCallContext {
    if let Some(host) = extensions.get::<HostCallContext>() {
        return host.clone();
    }
    #[cfg(feature = "transport-http")]
    if let Some(parts) = extensions.get::<axum::http::request::Parts>()
        && let Some(host) = parts.extensions.get::<HostCallContext>()
    {
        return host.clone();
    }
    HostCallContext::default()
}

/// Transport choice for [`RmcpFrontend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RmcpTransport {
    /// MCP over process stdio.
    #[cfg(feature = "transport-stdio")]
    Stdio,
    /// MCP over rmcp streamable HTTP, bound to the supplied address.
    #[cfg(feature = "transport-http")]
    Http { bind: SocketAddr },
}

/// Configuration for [`RmcpFrontend`].
#[derive(Clone)]
#[non_exhaustive]
pub struct RmcpFrontendConfig {
    pub transport: RmcpTransport,
    pub principal_resolver: Arc<dyn PrincipalResolver>,
}

impl RmcpFrontendConfig {
    pub fn new(transport: RmcpTransport, principal_resolver: Arc<dyn PrincipalResolver>) -> Self {
        Self {
            transport,
            principal_resolver,
        }
    }
}

#[cfg(feature = "transport-stdio")]
impl Default for RmcpFrontendConfig {
    fn default() -> Self {
        Self {
            transport: RmcpTransport::Stdio,
            principal_resolver: Arc::new(FixedPrincipalResolver::agent("mcp-stdio")),
        }
    }
}

/// Adopter hook that maps rmcp request context to Gaze's authorization principal.
#[async_trait]
pub trait PrincipalResolver: Send + Sync {
    async fn resolve(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<Principal, RmcpPrincipalError>;
}

/// Static resolver useful for local stdio servers and tests.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FixedPrincipalResolver {
    principal: Principal,
}

impl FixedPrincipalResolver {
    pub fn new(principal: Principal) -> Self {
        Self { principal }
    }

    pub fn agent(id: impl Into<String>) -> Self {
        Self::new(Principal::new(id))
    }

    pub fn operator(id: impl Into<String>) -> Self {
        Self::new(Principal::with_roles(id, vec!["operator".to_string()]))
    }
}

#[async_trait]
impl PrincipalResolver for FixedPrincipalResolver {
    async fn resolve(
        &self,
        _context: &RequestContext<RoleServer>,
    ) -> Result<Principal, RmcpPrincipalError> {
        Ok(self.principal.clone())
    }
}

/// Principal mapping failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RmcpPrincipalError {
    #[error("principal denied: {0}")]
    Denied(String),
    #[error("principal resolver failed: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl RmcpPrincipalError {
    pub fn internal<E>(err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Internal(Box::new(err))
    }
}

/// rmcp transport sink for `gaze-mcp-core`.
#[derive(Clone)]
#[non_exhaustive]
pub struct RmcpFrontend {
    config: RmcpFrontendConfig,
}

impl RmcpFrontend {
    pub fn new(config: RmcpFrontendConfig) -> Self {
        Self { config }
    }

    #[cfg(feature = "transport-stdio")]
    pub fn stdio(principal_resolver: Arc<dyn PrincipalResolver>) -> Self {
        Self::new(RmcpFrontendConfig::new(
            RmcpTransport::Stdio,
            principal_resolver,
        ))
    }

    #[cfg(feature = "transport-http")]
    pub fn http(bind: SocketAddr, principal_resolver: Arc<dyn PrincipalResolver>) -> Self {
        Self::new(RmcpFrontendConfig::new(
            RmcpTransport::Http { bind },
            principal_resolver,
        ))
    }

    pub fn config(&self) -> &RmcpFrontendConfig {
        &self.config
    }

    /// Build an embeddable rmcp handler that sends every call to `host`. HTTP hosts
    /// install middleware on their own StreamableHttpService and insert
    /// HostCallContext in request extensions. Every caller of this handler shares
    /// `host` and its Session. For private conversations use
    /// [`Self::into_conversation_handler`]: rmcp serves protocol 2026-07-28
    /// statelessly and calls the service factory per request, so building a host in
    /// that factory gives one Session per request, not per conversation.
    pub fn into_server_handler(self, host: Arc<dyn DispatchHost>) -> impl ServerHandler {
        RmcpServer::new(HostSource::Fixed(host), self.config.principal_resolver)
    }

    /// Build an embeddable handler that routes each call to the host of its
    /// conversation. Host middleware must set [`HostCallContext::conversation`]
    /// from an authenticated identity; a call without one is refused before
    /// dispatch. Tool arguments and `_meta` never select a conversation.
    pub fn into_conversation_handler(
        self,
        conversations: Arc<gaze_mcp_core::ConversationRegistry>,
    ) -> impl ServerHandler {
        RmcpServer::new(
            HostSource::Conversations(conversations),
            self.config.principal_resolver,
        )
    }
}

#[cfg(feature = "transport-stdio")]
impl Default for RmcpFrontend {
    fn default() -> Self {
        Self::new(RmcpFrontendConfig::default())
    }
}

#[async_trait]
impl Frontend for RmcpFrontend {
    // With no transport feature `RmcpTransport` is uninhabited and the match
    // below has no arms, so the arguments are never read.
    #[cfg_attr(
        not(any(feature = "transport-stdio", feature = "transport-http")),
        allow(unused_variables)
    )]
    async fn serve(
        self,
        host: Arc<dyn DispatchHost>,
        shutdown: ShutdownToken,
    ) -> Result<(), FrontendError> {
        match self.config.transport {
            #[cfg(feature = "transport-stdio")]
            RmcpTransport::Stdio => {
                serve_stdio(host, self.config.principal_resolver, shutdown).await
            }
            #[cfg(feature = "transport-http")]
            RmcpTransport::Http { bind } => {
                serve_http(bind, host, self.config.principal_resolver, shutdown).await
            }
        }
    }
}

#[derive(Clone)]
enum HostSource {
    Fixed(Arc<dyn DispatchHost>),
    Conversations(Arc<gaze_mcp_core::ConversationRegistry>),
}

#[derive(Clone)]
struct RmcpServer {
    hosts: HostSource,
    principal_resolver: Arc<dyn PrincipalResolver>,
}

impl RmcpServer {
    fn new(hosts: HostSource, principal_resolver: Arc<dyn PrincipalResolver>) -> Self {
        Self {
            hosts,
            principal_resolver,
        }
    }

    fn host(&self, call: &HostCallContext) -> Result<Arc<dyn DispatchHost>, &'static str> {
        match &self.hosts {
            HostSource::Fixed(host) => Ok(host.clone()),
            HostSource::Conversations(registry) => {
                let id = call.conversation.as_ref().ok_or("conversation-required")?;
                registry.host(id).map_err(|err| match err {
                    gaze_mcp_core::ConversationError::Capacity => "conversation-limit",
                    _ => "conversation-unavailable",
                })
            }
        }
    }

    async fn principal(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<Principal, ErrorData> {
        self.principal_resolver
            .resolve(context)
            .await
            .map_err(principal_error_to_mcp)
    }
}

impl ServerHandler for RmcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("gaze-mcp-rmcp", env!("CARGO_PKG_VERSION")),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let principal = self.principal(&context).await?;
        let principal_tier = tier_for_principal(&principal);
        let tools = self
            .tools(&host_call_context(&context.extensions))
            .map_err(|class| ErrorData::invalid_request(class, None))?
            .into_iter()
            .filter(|descriptor| descriptor_visible_to(descriptor, principal_tier))
            .map(|descriptor| descriptor_to_rmcp_tool(&descriptor))
            .collect();

        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let principal = self.principal(&context).await?;
        let result = self
            .call(&principal, host_call_context(&context.extensions), request)
            .await?;
        Ok(result.into())
    }
}

impl RmcpServer {
    /// Tools for the caller's conversation. Never opens a conversation when the
    /// registry already knows its catalog.
    fn tools(
        &self,
        call: &HostCallContext,
    ) -> Result<Vec<gaze_mcp_core::ToolDescriptor>, &'static str> {
        match &self.hosts {
            HostSource::Fixed(host) => Ok(host.list_tools()),
            HostSource::Conversations(registry) => {
                let id = call.conversation.as_ref().ok_or("conversation-required")?;
                match registry.tools(Some(id)) {
                    Some(tools) => Ok(tools),
                    None => Ok(self.host(call)?.list_tools()),
                }
            }
        }
    }

    /// Route one tool call to its conversation's host. Only host middleware state
    /// selects the host; tool arguments never do.
    async fn call(
        &self,
        principal: &Principal,
        host: HostCallContext,
        request: CallToolRequestParams,
    ) -> Result<CallToolResult, ErrorData> {
        let target = match self.host(&host) {
            Ok(target) => target,
            Err(class) => return Ok(CallToolResult::error(vec![ContentBlock::text(class)])),
        };
        let dispatch_args = rmcp_args_to_dispatch_args(request);
        let response = target
            .dispatch(
                principal,
                &dispatch_args.tool_name,
                dispatch_args.args,
                host.audit_session_id.as_deref(),
                host.selection,
            )
            .await;
        match response {
            Ok(response) => response_to_rmcp_call_tool_result(response).map_err(rmcp_error_to_mcp),
            Err(err) => Ok(dispatch_error_to_tool_result(err)),
        }
    }
}

#[cfg(feature = "transport-stdio")]
async fn serve_stdio(
    host: Arc<dyn DispatchHost>,
    principal_resolver: Arc<dyn PrincipalResolver>,
    shutdown: ShutdownToken,
) -> Result<(), FrontendError> {
    let ct = cancellation_token_from_shutdown(shutdown);
    let service = RmcpServer::new(HostSource::Fixed(host), principal_resolver);
    let running = rmcp::service::serve_server_with_ct(service, rmcp::transport::stdio(), ct)
        .await
        .map_err(FrontendError::backend)?;
    running.waiting().await.map_err(FrontendError::backend)?;
    Ok(())
}

#[cfg(feature = "transport-http")]
async fn serve_http(
    bind: SocketAddr,
    host: Arc<dyn DispatchHost>,
    principal_resolver: Arc<dyn PrincipalResolver>,
    shutdown: ShutdownToken,
) -> Result<(), FrontendError> {
    use axum::Router;
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(FrontendError::Io)?;
    let service = StreamableHttpService::new(
        move || {
            Ok(RmcpServer::new(
                HostSource::Fixed(host.clone()),
                principal_resolver.clone(),
            ))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = Router::new().route_service("/mcp", service);

    axum::serve(listener, app)
        .with_graceful_shutdown(wait_for_shutdown(shutdown))
        .await
        .map_err(FrontendError::Io)
}

#[cfg(feature = "transport-stdio")]
fn cancellation_token_from_shutdown(shutdown: ShutdownToken) -> CancellationToken {
    let ct = CancellationToken::new();
    let cancel = ct.clone();
    tokio::spawn(async move {
        wait_for_shutdown(shutdown).await;
        cancel.cancel();
    });
    ct
}

#[cfg(any(feature = "transport-stdio", feature = "transport-http"))]
async fn wait_for_shutdown(shutdown: ShutdownToken) {
    while !shutdown.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn tier_for_principal(principal: &Principal) -> ToolTier {
    if principal.has_role("operator") {
        ToolTier::Operator
    } else {
        ToolTier::Agent
    }
}

fn dispatch_error_to_tool_result(err: DispatchError) -> CallToolResult {
    // Closed code, then the closed reason; the field path stays out because keys can be PII.
    if let Some(reason) = err.untransformable_reason() {
        return CallToolResult::error(vec![
            ContentBlock::text("untransformable"),
            ContentBlock::text(reason),
        ]);
    }
    match err {
        DispatchError::HostContext(_) => {
            CallToolResult::error(vec![ContentBlock::text("host-context-rejected")])
        }
        DispatchError::ToolError(err) => error_to_rmcp_call_tool_result(err),
        DispatchError::UnknownTool(_) => {
            CallToolResult::error(vec![ContentBlock::text("not-found")])
        }
        DispatchError::SessionId(_) => {
            CallToolResult::error(vec![ContentBlock::text("invalid-session-id")])
        }
        DispatchError::Auth(_) => CallToolResult::error(vec![ContentBlock::text("auth-denied")]),
        DispatchError::Manifest(_) => {
            CallToolResult::error(vec![ContentBlock::text("manifest-persistence-failed")])
        }
        DispatchError::Protection(_)
        | DispatchError::Carrier(_)
        | DispatchError::Transaction(_)
        | DispatchError::Redaction(_) => {
            CallToolResult::error(vec![ContentBlock::text("redaction-failed")])
        }
        DispatchError::ResourceLimit => {
            CallToolResult::error(vec![ContentBlock::text("resource_limit")])
        }
        DispatchError::ResponseSerialization(_) => {
            CallToolResult::error(vec![ContentBlock::text("response-serialization-failed")])
        }
        _ => CallToolResult::error(vec![ContentBlock::text("internal")]),
    }
}

fn rmcp_error_to_mcp(err: crate::RmcpFrontendError) -> ErrorData {
    ErrorData::invalid_params(err.class(), None)
}

fn principal_error_to_mcp(err: RmcpPrincipalError) -> ErrorData {
    match err {
        RmcpPrincipalError::Denied(_) => ErrorData::invalid_request("principal-denied", None),
        RmcpPrincipalError::Internal(_) => ErrorData::internal_error("principal-resolver", None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_context_extensions_are_explicit_and_debug_is_redacted() {
        let mut extensions = rmcp::model::Extensions::new();
        assert!(matches!(
            host_call_context(&extensions).selection,
            gaze_mcp_core::HostContextSelection::Absent
        ));
        extensions.insert(HostCallContext {
            selection: gaze_mcp_core::HostContextSelection::present(
                r#"{"fields":{"private_canary_key":"private_canary_value"}}"#.into(),
            ),
            audit_session_id: Some("private_canary_session".into()),
            conversation: Some(gaze_mcp_core::ConversationId::new(
                "private_canary_conversation",
            )),
        });
        let host = host_call_context(&extensions);
        assert!(matches!(
            host.selection,
            gaze_mcp_core::HostContextSelection::Present(_)
        ));
        assert_eq!(
            host.audit_session_id.as_deref(),
            Some("private_canary_session")
        );
        assert!(!format!("{host:?} {:?}", host.selection).contains("private_canary"));
    }

    #[cfg(feature = "transport-http")]
    #[test]
    fn embedded_http_reads_host_middleware_request_extensions() {
        let (mut parts, _) = axum::http::Request::new(()).into_parts();
        parts.extensions.insert(HostCallContext {
            selection: gaze_mcp_core::HostContextSelection::present("{}".into()),
            audit_session_id: Some("host-audit".into()),
            conversation: None,
        });
        let mut extensions = rmcp::model::Extensions::new();
        extensions.insert(parts);
        let host = host_call_context(&extensions);
        assert!(matches!(
            host.selection,
            gaze_mcp_core::HostContextSelection::Present(_)
        ));
        assert_eq!(host.audit_session_id.as_deref(), Some("host-audit"));
    }

    /// Records which conversation's host received each call, and its arguments.
    struct Recording {
        conversation: String,
        calls: Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>,
    }

    #[async_trait]
    impl DispatchHost for Recording {
        async fn dispatch(
            &self,
            _: &Principal,
            _: &str,
            args: serde_json::Value,
            _: Option<&str>,
            _: gaze_mcp_core::HostContextSelection,
        ) -> Result<gaze_mcp_core::ToolResponse, DispatchError> {
            self.calls
                .lock()
                .unwrap()
                .push((self.conversation.clone(), args));
            Ok(gaze_mcp_core::ToolResponse::text("ok"))
        }

        fn list_tools(&self) -> Vec<gaze_mcp_core::ToolDescriptor> {
            vec![gaze_mcp_core::ToolDescriptor::agent(
                "lookup",
                serde_json::json!({"type": "object"}),
            )]
        }
    }

    #[tokio::test]
    async fn calls_route_by_host_conversation_never_by_arguments() {
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let built = calls.clone();
        let registry = Arc::new(gaze_mcp_core::ConversationRegistry::new(
            move |id: &gaze_mcp_core::ConversationId| {
                Ok(Arc::new(Recording {
                    conversation: id.as_str().to_owned(),
                    calls: built.clone(),
                }) as Arc<dyn DispatchHost>)
            },
            gaze_mcp_core::ConversationLimits::new(2, std::time::Duration::from_secs(60)),
        ));
        let server = RmcpServer::new(
            HostSource::Conversations(registry.clone()),
            Arc::new(FixedPrincipalResolver::agent("synthetic")),
        );
        let principal = Principal::new("synthetic");
        let context = |id: Option<&str>| HostCallContext {
            conversation: id.map(gaze_mcp_core::ConversationId::new),
            ..HostCallContext::default()
        };
        let request = |session: &str| {
            let mut arguments = serde_json::Map::new();
            arguments.insert("_session_id".into(), serde_json::json!(session));
            CallToolRequestParams::new("lookup").with_arguments(arguments)
        };
        let text = |result: CallToolResult| result.content[0].as_text().unwrap().text.clone();

        let refused = server
            .call(&principal, context(None), request("a"))
            .await
            .unwrap();
        assert_eq!(refused.is_error, Some(true));
        assert_eq!(text(refused), "conversation-required");
        assert_eq!(
            server.tools(&context(None)).err(),
            Some("conversation-required")
        );

        // The model names conversation b; the host's conversation a still decides.
        server
            .call(&principal, context(Some("a")), request("b"))
            .await
            .unwrap();
        server
            .call(&principal, context(Some("b")), request("a"))
            .await
            .unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            [
                ("a".to_owned(), serde_json::json!({})),
                ("b".to_owned(), serde_json::json!({})),
            ]
        );

        // Listing tools for a new conversation spends no capacity.
        assert_eq!(server.tools(&context(Some("c"))).unwrap().len(), 1);
        assert_eq!(registry.len(), 2);
        let full = server
            .call(&principal, context(Some("c")), request("c"))
            .await
            .unwrap();
        assert_eq!(text(full), "conversation-limit");
    }

    #[test]
    fn operator_role_maps_to_operator_tier() {
        let principal = Principal::with_roles("u", vec!["operator".to_string()]);
        assert_eq!(tier_for_principal(&principal), ToolTier::Operator);
    }

    #[test]
    fn missing_operator_role_maps_to_agent_tier() {
        let principal = Principal::with_roles("u", vec!["agent".to_string()]);
        assert_eq!(tier_for_principal(&principal), ToolTier::Agent);
    }

    #[test]
    fn manifest_failure_returns_error_result_without_detail() {
        let err = DispatchError::Manifest(gaze_mcp_core::ManifestError::backend(
            std::io::Error::other("db unavailable"),
        ));
        let result = dispatch_error_to_tool_result(err);
        assert_eq!(result.is_error, Some(true));
        let text = &result.content[0].as_text().unwrap().text;
        assert_eq!(text, "manifest-persistence-failed");
    }
    #[test]
    fn untransformable_returns_closed_reason_without_field_path() {
        let err = DispatchError::Protection(gaze::ProtectionError::Untransformable {
            reason: gaze::UntransformableReason::EncodedCarrier,
            field_path: gaze::FieldPath::new(["synthetic-key@example.invalid".into()]),
        });
        let result = dispatch_error_to_tool_result(err);
        assert_eq!(result.is_error, Some(true));
        let texts: Vec<_> = result
            .content
            .iter()
            .map(|block| block.as_text().unwrap().text.as_str())
            .collect();
        assert_eq!(texts, ["untransformable", "encoded_carrier"]);
        let host = dispatch_error_to_tool_result(DispatchError::HostContext(
            gaze_mcp_core::HostContextError::Untransformable(
                gaze::UntransformableReason::CallerValueTooShort,
            ),
        ));
        let texts: Vec<_> = host
            .content
            .iter()
            .map(|block| block.as_text().unwrap().text.as_str())
            .collect();
        assert_eq!(texts, ["untransformable", "caller_value_too_short"]);
        let other = dispatch_error_to_tool_result(DispatchError::Protection(
            gaze::ProtectionError::Residual,
        ));
        assert_eq!(other.content[0].as_text().unwrap().text, "redaction-failed");
    }

    #[test]
    fn resource_limit_returns_stable_error_class() {
        let result = dispatch_error_to_tool_result(DispatchError::ResourceLimit);
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].as_text().unwrap().text, "resource_limit");
    }
}
