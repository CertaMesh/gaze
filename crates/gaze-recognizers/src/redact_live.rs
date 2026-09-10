//! Opt-in bridge to a separately provisioned, privately licensed Redact executable.
//! Gaze owns tokens and manifests. No vendor source or weights ship in this crate.
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gaze_types::{Detection, Detector, PiiClass, RecognizerRuntimeError};
use serde::{Deserialize, Serialize};

const SOURCE: &str = "redact-patched-coreml-v1";
const MAX_INPUT: usize = 65_536;
const MAX_REPLY: usize = 1_048_576;
const DEADLINE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeError {
    Configuration,
    Protocol,
    Limit,
    Timeout,
    Backend,
    Alignment,
    UnknownLabel,
    Nonfinite,
    InvalidSpan,
    IncompleteWindow,
    Artifact,
    Schema,
    Busy,
}
impl BridgeError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Protocol => "protocol",
            Self::Limit => "limit",
            Self::Timeout => "timeout",
            Self::Backend => "backend",
            Self::Alignment => "alignment",
            Self::UnknownLabel => "unknown_label",
            Self::Nonfinite => "nonfinite",
            Self::InvalidSpan => "invalid_span",
            Self::IncompleteWindow => "incomplete_window",
            Self::Artifact => "artifact",
            Self::Schema => "schema",
            Self::Busy => "busy",
        }
    }
    fn remote(code: &str) -> Self {
        match code {
            "limit" => Self::Limit,
            "alignment" => Self::Alignment,
            "unknown_label" => Self::UnknownLabel,
            "nonfinite" => Self::Nonfinite,
            "invalid_span" => Self::InvalidSpan,
            "incomplete_window" => Self::IncompleteWindow,
            "artifact" => Self::Artifact,
            "schema" => Self::Schema,
            "backend" => Self::Backend,
            _ => Self::Protocol,
        }
    }
}
impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for BridgeError {}

/// One serialized, bounded process shared across calls, with no text cache.
/// On any protocol/runtime failure the process is discarded before another request.
pub struct RedactDetector {
    executable: PathBuf,
    model: PathBuf,
    state: Mutex<State>,
}
struct State {
    process: Option<Worker>,
    next_id: u64,
}
struct Worker {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}
impl Drop for Worker {
    fn drop(&mut self) {
        // The child creates its own process group; cancellation includes descendants.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.wait();
    }
}
#[derive(Serialize)]
struct Request<'a> {
    version: u8,
    id: u64,
    kind: &'static str,
    text: &'a str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    version: u8,
    id: u64,
    kind: String,
    complete: bool,
    threshold: f64,
    org: bool,
    content_tokens: usize,
    planned_windows: usize,
    completed_windows: usize,
    spans: Vec<Span>,
    dispositions: Dispositions,
    error: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dispositions {
    threshold: usize,
    arbitration: usize,
    cleanup: usize,
    special_tokens: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Span {
    start: usize,
    end: usize,
    label: String,
    score: f64,
}

impl RedactDetector {
    pub fn new(executable: PathBuf, model: PathBuf) -> Result<Self, BridgeError> {
        if !executable.is_absolute()
            || !executable.is_file()
            || !model.is_absolute()
            || !model.is_dir()
        {
            return Err(BridgeError::Configuration);
        }
        Ok(Self {
            executable,
            model,
            state: Mutex::new(State {
                process: None,
                next_id: 1,
            }),
        })
    }
    fn spawn(&self) -> Result<Worker, BridgeError> {
        let mut command = Command::new(&self.executable);
        // Explicit custody: no corpus-dependent environment or telemetry overrides.
        command.env_clear();
        for key in ["HOME", "TMPDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("DAL_APP_ID", "gaze-local-redact-primary")
            .env("DAL_COREML_COMPUTE_UNITS", "all")
            .arg(&self.model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn().map_err(|_| BridgeError::Backend)?;
        let input = child.stdin.take().ok_or(BridgeError::Backend)?;
        let output = child.stdout.take().ok_or(BridgeError::Backend)?;
        let worker = Worker {
            child,
            input,
            output,
        };
        for fd in [worker.input.as_raw_fd(), worker.output.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(BridgeError::Backend);
            }
        }
        Ok(worker)
    }
    fn batch(&self, text: &str) -> Result<Vec<Detection>, BridgeError> {
        if text.len() > MAX_INPUT {
            return Err(BridgeError::Limit);
        }
        let deadline = Instant::now() + DEADLINE;
        let mut state = self.state.try_lock().map_err(|_| BridgeError::Busy)?;
        let id = state.next_id;
        state.next_id = id.checked_add(1).ok_or(BridgeError::Limit)?;
        if state.process.is_none() {
            state.process = Some(self.spawn()?);
        }
        let result = (|| {
            let mut bytes = serde_json::to_vec(&Request {
                version: 1,
                id,
                kind: "primary",
                text,
            })
            .map_err(|_| BridgeError::Protocol)?;
            bytes.push(b'\n');
            let worker = state.process.as_mut().ok_or(BridgeError::Backend)?;
            let mut sent = 0;
            while sent < bytes.len() {
                ready(worker.input.as_raw_fd(), libc::POLLOUT, deadline)?;
                match worker.input.write(&bytes[sent..]) {
                    Ok(0) => return Err(BridgeError::Backend),
                    Ok(n) => sent += n,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => return Err(BridgeError::Backend),
                }
            }
            let mut response = Vec::new();
            loop {
                ready(worker.output.as_raw_fd(), libc::POLLIN, deadline)?;
                let mut chunk = [0u8; 8192];
                match worker.output.read(&mut chunk) {
                    Ok(0) => return Err(BridgeError::Backend),
                    Ok(n) => {
                        if response.len() + n > MAX_REPLY {
                            return Err(BridgeError::Limit);
                        }
                        response.extend_from_slice(&chunk[..n]);
                        if let Some(end) = response.iter().position(|b| *b == b'\n') {
                            if end + 1 != response.len() {
                                return Err(BridgeError::Protocol);
                            }
                            let reply: Reply = serde_json::from_slice(&response[..end])
                                .map_err(|_| BridgeError::Protocol)?;
                            return validate(reply, id, text);
                        }
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => return Err(BridgeError::Backend),
                }
            }
        })();
        if result.is_err() {
            state.process.take();
        }
        result
    }
}
fn ready(fd: i32, events: i16, deadline: Instant) -> Result<(), BridgeError> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(BridgeError::Timeout)?;
        let mut poll = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let result = unsafe {
            libc::poll(
                &mut poll,
                1,
                remaining.as_millis().clamp(1, i32::MAX as u128) as i32,
            )
        };
        if result == 0 {
            return Err(BridgeError::Timeout);
        }
        if result < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(BridgeError::Backend);
        }
        if poll.revents & events != 0 {
            return Ok(());
        }
        return Err(BridgeError::Backend);
    }
}
fn validate(reply: Reply, id: u64, text: &str) -> Result<Vec<Detection>, BridgeError> {
    if reply.version != 1
        || reply.id != id
        || reply.kind != "primary"
        || reply.threshold != 0.6
        || !reply.org
    {
        return Err(BridgeError::Protocol);
    }
    if let Some(error) = reply.error {
        if reply.complete || !reply.spans.is_empty() {
            return Err(BridgeError::Protocol);
        }
        return Err(BridgeError::remote(&error));
    }
    let planned = if reply.content_tokens == 0 {
        0
    } else {
        1 + reply.content_tokens.saturating_sub(254).div_ceil(190)
    };
    if !reply.complete || reply.planned_windows != planned || reply.completed_windows != planned {
        return Err(BridgeError::IncompleteWindow);
    }
    if reply.content_tokens > 32_768 || planned > 173 || reply.spans.len() > 4096 {
        return Err(BridgeError::Limit);
    }
    let d = reply.dispositions;
    if [d.threshold, d.arbitration, d.cleanup, d.special_tokens]
        .iter()
        .any(|v| *v > 1_000_000)
    {
        return Err(BridgeError::Limit);
    }
    let mut end = 0;
    reply
        .spans
        .into_iter()
        .map(|span| {
            let class = label_class(&span.label)?;
            if !span.score.is_finite() {
                return Err(BridgeError::Nonfinite);
            }
            if !(0.0..=1.0).contains(&span.score) {
                return Err(BridgeError::InvalidSpan);
            }
            if span.start < end || span.start >= span.end || span.end > text.len() {
                return Err(BridgeError::InvalidSpan);
            }
            if !text.is_char_boundary(span.start) || !text.is_char_boundary(span.end) {
                return Err(BridgeError::Alignment);
            }
            end = span.end;
            // Legacy Detector score 1.0 is routing metadata, not model confidence.
            Ok(Detection::new(
                span.start..span.end,
                class,
                format!("{SOURCE}:{}", span.label),
            ))
        })
        .collect()
}
pub fn label_class(label: &str) -> Result<PiiClass, BridgeError> {
    Ok(match label {
        "GIVEN_NAME" | "SURNAME" => PiiClass::Name,
        "ORG" => PiiClass::Organization,
        "STREET_NAME" | "BUILDING_NUMBER" | "SECONDARY_ADDRESS" | "CITY" | "STATE" => {
            PiiClass::Location
        }
        "EMAIL" => PiiClass::Email,
        other => PiiClass::Custom(
            match other {
                "ZIP_CODE" => "postal_code",
                "PHONE" => "phone",
                "URL" => "url",
                "CREDIT_CARD" => "credit_card",
                "SSN" => "ssn",
                "PASSPORT" => "passport",
                "DRIVERS_LICENSE" => "driver_license",
                "TAX_ID" => "tax_number",
                "BANK_ACCOUNT" => "bank_account",
                "ROUTING_NUMBER" => "routing_number",
                "GOVERNMENT_ID" => "government_id",
                "IMEI" => "imei",
                "IP_ADDRESS" => "redact_network_identifier",
                _ => return Err(BridgeError::UnknownLabel),
            }
            .into(),
        ),
    })
}
impl Detector for RedactDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        self.try_detect(input)
            .unwrap_or_else(|_| panic!("redact-live failed closed"))
    }
    fn try_detect(&self, input: &str) -> Result<Vec<Detection>, RecognizerRuntimeError> {
        self.batch(input)
            .map_err(|error| RecognizerRuntimeError::new(SOURCE, error.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reply() -> Reply {
        serde_json::from_value(serde_json::json!({
            "version":1,"id":1,"kind":"primary","complete":true,"threshold":0.6,"org":true,
            "content_tokens":1,"planned_windows":1,"completed_windows":1,
            "spans":[{"start":0,"end":1,"label":"GIVEN_NAME","score":0.8}],
            "dispositions":{"threshold":0,"arbitration":0,"cleanup":0,"special_tokens":2},"error":null
        })).unwrap()
    }
    #[test]
    fn full_batch_rejects_malformed_metadata_before_any_span_survives() {
        let mut r = reply();
        r.spans[0].score = f64::NAN;
        assert_eq!(validate(r, 1, "a").unwrap_err(), BridgeError::Nonfinite);
        let mut r = reply();
        r.spans[0].label = "UNKNOWN".into();
        assert_eq!(validate(r, 1, "a").unwrap_err(), BridgeError::UnknownLabel);
        let mut r = reply();
        r.completed_windows = 0;
        assert_eq!(
            validate(r, 1, "a").unwrap_err(),
            BridgeError::IncompleteWindow
        );
        let mut r = reply();
        r.id = 2;
        assert_eq!(validate(r, 1, "a").unwrap_err(), BridgeError::Protocol);
        let mut r = reply();
        r.spans[0].start = 1;
        r.spans[0].end = 4;
        assert_eq!(validate(r, 1, "😀").unwrap_err(), BridgeError::Alignment);
        let mut r = reply();
        r.spans.push(Span {
            start: 0,
            end: 1,
            label: "EMAIL".into(),
            score: 1.0,
        });
        assert_eq!(validate(r, 1, "a").unwrap_err(), BridgeError::InvalidSpan);
        let mut r = reply();
        r.error = Some("raw unknown foreign error".into());
        r.complete = false;
        r.spans.clear();
        assert_eq!(validate(r, 1, "a").unwrap_err(), BridgeError::Protocol);
    }
    #[test]
    fn empty_completed_batch_is_distinct_from_incomplete() {
        let mut r = reply();
        r.spans.clear();
        r.content_tokens = 0;
        r.planned_windows = 0;
        r.completed_windows = 0;
        assert!(validate(r, 1, "").unwrap().is_empty());
        let mut r = reply();
        r.complete = false;
        assert_eq!(
            validate(r, 1, "a").unwrap_err(),
            BridgeError::IncompleteWindow
        );
    }
}
