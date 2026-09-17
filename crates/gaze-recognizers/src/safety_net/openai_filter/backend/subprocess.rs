use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use gaze_types::SafetyNetError;

#[cfg(test)]
use crate::safety_net::subprocess_diagnostics::sanitize_stderr;
use crate::safety_net::subprocess_diagnostics::{read_stderr, sanitize_error};
use crate::safety_net::subprocess_io::{Cancellation, Pipe};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{normalize_raw_spans, OpenAiFilterBackend, RawSpan};
use crate::safety_net::openai_filter::class_map::map_openai_label;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_INPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_STDOUT_BYTES: usize = 4 * 1024 * 1024;
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Appended after the configured args on every call. Piped stdin makes `opf` analyse each
/// non-blank line as its own input, with offsets relative to that line, so the text must be read
/// as one file. `/dev/stdin` keeps the pipe transport: no PII is written to disk. The ANSI
/// colour section `opf` prints after the JSON by default would make every output unparseable.
/// Windows has no `/dev/stdin`; there `opf` still splits lines, and the echo check in
/// `ensure_whole_text_was_analysed` refuses any multi-line text instead of mis-mapping it.
#[cfg(unix)]
const WHOLE_TEXT_INPUT_ARGS: &[&str] =
    &["--no-print-color-coded-text", "--text-file", "/dev/stdin"];
#[cfg(not(unix))]
const WHOLE_TEXT_INPUT_ARGS: &[&str] = &["--no-print-color-coded-text"];

/// Upstream OPF source repository.
pub const OPF_SOURCE_REPO: &str = "openai/privacy-filter";

/// Pinned upstream commit of the OPF source tree. Verified 2026-05-15.
pub const OPF_SOURCE_COMMIT: &str = "f7f00ca7fb869683eb732c010299d901457f19c3";

/// SHA256 of the checkpoint bundle downloaded by `opf` at `OPF_SOURCE_COMMIT`.
///
/// Computed as SHA256 over the Kiji-style line-per-file SHA256SUMS manifest for
/// `REQUIRED_OPF_ARTIFACTS` in declaration order. Verified 2026-05-15 from a
/// clean `opf download` into `~/.opf/privacy_filter`.
pub const OPF_CHECKPOINT_BUNDLE_SHA256: Option<&str> =
    Some("4680158333621f3f344f58366f59612d52eff67ce6f46cff7becede5be1853ae");

/// Required checkpoint artifact filenames inside the OPF bundle directory.
///
/// The Hugging Face downloader also writes `.cache/huggingface` metadata; those
/// cache files are excluded from the runtime bundle hash.
pub const REQUIRED_OPF_ARTIFACTS: &[&str] = &[
    "config.json",
    "dtypes.json",
    "model.safetensors",
    "viterbi_calibration.json",
];

#[cfg(test)]
const OPF_CHECKPOINT_ARTIFACT_SHA256SUMS: &[(&str, &str)] = &[
    (
        "config.json",
        "048a20604a3622de208d30df57cd5424bb583639b9ba20ddd7da593d3f89a248",
    ),
    (
        "dtypes.json",
        "e936acb3d039b35ec55438af2fffd424a53c7685b895775c186b26c7df79fcc7",
    ),
    (
        "model.safetensors",
        "9c262cbe68a0c8a50590a648ef8341a2b7d3be1fa11dfb79893fe0b03ce57b5c",
    ),
    (
        "viterbi_calibration.json",
        "bbc8611ef08a55ed72d64856cbbbb9a91db8dfa881f0a92e2afbad6e4bbc775a",
    ),
];

/// Configuration for the local OPF subprocess backend.
#[derive(Debug, Clone)]
pub struct SubprocessOpenAiFilterConfig {
    command: PathBuf,
    args: Vec<OsString>,
    checkpoint_path: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    timeout: Duration,
    max_input_bytes: usize,
    max_stdout_bytes: usize,
    capture_stderr: bool,
    verify_checkpoint_bundle_sha256: bool,
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
            cache_dir: None,
            timeout: DEFAULT_TIMEOUT,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
            capture_stderr: false,
            verify_checkpoint_bundle_sha256: false,
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

    pub fn with_cache_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(path.into());
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

    pub fn with_checkpoint_bundle_sha256_verification(mut self, enabled: bool) -> Self {
        self.verify_checkpoint_bundle_sha256 = enabled;
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

    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
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
        verify_command_path(&config.command)?;
        verify_sensitive_paths(&config)?;
        verify_checkpoint_bundle_integrity(&config)?;

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

        Cancellation::check_platform().map_err(|error| SafetyNetError::ModelUnavailable {
            reason: error.to_string(),
        })?;
        let cancellation = Cancellation::default();
        let mut command = Command::new(&self.config.command);
        command.args(&self.config.args);
        if let Some(checkpoint_path) = &self.config.checkpoint_path {
            command.arg("--checkpoint").arg(checkpoint_path);
        }
        command.args(WHOLE_TEXT_INPUT_ARGS);
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

        // Configure every pipe before starting workers, so setup errors cannot
        // leave a running worker behind. Each worker owns and closes its pipe.
        let pipes = (|| -> std::io::Result<_> {
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| std::io::Error::other("missing stdin"))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| std::io::Error::other("missing stdout"))?;
            Ok((
                Pipe::new(stdin, &cancellation)?,
                Pipe::new(stdout, &cancellation)?,
                child
                    .stderr
                    .take()
                    .map(|pipe| Pipe::new(pipe, &cancellation))
                    .transpose()?,
            ))
        })();
        let (mut stdin, stdout, stderr) = match pipes {
            Ok(pipes) => pipes,
            Err(error) => {
                kill_reap(&mut child);
                return Err(SafetyNetError::Runtime {
                    message: format!(
                        "opf pipe setup failed: {}",
                        sanitize_error(&error.to_string())
                    ),
                });
            }
        };
        let input = clean.as_bytes().to_vec();
        let stdin_thread = thread::spawn(move || {
            stdin.write_all(&input)?;
            stdin.flush()
        });
        let max_stdout_bytes = self.config.max_stdout_bytes;
        let stdout_thread = thread::spawn(move || read_bounded(stdout, max_stdout_bytes));
        let stderr_thread = stderr.map(|stderr| thread::spawn(move || read_stderr(stderr)));

        let deadline = Instant::now() + self.config.timeout;
        let mut stdin_thread = Some(stdin_thread);
        let mut stdout_thread = Some(stdout_thread);
        let mut stderr_thread = stderr_thread;
        let mut status = None;
        let mut stdout = None;
        let mut stderr = if stderr_thread.is_some() {
            None
        } else {
            Some(String::new())
        };

        loop {
            if stdin_thread
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
            {
                let thread = stdin_thread.take().expect("checked stdin thread");
                if let Err(error) = join_stdin(thread) {
                    cancellation.cancel();
                    kill_reap(&mut child);
                    join_remaining(stdin_thread, stdout_thread, stderr_thread);
                    return Err(error);
                }
            }

            if stdout_thread
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
            {
                let thread = stdout_thread.take().expect("checked stdout thread");
                match join_reader(thread, "stdout") {
                    Ok(output) => stdout = Some(output),
                    Err(error) => {
                        cancellation.cancel();
                        kill_reap(&mut child);
                        join_remaining(stdin_thread, stdout_thread, stderr_thread);
                        return Err(error);
                    }
                }
            }

            if stderr_thread
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
            {
                let thread = stderr_thread.take().expect("checked stderr thread");
                match join_reader(thread, "stderr") {
                    Ok(output) => stderr = Some(output),
                    Err(error) => {
                        cancellation.cancel();
                        kill_reap(&mut child);
                        join_remaining(stdin_thread, stdout_thread, stderr_thread);
                        return Err(error);
                    }
                }
            }

            if status.is_none() {
                match child.try_wait() {
                    Ok(Some(child_status)) => status = Some(child_status),
                    Ok(None) => {}
                    Err(error) => {
                        cancellation.cancel();
                        kill_reap(&mut child);
                        join_remaining(stdin_thread, stdout_thread, stderr_thread);
                        return Err(SafetyNetError::Runtime {
                            message: format!(
                                "failed waiting for opf subprocess: {}",
                                sanitize_error(&error.to_string())
                            ),
                        });
                    }
                }
            }

            if status.is_some() && stdin_thread.is_none() && stdout.is_some() && stderr.is_some() {
                break;
            }

            if Instant::now() >= deadline {
                cancellation.cancel();
                kill_reap(&mut child);
                join_remaining(stdin_thread, stdout_thread, stderr_thread);
                return Err(SafetyNetError::Runtime {
                    message: "opf subprocess timed out and was killed".to_string(),
                });
            }

            thread::sleep(WAIT_POLL_INTERVAL);
        }

        let status = status.expect("status checked before loop exit");
        let stdout = stdout.expect("stdout checked before loop exit");
        let stderr = stderr.expect("stderr checked before loop exit");

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
        // `opf` skips an empty input file and prints nothing; empty text holds no PII.
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        let stdout = self.run(clean)?;
        let output = parse_opf_output(&stdout)?;
        let opf_view = OpfTextView::new(clean);
        ensure_whole_text_was_analysed(&output, &opf_view)?;
        let spans = character_spans_to_byte_spans(output.into_raw_spans()?, &opf_view)?;
        normalize_raw_spans(spans, clean)
    }
}

/// The text as `opf` sees it after reading it as a file, and where each of its characters starts
/// in `clean`. Python's text mode turns `\r\n` and a lone `\r` into `\n`, so OPF offsets count
/// characters of `text`, not of `clean`.
struct OpfTextView {
    text: String,
    /// UTF-8 byte offset in `clean` of each character of `text`, then `clean.len()`.
    byte_offsets: Vec<usize>,
}

impl OpfTextView {
    fn new(clean: &str) -> Self {
        let mut text = String::with_capacity(clean.len());
        let mut byte_offsets = Vec::with_capacity(clean.len() + 1);
        let mut characters = clean.char_indices().peekable();
        while let Some((byte, character)) = characters.next() {
            byte_offsets.push(byte);
            if character == '\r' {
                characters.next_if(|&(_, next)| next == '\n');
                text.push('\n');
            } else {
                text.push(character);
            }
        }
        byte_offsets.push(clean.len());
        Self { text, byte_offsets }
    }
}

/// `opf` echoes the text it analysed. Anything other than the whole text Gaze sent means the
/// offsets are relative to some other text: piped stdin splits lines and skips blank ones, so
/// `"\n\nJohn Smith"` comes back as one valid-looking output whose spans land two characters
/// early. Refuse instead of protecting the wrong bytes.
fn ensure_whole_text_was_analysed(
    output: &OpfRedactionOutput,
    opf_view: &OpfTextView,
) -> Result<(), SafetyNetError> {
    if output.text.0 == opf_view.text {
        Ok(())
    } else {
        Err(SafetyNetError::InvalidOutput {
            message: "opf analysed a different text than the one sent".to_string(),
        })
    }
}

/// OPF reports `start`/`end` as Python `str` indices (Unicode scalar values) into the text it
/// read, while `RawSpan` and everything after it use UTF-8 byte offsets into `clean`. Convert
/// here, once. Read as bytes instead, every span after a multibyte character shifts left: it
/// either fails the char-boundary check or silently covers the wrong text. An end offset after a
/// translated `\r\n` maps past both bytes. An offset past the last character fails closed.
fn character_spans_to_byte_spans(
    spans: Vec<RawSpan>,
    opf_view: &OpfTextView,
) -> Result<Vec<RawSpan>, SafetyNetError> {
    let to_byte = |character: usize| {
        opf_view
            .byte_offsets
            .get(character)
            .copied()
            .ok_or_else(|| SafetyNetError::InvalidOutput {
                message: "opf returned out-of-bounds span".to_string(),
            })
    };

    spans
        .into_iter()
        .map(|span| {
            Ok(RawSpan {
                start: to_byte(span.start)?,
                end: to_byte(span.end)?,
                ..span
            })
        })
        .collect()
}

/// The official `opf --format json` object. A bare span array is not accepted: without the
/// echoed `text` there is no proof the offsets belong to the text Gaze sent.
#[derive(Debug, Deserialize)]
struct OpfRedactionOutput {
    detected_spans: Vec<PrivateOpfSpan>,
    text: PrivatePiiString,
}

impl OpfRedactionOutput {
    fn into_raw_spans(self) -> Result<Vec<RawSpan>, SafetyNetError> {
        self.detected_spans
            .into_iter()
            .map(PrivateOpfSpan::into_raw_span)
            .collect()
    }
}

#[derive(Deserialize)]
struct PrivateOpfSpan {
    label: String,
    start: usize,
    end: usize,
    #[serde(default)]
    score: Option<f32>,
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
        map_openai_label(&self.label)?;
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

/// Exactly one JSON document: `serde_json::from_str` rejects trailing content, so the one output
/// per line that piped stdin produces never parses as a single result.
fn parse_opf_output(stdout: &[u8]) -> Result<OpfRedactionOutput, SafetyNetError> {
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

fn join_reader<T>(
    thread: thread::JoinHandle<std::io::Result<T>>,
    stream: &'static str,
) -> Result<T, SafetyNetError> {
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

fn kill_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn join_remaining(
    stdin_thread: Option<thread::JoinHandle<std::io::Result<()>>>,
    stdout_thread: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    stderr_thread: Option<thread::JoinHandle<std::io::Result<String>>>,
) {
    if let Some(thread) = stdin_thread {
        let _ = join_stdin(thread);
    }
    if let Some(thread) = stdout_thread {
        let _ = join_reader(thread, "stdout");
    }
    if let Some(thread) = stderr_thread {
        let _ = join_reader(thread, "stderr");
    }
}

fn verify_command_path(command: &Path) -> Result<(), SafetyNetError> {
    if command.components().count() <= 1 {
        return Ok(());
    }

    let metadata =
        std::fs::symlink_metadata(command).map_err(|_| SafetyNetError::ModelUnavailable {
            reason: "opf command path is not accessible".to_string(),
        })?;

    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "opf command path is not a regular file".to_string(),
        });
    }

    Ok(())
}

fn verify_sensitive_paths(config: &SubprocessOpenAiFilterConfig) -> Result<(), SafetyNetError> {
    if let Some(cache_dir) = &config.cache_dir {
        ensure_secure_dir(cache_dir)?;
    }

    let Some(checkpoint_path) = &config.checkpoint_path else {
        return Ok(());
    };

    if !checkpoint_path.exists() {
        return Err(SafetyNetError::WeightsMissing {
            path: sanitize_path(checkpoint_path),
        });
    }

    verify_sensitive_tree(checkpoint_path)
}

fn verify_checkpoint_bundle_integrity(
    config: &SubprocessOpenAiFilterConfig,
) -> Result<(), SafetyNetError> {
    if !config.verify_checkpoint_bundle_sha256 {
        return Ok(());
    }

    let Some(expected_sha256) = OPF_CHECKPOINT_BUNDLE_SHA256 else {
        return Ok(());
    };

    if REQUIRED_OPF_ARTIFACTS.is_empty() {
        return Err(SafetyNetError::ModelIntegrityMismatch {
            expected: "non-empty REQUIRED_OPF_ARTIFACTS".to_string(),
            actual: "<empty>".to_string(),
        });
    }

    let checkpoint_dir = config
        .checkpoint_path
        .clone()
        .or_else(|| std::env::var_os("OPF_CHECKPOINT").map(PathBuf::from))
        .or_else(default_checkpoint_path)
        .ok_or_else(|| SafetyNetError::WeightsMissing {
            path: "<missing:privacy_filter>".to_string(),
        })?;

    if !checkpoint_dir.exists() {
        return Err(SafetyNetError::WeightsMissing {
            path: sanitize_path(&checkpoint_dir),
        });
    }
    verify_sensitive_tree(&checkpoint_dir)?;

    let mut manifest = String::new();
    for required in REQUIRED_OPF_ARTIFACTS {
        let artifact = checkpoint_dir.join(required);
        if !artifact.exists() {
            return Err(SafetyNetError::WeightsMissing {
                path: sanitize_path(&artifact),
            });
        }
        if required.contains('/') || required.contains('\\') {
            return Err(SafetyNetError::ModelIntegrityMismatch {
                expected: "flat OPF artifact names".to_string(),
                actual: "<nested>".to_string(),
            });
        }
        let bytes = std::fs::read(&artifact).map_err(|_| SafetyNetError::WeightsMissing {
            path: sanitize_path(&artifact),
        })?;
        push_sha256sum_manifest_line(&mut manifest, required, &hex_sha256(&bytes));
    }

    let actual_sha256 = hex_sha256(manifest.as_bytes());
    if actual_sha256 != expected_sha256 {
        return Err(SafetyNetError::ModelIntegrityMismatch {
            expected: expected_sha256.to_string(),
            actual: actual_sha256,
        });
    }

    Ok(())
}

fn default_checkpoint_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".opf").join("privacy_filter"))
}

fn hex_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn push_sha256sum_manifest_line(manifest: &mut String, artifact: &str, sha256: &str) {
    manifest.push_str(sha256);
    manifest.push_str("  ");
    manifest.push_str(artifact);
    manifest.push('\n');
}

fn ensure_secure_dir(path: &Path) -> Result<(), SafetyNetError> {
    if !path.exists() {
        std::fs::create_dir_all(path).map_err(|_| SafetyNetError::ModelUnavailable {
            reason: "failed to create opf cache directory".to_string(),
        })?;
        set_private_dir_permissions(path)?;
    }

    verify_sensitive_tree(path)
}

fn verify_sensitive_tree(path: &Path) -> Result<(), SafetyNetError> {
    let metadata = verify_one_sensitive_path(path)?;
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|_| SafetyNetError::ModelUnavailable {
            reason: "failed to read opf sensitive directory".to_string(),
        })? {
            let entry = entry.map_err(|_| SafetyNetError::ModelUnavailable {
                reason: "failed to read opf sensitive directory entry".to_string(),
            })?;
            verify_sensitive_tree(&entry.path())?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn verify_one_sensitive_path(path: &Path) -> Result<std::fs::Metadata, SafetyNetError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| SafetyNetError::ModelUnavailable {
            reason: "failed to inspect opf sensitive path".to_string(),
        })?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "opf sensitive path must not be a symlink".to_string(),
        });
    }

    let uid = current_euid();
    if metadata.uid() != uid {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "opf sensitive path owner mismatch".to_string(),
        });
    }

    let mode = metadata.permissions().mode() & 0o777;
    if file_type.is_dir() {
        if mode != 0o700 {
            return Err(SafetyNetError::ModelUnavailable {
                reason: "opf sensitive directory must be mode 0700".to_string(),
            });
        }
    } else if file_type.is_file() {
        if mode & 0o022 != 0 {
            return Err(SafetyNetError::ModelUnavailable {
                reason: "opf sensitive file must not be group/world writable".to_string(),
            });
        }
    } else {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "opf sensitive path must be a regular file or directory".to_string(),
        });
    }

    Ok(metadata)
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), SafetyNetError> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = std::fs::Permissions::from_mode(0o700);
    std::fs::set_permissions(path, permissions).map_err(|_| SafetyNetError::ModelUnavailable {
        reason: "failed to set opf cache directory permissions".to_string(),
    })
}

#[cfg(unix)]
fn current_euid() -> u32 {
    // Verification must depend on who runs the process, not on the current
    // directory's owner.
    unsafe { libc::geteuid() }
}

#[cfg(windows)]
fn verify_one_sensitive_path(path: &Path) -> Result<std::fs::Metadata, SafetyNetError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| SafetyNetError::ModelUnavailable {
            reason: "failed to inspect opf sensitive path".to_string(),
        })?;
    if metadata.file_type().is_symlink() {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "opf sensitive path must not be a symlink".to_string(),
        });
    }
    if !(metadata.file_type().is_file() || metadata.file_type().is_dir()) {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "opf sensitive path must be a regular file or directory".to_string(),
        });
    }
    if metadata.permissions().readonly() {
        return Ok(metadata);
    }
    Err(SafetyNetError::ModelUnavailable {
        reason: "opf sensitive Windows ACL could not be verified".to_string(),
    })
}

#[cfg(windows)]
fn set_private_dir_permissions(_path: &Path) -> Result<(), SafetyNetError> {
    Err(SafetyNetError::ModelUnavailable {
        reason: "opf sensitive Windows ACL could not be configured".to_string(),
    })
}

fn sanitize_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("<missing:{name}>"))
        .unwrap_or_else(|| "<missing:checkpoint>".to_string())
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

    #[test]
    fn opf_checkpoint_bundle_hash_matches_documented_artifacts() {
        let required = REQUIRED_OPF_ARTIFACTS.to_vec();
        let documented = OPF_CHECKPOINT_ARTIFACT_SHA256SUMS
            .iter()
            .map(|(artifact, _)| *artifact)
            .collect::<Vec<_>>();
        assert_eq!(documented, required);

        let mut manifest = String::new();
        for (artifact, sha256) in OPF_CHECKPOINT_ARTIFACT_SHA256SUMS {
            assert_eq!(sha256.len(), 64, "{artifact}");
            push_sha256sum_manifest_line(&mut manifest, artifact, sha256);
        }

        assert_eq!(
            Some(hex_sha256(manifest.as_bytes()).as_str()),
            OPF_CHECKPOINT_BUNDLE_SHA256
        );
    }

    #[test]
    fn missing_checkpoint_fails_before_spawn() {
        let missing = tempfile::tempdir().unwrap().path().join("checkpoint");
        let config = SubprocessOpenAiFilterConfig::new("opf").with_checkpoint_path(missing);
        let error = SubprocessOpenAiFilterBackend::new(config).unwrap_err();
        assert!(matches!(error, SafetyNetError::WeightsMissing { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn cache_dir_is_created_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("opf-cache");
        let config = SubprocessOpenAiFilterConfig::new("opf").with_cache_dir(&cache_dir);
        SubprocessOpenAiFilterBackend::new(config).unwrap();

        let mode = std::fs::metadata(cache_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn owner_check_uses_process_euid_not_cwd_owner() {
        let expected = unsafe { libc::geteuid() };
        if expected == 0 {
            eprintln!("skipping different-owner CWD check because the test runner is root");
            return;
        }

        let module = module_path!()
            .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
            .unwrap_or(module_path!());
        let helper = format!("{module}::owner_check_different_owner_cwd_helper");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &helper, "--nocapture"])
            .env("GAZE_EUID_OWNER_CHECK_HELPER", "1")
            .current_dir("/")
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "different-owner CWD helper failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(unix)]
    #[test]
    fn owner_check_different_owner_cwd_helper() {
        use std::os::unix::fs::MetadataExt;

        if std::env::var_os("GAZE_EUID_OWNER_CHECK_HELPER").is_none() {
            return;
        }

        let expected = unsafe { libc::geteuid() };
        let cwd_owner = std::fs::metadata(".").unwrap().uid();
        assert_ne!(
            cwd_owner, expected,
            "helper CWD owner must differ from euid"
        );
        assert_eq!(current_euid(), expected);
    }

    #[cfg(unix)]
    #[test]
    fn group_writable_checkpoint_file_fails_closed() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let checkpoint = dir.path().join("model.bin");
        std::fs::write(&checkpoint, b"weights").unwrap();
        std::fs::set_permissions(&checkpoint, std::fs::Permissions::from_mode(0o660)).unwrap();

        let config = SubprocessOpenAiFilterConfig::new("opf").with_checkpoint_path(checkpoint);
        let error = SubprocessOpenAiFilterBackend::new(config).unwrap_err();
        assert!(matches!(error, SafetyNetError::ModelUnavailable { .. }));
    }
}
#[test]
fn production_default_timeout_remains_five_seconds() {
    assert_eq!(
        SubprocessOpenAiFilterConfig::new("opf").timeout,
        Duration::from_secs(5)
    );
}
