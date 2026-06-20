use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use gaze_types::SafetyNetError;
use serde::Deserialize;

use super::artifacts::{verify_model_dir, KIJI_DISTILBERT_BUNDLE_SHA256};
use super::{normalize_raw_spans, KijiDistilbertBackend, RawSpan};
use crate::safety_net::kiji_distilbert::class_map::map_kiji_label;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_INPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_STDOUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_VERBOSE_STDERR_BYTES: usize = 256;
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Configuration for the local Kiji DistilBERT subprocess backend.
#[derive(Debug, Clone)]
pub struct SubprocessKijiConfig {
    command: PathBuf,
    args: Vec<OsString>,
    model_dir: Option<PathBuf>,
    timeout: Duration,
    max_input_bytes: usize,
    max_stdout_bytes: usize,
    capture_stderr: bool,
    version: String,
    decoding_params: Vec<(&'static str, String)>,
    #[cfg(any(test, feature = "test-support"))]
    expected_bundle_sha256: String,
}

impl SubprocessKijiConfig {
    /// Creates a config for an explicit `kiji` command path.
    pub fn new(command: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            args: vec![
                OsString::from("--format"),
                OsString::from("json"),
                OsString::from("--output-mode"),
                OsString::from("typed"),
            ],
            model_dir: None,
            timeout: DEFAULT_TIMEOUT,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
            capture_stderr: false,
            version: "kiji/distilbert:external".to_string(),
            decoding_params: vec![
                ("format", "json".to_string()),
                ("output_mode", "typed".to_string()),
            ],
            #[cfg(any(test, feature = "test-support"))]
            expected_bundle_sha256: KIJI_DISTILBERT_BUNDLE_SHA256.to_string(),
        }
    }

    /// Creates a config from `GAZE_KIJI_DISTILBERT_COMMAND`.
    pub fn from_env() -> Result<Self, SafetyNetError> {
        let command = std::env::var_os("GAZE_KIJI_DISTILBERT_COMMAND").ok_or_else(|| {
            SafetyNetError::Unavailable {
                reason: "GAZE_KIJI_DISTILBERT_COMMAND is not set".to_string(),
            }
        })?;
        Ok(Self::new(command))
    }

    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_model_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.model_dir = Some(path.into());
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

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn with_expected_bundle_sha256_for_tests(mut self, sha256: impl Into<String>) -> Self {
        self.expected_bundle_sha256 = sha256.into();
        self
    }

    pub fn command(&self) -> &Path {
        &self.command
    }

    pub fn model_dir(&self) -> Option<&Path> {
        self.model_dir.as_deref()
    }

    fn expected_bundle_sha256(&self) -> &str {
        #[cfg(any(test, feature = "test-support"))]
        {
            &self.expected_bundle_sha256
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            KIJI_DISTILBERT_BUNDLE_SHA256
        }
    }
}

/// Kiji DistilBERT subprocess backend.
#[derive(Debug, Clone)]
pub struct SubprocessKijiBackend {
    config: SubprocessKijiConfig,
}

impl SubprocessKijiBackend {
    pub fn new(config: SubprocessKijiConfig) -> Result<Self, SafetyNetError> {
        if config.command.as_os_str().is_empty() {
            return Err(SafetyNetError::Unavailable {
                reason: "kiji command path is empty".to_string(),
            });
        }
        verify_command_path(&config.command)?;
        verify_model_dir(config.model_dir.as_deref(), config.expected_bundle_sha256())?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &SubprocessKijiConfig {
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
        if let Some(model_dir) = &self.config.model_dir {
            command.arg("--model-dir").arg(model_dir);
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
                    "failed to spawn kiji subprocess: {}",
                    sanitize_error(&error.to_string())
                ),
            })?;

        let mut stdin = child.stdin.take().ok_or_else(|| SafetyNetError::Runtime {
            message: "failed to open kiji stdin".to_string(),
        })?;
        let input = clean.as_bytes().to_vec();
        let stdin_thread = thread::spawn(move || {
            stdin.write_all(&input)?;
            stdin.flush()
        });

        let stdout = child.stdout.take().ok_or_else(|| SafetyNetError::Runtime {
            message: "failed to open kiji stdout".to_string(),
        })?;
        let max_stdout_bytes = self.config.max_stdout_bytes;
        let stdout_thread = thread::spawn(move || read_bounded(stdout, max_stdout_bytes));

        let stderr_thread = child
            .stderr
            .take()
            .map(|stderr| thread::spawn(move || read_bounded(stderr, MAX_VERBOSE_STDERR_BYTES)));

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
                    Ok(output) => stderr = Some(sanitize_stderr(&output)),
                    Err(error) => {
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
                        kill_reap(&mut child);
                        join_remaining(stdin_thread, stdout_thread, stderr_thread);
                        return Err(SafetyNetError::Runtime {
                            message: format!(
                                "failed waiting for kiji subprocess: {}",
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
                kill_reap(&mut child);
                join_remaining(stdin_thread, stdout_thread, stderr_thread);
                return Err(SafetyNetError::Runtime {
                    message: "kiji subprocess timed out and was killed".to_string(),
                });
            }

            thread::sleep(WAIT_POLL_INTERVAL);
        }

        let status = status.expect("status checked before loop exit");
        let stdout = stdout.expect("stdout checked before loop exit");
        let stderr = stderr.expect("stderr checked before loop exit");

        if !status.success() {
            let detail = if stderr.is_empty() {
                format!("kiji subprocess exited with status {status}")
            } else {
                format!("kiji subprocess exited with status {status}: {stderr}")
            };
            return Err(SafetyNetError::Runtime { message: detail });
        }

        Ok(stdout)
    }
}

impl KijiDistilbertBackend for SubprocessKijiBackend {
    fn id(&self) -> &str {
        "kiji-distilbert-subprocess"
    }

    fn version(&self) -> &str {
        &self.config.version
    }

    fn decoding_params(&self) -> &[(&str, String)] {
        &self.config.decoding_params
    }

    fn infer(&self, clean: &str) -> Result<Vec<RawSpan>, SafetyNetError> {
        let stdout = self.run(clean)?;
        let spans = parse_kiji_output(&stdout)?
            .into_iter()
            .map(PrivateKijiSpan::into_raw_span)
            .collect::<Result<Vec<_>, _>>()?;
        normalize_raw_spans(spans, clean)
    }
}

#[derive(Deserialize)]
struct PrivateKijiSpan {
    label: String,
    start: usize,
    end: usize,
    #[serde(default)]
    score: Option<f32>,
    /// Drop-on-deser: PII-bearing literal source text from Kiji output. Treated
    /// as a private deserialization detail; never crosses the adapter boundary.
    #[serde(default)]
    _text: Option<PrivatePiiString>,
}

impl std::fmt::Debug for PrivateKijiSpan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateKijiSpan")
            .field("label", &self.label)
            .field("start", &self.start)
            .field("end", &self.end)
            .field("score", &self.score)
            .finish_non_exhaustive()
    }
}

impl PrivateKijiSpan {
    fn into_raw_span(self) -> Result<RawSpan, SafetyNetError> {
        map_kiji_label(&self.label)?;
        if let Some(score) = self.score {
            if !score.is_finite() {
                return Err(SafetyNetError::InvalidOutput {
                    message: "kiji returned non-finite score".to_string(),
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
        formatter.write_str("<private-kiji-field>")
    }
}

impl Drop for PrivatePiiString {
    fn drop(&mut self) {
        self.0.clear();
    }
}

fn parse_kiji_output(stdout: &[u8]) -> Result<Vec<PrivateKijiSpan>, SafetyNetError> {
    let text = std::str::from_utf8(stdout).map_err(|_| SafetyNetError::InvalidOutput {
        message: "kiji stdout was not valid UTF-8".to_string(),
    })?;
    serde_json::from_str(text).map_err(|_| SafetyNetError::InvalidOutput {
        message: "kiji stdout was not valid JSON".to_string(),
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
            message: format!("kiji {stream} reader panicked"),
        })?
        .map_err(|error| SafetyNetError::Runtime {
            message: format!(
                "kiji {stream} capture failed: {}",
                sanitize_error(&error.to_string())
            ),
        })
}

fn join_stdin(thread: thread::JoinHandle<std::io::Result<()>>) -> Result<(), SafetyNetError> {
    thread
        .join()
        .map_err(|_| SafetyNetError::Runtime {
            message: "kiji stdin writer panicked".to_string(),
        })?
        .map_err(|error| SafetyNetError::Runtime {
            message: format!(
                "kiji stdin write failed: {}",
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
    stderr_thread: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
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

fn verify_command_path(command: &Path) -> Result<(), SafetyNetError> {
    if command.components().count() <= 1 {
        return Ok(());
    }

    let metadata =
        std::fs::symlink_metadata(command).map_err(|_| SafetyNetError::ModelUnavailable {
            reason: "kiji command path is not accessible".to_string(),
        })?;

    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "kiji command path is not a regular file".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::artifacts::{
        hex_sha256, KIJI_DISTILBERT_BUNDLE_SHA256, KIJI_DISTILBERT_HF_COMMIT,
        KIJI_DISTILBERT_INT8_BUNDLE_SHA256, KIJI_DISTILBERT_INT8_SHA256SUMS,
        KIJI_DISTILBERT_SHA256SUMS, REQUIRED_KIJI_ARTIFACTS,
    };
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn write_secure_artifact(dir: &Path, name: &str, body: &[u8]) {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn populate_secure_model_dir(dir: &Path) -> String {
        #[cfg(unix)]
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        for required in REQUIRED_KIJI_ARTIFACTS {
            if *required == "SHA256SUMS" {
                continue;
            }
            write_secure_artifact(dir, required, b"fixture");
        }
        let sums = fixture_sha256sums();
        write_secure_artifact(dir, "SHA256SUMS", sums.as_bytes());
        hex_sha256(sums.as_bytes())
    }

    fn fixture_sha256sums() -> String {
        let artifact_sha = hex_sha256(b"fixture");
        format!(
            "{artifact_sha}  labels.json\n{artifact_sha}  model.onnx\n{artifact_sha}  tokenizer.json\n"
        )
    }

    #[test]
    fn private_fields_do_not_debug() {
        let span: PrivateKijiSpan =
            serde_json::from_str(r#"{"label":"person","start":0,"end":5,"text":"Alice Smith"}"#)
                .unwrap();
        let debug = format!("{span:?}");
        assert!(!debug.contains("Alice Smith"));
        assert!(debug.contains("person"));
    }

    #[test]
    fn parses_typed_span_array() {
        let spans = parse_kiji_output(
            br#"[{"label":"person","start":0,"end":11,"text":"Alice Smith","score":0.97}]"#,
        )
        .unwrap();
        assert_eq!(spans.len(), 1);
        let raw = spans.into_iter().next().unwrap().into_raw_span().unwrap();
        assert_eq!(raw, RawSpan::new(0, 11, "person", Some(0.97)));
    }

    #[test]
    fn sanitizes_stderr_pii() {
        let stderr = sanitize_stderr(b"failed for alice@example.invalid at +1-555-0101");
        assert!(!stderr.contains("alice@example.invalid"));
        assert!(!stderr.contains("+1-555-0101"));
        assert!(stderr.contains("<redacted>"));
    }

    #[test]
    fn missing_model_dir_fails_closed() {
        let missing = tempfile::tempdir().unwrap().path().join("kiji-distilbert");
        let config = SubprocessKijiConfig::new("kiji").with_model_dir(missing);
        let error = SubprocessKijiBackend::new(config).unwrap_err();
        assert!(matches!(error, SafetyNetError::WeightsMissing { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn missing_sha256sums_fails_closed_with_weights_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        // Provide every required artifact except SHA256SUMS — Axis-1 fail-closed.
        for required in REQUIRED_KIJI_ARTIFACTS {
            if *required == "SHA256SUMS" {
                continue;
            }
            write_secure_artifact(dir.path(), required, b"fixture");
        }
        let config = SubprocessKijiConfig::new("kiji").with_model_dir(dir.path());
        let error = SubprocessKijiBackend::new(config).unwrap_err();
        match error {
            SafetyNetError::WeightsMissing { path } => {
                assert!(
                    path.contains("SHA256SUMS"),
                    "missing-artifact path should name SHA256SUMS, got: {path}"
                );
            }
            other => panic!("expected WeightsMissing, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn complete_artifact_set_constructs_backend() {
        let dir = tempfile::tempdir().unwrap();
        let expected_sha = populate_secure_model_dir(dir.path());
        let config = SubprocessKijiConfig::new("kiji")
            .with_model_dir(dir.path())
            .with_expected_bundle_sha256_for_tests(expected_sha);
        SubprocessKijiBackend::new(config).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn checksum_mismatch_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let expected_sha = populate_secure_model_dir(dir.path());
        write_secure_artifact(dir.path(), "model.onnx", b"tampered");
        let config = SubprocessKijiConfig::new("kiji")
            .with_model_dir(dir.path())
            .with_expected_bundle_sha256_for_tests(expected_sha);
        let error = SubprocessKijiBackend::new(config).unwrap_err();
        assert!(matches!(
            error,
            SafetyNetError::ModelIntegrityMismatch { .. }
        ));
    }

    #[test]
    fn documented_bundle_sha_matches_canonical_checksum_file() {
        assert_eq!(
            hex_sha256(KIJI_DISTILBERT_SHA256SUMS.as_bytes()),
            KIJI_DISTILBERT_BUNDLE_SHA256
        );
        assert_eq!(
            hex_sha256(KIJI_DISTILBERT_INT8_SHA256SUMS.as_bytes()),
            KIJI_DISTILBERT_INT8_BUNDLE_SHA256
        );

        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest_dir.parent().unwrap().parent().unwrap();
        let setup_doc =
            std::fs::read_to_string(workspace.join("docs/how-to/safety-net/set-up-kiji-safetynet.md"))
                .unwrap();
        let fetcher =
            std::fs::read_to_string(workspace.join("scripts/fetch-kiji-safetynet-model.sh"))
                .unwrap();

        for content in [setup_doc, fetcher] {
            assert!(content.contains(KIJI_DISTILBERT_HF_COMMIT));
            assert!(content.contains(KIJI_DISTILBERT_BUNDLE_SHA256));
            assert!(content.contains(KIJI_DISTILBERT_INT8_BUNDLE_SHA256));
        }
    }

    #[cfg(unix)]
    #[test]
    fn group_writable_artifact_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        for required in REQUIRED_KIJI_ARTIFACTS {
            write_secure_artifact(dir.path(), required, b"fixture");
        }
        // Loosen mode on SHA256SUMS to trigger the verify_one_sensitive_path branch.
        std::fs::set_permissions(
            dir.path().join("SHA256SUMS"),
            std::fs::Permissions::from_mode(0o660),
        )
        .unwrap();
        let config = SubprocessKijiConfig::new("kiji").with_model_dir(dir.path());
        let error = SubprocessKijiBackend::new(config).unwrap_err();
        assert!(matches!(error, SafetyNetError::ModelUnavailable { .. }));
    }
}
