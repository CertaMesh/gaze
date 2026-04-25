mod manifest;
mod session;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use gaze::{SensitiveSnapshot, Session};

use crate::error::{CliError, RestoreMode};
use crate::io::{read_stdin_bytes, require_json_format};
use crate::restore::manifest::{RestoreRequest, RestoreResponse};
use crate::restore::session::{restore_pass1, restore_pass2_validate};

pub(crate) fn run_restore(
    format: &str,
    restore_mode: RestoreMode,
    max_bytes: u64,
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
    let restore_warning = restore_pass2_validate(
        &pass1.text,
        &pass1.substitution_spans,
        &session,
        restore_mode,
    )?;

    let response = RestoreResponse {
        text: pass1.text,
        restore_warning,
    };
    let json = serde_json::to_string(&response).map_err(|_| CliError::Pipeline)?;
    println!("{json}");
    Ok(())
}
