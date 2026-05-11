use crate::error::CliError;

use super::DoctorArgs;

pub(crate) fn run(args: DoctorArgs) -> Result<(), CliError> {
    let _ = (args.agents_md, args.strict, args.json);
    Err(CliError::McpDetail(
        "`gaze mcp doctor` implementation is incomplete".to_string(),
    ))
}
