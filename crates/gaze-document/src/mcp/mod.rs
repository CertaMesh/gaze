//! MCP tool adapters for `gaze-document`.
//!
//! Tools in this module are opt-in via the `mcp` feature and must be
//! registered explicitly by adopters. Invocation still happens through
//! `gaze_mcp_core::PiiEnvelope::dispatch`; this module only provides the
//! tool bodies and catalog metadata.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use gaze::{CleanDocument, RawDocument};
use gaze_mcp_core::{
    Tool, ToolCtx, ToolDescriptor, ToolError, ToolRegistry, ToolRegistryError, ToolResponse,
};
use serde::Serialize;
use serde_json::json;

#[cfg(feature = "ocr-tesseract")]
use crate::extract::InputKind;
#[cfg(feature = "ocr-tesseract")]
use crate::DocumentError;

/// Default `gaze_read_file` input cap: 25 MiB.
pub const DEFAULT_MAX_FILE_SIZE: u64 = 25 * 1024 * 1024;

/// Options used when registering `gaze-document` MCP tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct GazeReadOpts {
    /// Maximum accepted input file size in bytes for `gaze_read_file`.
    pub max_file_size: u64,
}

impl Default for GazeReadOpts {
    fn default() -> Self {
        Self {
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

/// Register `gaze_read_text` and `gaze_read_file` with their canonical names.
pub fn register_tools(
    registry: &mut ToolRegistry,
    opts: GazeReadOpts,
) -> Result<(), ToolRegistryError> {
    registry.register(GazeReadText::new())?;
    registry.register(GazeReadFile::with_max_file_size(opts.max_file_size))?;
    Ok(())
}

/// MCP tool that tokenizes already-extracted text via the document recognizer set.
#[derive(Debug)]
#[non_exhaustive]
pub struct GazeReadText {
    descriptor: ToolDescriptor,
}

impl GazeReadText {
    /// Construct a `gaze_read_text` tool.
    pub fn new() -> Self {
        Self {
            descriptor: ToolDescriptor::agent(
                "gaze_read_text",
                json!({
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Already-extracted text to pseudonymize before model use."
                        }
                    },
                    "required": ["text"]
                }),
            )
            .with_description("Pseudonymize already-extracted text before returning it to an MCP client.")
            .with_output_schema(response_schema()),
        }
    }
}

impl Default for GazeReadText {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GazeReadText {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        let text = required_string(ctx.redacted_args(), "text")?;
        let clean_text = redact_document_text(text, ctx)?;
        Ok(ToolResponse::json(json!(DocumentToolResponse {
            clean_markdown: format_text_markdown(&clean_text),
            manifest_id: ctx.call_id().to_string(),
            file_metadata: FileMetadata {
                source_kind: "text".to_string(),
                ocr_mean_confidence: None,
                bundle_version: crate::BUNDLE_VERSION,
                page_count: None,
            },
        })))
    }
}

/// MCP tool that OCRs an image/PDF file and tokenizes text before model use.
#[derive(Debug)]
#[non_exhaustive]
pub struct GazeReadFile {
    descriptor: ToolDescriptor,
    max_file_size: u64,
}

impl GazeReadFile {
    /// Construct a `gaze_read_file` tool with the default 25 MiB input cap.
    pub fn new() -> Self {
        Self::with_max_file_size(DEFAULT_MAX_FILE_SIZE)
    }

    /// Construct a `gaze_read_file` tool with a caller-supplied input cap.
    pub fn with_max_file_size(max_file_size: u64) -> Self {
        Self {
            descriptor: ToolDescriptor::agent(
                "gaze_read_file",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Filesystem path to a PNG, JPG, or PDF document."
                        }
                    },
                    "required": ["path"]
                }),
            )
            .with_description(
                "Read an image or PDF through OCR and Gaze pseudonymization before MCP return.",
            )
            .with_output_schema(response_schema()),
            max_file_size,
        }
    }
}

impl Default for GazeReadFile {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GazeReadFile {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        let path = PathBuf::from(required_string(ctx.redacted_args(), "path")?);
        validate_file(&path, self.max_file_size)?;
        read_file_response(&path, ctx).map(|response| ToolResponse::json(json!(response)))
    }
}

#[derive(Serialize)]
struct DocumentToolResponse {
    clean_markdown: String,
    manifest_id: String,
    file_metadata: FileMetadata,
}

#[derive(Serialize)]
struct FileMetadata {
    source_kind: String,
    ocr_mean_confidence: Option<f32>,
    bundle_version: u32,
    page_count: Option<u32>,
}

fn required_string<'a>(args: &'a serde_json::Value, field: &str) -> Result<&'a str, ToolError> {
    args.get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| ToolError::InvalidArgs(format!("missing required string field `{field}`")))
}

fn redact_document_text(text: &str, ctx: &ToolCtx<'_>) -> Result<String, ToolError> {
    let pipeline = crate::bundle::build_document_pipeline().map_err(map_document_error)?;
    let clean = pipeline
        .redact_with_context(
            ctx.resources().session(),
            RawDocument::Text(text.to_string()),
            ctx.resources().locale_chain(),
        )
        .map_err(|err| ToolError::BackendFailure(format!("document pipeline failed: {err}")))?;
    match clean {
        CleanDocument::Text(text) => Ok(text),
        _ => Err(ToolError::BackendFailure(
            "document pipeline returned non-text output".to_string(),
        )),
    }
}

fn validate_file(path: &Path, max_file_size: u64) -> Result<(), ToolError> {
    let metadata = std::fs::metadata(path).map_err(|err| map_file_metadata_error(path, err))?;
    if !metadata.is_file() {
        return Err(ToolError::InvalidArgs(format!(
            "path `{}` is not a regular file",
            path.display()
        )));
    }
    if metadata.len() > max_file_size {
        return Err(ToolError::LimitExceeded(format!(
            "file `{}` is {} bytes; configured cap is {} bytes",
            path.display(),
            metadata.len(),
            max_file_size
        )));
    }
    Ok(())
}

fn map_file_metadata_error(path: &Path, err: std::io::Error) -> ToolError {
    if err.kind() == std::io::ErrorKind::NotFound {
        ToolError::NotFound(format!("file `{}` not found", path.display()))
    } else {
        ToolError::internal(err)
    }
}

#[cfg(feature = "ocr-tesseract")]
fn read_file_response(path: &Path, ctx: &ToolCtx<'_>) -> Result<DocumentToolResponse, ToolError> {
    let kind = InputKind::detect(path).map_err(map_document_error)?;
    let backend = crate::ocr::TesseractBackend::new();
    let (ocr_result, pdf_page_count, _) =
        crate::bundle::run_ocr(path, kind, &backend).map_err(map_document_error)?;
    let normalized = crate::ocr::normalize_ocr_artifacts(&ocr_result.text);
    let clean_text = redact_document_text(&normalized, ctx)?;
    Ok(DocumentToolResponse {
        clean_markdown: crate::bundle::format_clean_markdown(&clean_text, kind),
        manifest_id: ctx.call_id().to_string(),
        file_metadata: FileMetadata {
            source_kind: source_kind(kind).to_string(),
            ocr_mean_confidence: ocr_result.mean_confidence,
            bundle_version: crate::BUNDLE_VERSION,
            page_count: pdf_page_count.and_then(|count| u32::try_from(count).ok()),
        },
    })
}

#[cfg(not(feature = "ocr-tesseract"))]
fn read_file_response(
    _path: &PathBuf,
    _ctx: &ToolCtx<'_>,
) -> Result<DocumentToolResponse, ToolError> {
    Err(ToolError::BackendUnavailable(
        "rebuild gaze-document with `--features ocr-tesseract` to enable `gaze_read_file`"
            .to_string(),
    ))
}

#[cfg(feature = "ocr-tesseract")]
fn source_kind(kind: InputKind) -> &'static str {
    match crate::bundle::kind_label(kind) {
        "png" | "jpeg" => "image",
        "pdf" => "pdf",
        other => other,
    }
}

#[cfg(feature = "ocr-tesseract")]
fn map_document_error(err: DocumentError) -> ToolError {
    match err {
        DocumentError::TesseractNotFound(hint) | DocumentError::PdfiumNotFound(hint) => {
            ToolError::BackendUnavailable(hint)
        }
        DocumentError::TesseractFailed { status, stderr } => {
            ToolError::BackendFailure(format!("tesseract exited with status {status}: {stderr}"))
        }
        DocumentError::PdfRasterFailed(detail) => ToolError::BackendFailure(detail),
        DocumentError::UnsupportedInput { path, reason } => {
            ToolError::InvalidArgs(format!("unsupported input `{}`: {reason}", path.display()))
        }
        other => ToolError::internal(other),
    }
}

#[cfg(not(feature = "ocr-tesseract"))]
fn map_document_error(err: crate::DocumentError) -> ToolError {
    ToolError::internal(err)
}

fn format_text_markdown(text: &str) -> String {
    let mut out = String::new();
    out.push_str("# gaze-document safe text\n\n");
    out.push_str("Source kind: `text`\n\n");
    out.push_str("---\n\n");
    out.push_str(text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn response_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "clean_markdown": { "type": "string" },
            "manifest_id": { "type": "string" },
            "file_metadata": {
                "type": "object",
                "properties": {
                    "source_kind": { "type": "string" },
                    "ocr_mean_confidence": { "type": ["number", "null"] },
                    "bundle_version": { "type": "integer" },
                    "page_count": { "type": ["integer", "null"] }
                },
                "required": [
                    "source_kind",
                    "ocr_mean_confidence",
                    "bundle_version",
                    "page_count"
                ]
            }
        },
        "required": ["clean_markdown", "manifest_id", "file_metadata"]
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;
    use gaze_mcp_core::{
        AuthError, AuthHook, DispatchError, ManifestStore, PiiEnvelope, Principal, SessionIdPolicy,
    };
    use gaze_mcp_core::{BeginCallContext, CallHandle, FailureReason, ManifestError, SnapshotRef};
    use serde_json::json;

    use super::*;

    struct AllowAllAuth;

    #[async_trait]
    impl AuthHook for AllowAllAuth {
        async fn authorize_agent(
            &self,
            _principal: &Principal,
            _tool_name: &str,
        ) -> Result<(), AuthError> {
            Ok(())
        }

        async fn authorize_operator(
            &self,
            _principal: &Principal,
            _tool_name: &str,
        ) -> Result<(), AuthError> {
            Err(AuthError::Denied("operator tier disabled in test".into()))
        }
    }

    struct RecordingManifest {
        begins: AtomicUsize,
        finishes: AtomicUsize,
        failures: AtomicUsize,
    }

    impl RecordingManifest {
        fn new() -> Self {
            Self {
                begins: AtomicUsize::new(0),
                finishes: AtomicUsize::new(0),
                failures: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ManifestStore for RecordingManifest {
        async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
            self.begins.fetch_add(1, Ordering::SeqCst);
            Ok(CallHandle::new(ctx.call_id))
        }

        async fn finish_call(
            &self,
            _handle: CallHandle,
            _snapshot: SnapshotRef,
        ) -> Result<(), ManifestError> {
            self.finishes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn fail_call(
            &self,
            _handle: CallHandle,
            _reason: FailureReason,
        ) -> Result<(), ManifestError> {
            self.failures.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Harness {
        registry: ToolRegistry,
        auth: AllowAllAuth,
        manifest: Arc<RecordingManifest>,
        pipeline: gaze::Pipeline,
        session: gaze::Session,
        session_id_policy: SessionIdPolicy,
    }

    impl Harness {
        fn new() -> Self {
            let mut registry = ToolRegistry::new();
            register_tools(&mut registry, GazeReadOpts::default()).expect("register tools");
            Self {
                registry,
                auth: AllowAllAuth,
                manifest: Arc::new(RecordingManifest::new()),
                pipeline: crate::bundle::build_document_pipeline().expect("pipeline"),
                session: gaze::Session::new(gaze::Scope::Ephemeral).expect("session"),
                session_id_policy: SessionIdPolicy::default_strict(),
            }
        }

        async fn dispatch(
            &self,
            tool_name: &str,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, DispatchError> {
            let envelope = PiiEnvelope::new(
                &self.registry,
                &self.auth,
                self.manifest.as_ref(),
                &self.pipeline,
                &self.session,
                &[gaze::LocaleTag::Global],
                &self.session_id_policy,
            );
            envelope
                .dispatch(&Principal::new("unit-test"), tool_name, args, None)
                .await
                .map(|response| response.payload)
        }
    }

    fn assert_no_raw_fixture_values(clean_markdown: &str) {
        assert!(!clean_markdown.contains("Jane Doe"), "{clean_markdown}");
        assert!(!clean_markdown.contains("@example.com"), "{clean_markdown}");
        assert!(!clean_markdown.contains("555-0142"), "{clean_markdown}");
    }

    #[tokio::test]
    async fn read_text_dispatch_returns_clean_markdown_and_manifest_id() {
        let harness = Harness::new();
        let payload = harness
            .dispatch(
                "gaze_read_text",
                json!({
                    "text": "Bill to: Jane Doe\nEmail: jane.doe@example.com\nPhone: +1-555-0142"
                }),
            )
            .await
            .expect("dispatch succeeds");

        let clean_markdown = payload["clean_markdown"].as_str().expect("clean markdown");
        assert!(clean_markdown.contains(":Email_"), "{clean_markdown}");
        assert!(clean_markdown.contains(":Name_"), "{clean_markdown}");
        assert!(
            clean_markdown.contains(":Custom:phone_"),
            "{clean_markdown}"
        );
        assert_no_raw_fixture_values(clean_markdown);
        assert!(!payload["manifest_id"].as_str().unwrap().is_empty());
        assert_eq!(payload["file_metadata"]["source_kind"], "text");
        assert_eq!(
            payload["file_metadata"]["ocr_mean_confidence"],
            serde_json::Value::Null
        );
        assert_eq!(harness.manifest.begins.load(Ordering::SeqCst), 1);
        assert_eq!(harness.manifest.finishes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn read_file_missing_path_fails_closed_as_not_found() {
        let harness = Harness::new();
        let err = harness
            .dispatch(
                "gaze_read_file",
                json!({ "path": "testdata/does-not-exist.png" }),
            )
            .await
            .expect_err("missing file fails");

        match err {
            DispatchError::ToolError(ToolError::NotFound(message)) => {
                assert!(message.contains("not found"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(harness.manifest.failures.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn read_file_limit_fails_closed_before_ocr() {
        let mut registry = ToolRegistry::new();
        registry
            .register(GazeReadFile::with_max_file_size(1))
            .expect("register file tool");
        let harness = Harness {
            registry,
            auth: AllowAllAuth,
            manifest: Arc::new(RecordingManifest::new()),
            pipeline: crate::bundle::build_document_pipeline().expect("pipeline"),
            session: gaze::Session::new(gaze::Scope::Ephemeral).expect("session"),
            session_id_policy: SessionIdPolicy::default_strict(),
        };
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("synthetic_image.png");
        let err = harness
            .dispatch("gaze_read_file", json!({ "path": fixture }))
            .await
            .expect_err("oversized file fails");

        match err {
            DispatchError::ToolError(ToolError::LimitExceeded(message)) => {
                assert!(message.contains("configured cap is 1 bytes"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(feature = "ocr-tesseract")]
    #[tokio::test]
    async fn read_file_dispatch_returns_clean_markdown_for_fixture_when_backend_available() {
        let harness = Harness::new();
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("synthetic_image.png");
        let payload = match harness
            .dispatch("gaze_read_file", json!({ "path": fixture }))
            .await
        {
            Ok(payload) => payload,
            Err(DispatchError::ToolError(ToolError::BackendUnavailable(message))) => {
                eprintln!("SKIP: document backend unavailable: {message}");
                return;
            }
            Err(other) => panic!("unexpected dispatch error: {other:?}"),
        };

        let clean_markdown = payload["clean_markdown"].as_str().expect("clean markdown");
        assert!(clean_markdown.contains(":Email_"), "{clean_markdown}");
        assert!(clean_markdown.contains(":Name_"), "{clean_markdown}");
        assert!(
            clean_markdown.contains(":Custom:phone_"),
            "{clean_markdown}"
        );
        assert_no_raw_fixture_values(clean_markdown);
        assert_eq!(payload["file_metadata"]["source_kind"], "image");
        assert!(!payload["manifest_id"].as_str().unwrap().is_empty());
    }
}
