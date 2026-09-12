//! MCP tool adapters for `gaze-document`.
//!
//! Tools in this module are opt-in via the `mcp` feature and must be
//! registered explicitly by adopters. Invocation still happens through
//! `gaze_mcp_core::PiiEnvelope::dispatch`; this module only provides the
//! tool bodies and catalog metadata.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
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
            .with_output_schema(response_schema())
            .with_carriers(gaze_mcp_core::CarrierDeclaration::text_fields(&["text"]), response_carriers()),
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
            .with_output_schema(response_schema())
            .with_carriers(
                gaze_mcp_core::CarrierDeclaration::text_fields(&["path"]),
                response_carriers(),
            ),
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
        // Restore only inside the trusted tool, after protected manifest args
        // were recorded. Opening the token spelling can select a different file.
        let protected_path = required_string(ctx.redacted_args(), "path")?;
        let raw_path = restore_path(ctx.resources().session(), protected_path)?;
        let path = PathBuf::from(raw_path);
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

fn response_carriers() -> gaze_mcp_core::CarrierDeclaration {
    use gaze_mcp_core::CarrierSegment::Member;
    let path = |names: &[&str]| {
        names
            .iter()
            .map(|name| Member((*name).into()))
            .collect::<Vec<_>>()
    };
    let mut members = ["clean_markdown", "manifest_id", "file_metadata"]
        .iter()
        .map(|name| path(&[name]))
        .collect::<Vec<_>>();
    for name in [
        "source_kind",
        "ocr_mean_confidence",
        "bundle_version",
        "page_count",
    ] {
        members.push(path(&["file_metadata", name]));
    }
    let numbers = ["ocr_mean_confidence", "bundle_version", "page_count"]
        .iter()
        .map(|name| path(&["file_metadata", name]))
        .collect();
    gaze_mcp_core::CarrierDeclaration::new(members, numbers)
}

fn redact_document_text(text: &str, ctx: &ToolCtx<'_>) -> Result<String, ToolError> {
    let pipeline = crate::bundle::build_document_pipeline().map_err(map_document_error)?;
    // The envelope may have already protected arguments. Preserve the session's
    // exact owned tokens while applying the document-specific primary graph.
    let mut transaction = ctx.resources().session().begin_transaction();
    let clean = pipeline
        .protect_text_transaction(&mut transaction, text, ctx.resources().protection_context())
        .map_err(ToolError::internal)?;
    transaction.commit().map_err(ToolError::internal)?;
    Ok(clean)
}

/// Restore the `path` carrier of `gaze_read_file` to its raw form.
///
/// Arg protection only rewrites spans matched by the installed PII detectors
/// and already-known session tokens; ordinary non-PII filenames such as
/// `scan_1.png` pass through protection unchanged and are never registered as
/// tokens. Unlike `Session::restore_strict_text`, which fails closed on *any*
/// unowned token-shaped substring, this restores owned session tokens and only
/// fails closed when:
/// - the path contains a malformed token spelling (e.g. `<Email_>`,
///   `<deadbeef:Email_>`), or
/// - the path contains a nested-wrapper token spelling (e.g.
///   `<<deadbeef:Email_1>>`), or
/// - an unowned **non-bare** token spelling remains after restoration (e.g.
///   an injected `<deadbeef:Email_999>`, `<Email_1>`, or `email_1`).
///
/// Broad bare `<word>_<digits>` identifiers that are not session-issued and are
/// not built-in-class aliases (e.g. `scan_1`, `Scan_1`) are passed through to
/// file validation, preserving the `restore(protect(path)) == path` round-trip
/// for non-PII paths.
fn restore_path(session: &gaze::Session, protected_path: &str) -> Result<String, ToolError> {
    // Gate 1: reject malformed and nested-wrapper spellings before any
    // substitution or filesystem access.
    gaze::token_shape::validate_restore_shapes(protected_path)
        .map_err(|_| ToolError::InvalidArgs("path restoration failed".into()))?;

    // Gate 2: substitute known tokens and classify remaining shapes.
    // Bare identifiers (e.g. `scan_1`) are audit-only; other unowned shapes
    // (e.g. `<Email_1>`, `email_1`, `<deadbeef:Email_999>`) are rejected.
    let assessment = session
        .assess_restore_text(protected_path)
        .map_err(|_| ToolError::InvalidArgs("path restoration failed".into()))?;
    if let Some(unknown) = assessment.unknown_tokens().first() {
        let _ = unknown; // error message is constant to avoid leaking token content
        return Err(ToolError::InvalidArgs("path restoration failed".into()));
    }
    Ok(assessment.into_restored().text)
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
        args: std::sync::Mutex<Vec<serde_json::Value>>,
    }

    impl RecordingManifest {
        fn new() -> Self {
            Self {
                begins: AtomicUsize::new(0),
                finishes: AtomicUsize::new(0),
                failures: AtomicUsize::new(0),
                args: Default::default(),
            }
        }
    }

    #[async_trait]
    impl ManifestStore for RecordingManifest {
        async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
            self.begins.fetch_add(1, Ordering::SeqCst);
            self.args.lock().unwrap().push(ctx.redacted_args.clone());
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
        assert!(
            !clean_markdown.contains("@example.invalid"),
            "{clean_markdown}"
        );
        assert!(!clean_markdown.contains("555-0142"), "{clean_markdown}");
    }

    #[tokio::test]
    async fn read_text_dispatch_returns_clean_markdown_and_manifest_id() {
        let harness = Harness::new();
        let payload = harness
            .dispatch(
                "gaze_read_text",
                json!({
                    "text": "Bill to: Jane Doe\nEmail: jane.doe@example.invalid\nPhone: +1-555-0142"
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

    #[tokio::test]
    async fn configured_dispatch_restores_owner_path_before_file_validation() {
        let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
        let session = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let raw_path = directory
            .path()
            .join("alice@example.invalid")
            .join("input.png");
        std::fs::create_dir_all(raw_path.parent().unwrap()).unwrap();
        std::fs::write(&raw_path, b"oversized").unwrap();
        let gaze::CleanDocument::Text(protected_path) = core
            .pseudonymize_text(&session, raw_path.to_str().unwrap())
            .unwrap()
        else {
            panic!("text expected")
        };
        assert_ne!(protected_path, raw_path.to_str().unwrap());
        // A distinct literal-token file must never replace the owner's target.
        let literal_path = PathBuf::from(&protected_path);
        assert!(literal_path.starts_with(directory.path()));
        std::fs::create_dir_all(literal_path.parent().unwrap()).unwrap();
        std::fs::write(&literal_path, b"").unwrap();
        let mut registry = ToolRegistry::new();
        registry
            .register(GazeReadFile::with_max_file_size(1))
            .unwrap();
        let manifest = RecordingManifest::new();
        let policy = SessionIdPolicy::default_strict();
        let envelope = PiiEnvelope::new(
            &registry,
            &AllowAllAuth,
            &manifest,
            core.pipeline(),
            &session,
            core.locale_chain().as_slice(),
            &policy,
        );
        for path in [raw_path.to_str().unwrap(), &protected_path] {
            let err = envelope
                .dispatch(
                    &Principal::new("unit-test"),
                    "gaze_read_file",
                    json!({"path": path}),
                    None,
                )
                .await
                .unwrap_err();
            assert!(
                matches!(err, DispatchError::ToolError(ToolError::LimitExceeded(_))),
                "{err:?}"
            );
        }
        for args in manifest.args.lock().unwrap().iter() {
            let path = args["path"].as_str().unwrap();
            assert!(!path.contains("alice@example.invalid"));
            assert_eq!(
                session.restore_strict_text(path).unwrap(),
                raw_path.to_str().unwrap()
            );
        }
        let unknown_path = directory
            .path()
            .join("<deadbeef:Email_999>")
            .join("input.png");
        std::fs::create_dir_all(unknown_path.parent().unwrap()).unwrap();
        std::fs::write(&unknown_path, b"oversized").unwrap();
        let err = envelope
            .dispatch(
                &Principal::new("unit-test"),
                "gaze_read_file",
                json!({"path": unknown_path}),
                None,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, DispatchError::ToolError(ToolError::InvalidArgs(_))),
            "{err:?}"
        );
        // Ordinary bare `<word>_<digits>` filenames were previously rejected at
        // the restore gate with `InvalidArgs`; they must now reach `validate_file`
        // and surface `NotFound` for non-existent paths. Run one lowercase- and
        // one capital-initial shape under the production-equivalent core rulepack
        // (28 recognizers) to confirm none rewrites the shape before the gate.
        for bare in ["scan_1.png", "Scan_1.png"] {
            let path = directory.path().join(bare);
            assert!(!path.exists(), "fixture `{bare}` must not exist");
            let err = envelope
                .dispatch(
                    &Principal::new("unit-test"),
                    "gaze_read_file",
                    json!({ "path": path }),
                    None,
                )
                .await
                .unwrap_err();
            assert!(
                matches!(err, DispatchError::ToolError(ToolError::NotFound(_))),
                "bare-shape `{bare}` must reach validate_file, got: {err:?}",
            );
        }
        // Every dispatch in this test failed before `finish_call`.
        assert_eq!(manifest.finishes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn read_file_dispatch_rejects_unowned_session_prefixed_token_in_path() {
        let harness = Harness::new();
        // The realistic attack vector — an injected unowned session-prefixed
        // token that a different session's manifest authorizes — must still be
        // blocked at the restore gate before any filesystem access.
        let err = harness
            .dispatch(
                "gaze_read_file",
                json!({ "path": "directory/<deadbeef:Email_999>/input.png" }),
            )
            .await
            .expect_err("unowned session-prefixed token must fail at the restore gate");
        match err {
            DispatchError::ToolError(ToolError::InvalidArgs(message)) => {
                assert_eq!(message, "path restoration failed");
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(harness.manifest.failures.load(Ordering::SeqCst), 1);
        assert_eq!(harness.manifest.finishes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn read_file_dispatch_passes_ordinary_bare_shape_filenames_to_validation() {
        let harness = Harness::new();
        let directory = tempfile::tempdir().unwrap();

        // Ordinary filenames containing a bare `<word>_<digits>` shape were
        // previously rejected at the restore gate with `InvalidArgs`. They must
        // now reach `validate_file` and surface `NotFound` for non-existent
        // paths. Covers the lowercase- and capital-initial regex alternations
        // plus a long-ordinal shape; `doc_v2.png` is a near-miss (underscore
        // followed by `v`, not a digit) that already worked and must not regress.
        for bare in [
            "scan_1.png",
            "Scan_1.png",
            "invoice_20250111.pdf",
            "doc_v2.png",
        ] {
            let path = directory.path().join(bare);
            assert!(!path.exists(), "fixture `{bare}` must not exist");
            let err = harness
                .dispatch("gaze_read_file", json!({ "path": path }))
                .await
                .expect_err("path must reach validate_file, not the restore gate");
            match err {
                DispatchError::ToolError(ToolError::NotFound(message)) => {
                    assert!(
                        message.contains(bare),
                        "NotFound for `{bare}` should mention the filename: {message}"
                    );
                }
                other => panic!("unexpected error for `{bare}`: {other:?}"),
            }
        }

        // Every dispatch failed at `validate_file` (NotFound), none at the
        // restore gate, so no call reached `finish_call`.
        assert_eq!(harness.manifest.finishes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn read_file_dispatch_rejects_malformed_session_prefixed_token_in_path() {
        let harness = Harness::new();
        // `<deadbeef:Email_>` has a missing ordinal — the malformed-token gate
        // must reject it before any filesystem access, regardless of whether a
        // session-prefixed prefix is present.
        for malformed in [
            "directory/<deadbeef:Email_>/input.png",
            "path/<Email_>/file.pdf",
            "/tmp/<deadbeef:Name_>/doc.png",
        ] {
            let err = harness
                .dispatch("gaze_read_file", json!({ "path": malformed }))
                .await
                .expect_err("malformed token in path must fail at the restore gate");
            assert!(
                matches!(
                    err,
                    DispatchError::ToolError(ToolError::InvalidArgs(ref msg))
                    if msg == "path restoration failed"
                ),
                "malformed `{malformed}` must return InvalidArgs, got: {err:?}",
            );
        }
        assert_eq!(harness.manifest.finishes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn read_file_dispatch_rejects_nested_wrapper_token_in_path() {
        let harness = Harness::new();
        // `<<deadbeef:Email_1>>` wraps a valid token-shaped match in an extra
        // layer of angle brackets; the nested-wrapper gate must reject it before
        // any filesystem access.
        for nested in [
            "directory/<<deadbeef:Email_1>>/input.png",
            "path/<<Email_1>>/file.pdf",
        ] {
            let err = harness
                .dispatch("gaze_read_file", json!({ "path": nested }))
                .await
                .expect_err("nested-wrapper token in path must fail at the restore gate");
            assert!(
                matches!(
                    err,
                    DispatchError::ToolError(ToolError::InvalidArgs(ref msg))
                    if msg == "path restoration failed"
                ),
                "nested `{nested}` must return InvalidArgs, got: {err:?}",
            );
        }
        assert_eq!(harness.manifest.finishes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn read_file_dispatch_rejects_unowned_wrapped_and_legacy_placeholders() {
        let harness = Harness::new();
        // Unowned wrapped (`<Email_1>`) and legacy (`email_1`) placeholders are
        // not bare identifiers and must be rejected at the restore gate rather
        // than passed through to filesystem access.
        for unowned in [
            "<Email_1>/input.png",
            "directory/<Email_1>/input.png",
            "email_1/input.png",
        ] {
            let err = harness
                .dispatch("gaze_read_file", json!({ "path": unowned }))
                .await
                .expect_err("unowned trap placeholder in path must fail at the restore gate");
            assert!(
                matches!(
                    err,
                    DispatchError::ToolError(ToolError::InvalidArgs(ref msg))
                    if msg == "path restoration failed"
                ),
                "unowned trap `{unowned}` must return InvalidArgs, got: {err:?}",
            );
        }
        assert_eq!(harness.manifest.finishes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn read_file_dispatch_restores_owned_token_alongside_negative_cases() {
        // Owned token must restore to raw PII; negative cases must still be
        // blocked. This test covers both paths in the same session to confirm
        // the assessment and malformed/nested gates interact correctly.
        let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
        let session = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let raw_path = directory
            .path()
            .join("alice@example.invalid")
            .join("report.pdf");
        std::fs::create_dir_all(raw_path.parent().unwrap()).unwrap();
        std::fs::write(&raw_path, b"dummy").unwrap();
        let gaze::CleanDocument::Text(protected_path) = core
            .pseudonymize_text(&session, raw_path.to_str().unwrap())
            .unwrap()
        else {
            panic!("text expected")
        };
        assert_ne!(protected_path, raw_path.to_str().unwrap());

        let mut registry = ToolRegistry::new();
        registry
            .register(GazeReadFile::with_max_file_size(1))
            .unwrap();
        let manifest = RecordingManifest::new();
        let policy = SessionIdPolicy::default_strict();
        let envelope = PiiEnvelope::new(
            &registry,
            &AllowAllAuth,
            &manifest,
            core.pipeline(),
            &session,
            core.locale_chain().as_slice(),
            &policy,
        );

        // Owned token restores and reaches validate_file (LimitExceeded).
        let err = envelope
            .dispatch(
                &Principal::new("unit-test"),
                "gaze_read_file",
                json!({"path": protected_path}),
                None,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, DispatchError::ToolError(ToolError::LimitExceeded(_))),
            "owned-token path must reach validate_file: {err:?}",
        );

        // Malformed token in path is rejected at the restore gate.
        let malformed = format!("{}/file.pdf", directory.path().join("<deadbeef:Email_>").display());
        let err = envelope
            .dispatch(
                &Principal::new("unit-test"),
                "gaze_read_file",
                json!({"path": malformed}),
                None,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, DispatchError::ToolError(ToolError::InvalidArgs(_))),
            "malformed token in path must be rejected: {err:?}",
        );

        // Unowned wrapped placeholder is rejected at the restore gate.
        let unowned_wrapped = format!("{}/file.pdf", directory.path().join("<Email_1>").display());
        let err = envelope
            .dispatch(
                &Principal::new("unit-test"),
                "gaze_read_file",
                json!({"path": unowned_wrapped}),
                None,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, DispatchError::ToolError(ToolError::InvalidArgs(_))),
            "unowned wrapped placeholder must be rejected: {err:?}",
        );
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
