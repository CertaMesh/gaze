use crate::error::{CliError, RestoreMode};
use crate::restore::run_restore;
use std::path::PathBuf;

pub(crate) struct Args {
    pub(crate) format: String,
    pub(crate) restore_mode: RestoreMode,
    pub(crate) telemetry: bool,
    pub(crate) audit_db: Option<PathBuf>,
    pub(crate) max_bytes: u64,
}

pub(crate) fn run(args: Args) -> std::result::Result<(), CliError> {
    run_restore(
        &args.format,
        args.restore_mode,
        args.telemetry,
        args.audit_db.as_deref(),
        args.max_bytes,
    )
}
