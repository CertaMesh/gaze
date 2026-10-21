//! Owner-side session files for plain-shell `clean` → `restore` round trips.
//!
//! `gaze clean --session-out <PATH>` writes `{"session_blob": "<base64>"}` to a
//! new owner-only (`0600`, verified empty extended ACL on macOS/Linux) file
//! and prints only the clean text. Other platforms refuse session creation.
//! `gaze restore --session-file <PATH>` reads it back and restores plain text
//! from stdin. The session holds raw values, so it never reaches stdout.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::CliError;

mod acl;

/// Read cap for a session file. Sessions are owner-side and grow with the
/// number of distinct values; this only stops a wrong path from exhausting memory.
const MAX_SESSION_FILE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct SessionFileJson {
    session_blob: String,
}

/// Where `gaze clean` writes its session, and whether replacing a file is allowed.
pub(crate) struct SessionOut<'a> {
    pub(crate) path: &'a Path,
    pub(crate) overwrite: bool,
}

impl SessionOut<'_> {
    /// Refuse an existing path before any work is done. Symlinks count as
    /// existing, dangling or not, so a session is never written through one.
    pub(crate) fn preflight(&self) -> Result<(), CliError> {
        if !cfg!(any(target_os = "macos", target_os = "linux")) {
            return Err(CliError::SessionFile(
                "owner-only --session-out creation is supported only on macOS and Linux".into(),
            ));
        }
        if !self.overwrite && fs::symlink_metadata(self.path).is_ok() {
            return Err(self.exists_error());
        }
        Ok(())
    }

    /// Write the session. Without `overwrite` the file is created exclusively;
    /// with it, a fresh owner-only file is renamed over the old path.
    pub(crate) fn write(&self, session_blob: &str) -> Result<(), CliError> {
        let mut body = serde_json::to_vec(&SessionFileJson {
            session_blob: session_blob.to_string(),
        })
        .map_err(|_| CliError::Pipeline)?;
        body.push(b'\n');

        if !self.overwrite {
            return match create_owner_only(self.path) {
                Ok(file) => write_and_sync(file, &body, self.path),
                Err(err) if err.kind() == ErrorKind::AlreadyExists => Err(self.exists_error()),
                Err(err) => Err(write_error(self.path, &err)),
            };
        }

        let temp = temp_path(self.path);
        let file = create_owner_only(&temp).map_err(|err| write_error(self.path, &err))?;
        if let Err(err) = write_and_sync(file, &body, self.path) {
            let _ = fs::remove_file(&temp);
            return Err(err);
        }
        fs::rename(&temp, self.path).map_err(|err| {
            let _ = fs::remove_file(&temp);
            write_error(self.path, &err)
        })
    }

    fn exists_error(&self) -> CliError {
        CliError::SessionFile(format!(
            "session file `{}` already exists; pass --overwrite-session-out to replace it",
            self.path.display()
        ))
    }
}

/// Read a session file written by `gaze clean --session-out` and return the
/// decoded snapshot bytes. Signature, version and expiry checks stay with
/// `Session::import`, so they fail with the same typed errors as JSON stdin.
pub(crate) fn read_session_blob(path: &Path) -> Result<Vec<u8>, CliError> {
    let file = File::open(path).map_err(|err| match err.kind() {
        ErrorKind::NotFound => {
            CliError::SessionFile(format!("session file `{}` does not exist", path.display()))
        }
        _ => CliError::SessionFile(format!(
            "cannot read session file `{}`: {}",
            path.display(),
            err.kind()
        )),
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_SESSION_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| {
            CliError::SessionFile(format!(
                "cannot read session file `{}`: {}",
                path.display(),
                err.kind()
            ))
        })?;
    if bytes.len() as u64 > MAX_SESSION_FILE_BYTES {
        return Err(CliError::SessionFile(format!(
            "session file `{}` is larger than {MAX_SESSION_FILE_BYTES} bytes",
            path.display()
        )));
    }
    let not_a_session =
        || CliError::SessionFile(format!("`{}` is not a gaze session file", path.display()));
    let parsed: SessionFileJson = serde_json::from_slice(&bytes).map_err(|_| not_a_session())?;
    BASE64
        .decode(parsed.session_blob.as_bytes())
        .map_err(|_| not_a_session())
}

fn create_owner_only(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    if let Err(err) = acl::clear_and_verify(&file) {
        // No session bytes have been written; discard the unsafe empty file.
        drop(file);
        let _ = fs::remove_file(path);
        return Err(err);
    }
    Ok(file)
}

fn write_and_sync(mut file: File, body: &[u8], path: &Path) -> Result<(), CliError> {
    file.write_all(body)
        .and_then(|()| file.sync_all())
        .map_err(|err| write_error(path, &err))
}

fn write_error(path: &Path, err: &std::io::Error) -> CliError {
    CliError::SessionFile(format!(
        "cannot write session file `{}`: {}",
        path.display(),
        err.kind()
    ))
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "session".to_string());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    path.with_file_name(format!(".{name}.{}.{nanos}.tmp", std::process::id()))
}
