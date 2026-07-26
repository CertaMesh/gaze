mod manifest;
mod session;

use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use gaze::{Pipeline, RestorePolicy, RestoreTelemetry, SensitiveSnapshot, Session};

use crate::error::{CliError, RestoreMode};
use crate::io::{read_stdin_bytes, require_json_format};
use crate::restore::manifest::{RestoreRequest, RestoreResponse};
use crate::restore::session::{restore_pass1, restore_pass2_validate};

pub(crate) fn run_restore(
    format: &str,
    restore_mode: RestoreMode,
    telemetry_enabled: bool,
    audit_db: Option<&Path>,
    max_bytes: u64,
    persist_restore_telemetry: fn(
        &Path,
        &Session,
        RestoreTelemetry,
    ) -> std::result::Result<(), CliError>,
) -> std::result::Result<(), CliError> {
    require_json_format(format)?;
    let stdin_bytes = read_stdin_bytes(max_bytes)?;

    let request: RestoreRequest =
        serde_json::from_slice(&stdin_bytes).map_err(|_| CliError::StdinParse)?;

    let blob_bytes = BASE64
        .decode(request.session_blob.as_bytes())
        .map_err(|_| CliError::StdinParse)?;

    let session =
        Session::import(SensitiveSnapshot::from(blob_bytes)).map_err(|err| match err {
            gaze::Error::InvalidSnapshotSignature => CliError::InvalidSignature,
            gaze::Error::InvalidSnapshotVersion(_) => CliError::InvalidBlobVersion,
            gaze::Error::InvalidSnapshotPayload => CliError::InvalidBlobVersion,
            gaze::Error::BlobExpired { .. } => CliError::BlobExpired,
            _ => CliError::Pipeline,
        })?;

    let pass1 = restore_pass1(&session, &request.text)?;
    let restore_telemetry = if telemetry_enabled || audit_db.is_some() {
        let pipeline = Pipeline::builder()
            .build()
            .map_err(|_| CliError::Pipeline)?;
        let (_, telemetry) = pipeline
            .restore_with_policy_telemetry(&session, &request.text, restore_policy(restore_mode))
            .map_err(|_| CliError::Pipeline)?;
        if let Some(path) = audit_db {
            persist_restore_telemetry(path, &session, telemetry.clone())?;
        }
        telemetry_enabled.then_some(telemetry)
    } else {
        None
    };
    let restore_warning = restore_pass2_validate(
        &pass1.text,
        &pass1.substitution_spans,
        &session,
        restore_mode,
    )?;

    let response = RestoreResponse {
        text: pass1.text,
        restore_warning,
        restore_telemetry,
    };
    let json = serde_json::to_string(&response).map_err(|_| CliError::Pipeline)?;
    println!("{json}");
    Ok(())
}

fn restore_policy(mode: RestoreMode) -> RestorePolicy {
    match mode {
        RestoreMode::Strict => RestorePolicy::Strict,
        RestoreMode::Tolerant => RestorePolicy::Lenient,
    }
}
