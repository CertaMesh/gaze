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
    EvalSchema,
    EvalSchemaLine {
        line: usize,
    },
    EvalSchemaLocation {
        line: usize,
        column: usize,
    },
    PolicyConfig,
    PolicyConfigDetail(String),
    PolicySchemaUnsupported {
        found: String,
        supported: &'static str,
    },
    SafetyNetConfigDetail(String),
    /// Nym policy or bundle configuration errors are setup failures (exit 2).
    SafetyNetPolicyConfigDetail(String),
    SafetyNetUsageDetail(String),
    SafetyNetFailure {
        variant: &'static str,
    },
    // Only `gaze setup` builds this; keep the closed variant set stable without it.
    #[cfg_attr(not(feature = "setup"), allow(dead_code))]
    SetupDetail(String),
    /// Pinned-artifact contract violation: a safety-net backend was requested
    /// but its required artifact (e.g. `SHA256SUMS`) is missing on disk. Exit
    /// code 2 (config-level error). Axis-1 fail-closed: never silent-disable.
    #[allow(dead_code)]
    SafetyNetArtifactMissing {
        backend: &'static str,
        path: String,
    },
    AuditPurgeIso8601 {
        input: String,
    },
    UnknownToken {
        token: String,
    },
    InvalidSignature,
    InvalidBlobVersion,
    BlobExpired,
    Pipeline,
    Io,
    PolicyOpen,
    /// The policy exists but this account may not read it; the detail names the file and the
    /// chmod/chown fix. Same `PolicyOpen` variant name and exit code as a missing policy.
    PolicyOpenDetail(String),
    /// A `clean --session-out` or `restore --session-file` path is missing,
    /// already exists, cannot be read or written securely, or is not a session file.
    /// Includes unsupported owner-only creation and ACL verification failures.
    /// Signature, version and expiry failures keep their own variants.
    SessionFile(String),
    /// `gaze index ingest` has no pinned NER bundle to detect prose names and organizations.
    #[cfg(feature = "index")]
    IndexNerModelMissing(String),
    #[cfg(feature = "document")]
    DocumentDetail(String),
    #[cfg(feature = "mcp")]
    McpDetail(String),
    #[cfg(feature = "proxy")]
    ProxyDetail(String),
    /// A confirmed sensitive value has no safe reversible rendering, so nothing was written.
    /// Exit code 8 is the public contract. Only the closed reason reaches stderr.
    Untransformable {
        reason: gaze_types::UntransformableReason,
    },
}

impl CliError {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::StdinParse
            | Self::EmptyInput
            | Self::InputTooLarge
            | Self::InvalidEncoding
            | Self::EvalSchema
            | Self::EvalSchemaLine { .. }
            | Self::EvalSchemaLocation { .. } => 1,
            Self::PolicyConfig
            | Self::PolicyConfigDetail(_)
            | Self::PolicySchemaUnsupported { .. }
            | Self::AuditPurgeIso8601 { .. }
            | Self::SetupDetail(_)
            | Self::SafetyNetArtifactMissing { .. }
            | Self::SafetyNetPolicyConfigDetail(_)
            | Self::SafetyNetUsageDetail(_) => 2,
            #[cfg(feature = "index")]
            Self::IndexNerModelMissing(_) => 2,
            Self::SafetyNetConfigDetail(_) | Self::SafetyNetFailure { .. } => 3,
            Self::UnknownToken { .. }
            | Self::InvalidSignature
            | Self::InvalidBlobVersion
            | Self::BlobExpired
            | Self::Pipeline => 3,
            Self::Io | Self::PolicyOpen | Self::PolicyOpenDetail(_) | Self::SessionFile(_) => 4,
            #[cfg(feature = "document")]
            Self::DocumentDetail(_) => 5,
            #[cfg(feature = "mcp")]
            Self::McpDetail(_) => 6,
            #[cfg(feature = "proxy")]
            Self::ProxyDetail(_) => 7,
            Self::Untransformable { .. } => 8,
        }
    }

    fn variant_name(&self) -> &'static str {
        match self {
            Self::StdinParse => "StdinParse",
            Self::EmptyInput => "EmptyInput",
            Self::InputTooLarge => "InputTooLarge",
            Self::InvalidEncoding => "InvalidEncoding",
            Self::EvalSchema | Self::EvalSchemaLine { .. } | Self::EvalSchemaLocation { .. } => {
                "EvalSchema"
            }
            Self::PolicyConfig | Self::PolicyConfigDetail(_) => "PolicyConfig",
            Self::PolicySchemaUnsupported { .. } => "PolicySchemaUnsupported",
            Self::SafetyNetConfigDetail(_) => "SafetyNetConfig",
            Self::SafetyNetPolicyConfigDetail(_) => "SafetyNetConfig",
            Self::SafetyNetUsageDetail(_) => "SafetyNetUsage",
            Self::SafetyNetFailure { .. } => "SafetyNet",
            Self::SetupDetail(_) => "Setup",
            Self::SafetyNetArtifactMissing { .. } => "SafetyNetArtifactMissing",
            #[cfg(feature = "index")]
            Self::IndexNerModelMissing(_) => "IndexNerModelMissing",
            Self::AuditPurgeIso8601 { .. } => "AuditPurgeIso8601",
            Self::UnknownToken { .. } => "UnknownToken",
            Self::InvalidSignature => "InvalidSignature",
            Self::InvalidBlobVersion => "InvalidBlobVersion",
            Self::BlobExpired => "BlobExpired",
            Self::Pipeline => "Pipeline",
            Self::Io => "Io",
            Self::PolicyOpen | Self::PolicyOpenDetail(_) => "PolicyOpen",
            Self::SessionFile(_) => "SessionFile",
            #[cfg(feature = "document")]
            Self::DocumentDetail(_) => "Document",
            #[cfg(feature = "mcp")]
            Self::McpDetail(_) => "Mcp",
            #[cfg(feature = "proxy")]
            Self::ProxyDetail(_) => "Proxy",
            Self::Untransformable { .. } => "Untransformable",
        }
    }

    pub(crate) fn emit_stderr(&self) {
        match self {
            Self::EvalSchemaLine { line } => eprintln!(
                r#"{{"error":"{}","exit":{},"line":{}}}"#,
                self.variant_name(),
                self.exit_code(),
                line
            ),
            Self::EvalSchemaLocation { line, column } => eprintln!(
                r#"{{"error":"{}","exit":{},"line":{},"column":{}}}"#,
                self.variant_name(),
                self.exit_code(),
                line,
                column
            ),
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
            Self::PolicyConfigDetail(detail)
            | Self::PolicyOpenDetail(detail)
            | Self::SessionFile(detail)
            | Self::SafetyNetConfigDetail(detail)
            | Self::SafetyNetPolicyConfigDetail(detail)
            | Self::SafetyNetUsageDetail(detail) => {
                let detail = serde_json::to_string(detail)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"detail":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    detail
                )
            }
            #[cfg(feature = "index")]
            Self::IndexNerModelMissing(detail) => {
                let detail = serde_json::to_string(detail)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"detail":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    detail
                )
            }
            Self::SetupDetail(detail) => {
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
            Self::Untransformable { reason } => eprintln!("{}", untransformable_line(*reason)),
            Self::SafetyNetFailure { variant } => eprintln!(
                r#"{{"error":"{}","exit":{},"variant":"{}"}}"#,
                self.variant_name(),
                self.exit_code(),
                variant
            ),
            Self::SafetyNetArtifactMissing { backend, path } => {
                let path = serde_json::to_string(path)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"backend":"{}","path":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    backend,
                    path
                )
            }
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
            #[cfg(feature = "proxy")]
            Self::ProxyDetail(detail) => {
                let detail = serde_json::to_string(detail)
                    .unwrap_or_else(|_| "\"<unserializable>\"".to_string());
                eprintln!(
                    r#"{{"error":"{}","exit":{},"detail":{}}}"#,
                    self.variant_name(),
                    self.exit_code(),
                    detail
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

fn untransformable_line(reason: gaze_types::UntransformableReason) -> String {
    let error = CliError::Untransformable { reason };
    format!(
        r#"{{"error":"{}","exit":{},"reason":"{}"}}"#,
        error.variant_name(),
        error.exit_code(),
        reason.as_str()
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_types::UntransformableReason;

    #[test]
    fn untransformable_is_exit_eight_with_only_the_closed_reason() {
        for reason in [
            UntransformableReason::JsonNumberNoTextOnlyPath,
            UntransformableReason::EncodedCarrier,
            UntransformableReason::NoSafeRendering,
            UntransformableReason::CallerValueTooShort,
        ] {
            assert_eq!(CliError::Untransformable { reason }.exit_code(), 8);
            let line = untransformable_line(reason);
            assert_eq!(
                line,
                format!(
                    r#"{{"error":"Untransformable","exit":8,"reason":"{}"}}"#,
                    reason.as_str()
                )
            );
            serde_json::from_str::<serde_json::Value>(&line).expect("stderr line is JSON");
        }
    }
}
