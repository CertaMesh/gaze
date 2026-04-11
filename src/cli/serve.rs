//! `gaze serve` — start the MCP stdio server. Real wiring lives in
//! `crate::mcp::server`; this module is the thin CLI-facing entry point.

use std::path::Path;

pub use crate::mcp::server::ServerError;

/// CLI entry point: loads policy, opens tunnel/DB, and hands off to the
/// rmcp stdio transport.
pub async fn run_cmd(policy: &Path, global_audit: bool) -> Result<(), ServerError> {
    crate::mcp::server::run(policy, global_audit).await
}
