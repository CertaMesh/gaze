use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use thiserror::Error;

/// Agent-controlled execution input. This is never handed directly to a
/// sandbox backend; core validates it first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrustedExecRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

impl UntrustedExecRequest {
    pub fn validate(self, policy: &ExecPolicy) -> Result<ValidatedExecRequest, SandboxError> {
        policy.validate(self)
    }
}

/// Trusted execution request after core-side validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedExecRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

/// Sandbox backends operate only on validated requests. The trust boundary
/// is explicit: core validates agent-controlled argv/env/path input before
/// any backend-specific wrapping is applied.
pub trait Sandbox: Send + Sync {
    fn prepare(&self, request: &ValidatedExecRequest) -> Result<SandboxPlan, SandboxError>;
}

/// Backend-produced execution plan. v0.2 only lands the trait shape, so this
/// remains a simple command description rather than a spawned process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

impl SandboxPlan {
    pub fn passthrough(request: &ValidatedExecRequest) -> Self {
        Self {
            program: request.program.clone(),
            args: request.args.clone(),
            env: request.env.clone(),
            cwd: request.cwd.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecPolicy {
    allowed_programs: BTreeSet<PathBuf>,
    allowed_env: BTreeSet<String>,
}

impl ExecPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_program(mut self, path: impl Into<PathBuf>) -> Self {
        self.allowed_programs.insert(path.into());
        self
    }

    pub fn allow_env(mut self, key: impl Into<String>) -> Self {
        self.allowed_env.insert(key.into());
        self
    }

    pub fn validate(
        &self,
        request: UntrustedExecRequest,
    ) -> Result<ValidatedExecRequest, SandboxError> {
        if !self
            .allowed_programs
            .iter()
            .any(|allowed| allowed == &request.program)
        {
            return Err(SandboxError::ProgramNotAllowed(request.program));
        }

        if request
            .args
            .iter()
            .any(|arg| contains_shell_metachar(arg) || contains_control_chars(arg))
        {
            return Err(SandboxError::UnsafeArgv);
        }

        if request
            .env
            .keys()
            .any(|key| !self.allowed_env.contains(key))
        {
            let rejected = request
                .env
                .keys()
                .find(|key| !self.allowed_env.contains(*key))
                .cloned()
                .expect("rejected env key");
            return Err(SandboxError::EnvNotAllowed(rejected));
        }

        if request
            .env
            .iter()
            .any(|(key, value)| contains_control_chars(key) || contains_control_chars(value))
        {
            return Err(SandboxError::InvalidEnvEncoding);
        }

        if request
            .cwd
            .as_deref()
            .is_some_and(|cwd| !cwd.is_absolute())
        {
            return Err(SandboxError::RelativeWorkingDirectory(
                request.cwd.expect("relative cwd"),
            ));
        }

        Ok(ValidatedExecRequest {
            program: request.program,
            args: request.args,
            env: request.env,
            cwd: request.cwd,
        })
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SandboxError {
    #[error("program not allowed: {0}")]
    ProgramNotAllowed(PathBuf),
    #[error("argv contains unsafe shell metacharacters")]
    UnsafeArgv,
    #[error("env key not allowed: {0}")]
    EnvNotAllowed(String),
    #[error("env contains invalid control characters")]
    InvalidEnvEncoding,
    #[error("working directory must be absolute: {0}")]
    RelativeWorkingDirectory(PathBuf),
    #[error("sandbox backend error: {0}")]
    Backend(String),
}

fn contains_shell_metachar(input: &str) -> bool {
    input.chars().any(|ch| matches!(ch, ';' | '|' | '&' | '$' | '`'))
}

fn contains_control_chars(input: &str) -> bool {
    input
        .chars()
        .any(|ch| ch.is_control() && ch != '\t')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_accepts_allowed_program_and_env() {
        let policy = ExecPolicy::new()
            .allow_program("/usr/local/bin/gaze-hook")
            .allow_env("MAIL_FROM");
        let request = UntrustedExecRequest {
            program: PathBuf::from("/usr/local/bin/gaze-hook"),
            args: vec!["send-email".to_string(), "<Email_1>".to_string()],
            env: BTreeMap::from([("MAIL_FROM".to_string(), "bot@example.test".to_string())]),
            cwd: Some(PathBuf::from("/tmp")),
        };

        let validated = request.validate(&policy).expect("validated request");
        assert_eq!(validated.program, PathBuf::from("/usr/local/bin/gaze-hook"));
        assert_eq!(validated.args[1], "<Email_1>");
    }

    #[test]
    fn validation_rejects_shell_metacharacters() {
        let policy = ExecPolicy::new().allow_program("/usr/local/bin/gaze-hook");
        let request = UntrustedExecRequest {
            program: PathBuf::from("/usr/local/bin/gaze-hook"),
            args: vec!["send-email;cat".to_string()],
            env: BTreeMap::new(),
            cwd: None,
        };

        assert_eq!(request.validate(&policy), Err(SandboxError::UnsafeArgv));
    }
}
