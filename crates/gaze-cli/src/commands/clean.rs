use std::path::PathBuf;

use clap::Args as ClapArgs;

use super::shared_args::CleanPipelineArgs;
use crate::error::CliError;
use crate::io::DEFAULT_MAX_BYTES;
use crate::pipeline::run_clean;

/// The complete `gaze clean` flag surface.
#[derive(ClapArgs, Debug)]
pub(crate) struct Args {
    #[command(flatten)]
    pub(crate) pipeline: CleanPipelineArgs,
    /// Output format. Only `json` is supported today.
    #[arg(long, default_value = "json")]
    pub(crate) format: String,
    /// Max stdin size in bytes. stdin longer than this exits 1 InputTooLarge.
    #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
    pub(crate) max_bytes: u64,
    /// Optional SQLite redaction-log database path.
    #[arg(long)]
    pub(crate) audit_db: Option<PathBuf>,
}

pub(crate) fn run(args: Args) -> std::result::Result<(), CliError> {
    run_clean(
        args.pipeline
            .options(&args.format, args.max_bytes, args.audit_db.as_deref()),
    )
}
