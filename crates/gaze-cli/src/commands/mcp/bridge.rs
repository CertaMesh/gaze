use std::sync::Arc;

use gaze_mcp_bridge::{BridgeConfig, BridgeHost, FileBridgeAuditSink};
use gaze_mcp_core::{Frontend, ShutdownToken};
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};

use crate::error::CliError;

use super::serve::default_manifest_dir;
use super::BridgeArgs;

pub(crate) fn run(args: BridgeArgs) -> Result<(), CliError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| CliError::McpDetail(format!("tokio runtime init failed: {err}")))?;
    runtime.block_on(run_async(args))
}

async fn run_async(args: BridgeArgs) -> Result<(), CliError> {
    let mut config = BridgeConfig::from_path(&args.config)
        .map_err(|err| CliError::McpDetail(err.to_string()))?;
    if let Some(session_dir) = args.session_dir {
        config.session.dir = Some(session_dir);
        config
            .validate()
            .map_err(|err| CliError::McpDetail(err.to_string()))?;
    }

    let audit = Arc::new(FileBridgeAuditSink::new(
        default_manifest_dir().join("bridge-audit.jsonl"),
    ));
    let host = BridgeHost::from_config(config, audit)
        .await
        .map_err(|err| CliError::McpDetail(err.to_string()))?;

    if args.dry_run || args.print_tools {
        for row in host.registry().print_surface() {
            println!("{row}");
        }
        if args.dry_run {
            println!("policy loaded fail-closed");
        }
        return Ok(());
    }

    let frontend = RmcpFrontend::stdio(Arc::new(FixedPrincipalResolver::agent("gaze-cli")));
    frontend
        .serve(Arc::new(host), ShutdownToken::new())
        .await
        .map_err(|err| CliError::McpDetail(format!("mcp bridge stdio server failed: {err}")))
}
