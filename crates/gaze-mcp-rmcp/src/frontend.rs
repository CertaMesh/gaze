//! rmcp-backed [`gaze_mcp_core::Frontend`] implementation.
//!
//! The frontend owns transport details only. Tool catalog and invocation stay
//! behind `DispatchHost`, preserving `PiiEnvelope::dispatch` as the only
//! response-returning path.

#[cfg(feature = "transport-http")]
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use gaze_mcp_core::{
    DispatchError, DispatchHost, Frontend, FrontendError, Principal, ShutdownToken, ToolTier,
};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParam, CallToolResult, Content, ErrorData, Implementation, ListToolsResult,
    PaginatedRequestParam, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use tokio_util::sync::CancellationToken;

use crate::adapter::{
    descriptor_to_rmcp_tool, descriptor_visible_to, error_to_rmcp_call_tool_result,
    response_to_rmcp_call_tool_result, rmcp_args_to_dispatch_args,
};

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

    #[doc(hidden)]
    pub fn into_server_handler(self, host: Arc<dyn DispatchHost>) -> impl ServerHandler {
        RmcpServer::new(host, self.config.principal_resolver)
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
struct RmcpServer {
    host: Arc<dyn DispatchHost>,
    principal_resolver: Arc<dyn PrincipalResolver>,
}

impl RmcpServer {
    fn new(host: Arc<dyn DispatchHost>, principal_resolver: Arc<dyn PrincipalResolver>) -> Self {
        Self {
            host,
            principal_resolver,
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
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "gaze-mcp-rmcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            ..ServerInfo::default()
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let principal = self.principal(&context).await?;
        let principal_tier = tier_for_principal(&principal);
        let tools = self
            .host
            .list_tools()
            .into_iter()
            .filter(|descriptor| descriptor_visible_to(descriptor, principal_tier))
            .map(|descriptor| descriptor_to_rmcp_tool(&descriptor))
            .collect();

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let principal = self.principal(&context).await?;
        let dispatch_args = rmcp_args_to_dispatch_args(request).map_err(rmcp_error_to_mcp)?;
        let response = self
            .host
            .dispatch(
                &principal,
                &dispatch_args.tool_name,
                dispatch_args.args,
                dispatch_args.session_id.as_deref(),
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
    let service = RmcpServer::new(host, principal_resolver);
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
        move || Ok(RmcpServer::new(host.clone(), principal_resolver.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = Router::new().route_service("/mcp", service);

    axum::serve(listener, app)
        .with_graceful_shutdown(wait_for_shutdown(shutdown))
        .await
        .map_err(FrontendError::Io)
}

fn cancellation_token_from_shutdown(shutdown: ShutdownToken) -> CancellationToken {
    let ct = CancellationToken::new();
    let cancel = ct.clone();
    tokio::spawn(async move {
        wait_for_shutdown(shutdown).await;
        cancel.cancel();
    });
    ct
}

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
    match err {
        DispatchError::ToolError(err) => error_to_rmcp_call_tool_result(err),
        DispatchError::UnknownTool(_) => CallToolResult::error(vec![Content::text("not-found")]),
        DispatchError::SessionId(_) => {
            CallToolResult::error(vec![Content::text("invalid-session-id")])
        }
        DispatchError::Auth(_) => CallToolResult::error(vec![Content::text("auth-denied")]),
        DispatchError::Manifest(_) => {
            CallToolResult::error(vec![Content::text("manifest-persistence-failed")])
        }
        DispatchError::Redaction(_) => {
            CallToolResult::error(vec![Content::text("redaction-failed")])
        }
        DispatchError::ResponseSerialization(_) => {
            CallToolResult::error(vec![Content::text("response-serialization-failed")])
        }
        _ => CallToolResult::error(vec![Content::text("internal")]),
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
        let text = &result.content[0].raw.as_text().unwrap().text;
        assert_eq!(text, "manifest-persistence-failed");
    }
}
