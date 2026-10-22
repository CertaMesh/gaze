use std::io::{self, Read};

use crate::error::CliError;

/// Default max-bytes cap for stdin. Keeps a runaway or attacker-controlled
/// upstream from OOM'ing the worker. Override with `--max-bytes`.
pub(crate) const DEFAULT_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// Read stdin up to `max_bytes + 1` and return the bytes.
///
/// Reading one extra byte past the cap lets us distinguish "input exactly
/// at the limit" from "input exceeds the limit" without a second probe.
pub(crate) fn read_stdin_bytes(max_bytes: u64) -> std::result::Result<Vec<u8>, CliError> {
    let mut buf = Vec::new();
    let limit = max_bytes.saturating_add(1);
    io::stdin()
        .take(limit)
        .read_to_end(&mut buf)
        .map_err(|_| CliError::Io)?;
    if buf.len() as u64 > max_bytes {
        return Err(CliError::InputTooLarge);
    }
    Ok(buf)
}

/// Read stdin as UTF-8 text, enforcing the size cap. Distinguishes:
///   - 0 bytes              -> `EmptyInput`     (exit 1)
///   - > max_bytes          -> `InputTooLarge` (exit 1)
///   - non-UTF-8            -> `InvalidEncoding` (exit 1)
///   - IO / OS error        -> `Io`            (exit 4)
///
/// `clean` calls this; `restore` uses the bytes path directly since the
/// restore stdin is JSON and serde_json does its own UTF-8 validation.
pub(crate) fn read_stdin_text(max_bytes: u64) -> std::result::Result<String, CliError> {
    let bytes = read_stdin_bytes(max_bytes)?;
    if bytes.is_empty() {
        return Err(CliError::EmptyInput);
    }
    String::from_utf8(bytes).map_err(|_| CliError::InvalidEncoding)
}

pub(crate) fn require_json_format(format: &str) -> std::result::Result<(), CliError> {
    if format == "json" {
        Ok(())
    } else {
        Err(CliError::PolicyConfigDetail(format!(
            "--format must be 'json', got '{format}'"
        )))
    }
}

/// Stdout shape for `clean` and `restore`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputFormat {
    /// The JSON contract for programs: session and text travel in JSON.
    Json,
    /// Plain text only; the session lives in an owner-side file.
    Text,
}

/// Resolve `--format` against the session-file flag. A session file means
/// plain text, `json` keeps the session in the JSON body, and every mixed
/// combination is refused so the session never lands somewhere unexpected.
pub(crate) fn resolve_output_format(
    format: Option<&str>,
    has_session_file: bool,
    session_flag: &str,
) -> std::result::Result<OutputFormat, CliError> {
    match (format, has_session_file) {
        (None | Some("json"), false) => Ok(OutputFormat::Json),
        (None | Some("text"), true) => Ok(OutputFormat::Text),
        (Some("text"), false) => Err(CliError::PolicyConfigDetail(format!(
            "--format text requires {session_flag} <PATH>"
        ))),
        (Some("json"), true) => Err(CliError::PolicyConfigDetail(format!(
            "{session_flag} uses plain text on stdin/stdout; it cannot be combined with --format json"
        ))),
        (Some(other), _) => Err(CliError::PolicyConfigDetail(format!(
            "--format must be 'json' or 'text', got '{other}'"
        ))),
    }
}

/// Write `bytes` to stdout exactly, with no added newline.
pub(crate) fn write_stdout_exact(bytes: &[u8]) -> std::result::Result<(), CliError> {
    use std::io::Write;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(bytes)
        .and_then(|()| stdout.flush())
        .map_err(|_| CliError::Io)
}
