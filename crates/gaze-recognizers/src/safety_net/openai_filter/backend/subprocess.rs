use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use gaze_types::SafetyNetError;
use serde::Deserialize;

use super::{normalize_raw_spans, OpenAiFilterBackend, RawSpan};
use crate::safety_net::openai_filter::class_map::validate_openai_label;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_INPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_STDOUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_VERBOSE_STDERR_BYTES: usize = 256;
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Configuration for the local OPF subprocess backend.
#[derive(Debug, Clone)]
pub struct SubprocessOpenAiFilterConfig {
    command: PathBuf,
    args: Vec<OsString>,
    checkpoint_path: Option<PathBuf>,
    timeout: Duration,
    max_input_bytes: usize,
    max_stdout_bytes: usize,
    capture_stderr: bool,
    version: String,
    decoding_params: Vec<(&'static str, String)>,
}

impl SubprocessOpenAiFilterConfig {
    /// Creates a config for an explicit `opf` command path.
    pub fn new(command: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            args: vec![
                OsString::from("--format"),
                OsString::from("json"),
                OsString::from("--output-mode"),
                OsString::from("typed"),
            ],
            checkpoint_path: None,
            timeout: DEFAULT_TIMEOUT,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
            capture_stderr: false,
            version: "openai/privacy-filter:external".to_string(),
            decoding_params: vec![
                ("format", "json".to_string()),
                ("output_mode", "typed".to_string()),
            ],
        }
    }

    /// Creates a config from `GAZE_OPENAI_FILTER_OPF`.
    pub fn from_env() -> Result<Self, SafetyNetError> {
        let command = std::env::var_os("GAZE_OPENAI_FILTER_OPF").ok_or_else(|| {
            SafetyNetError::Unavailable {
                reason: "GAZE_OPENAI_FILTER_OPF is not set".to_string(),
            }
        })?;

        Ok(Self::new(command))
    }

    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_checkpoint_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.checkpoint_path = Some(path.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_max_input_bytes(mut self, max_input_bytes: usize) -> Self {
        self.max_input_bytes = max_input_bytes;
        self
    }

    pub fn with_max_stdout_bytes(mut self, max_stdout_bytes: usize) -> Self {
        self.max_stdout_bytes = max_stdout_bytes;
        self
    }

    pub fn with_stderr_diagnostics(mut self, enabled: bool) -> Self {
        self.capture_stderr = enabled;
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    pub fn with_decoding_param(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.decoding_params.push((key, value.into()));
        self
    }

    pub fn command(&self) -> &Path {
        &self.command
    }

    pub fn checkpoint_path(&self) -> Option<&Path> {
        self.checkpoint_path.as_deref()
    }
}

/// `opf --format json` subprocess backend.
#[derive(Debug, Clone)]
pub struct SubprocessOpenAiFilterBackend {
    config: SubprocessOpenAiFilterConfig,
}

impl SubprocessOpenAiFilterBackend {
    pub fn new(config: SubprocessOpenAiFilterConfig) -> Result<Self, SafetyNetError> {
        if config.command.as_os_str().is_empty() {
            return Err(SafetyNetError::Unavailable {
                reason: "opf command path is empty".to_string(),
            });
        }

        Ok(Self { config })
    }

    pub fn config(&self) -> &SubprocessOpenAiFilterConfig {
        &self.config
    }

    fn run(&self, clean: &str) -> Result<Vec<u8>, SafetyNetError> {
        let actual = clean.len();
        if actual > self.config.max_input_bytes {
            return Err(SafetyNetError::InputTooLarge {
                limit: self.config.max_input_bytes,
                actual,
            });
        }

        let mut command = Command::new(&self.config.command);
        command.args(&self.config.args);
        if let Some(checkpoint_path) = &self.config.checkpoint_path {
            command.arg("--checkpoint").arg(checkpoint_path);
        }
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(
            if self.config.capture_stderr {
                Stdio::piped()
            } else {
                Stdio::null()
            },
        );

        let mut child = command
            .spawn()
            .map_err(|error| SafetyNetError::ModelUnavailable {
                reason: format!(
                    "failed to spawn opf subprocess: {}",
                    sanitize_error(&error.to_string())
                ),
            })?;

        let mut stdin = child.stdin.take().ok_or_else(|| SafetyNetError::Runtime {
            message: "failed to open opf stdin".to_string(),
        })?;
        let input = clean.as_bytes().to_vec();
        let stdin_thread = thread::spawn(move || {
            stdin.write_all(&input)?;
            stdin.flush()
        });

        let stdout = child.stdout.take().ok_or_else(|| SafetyNetError::Runtime {
            message: "failed to open opf stdout".to_string(),
        })?;
        let max_stdout_bytes = self.config.max_stdout_bytes;
        let stdout_thread = thread::spawn(move || read_bounded(stdout, max_stdout_bytes));

        let stderr_thread = child
            .stderr
            .take()
            .map(|stderr| thread::spawn(move || read_bounded(stderr, MAX_VERBOSE_STDERR_BYTES)));

        let deadline = Instant::now() + self.config.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SafetyNetError::Runtime {
                        message: "opf subprocess timed out and was killed".to_string(),
                    });
                }
                Ok(None) => thread::sleep(WAIT_POLL_INTERVAL),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SafetyNetError::Runtime {
                        message: format!(
                            "failed waiting for opf subprocess: {}",
                            sanitize_error(&error.to_string())
                        ),
                    });
                }
            }
        };

        join_stdin(stdin_thread)?;
        let stdout = join_reader(stdout_thread, "stdout")?;
        let stderr = match stderr_thread {
            Some(thread) => sanitize_stderr(&join_reader(thread, "stderr")?),
            None => String::new(),
        };

        if !status.success() {
            let detail = if stderr.is_empty() {
                format!("opf subprocess exited with status {status}")
            } else {
                format!("opf subprocess exited with status {status}: {stderr}")
            };
            return Err(SafetyNetError::Runtime { message: detail });
        }

        Ok(stdout)
    }
}

impl OpenAiFilterBackend for SubprocessOpenAiFilterBackend {
    fn id(&self) -> &str {
        "openai-privacy-filter-subprocess"
    }

    fn version(&self) -> &str {
        &self.config.version
    }

    fn decoding_params(&self) -> &[(&str, String)] {
        &self.config.decoding_params
    }

    fn infer(&self, clean: &str) -> Result<Vec<RawSpan>, SafetyNetError> {
        let stdout = self.run(clean)?;
        let output = parse_opf_output(&stdout)?;
        let spans = output.into_raw_spans()?;
        normalize_raw_spans(spans, clean)
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OpfOutput {
    Redaction(OpfRedactionOutput),
    Spans(Vec<PrivateOpfSpan>),
}

impl OpfOutput {
    fn into_raw_spans(self) -> Result<Vec<RawSpan>, SafetyNetError> {
        match self {
            Self::Redaction(output) => output
                .detected_spans
                .into_iter()
                .map(PrivateOpfSpan::into_raw_span)
                .collect(),
            Self::Spans(spans) => spans
                .into_iter()
                .map(PrivateOpfSpan::into_raw_span)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpfRedactionOutput {
    detected_spans: Vec<PrivateOpfSpan>,
    #[serde(default)]
    _schema_version: Option<u64>,
    #[serde(default)]
    _summary: Option<serde_json::Value>,
    #[serde(default)]
    _warning: Option<String>,
    #[serde(default)]
    _text: Option<PrivatePiiString>,
    #[serde(default)]
    _redacted_text: Option<PrivatePiiString>,
}

#[derive(Deserialize)]
struct PrivateOpfSpan {
    label: String,
    start: usize,
    end: usize,
    #[serde(default)]
    score: Option<f32>,
    #[serde(default)]
    _text: Option<PrivatePiiString>,
    #[serde(default)]
    _placeholder: Option<PrivatePiiString>,
}

impl std::fmt::Debug for PrivateOpfSpan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateOpfSpan")
            .field("label", &self.label)
            .field("start", &self.start)
            .field("end", &self.end)
            .field("score", &self.score)
            .finish_non_exhaustive()
    }
}

impl PrivateOpfSpan {
    fn into_raw_span(self) -> Result<RawSpan, SafetyNetError> {
        validate_openai_label(&self.label)?;
        if let Some(score) = self.score {
            if !score.is_finite() {
                return Err(SafetyNetError::InvalidOutput {
                    message: "opf returned non-finite score".to_string(),
                });
            }
        }

        Ok(RawSpan::new(self.start, self.end, self.label, self.score))
    }
}

#[derive(Deserialize)]
struct PrivatePiiString(String);

impl std::fmt::Debug for PrivatePiiString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<private-opf-field>")
    }
}

impl Drop for PrivatePiiString {
    fn drop(&mut self) {
        self.0.clear();
    }
}

fn parse_opf_output(stdout: &[u8]) -> Result<OpfOutput, SafetyNetError> {
    let text = std::str::from_utf8(stdout).map_err(|_| SafetyNetError::InvalidOutput {
        message: "opf stdout was not valid UTF-8".to_string(),
    })?;

    serde_json::from_str(text).map_err(|_| SafetyNetError::InvalidOutput {
        message: "opf stdout was not valid JSON".to_string(),
    })
}

fn read_bounded(mut reader: impl Read, max_bytes: usize) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(out);
        }
        if out.len().saturating_add(read) > max_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stream exceeded configured byte cap",
            ));
        }
        out.extend_from_slice(&chunk[..read]);
    }
}

fn join_reader(
    thread: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stream: &'static str,
) -> Result<Vec<u8>, SafetyNetError> {
    thread
        .join()
        .map_err(|_| SafetyNetError::Runtime {
            message: format!("opf {stream} reader panicked"),
        })?
        .map_err(|error| SafetyNetError::Runtime {
            message: format!(
                "opf {stream} capture failed: {}",
                sanitize_error(&error.to_string())
            ),
        })
}

fn join_stdin(thread: thread::JoinHandle<std::io::Result<()>>) -> Result<(), SafetyNetError> {
    thread
        .join()
        .map_err(|_| SafetyNetError::Runtime {
            message: "opf stdin writer panicked".to_string(),
        })?
        .map_err(|error| SafetyNetError::Runtime {
            message: format!(
                "opf stdin write failed: {}",
                sanitize_error(&error.to_string())
            ),
        })
}

fn sanitize_stderr(bytes: &[u8]) -> String {
    let ascii = bytes
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                ' '
            }
        })
        .collect::<String>();

    sanitize_error(&ascii)
        .chars()
        .take(MAX_VERBOSE_STDERR_BYTES)
        .collect()
}

fn sanitize_error(message: &str) -> String {
    message
        .split_ascii_whitespace()
        .map(sanitize_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_token(token: &str) -> String {
    if token.contains('@') {
        return "<redacted>".to_string();
    }

    let digit_count = token.bytes().filter(u8::is_ascii_digit).count();
    if digit_count >= 7 {
        return "<redacted>".to_string();
    }

    token.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_fields_do_not_debug() {
        let span: PrivateOpfSpan = serde_json::from_str(
            r#"{"label":"private_email","start":0,"end":5,"text":"alice@example.invalid","placeholder":"<EMAIL>"}"#,
        )
        .unwrap();

        let debug = format!("{span:?}");
        assert!(!debug.contains("alice@example.invalid"));
        assert!(!debug.contains("<EMAIL>"));
        assert!(debug.contains("private_email"));
    }

    #[test]
    fn parses_redaction_output_to_raw_spans() {
        let output = parse_opf_output(
            br#"{"schema_version":1,"text":"alice@example.invalid","detected_spans":[{"label":"private_email","start":0,"end":21,"text":"alice@example.invalid","placeholder":""}],"redacted_text":""}"#,
        )
        .unwrap();

        let spans = output.into_raw_spans().unwrap();
        assert_eq!(spans, vec![RawSpan::new(0, 21, "private_email", None)]);
    }

    #[test]
    fn sanitizes_stderr_pii() {
        let stderr = sanitize_stderr(b"failed for alice@example.invalid at +1-555-0101");
        assert!(!stderr.contains("alice@example.invalid"));
        assert!(!stderr.contains("+1-555-0101"));
        assert!(stderr.contains("<redacted>"));
    }
}
