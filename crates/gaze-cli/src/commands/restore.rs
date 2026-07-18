use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gaze::{
    Action, ConflictTier, DocumentKind, PiiClass, RedactionEntry, RestoreTelemetry, Session,
};
use gaze_audit::SqliteLogger;

use crate::error::{CliError, RestoreMode};
use crate::restore::run_restore;

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
        persist_restore_telemetry,
    )
}

fn persist_restore_telemetry(
    audit_db: &std::path::Path,
    session: &Session,
    telemetry: RestoreTelemetry,
) -> std::result::Result<(), CliError> {
    let logger = SqliteLogger::new(audit_db).map_err(|_| CliError::Pipeline)?;
    let entry = RedactionEntry::new(
        "restore",
        PiiClass::Custom("restore.telemetry".to_string()),
        Action::Preserve,
        None,
        DocumentKind::Text,
        false,
        ConflictTier::None,
        current_epoch_ms(),
        Some(session.audit_session_id().to_string()),
    )
    .with_restore_telemetry(telemetry);
    logger.log(&entry).map_err(|_| CliError::Pipeline)
}

fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
