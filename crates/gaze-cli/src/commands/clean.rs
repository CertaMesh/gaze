use std::path::PathBuf;

use clap::Args as ClapArgs;

use super::shared_args::CleanPipelineArgs;
use crate::error::CliError;
use crate::io::{resolve_output_format, OutputFormat, DEFAULT_MAX_BYTES};
use crate::pipeline::run_clean;
use crate::session_file::SessionOut;

/// The complete `gaze clean` flag surface.
#[derive(ClapArgs, Debug)]
pub(crate) struct Args {
    #[command(flatten)]
    pub(crate) pipeline: CleanPipelineArgs,
    /// Output format: `json` (default) or `text`. `text` prints only the clean text and requires --session-out.
    #[arg(long)]
    pub(crate) format: Option<String>,
    /// Max stdin size in bytes. stdin longer than this exits 1 InputTooLarge.
    #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
    pub(crate) max_bytes: u64,
    /// Optional SQLite redaction-log database path.
    #[arg(long)]
    pub(crate) audit_db: Option<PathBuf>,
    /// Write the restore session to this new owner-only file and print only the clean text.
    #[arg(long, value_name = "PATH")]
    pub(crate) session_out: Option<PathBuf>,
    /// Replace an existing --session-out file instead of refusing.
    #[arg(long, requires = "session_out")]
    pub(crate) overwrite_session_out: bool,
}

pub(crate) fn run(args: Args) -> std::result::Result<(), CliError> {
    let format = match resolve_output_format(
        args.format.as_deref(),
        args.session_out.is_some(),
        "--session-out",
    )? {
        OutputFormat::Json => "json",
        OutputFormat::Text => "text",
    };
    let mut options = args
        .pipeline
        .options(format, args.max_bytes, args.audit_db.as_deref());
    options.session_out = args.session_out.as_deref().map(|path| SessionOut {
        path,
        overwrite: args.overwrite_session_out,
    });
    run_clean(options)
}
