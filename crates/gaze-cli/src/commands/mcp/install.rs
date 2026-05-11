use crate::error::CliError;

use super::InstallArgs;

pub(crate) fn run(args: InstallArgs) -> Result<(), CliError> {
    let _ = (
        args.client,
        args.agents_md,
        args.dry_run,
        args.skip_agents_md,
    );
    Err(CliError::McpDetail(
        "`gaze mcp install` implementation is incomplete".to_string(),
    ))
}
