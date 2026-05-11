use crate::error::CliError;

use super::ServeArgs;

pub(crate) fn run(args: ServeArgs) -> Result<(), CliError> {
    let _ = (args.manifest_dir, args.max_file_size);
    Err(CliError::McpDetail(
        "`gaze mcp serve` implementation is incomplete".to_string(),
    ))
}
