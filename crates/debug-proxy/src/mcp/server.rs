use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use gaze::{Scope, Session};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::transport::stdio;
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::adapter::laravel_log::LaravelLogAdapter;
use crate::adapter::mysql::MysqlAdapter;
use crate::adapter::ssh_tunnel::{SshTunnel, TunnelError, TunnelSpec};
use crate::adapter::{TableSchemaOut, ToolContext};
use crate::cli::serve::{self, ServeError};

/// Hard caps on caller-supplied limits. MCP clients (LLMs, operators) cannot
/// exceed these, regardless of what they request. Prevents unbounded fetches.
const MAX_DB_SAMPLE_ROWS: usize = 10_000;
const MAX_DB_DISTINCT_ROWS: usize = 10_000;
const MAX_LOG_LINES: usize = 10_000;

fn clamp_limit(requested: Option<usize>, default: usize, max: usize) -> usize {
    requested.unwrap_or(default).min(max)
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error(transparent)]
    Serve(#[from] ServeError),
    #[error(transparent)]
    Tunnel(#[from] TunnelError),
    #[error("mysql adapter: {0}")]
    Adapter(#[from] crate::adapter::AdapterError),
    #[error("session: {0}")]
    Session(#[from] gaze::Error),
    #[error("rmcp service: {0}")]
    Rmcp(String),
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DbSchemaArgs {
    pub table: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DbSampleArgs {
    pub table: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DbCountArgs {
    pub table: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DbDistinctArgs {
    pub table: String,
    pub column: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsSearchArgs {
    pub pattern: String,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsTailArgs {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogsContextArgs {
    pub request_id: String,
}

#[derive(Debug, Serialize)]
pub struct DbTablesResult {
    pub tables: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DbSampleResult {
    pub rows: Vec<gaze::CleanDocument>,
}

#[derive(Debug, Serialize)]
pub struct DbCountResult {
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct DbDistinctResult {
    pub values: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct LogsResult {
    pub lines: Vec<String>,
}

#[derive(Clone)]
pub struct DebugProxyServer {
    ctx: Arc<ToolContext<MysqlAdapter, LaravelLogAdapter>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router(router = tool_router)]
impl DebugProxyServer {
    fn new(ctx: Arc<ToolContext<MysqlAdapter, LaravelLogAdapter>>) -> Self {
        Self {
            ctx,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "db.tables",
        description = "List tables visible through the debug proxy."
    )]
    async fn db_tables(&self) -> Result<CallToolResult, ErrorData> {
        to_result(
            self.ctx
                .db_tables()
                .await
                .map(|tables| DbTablesResult { tables }),
        )
    }

    #[tool(
        name = "db.schema",
        description = "Describe one table's schema and policy classes."
    )]
    async fn db_schema(
        &self,
        Parameters(args): Parameters<DbSchemaArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        to_result(self.ctx.db_schema(&args.table).await)
    }

    #[tool(
        name = "db.sample",
        description = "Return redacted sample rows from a table."
    )]
    async fn db_sample(
        &self,
        Parameters(args): Parameters<DbSampleArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        to_result(
            self.ctx
                .db_sample(&args.table, clamp_limit(args.limit, 10, MAX_DB_SAMPLE_ROWS))
                .await
                .map(|rows| DbSampleResult { rows }),
        )
    }

    #[tool(name = "db.count", description = "Count rows in a table.")]
    async fn db_count(
        &self,
        Parameters(args): Parameters<DbCountArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        to_result(
            self.ctx
                .db_count(&args.table)
                .await
                .map(|count| DbCountResult { count }),
        )
    }

    #[tool(
        name = "db.distinct",
        description = "Return redacted distinct values from one column."
    )]
    async fn db_distinct(
        &self,
        Parameters(args): Parameters<DbDistinctArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        to_result(
            self.ctx
                .db_distinct(
                    &args.table,
                    &args.column,
                    clamp_limit(args.limit, 50, MAX_DB_DISTINCT_ROWS),
                )
                .await
                .map(|values| DbDistinctResult { values }),
        )
    }

    #[tool(
        name = "logs.search",
        description = "Search the configured Laravel log."
    )]
    async fn logs_search(
        &self,
        Parameters(args): Parameters<LogsSearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        to_result(
            self.ctx
                .logs_search(
                    &args.pattern,
                    args.level.as_deref(),
                    clamp_limit(args.limit, 100, MAX_LOG_LINES),
                )
                .await
                .map(|lines| LogsResult { lines }),
        )
    }

    #[tool(name = "logs.tail", description = "Return the last N log lines.")]
    async fn logs_tail(
        &self,
        Parameters(args): Parameters<LogsTailArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        to_result(
            self.ctx
                .log_tail(clamp_limit(args.limit, 100, MAX_LOG_LINES))
                .await
                .map(|lines| LogsResult {
                    lines: lines.into_iter().map(clean_document_to_text).collect(),
                }),
        )
    }

    #[tool(
        name = "logs.context",
        description = "Return log lines for one request id."
    )]
    async fn logs_context(
        &self,
        Parameters(args): Parameters<LogsContextArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        to_result(
            self.ctx
                .logs_context(&args.request_id)
                .await
                .map(|lines| LogsResult { lines }),
        )
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DebugProxyServer {}

pub async fn run(policy_path: &Path) -> Result<(), ServerError> {
    let prepared = serve::prepare(policy_path)?;
    let _tunnel = SshTunnel::open(&TunnelSpec {
        ssh_host: prepared.connection.ssh_host.clone(),
        local_port: prepared.connection.local_port,
        remote_host: prepared.connection.remote_host.clone(),
        remote_port: prepared.connection.remote_port,
    })?;
    let db = Arc::new(
        MysqlAdapter::connect(&serve::mysql_url(&prepared.connection, &prepared.password)).await?,
    );
    let logs = prepared
        .policy
        .policy
        .logs
        .as_ref()
        .and_then(|logs| logs.path.clone())
        .map(|path| Arc::new(LaravelLogAdapter::new(path)));
    let session = Arc::new(Session::new(Scope::Conversation(
        "debug-proxy-stdio".to_string(),
    ))?);
    let ctx = Arc::new(ToolContext::with_policy(
        prepared.pipeline,
        session,
        Some(Arc::new(prepared.policy)),
        db,
        logs,
    ));
    let server = DebugProxyServer::new(ctx);
    let running = server
        .serve(stdio())
        .await
        .map_err(|err| ServerError::Rmcp(err.to_string()))?;
    running
        .waiting()
        .await
        .map_err(|err| ServerError::Rmcp(err.to_string()))?;
    Ok(())
}

fn to_result<T: Serialize>(
    result: Result<T, crate::adapter::ProxyError>,
) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(value) => {
            let json = serde_json::to_string(&value)
                .map_err(|err| ErrorData::internal_error(err.to_string(), None))?;
            Ok(CallToolResult::success(vec![Content::text(json)]))
        }
        Err(err) => Err(ErrorData::internal_error(err.to_string(), None)),
    }
}

fn clean_document_to_text(document: gaze::CleanDocument) -> String {
    match document {
        gaze::CleanDocument::Text(text) => text,
        gaze::CleanDocument::Structured(fields) => {
            serde_json::to_string(&fields).unwrap_or_default()
        }
    }
}

#[allow(dead_code)]
fn _keep_schema_out_serializable(_schema: &TableSchemaOut) {}
