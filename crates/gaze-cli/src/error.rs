use clap::ValueEnum;
use serde::Serialize;

/// Structured CLI error. Each variant maps to an exit code; only the variant
/// name reaches stderr so raw input or plaintext blob entries never leak into
/// caller logs (see `ROADMAP.md` for roadmap context).
#[derive(Debug)]
pub(crate) enum CliError {
    StdinParse,
    EmptyInput,
    InputTooLarge,
    InvalidEncoding,
    PolicyConfig,
    PolicyConfigDetail(String),
    SafetyNetConfigDetail(String),
    SafetyNetFailure { variant: &'static str },
    AuditPurgeIso8601 { input: String },
    UnknownToken { token: String },
    InvalidSignature,
    InvalidBlobVersion,
    BlobExpired,
    Pipeline,
    Io,
    PolicyOpen,
}

impl CliError {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::StdinParse | Self::EmptyInput | Self::InputTooLarge | Self::InvalidEncoding => 1,
            Self::PolicyConfig | Self::PolicyConfigDetail(_) | Self::AuditPurgeIso8601 { .. } => 2,
            Self::SafetyNetConfigDetail(_) | Self::SafetyNetFailure { .. } => 3,
            Self::UnknownToken { .. }
            | Self::InvalidSignature
            | Self::InvalidBlobVersion
            | Self::BlobExpired
            | Self::Pipeline => 3,
            Self::Io | Self::PolicyOpen => 4,
        }
    }

    fn variant_name(&self) -> &'static str {
        match self {
            Self::StdinParse => "StdinParse",
            Self::EmptyInput => "EmptyInput",
            Self::InputTooLarge => "InputTooLarge",
            Self::InvalidEncoding => "InvalidEncoding",
            Self::PolicyConfig | Self::PolicyConfigDetail(_) => "PolicyConfig",
            Self::SafetyNetConfigDetail(_) => "SafetyNetConfig",
            Self::SafetyNetFailure { .. } => "SafetyNet",
            Self::AuditPurgeIso8601 { .. } => "AuditPurgeIso8601",
            Self::UnknownToken { .. } => "UnknownToken",
            Self::InvalidSignature => "InvalidSignature",
            Self::InvalidBlobVersion => "InvalidBlobVersion",
            Self::BlobExpired => "BlobExpired",
            Self::Pipeline => "Pipeline",
            Self::Io => "Io",
            Self::PolicyOpen => "PolicyOpen",
        }
    }

    pub(crate) fn emit_stderr(&self) {
        match self {
            Self::PolicyConfigDetail(detail) | Self::SafetyNetConfigDetail(detail) => {
                let detail = serde_json::to_string(detail)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"detail":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    detail
                )
            }
            Self::AuditPurgeIso8601 { input } => {
                let input = serde_json::to_string(input)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"input":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    input
                )
            }
            Self::UnknownToken { token } => {
                let token = serde_json::to_string(token)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"token":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    token
                )
            }
            Self::SafetyNetFailure { variant } => eprintln!(
                r#"{{"error":"{}","exit":{},"variant":"{}"}}"#,
                self.variant_name(),
                self.exit_code(),
                variant
            ),
            _ => eprintln!(
                r#"{{"error":"{}","exit":{}}}"#,
                self.variant_name(),
                self.exit_code()
            ),
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RestoreMode {
    Strict,
    Tolerant,
}

#[derive(Serialize)]
pub(crate) struct RestoreWarning {
    pub(crate) variant: String,
    pub(crate) token: String,
}
