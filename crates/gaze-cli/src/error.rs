use clap::ValueEnum;
use serde::Serialize;

/// Structured CLI error. Each variant maps to an exit code; only the variant
/// name reaches stderr so raw input or plaintext blob entries never leak into
/// caller logs.
#[derive(Debug)]
pub(crate) enum CliError {
    StdinParse,
    EmptyInput,
    InputTooLarge,
    InvalidEncoding,
    PolicyConfig,
    PolicyConfigDetail(String),
    PolicySchemaUnsupported {
        found: String,
        supported: &'static str,
    },
    SafetyNetConfigDetail(String),
    SafetyNetFailure {
        variant: &'static str,
    },
    AuditPurgeIso8601 {
        input: String,
    },
    UnknownToken {
        token: String,
    },
    UnsupportedSessionScope {
        variant: String,
    },
    InvalidSignature,
    InvalidBlobVersion,
    BlobExpired,
    Pipeline,
    Io,
    PolicyOpen,
    #[cfg(feature = "document")]
    DocumentDetail(String),
    #[cfg(feature = "mcp")]
    McpDetail(String),
}

impl CliError {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::StdinParse | Self::EmptyInput | Self::InputTooLarge | Self::InvalidEncoding => 1,
            Self::PolicyConfig
            | Self::PolicyConfigDetail(_)
            | Self::PolicySchemaUnsupported { .. }
            | Self::AuditPurgeIso8601 { .. } => 2,
            Self::SafetyNetConfigDetail(_) | Self::SafetyNetFailure { .. } => 3,
            Self::UnknownToken { .. }
            | Self::UnsupportedSessionScope { .. }
            | Self::InvalidSignature
            | Self::InvalidBlobVersion
            | Self::BlobExpired
            | Self::Pipeline => 3,
            Self::Io | Self::PolicyOpen => 4,
            #[cfg(feature = "document")]
            Self::DocumentDetail(_) => 5,
            #[cfg(feature = "mcp")]
            Self::McpDetail(_) => 6,
        }
    }

    fn variant_name(&self) -> &'static str {
        match self {
            Self::StdinParse => "StdinParse",
            Self::EmptyInput => "EmptyInput",
            Self::InputTooLarge => "InputTooLarge",
            Self::InvalidEncoding => "InvalidEncoding",
            Self::PolicyConfig | Self::PolicyConfigDetail(_) => "PolicyConfig",
            Self::PolicySchemaUnsupported { .. } => "PolicySchemaUnsupported",
            Self::SafetyNetConfigDetail(_) => "SafetyNetConfig",
            Self::SafetyNetFailure { .. } => "SafetyNet",
            Self::AuditPurgeIso8601 { .. } => "AuditPurgeIso8601",
            Self::UnknownToken { .. } => "UnknownToken",
            Self::UnsupportedSessionScope { .. } => "UnsupportedSessionScope",
            Self::InvalidSignature => "InvalidSignature",
            Self::InvalidBlobVersion => "InvalidBlobVersion",
            Self::BlobExpired => "BlobExpired",
            Self::Pipeline => "Pipeline",
            Self::Io => "Io",
            Self::PolicyOpen => "PolicyOpen",
            #[cfg(feature = "document")]
            Self::DocumentDetail(_) => "Document",
            #[cfg(feature = "mcp")]
            Self::McpDetail(_) => "Mcp",
        }
    }

    pub(crate) fn emit_stderr(&self) {
        match self {
            Self::PolicySchemaUnsupported { found, supported } => {
                let found = serde_json::to_string(found)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                let supported = serde_json::to_string(supported)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"found":{},"supported":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    found,
                    supported
                )
            }
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
            #[cfg(feature = "document")]
            Self::DocumentDetail(detail) => {
                let detail = serde_json::to_string(detail)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"detail":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    detail
                )
            }
            #[cfg(feature = "mcp")]
            Self::McpDetail(detail) => {
                let detail = serde_json::to_string(detail)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"detail":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    detail
                )
            }
            Self::UnsupportedSessionScope { variant } => {
                let variant = serde_json::to_string(variant)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"variant":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    variant
                )
            }
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
