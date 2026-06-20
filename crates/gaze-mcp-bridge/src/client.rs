use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rmcp::model::{
    CallToolRequest, CallToolRequestParams, CallToolResult, ClientRequest, JsonObject,
    ServerResult, Tool,
};
use rmcp::service::{PeerRequestOptions, RoleClient, RunningService, ServiceError};
use rmcp::transport::TokioChildProcess;
use rmcp::{serve_client, Peer};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::config::ServerSpec;
use crate::error::{BridgeError, BridgeResult};

#[derive(Debug, Clone)]
pub struct DownstreamTool {
    pub raw_name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
    pub output_schema: Option<serde_json::Value>,
}

impl DownstreamTool {
    pub fn from_rmcp(tool: Tool) -> Self {
        Self {
            raw_name: tool.name.into_owned(),
            description: tool.description.map(|value| value.into_owned()),
            input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
            output_schema: tool
                .output_schema
                .map(|schema| serde_json::Value::Object((*schema).clone())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DownstreamError {
    #[error("downstream json-rpc error: {message}")]
    Mcp {
        message: String,
        data: Option<serde_json::Value>,
    },
    #[error("downstream service error: {0}")]
    Service(String),
}

#[async_trait]
pub trait DownstreamClient: Send + Sync {
    async fn list_tools(&self, timeout: Duration) -> BridgeResult<Vec<DownstreamTool>>;

    async fn list_resources(&self, timeout: Duration) -> BridgeResult<usize>;

    async fn list_prompts(&self, timeout: Duration) -> BridgeResult<usize>;

    async fn call_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> Result<CallToolResult, DownstreamError>;
}

pub struct RmcpChildClient {
    peer: Peer<RoleClient>,
    _service: Mutex<RunningService<RoleClient, ()>>,
}

impl RmcpChildClient {
    pub async fn spawn(spec: &ServerSpec) -> BridgeResult<Arc<Self>> {
        let mut command = Command::new(&spec.command);
        command.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &spec.env {
            command.env(key, value);
        }

        let (transport, stderr) = TokioChildProcess::builder(command)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| BridgeError::Downstream(format!("child spawn failed: {err}")))?;
        if let Some(stderr) = stderr {
            tokio::spawn(drain_stderr(stderr));
        }
        let service = serve_client((), transport)
            .await
            .map_err(|err| BridgeError::Downstream(format!("client initialize failed: {err}")))?;
        let peer = service.peer().clone();
        Ok(Arc::new(Self {
            peer,
            _service: Mutex::new(service),
        }))
    }

    fn peer(&self) -> Peer<RoleClient> {
        self.peer.clone()
    }
}

#[async_trait]
impl DownstreamClient for RmcpChildClient {
    async fn list_tools(&self, timeout: Duration) -> BridgeResult<Vec<DownstreamTool>> {
        let peer = self.peer();
        let tools = tokio::time::timeout(timeout, peer.list_all_tools())
            .await
            .map_err(|_| BridgeError::Downstream("list_tools timed out".to_string()))?
            .map_err(service_error_to_bridge)?;
        Ok(tools.into_iter().map(DownstreamTool::from_rmcp).collect())
    }

    async fn list_resources(&self, timeout: Duration) -> BridgeResult<usize> {
        let peer = self.peer();
        match tokio::time::timeout(timeout, peer.list_all_resources()).await {
            Ok(Ok(resources)) => Ok(resources.len()),
            Ok(Err(_)) | Err(_) => Ok(0),
        }
    }

    async fn list_prompts(&self, timeout: Duration) -> BridgeResult<usize> {
        let peer = self.peer();
        match tokio::time::timeout(timeout, peer.list_all_prompts()).await {
            Ok(Ok(prompts)) => Ok(prompts.len()),
            Ok(Err(_)) | Err(_) => Ok(0),
        }
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> Result<CallToolResult, DownstreamError> {
        let arguments = match args {
            serde_json::Value::Object(map) => map,
            _ => JsonObject::new(),
        };
        let params = CallToolRequestParams::new(tool_name.to_string()).with_arguments(arguments);
        let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
        let mut options = PeerRequestOptions::no_options();
        options.timeout = Some(timeout);
        let handle = self
            .peer()
            .send_request_with_option(request, options)
            .await
            .map_err(downstream_error)?;
        let response = handle.await_response().await.map_err(downstream_error)?;
        match response {
            ServerResult::CallToolResult(result) => Ok(result),
            _ => Err(DownstreamError::Service(
                "unexpected downstream response type".to_string(),
            )),
        }
    }
}

async fn drain_stderr(mut stderr: tokio::process::ChildStderr) {
    let mut buf = [0_u8; 4096];
    loop {
        match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

fn service_error_to_bridge(err: ServiceError) -> BridgeError {
    match err {
        ServiceError::McpError(mcp) => BridgeError::Downstream(format!(
            "downstream method rejected with code {:?}",
            mcp.code
        )),
        other => BridgeError::Downstream(other.to_string()),
    }
}

fn downstream_error(err: ServiceError) -> DownstreamError {
    match err {
        ServiceError::McpError(mcp) => DownstreamError::Mcp {
            message: mcp.message.into_owned(),
            data: mcp.data,
        },
        other => DownstreamError::Service(other.to_string()),
    }
}
