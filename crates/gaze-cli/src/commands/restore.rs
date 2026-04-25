use crate::error::{CliError, RestoreMode};
use crate::restore::run_restore;

pub(crate) struct Args {
    pub(crate) format: String,
    pub(crate) restore_mode: RestoreMode,
    pub(crate) max_bytes: u64,
}

pub(crate) fn run(args: Args) -> std::result::Result<(), CliError> {
    run_restore(&args.format, args.restore_mode, args.max_bytes)
}
