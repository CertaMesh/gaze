mod manifest;
pub(crate) mod session;

use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use gaze::{RestorePolicy, RestoreTelemetry, SensitiveSnapshot, Session};

use crate::error::{CliError, RestoreMode};
use crate::io::{read_stdin_bytes, resolve_output_format, write_stdout_exact, OutputFormat};
use crate::restore::manifest::{RestoreRequest, RestoreResponse};
use crate::restore::session::restore_pass2_validate;
use crate::session_file::read_session_blob;

pub(crate) fn run_restore(
    format: Option<&str>,
    session_file: Option<&Path>,
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
    let output = resolve_output_format(format, session_file.is_some(), "--session-file")?;
    if output == OutputFormat::Text && telemetry_enabled {
        return Err(CliError::PolicyConfigDetail(
            "--telemetry is reported in the JSON response; use --audit-db with --session-file"
                .into(),
        ));
    }
    let stdin_bytes = read_stdin_bytes(max_bytes)?;

    let (blob_bytes, text) = match session_file {
        // Plain-text mode: the session comes from the owner-side file and stdin
        // is the text itself.
        Some(path) => {
            let text = String::from_utf8(stdin_bytes).map_err(|_| CliError::InvalidEncoding)?;
            (read_session_blob(path)?, text)
        }
        None => {
            let request: RestoreRequest =
                serde_json::from_slice(&stdin_bytes).map_err(|_| CliError::StdinParse)?;
            let blob_bytes = BASE64
                .decode(request.session_blob.as_bytes())
                .map_err(|_| CliError::StdinParse)?;
            (blob_bytes, request.text)
        }
    };

    let session =
        Session::import(SensitiveSnapshot::from(blob_bytes)).map_err(|err| match err {
            gaze::Error::InvalidSnapshotSignature => CliError::InvalidSignature,
            gaze::Error::InvalidSnapshotVersion(_) => CliError::InvalidBlobVersion,
            gaze::Error::InvalidSnapshotPayload => CliError::InvalidBlobVersion,
            gaze::Error::BlobExpired { .. } => CliError::BlobExpired,
            _ => CliError::Pipeline,
        })?;

    let assessment = session
        .assess_restore_text(&text)
        .map_err(|_| CliError::Pipeline)?;
    let restore_telemetry = if telemetry_enabled || audit_db.is_some() {
        let telemetry = assessment.telemetry(restore_policy(restore_mode));
        if let Some(path) = audit_db {
            persist_restore_telemetry(path, &session, telemetry.clone())?;
        }
        telemetry_enabled.then_some(telemetry)
    } else {
        None
    };
    let restore_warning = restore_pass2_validate(&assessment, restore_mode)?;
    let text = assessment.into_restored().text;

    if output == OutputFormat::Text {
        // Tolerant warnings name only Gaze-issued token shapes, never raw values.
        for warning in &restore_warning {
            let line = serde_json::json!({ "warning": warning.variant, "token": warning.token });
            eprintln!("{line}");
        }
        return write_stdout_exact(text.as_bytes());
    }

    let response = RestoreResponse {
        text,
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
