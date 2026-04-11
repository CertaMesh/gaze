//! `gaze audit` — dump the audit log. Real impl lands in M5 alongside the
//! SQLite audit schema; stub for now.

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("not yet implemented (wired in M5)")]
    NotYet,
}

pub fn run() -> Result<(), AuditError> {
    Err(AuditError::NotYet)
}
