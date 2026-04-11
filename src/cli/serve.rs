//! `gaze serve` — start the MCP stdio server. Real wiring lands in M5;
//! for M3 this stub prints "not yet implemented" so downstream CLI
//! dispatching compiles.

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("not yet implemented (wired in M5)")]
    NotYet,
}

pub async fn run() -> Result<(), ServeError> {
    Err(ServeError::NotYet)
}
