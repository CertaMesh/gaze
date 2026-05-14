//! SafeBundle generation: OCR + Gaze redact → on-disk artifacts.
//!
//! The top-level [`clean`] function is the public adopter entry point. It
//! routes any supported input (PNG / JPG / single-page PDF) through OCR,
//! pipes the extracted text through a [`gaze::Pipeline`], and persists the
//! result as three files in a target directory:
//!
//! ```text
//! out/
//!   clean.md        # OCR text with PII replaced by reversible tokens
//!   manifest.json   # gaze::Manifest — restorable, canonical
//!   report.json     # BundleReport — OCR + PII counts + provenance
//! ```
//!
//! The manifest contract is the same one the rest of the gaze runtime
//! uses (`gaze::Manifest`). Adopters can pair `clean.md` with `manifest.json`
//! and restore via the standard gaze session APIs.

use std::path::PathBuf;

use gaze::Manifest;
use serde::{Deserialize, Serialize};

use crate::ocr::{
    detect_image_format, ImageFormat, ImageInput, OcrBackend, OcrError, OcrHints, OcrResult,
};

#[cfg(feature = "ocr-tesseract")]
use std::collections::BTreeMap;
#[cfg(feature = "ocr-tesseract")]
use std::fs;
#[cfg(feature = "ocr-tesseract")]
use std::path::Path;

#[cfg(any(feature = "ocr-tesseract", feature = "mcp"))]
use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, LocaleTag, Pipeline as GazePipeline,
    RawDocument, Scope, Session,
};
#[cfg(any(feature = "ocr-tesseract", feature = "mcp"))]
use gaze_recognizers::{
    AnchoredBoundary, AnchoredMatchRecognizer, CuePosition, NameShape, RegexDetector,
};
#[cfg(any(feature = "ocr-tesseract", feature = "mcp"))]
use gaze_types::{EmittedTokenSpan, PiiClass};

#[cfg(feature = "ocr-tesseract")]
use crate::extract::InputKind;
#[cfg(feature = "ocr-tesseract")]
use crate::DocumentError;

/// Versioned `report.json` schema tag (bump on breaking shape changes).
pub const BUNDLE_VERSION: u32 = 2;
const DEFAULT_LOW_CONFIDENCE_THRESHOLD: f32 = 0.65;

/// Bundle filename written into `--out` for tokenized Markdown.
pub const CLEAN_MARKDOWN_FILE: &str = "clean.md";
/// Bundle filename written into `--out` for the restorable manifest.
pub const MANIFEST_FILE: &str = "manifest.json";
/// Bundle filename written into `--out` for the OCR + PII provenance report.
pub const REPORT_FILE: &str = "report.json";

/// Post-ingestion artifact paired with a Gaze [`Manifest`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SafeBundle {
    /// Tokenized Markdown safe to hand to an LLM.
    pub clean_markdown: String,
    /// Reversible manifest produced by the gaze pipeline.
    pub manifest: Manifest,
    /// Opaque layout summary (reserved — single-page in v0.0.x).
    pub layout: LayoutSummary,
    /// Optional rasterized preview of the source document (reserved).
    pub preview_png: Option<Vec<u8>>,
    /// Per-bundle audit + provenance report.
    pub report: BundleReport,
    /// Absolute path of the input that produced this bundle.
    pub source_path: PathBuf,
    /// Absolute path of the output directory that received this bundle.
    pub out_dir: PathBuf,
}

impl SafeBundle {
    /// Build a [`SafeBundle`] from its component parts.
    pub fn new(
        clean_markdown: String,
        manifest: Manifest,
        layout: LayoutSummary,
        preview_png: Option<Vec<u8>>,
        report: BundleReport,
        source_path: PathBuf,
        out_dir: PathBuf,
    ) -> Self {
        Self {
            clean_markdown,
            manifest,
            layout,
            preview_png,
            report,
            source_path,
            out_dir,
        }
    }
}

/// Per-class PII detection count for [`BundleReport`].
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClassCount {
    /// Audit-canonical class name (e.g., `"email"`, `"custom:phone"`).
    pub class: String,
    /// Number of token spans emitted for that class.
    pub count: u32,
}

impl ClassCount {
    /// Build a class-count entry.
    pub fn new(class: impl Into<String>, count: u32) -> Self {
        Self {
            class: class.into(),
            count,
        }
    }
}

/// Per-page extraction source.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OcrSource {
    /// Selectable text extracted directly from a PDF page.
    VectorPdf,
    /// Raster OCR from an image page.
    Ocr,
}

/// Per-page OCR/layout provenance.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageReport {
    /// Zero-based page index.
    pub page_index: i32,
    /// Extraction path used for this page.
    pub ocr_source: OcrSource,
    /// OCR backend name when [`OcrSource::Ocr`] produced the page.
    pub ocr_backend: Option<String>,
    /// Aggregated page confidence in `0.0..=1.0`. `None` for vector PDF text.
    pub confidence: Option<f32>,
    /// True when confidence is present and below the configured threshold.
    pub low_confidence: bool,
    /// Detected text column count. `1` means single-column.
    pub column_count: u32,
    /// Number of OCR words with confidence for this page.
    pub ocr_word_count: usize,
    /// Legacy percent-scale mean confidence for this page.
    pub ocr_mean_confidence: Option<f32>,
}

impl PageReport {
    fn new(
        page_index: i32,
        ocr_source: OcrSource,
        ocr_backend: Option<String>,
        ocr: &OcrResult,
        column_count: u32,
        low_confidence_threshold: f32,
    ) -> Self {
        let confidence = ocr.mean_confidence_unit();
        Self {
            page_index,
            ocr_source,
            ocr_backend,
            confidence,
            low_confidence: confidence
                .map(|confidence| confidence < low_confidence_threshold)
                .unwrap_or(false),
            column_count,
            ocr_word_count: ocr.word_count,
            ocr_mean_confidence: ocr.mean_confidence,
        }
    }
}

/// Bundle audit + provenance report serialized to `report.json`.
///
/// Schema versioned via [`BUNDLE_VERSION`]; older readers can branch on the
/// `bundle_version` field. Field set is `#[non_exhaustive]` so additive
/// extensions are SemVer-safe.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleReport {
    /// Schema version (currently [`BUNDLE_VERSION`]).
    pub bundle_version: u32,
    /// Input kind detected from the source path.
    pub input_kind: String,
    /// Mean per-word Tesseract confidence (0..100). `None` when zero words.
    pub ocr_mean_confidence: Option<f32>,
    /// Number of words Tesseract emitted with non-negative confidence.
    pub ocr_word_count: usize,
    /// Tesseract language code used for OCR (e.g., `"eng"`).
    pub ocr_lang: String,
    /// Character count of the tokenized Markdown output.
    pub clean_char_count: usize,
    /// Total PII token spans across all classes.
    pub pii_token_count: u32,
    /// Per-class breakdown of PII token counts.
    pub pii_tokens_by_class: Vec<ClassCount>,
    /// PDF page count when the input was a PDF. `None` for image inputs.
    pub pdf_page_count: Option<i32>,
    /// PDF page index that was rasterized. `None` for image inputs.
    pub pdf_page_index: Option<i32>,
    /// Per-page extraction, confidence, and layout provenance.
    #[serde(default)]
    pub pages: Vec<PageReport>,
    /// Confidence threshold used to set [`PageReport::low_confidence`].
    #[serde(default = "default_low_confidence_threshold")]
    pub low_confidence_threshold: f32,
}

impl BundleReport {
    /// Build a [`BundleReport`] from its component parts.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        input_kind: impl Into<String>,
        ocr: &OcrResult,
        clean_char_count: usize,
        pii_token_count: u32,
        pii_tokens_by_class: Vec<ClassCount>,
        pdf_page_count: Option<i32>,
        pdf_page_index: Option<i32>,
        pages: Vec<PageReport>,
        low_confidence_threshold: f32,
    ) -> Self {
        Self {
            bundle_version: BUNDLE_VERSION,
            input_kind: input_kind.into(),
            ocr_mean_confidence: ocr.mean_confidence,
            ocr_word_count: ocr.word_count,
            ocr_lang: ocr.lang.clone(),
            clean_char_count,
            pii_token_count,
            pii_tokens_by_class,
            pdf_page_count,
            pdf_page_index,
            pages,
            low_confidence_threshold,
        }
    }
}

fn default_low_confidence_threshold() -> f32 {
    DEFAULT_LOW_CONFIDENCE_THRESHOLD
}

/// Configurable document-cleaning pipeline.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct Pipeline {
    low_confidence_threshold: f32,
    column_detection: bool,
}

impl Pipeline {
    /// Build a pipeline with conservative defaults.
    pub fn new() -> Self {
        Self {
            low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            column_detection: true,
        }
    }

    /// Override the per-page low-confidence threshold.
    pub fn with_low_confidence_threshold(mut self, threshold: f32) -> Self {
        self.low_confidence_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Enable or disable multi-column OCR span ordering.
    pub fn with_column_detection(mut self, enabled: bool) -> Self {
        self.column_detection = enabled;
        self
    }

    /// Clean one document with an adopter-supplied OCR backend.
    ///
    /// # Errors
    /// Returns [`DocumentError`] for any extraction, OCR, redaction, or write failure.
    #[cfg(feature = "ocr-tesseract")]
    #[cfg_attr(docsrs, doc(cfg(feature = "ocr-tesseract")))]
    pub fn clean_with_ocr_backend(
        &self,
        input: &Path,
        out_dir: &Path,
        ocr_backend: &dyn OcrBackend,
    ) -> Result<SafeBundle, DocumentError> {
        clean_with_options(input, out_dir, ocr_backend, *self)
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Opaque layout summary placeholder.
///
/// Reserved until the multi-page + reading-order PR lands. Construction
/// records only the page count surfaced by the input layer.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct LayoutSummary {
    /// Number of pages handed to the OCR pass (always `1` in v0.0.x).
    pub page_count: u32,
}

impl LayoutSummary {
    /// Build a single-page layout summary.
    pub fn single_page() -> Self {
        Self { page_count: 1 }
    }

    /// Build a layout summary with an explicit page count.
    pub fn new(page_count: u32) -> Self {
        Self { page_count }
    }
}

/// Top-level entry point: ingest one document, write a [`SafeBundle`] to disk.
///
/// `input` must be a regular file with extension `.png`, `.jpg`, `.jpeg`, or
/// `.pdf`. `out_dir` is created if missing and populated with three files
/// (see module docs).
///
/// # Errors
///
/// Returns [`DocumentError`] for any failure in the OCR → redact → write
/// chain. Fail-closed: every error variant carries enough context to
/// diagnose without inspecting partial bundle state.
#[cfg(feature = "ocr-tesseract")]
#[cfg_attr(docsrs, doc(cfg(feature = "ocr-tesseract")))]
pub fn clean(input: &Path, out_dir: &Path) -> Result<SafeBundle, DocumentError> {
    let backend = crate::ocr::TesseractBackend::new();
    Pipeline::new().clean_with_ocr_backend(input, out_dir, &backend)
}

/// Top-level entry point with an adopter-supplied OCR backend.
///
/// The backend receives finalized single-image bytes. PDF rasterization and
/// downstream layout/report generation remain owned by `gaze-document`.
///
/// # Errors
///
/// Returns [`DocumentError`] for any failure in the OCR → redact → write
/// chain. OCR backend errors are mapped into the existing document error
/// surface so current callers keep the same high-level behavior.
#[cfg(feature = "ocr-tesseract")]
#[cfg_attr(docsrs, doc(cfg(feature = "ocr-tesseract")))]
pub fn clean_with_ocr_backend(
    input: &Path,
    out_dir: &Path,
    ocr_backend: &dyn OcrBackend,
) -> Result<SafeBundle, DocumentError> {
    Pipeline::new().clean_with_ocr_backend(input, out_dir, ocr_backend)
}

#[cfg(feature = "ocr-tesseract")]
fn clean_with_options(
    input: &Path,
    out_dir: &Path,
    ocr_backend: &dyn OcrBackend,
    options: Pipeline,
) -> Result<SafeBundle, DocumentError> {
    let kind = InputKind::detect(input)?;
    let absolute_input = absolutize(input);
    let absolute_out = absolutize(out_dir);

    fs::create_dir_all(out_dir)
        .map_err(|err| DocumentError::OutputDir(absolute_out.clone(), err))?;

    let extraction = run_document_extraction(input, kind, ocr_backend, options)?;
    // Repair known narrow OCR artifacts (e.g. spurious whitespace around
    // `@` in emails) before the redact pipeline sees the text. See
    // `crate::ocr::normalize` for the documented rule set. Axis 1
    // (never leak) requires this — the OCR pass occasionally inserts a
    // single space inside an email that would otherwise slip past strict
    // recognizers and survive into clean.md.
    let normalized_text = crate::ocr::normalize_ocr_artifacts(&extraction.ocr_result.text);
    let pipeline = build_document_pipeline()?;
    let session = Session::new(Scope::Ephemeral).map_err(|err| pipeline_err("session", err))?;
    let locale_chain = [LocaleTag::Global];
    let (clean_doc, spans, _leak_report) = pipeline
        .clean_with_safety_net(&session, RawDocument::Text(normalized_text), &locale_chain)
        .map_err(|err| pipeline_err("redact", err))?;

    let clean_text = match clean_doc {
        CleanDocument::Text(text) => text,
        _ => {
            return Err(DocumentError::Pipeline(
                "pipeline returned non-text variant for text input".to_string(),
            ));
        }
    };

    let manifest = Manifest::from_spans(spans.clone());
    let counts = count_pii_by_class(&spans);
    let pii_token_count: u32 = counts.iter().map(|c| c.count).sum();

    let report = BundleReport::new(
        kind_label(kind),
        &extraction.ocr_result,
        clean_text.chars().count(),
        pii_token_count,
        counts,
        extraction.pdf_page_count,
        extraction.pdf_page_index,
        extraction.pages,
        options.low_confidence_threshold,
    );

    let clean_markdown = format_clean_markdown(&clean_text, kind);
    write_bundle(out_dir, &clean_markdown, &manifest, &report)?;

    Ok(SafeBundle::new(
        clean_markdown,
        manifest,
        LayoutSummary::new(extraction.page_count),
        None,
        report,
        absolute_input,
        absolute_out,
    ))
}

#[cfg(feature = "ocr-tesseract")]
struct DocumentExtraction {
    ocr_result: OcrResult,
    pdf_page_count: Option<i32>,
    pdf_page_index: Option<i32>,
    pages: Vec<PageReport>,
    page_count: u32,
}

#[cfg(feature = "ocr-tesseract")]
#[cfg_attr(not(feature = "mcp"), allow(dead_code))]
pub(crate) fn run_ocr(
    input: &Path,
    kind: InputKind,
    ocr_backend: &dyn OcrBackend,
) -> Result<(OcrResult, Option<i32>, Option<i32>), DocumentError> {
    let extraction = run_document_extraction(input, kind, ocr_backend, Pipeline::new())?;
    Ok((
        extraction.ocr_result,
        extraction.pdf_page_count,
        extraction.pdf_page_index,
    ))
}

#[cfg(feature = "ocr-tesseract")]
fn run_document_extraction(
    input: &Path,
    kind: InputKind,
    ocr_backend: &dyn OcrBackend,
    options: Pipeline,
) -> Result<DocumentExtraction, DocumentError> {
    match kind {
        InputKind::Png | InputKind::Jpeg => {
            let bytes = fs::read(input)?;
            let format = detect_image_format(&bytes)?;
            let (result, column_count) = recognize_image(
                ocr_backend,
                ImageInput {
                    bytes,
                    format,
                    dpi: None,
                },
                options.column_detection,
            )?;
            let page_report = PageReport::new(
                0,
                OcrSource::Ocr,
                Some(ocr_backend.name().to_string()),
                &result,
                column_count,
                options.low_confidence_threshold,
            );
            Ok(DocumentExtraction {
                ocr_result: result,
                pdf_page_count: None,
                pdf_page_index: None,
                pages: vec![page_report],
                page_count: 1,
            })
        }
        InputKind::Pdf => {
            #[cfg(feature = "pdf-input")]
            {
                use crate::extract::pdf::{extract_pages, PdfPagePayload, PdfRasterConfig};
                let payloads = extract_pages(input, PdfRasterConfig::new())?;
                let mut page_results = Vec::with_capacity(payloads.len());
                let mut pages = Vec::with_capacity(payloads.len());
                let mut pdf_page_count = None;
                let mut first_page_index = None;

                for payload in payloads {
                    pdf_page_count = Some(payload.page_count());
                    if first_page_index.is_none() {
                        first_page_index = Some(payload.page_index());
                    }
                    match payload {
                        PdfPagePayload::VectorText {
                            text, page_index, ..
                        } => {
                            let result = OcrResult::new(text, None, 0, "vector-pdf".to_string());
                            pages.push(PageReport::new(
                                page_index,
                                OcrSource::VectorPdf,
                                None,
                                &result,
                                1,
                                options.low_confidence_threshold,
                            ));
                            page_results.push(result);
                        }
                        PdfPagePayload::Raster(raster) => {
                            let (result, column_count) = recognize_image(
                                ocr_backend,
                                ImageInput {
                                    bytes: raster.png_bytes,
                                    format: ImageFormat::Png,
                                    dpi: None,
                                },
                                options.column_detection,
                            )?;
                            pages.push(PageReport::new(
                                raster.page_index,
                                OcrSource::Ocr,
                                Some(ocr_backend.name().to_string()),
                                &result,
                                column_count,
                                options.low_confidence_threshold,
                            ));
                            page_results.push(result);
                        }
                    }
                }

                Ok(DocumentExtraction {
                    ocr_result: merge_page_results(&page_results),
                    pdf_page_count,
                    pdf_page_index: first_page_index,
                    page_count: pages.len() as u32,
                    pages,
                })
            }
            #[cfg(not(feature = "pdf-input"))]
            {
                Err(DocumentError::UnsupportedInput {
                    path: input.to_path_buf(),
                    reason: "rebuild gaze-document with `--features pdf-input` for PDF support",
                })
            }
        }
    }
}

#[cfg(feature = "ocr-tesseract")]
fn recognize_image(
    ocr_backend: &dyn OcrBackend,
    image: ImageInput,
    column_detection: bool,
) -> Result<(OcrResult, u32), DocumentError> {
    let hints = OcrHints::default();
    let lang = hints.primary_language().to_string();
    let image = crate::preprocess::preprocess_image(image);
    let spans = ocr_backend
        .recognize(image, hints)
        .map_err(map_ocr_error_to_document_error)?;
    Ok(OcrResult::from_spans_with_column_detection(
        &spans,
        lang,
        column_detection,
    ))
}

#[cfg(feature = "ocr-tesseract")]
fn merge_page_results(results: &[OcrResult]) -> OcrResult {
    let text = results
        .iter()
        .map(|result| result.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut conf_sum = 0.0f64;
    let mut conf_count = 0usize;
    for result in results {
        if let Some(confidence) = result.mean_confidence {
            conf_sum += confidence as f64 * result.word_count as f64;
            conf_count += result.word_count;
        }
    }
    let mean_confidence = if conf_count == 0 {
        None
    } else {
        Some((conf_sum / conf_count as f64) as f32)
    };
    OcrResult::new(text, mean_confidence, conf_count, "mixed".to_string())
}

#[cfg(feature = "ocr-tesseract")]
fn map_ocr_error_to_document_error(err: OcrError) -> DocumentError {
    match err {
        OcrError::InitFailed(hint) => DocumentError::TesseractNotFound(hint),
        OcrError::RecognizeFailed(detail) => DocumentError::TesseractFailed {
            status: -1,
            stderr: detail,
        },
        OcrError::UnsupportedFormat(format) => DocumentError::UnsupportedInput {
            path: PathBuf::new(),
            reason: match format {
                ImageFormat::Png => "png image format is not supported by the OCR backend",
                ImageFormat::Jpeg => "jpeg image format is not supported by the OCR backend",
                ImageFormat::Tiff => "tiff image format is not supported by the OCR backend",
            },
        },
        OcrError::Internal(detail) => DocumentError::Pipeline(format!("ocr: {detail}")),
    }
}

#[cfg(feature = "ocr-tesseract")]
#[cfg(any(feature = "ocr-tesseract", feature = "mcp"))]
pub(crate) fn build_document_pipeline() -> Result<GazePipeline, DocumentError> {
    let email = RegexDetector::emails().map_err(|err| pipeline_err("email-regex", err))?;
    // Conservative phone pattern: optional `+CC`, area, exchange, line, with
    // common separators. Synthetic fixture uses `+1-555-0142`-style numbers.
    let phone = RegexDetector::new(
        r"\+?\d{1,3}[-.\s]\(?\d{3}\)?[-.\s]?\d{3,4}[-.\s]?\d{0,4}",
        PiiClass::custom("phone"),
    )
    .map_err(|err| pipeline_err("phone-regex", err))?;
    // Invoice / shipping recipient block names. Scope is intentionally
    // local to gaze-document (rather than extending the locale-en
    // `forward_markers` bucket): forwarded-email cues and document
    // recipient blocks are semantically distinct anchors and should not
    // share a bucket. `LineEnd` boundary stops the name span at the
    // newline that ends the recipient line so a follow-up `Email:` row
    // cannot be absorbed into the Name match.
    let recipient_name = AnchoredMatchRecognizer::new(
        "gaze_document.name.recipient".to_string(),
        vec![
            "Bill to".to_string(),
            "Invoice to".to_string(),
            "Ship to".to_string(),
            "Attention".to_string(),
            "Attn".to_string(),
        ],
        AnchoredBoundary::LineEnd,
        48,
        NameShape::PersonName,
        CuePosition::Before,
        "invoice_recipient".to_string(),
        2,
        0.88,
        110,
    );
    GazePipeline::builder()
        .detector(email)
        .detector(phone)
        .recognizer(recipient_name)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(PiiClass::custom("phone"), Action::Tokenize))
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .map_err(|err| pipeline_err("build", err))
}

#[cfg(feature = "ocr-tesseract")]
fn count_pii_by_class(spans: &[EmittedTokenSpan]) -> Vec<ClassCount> {
    let mut by_class: BTreeMap<String, u32> = BTreeMap::new();
    for span in spans {
        *by_class.entry(span.class.to_canonical_str()).or_insert(0) += 1;
    }
    by_class
        .into_iter()
        .map(|(class, count)| ClassCount::new(class, count))
        .collect()
}

#[cfg(feature = "ocr-tesseract")]
fn write_bundle(
    out_dir: &Path,
    clean_markdown: &str,
    manifest: &Manifest,
    report: &BundleReport,
) -> Result<(), DocumentError> {
    fs::write(out_dir.join(CLEAN_MARKDOWN_FILE), clean_markdown)?;
    let manifest_json = serde_json::to_vec_pretty(manifest)?;
    fs::write(out_dir.join(MANIFEST_FILE), manifest_json)?;
    let report_json = serde_json::to_vec_pretty(report)?;
    fs::write(out_dir.join(REPORT_FILE), report_json)?;
    Ok(())
}

#[cfg(feature = "ocr-tesseract")]
pub(crate) fn format_clean_markdown(text: &str, kind: InputKind) -> String {
    let mut out = String::new();
    out.push_str("# gaze-document safe bundle\n\n");
    out.push_str(&format!("Source kind: `{}`\n\n", kind_label(kind)));
    out.push_str("---\n\n");
    out.push_str(text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[cfg(feature = "ocr-tesseract")]
pub(crate) fn kind_label(kind: InputKind) -> &'static str {
    match kind {
        InputKind::Png => "png",
        InputKind::Jpeg => "jpeg",
        InputKind::Pdf => "pdf",
    }
}

#[cfg(feature = "ocr-tesseract")]
fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

#[cfg(feature = "ocr-tesseract")]
fn pipeline_err(stage: &'static str, err: impl std::fmt::Display) -> DocumentError {
    DocumentError::Pipeline(format!("{stage}: {err}"))
}

#[cfg(all(test, feature = "ocr-tesseract"))]
mod tests {
    use super::*;
    use crate::ocr::{BBox, OcrSpan};

    #[derive(Debug)]
    struct MockBackend {
        spans: Vec<OcrSpan>,
    }

    impl OcrBackend for MockBackend {
        fn name(&self) -> &str {
            "mock-ocr"
        }

        fn recognize(
            &self,
            _image: ImageInput,
            _hints: OcrHints,
        ) -> Result<Vec<OcrSpan>, OcrError> {
            Ok(self.spans.clone())
        }
    }

    fn span(text: &str, x: u32, y: u32, confidence: f32) -> OcrSpan {
        OcrSpan {
            text: text.to_string(),
            bbox: BBox { x, y, w: 90, h: 16 },
            confidence: Some(confidence),
        }
    }

    #[test]
    fn count_pii_by_class_groups_email_and_phone() {
        let spans = vec![
            EmittedTokenSpan::new(0..10, 0..10, PiiClass::Email),
            EmittedTokenSpan::new(20..28, 20..28, PiiClass::Email),
            EmittedTokenSpan::new(40..50, 40..50, PiiClass::custom("phone")),
        ];
        let counts = count_pii_by_class(&spans);
        assert_eq!(counts.len(), 2);
        let by_class: BTreeMap<_, _> = counts.iter().map(|c| (c.class.as_str(), c.count)).collect();
        assert_eq!(by_class.get("email"), Some(&2));
        assert_eq!(by_class.get("custom:phone"), Some(&1));
    }

    #[test]
    fn report_serializes_with_bundle_version() {
        let ocr = OcrResult::new("body".into(), Some(91.5), 2, "eng".into());
        let report = BundleReport::new(
            "png",
            &ocr,
            42,
            3,
            vec![
                ClassCount::new("email", 2),
                ClassCount::new("custom:phone", 1),
            ],
            None,
            None,
            vec![PageReport::new(
                0,
                OcrSource::Ocr,
                Some("tesseract".to_string()),
                &ocr,
                1,
                DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            )],
            DEFAULT_LOW_CONFIDENCE_THRESHOLD,
        );
        let json = serde_json::to_value(&report).expect("serialize");
        assert_eq!(json["bundle_version"], BUNDLE_VERSION);
        assert_eq!(json["input_kind"], "png");
        assert_eq!(json["pii_token_count"], 3);
        assert_eq!(json["pages"][0]["ocr_source"], "ocr");
        assert_eq!(
            json["low_confidence_threshold"],
            DEFAULT_LOW_CONFIDENCE_THRESHOLD
        );
    }

    #[test]
    fn v1_report_without_page_fields_still_deserializes() {
        let json = serde_json::json!({
            "bundle_version": 1,
            "input_kind": "png",
            "ocr_mean_confidence": 90.0,
            "ocr_word_count": 2,
            "ocr_lang": "eng",
            "clean_char_count": 12,
            "pii_token_count": 1,
            "pii_tokens_by_class": [{ "class": "email", "count": 1 }],
            "pdf_page_count": null,
            "pdf_page_index": null
        });

        let report: BundleReport = serde_json::from_value(json).expect("v1 parses");

        assert_eq!(report.bundle_version, 1);
        assert!(report.pages.is_empty());
        assert_eq!(
            report.low_confidence_threshold,
            DEFAULT_LOW_CONFIDENCE_THRESHOLD
        );
    }

    #[test]
    fn clean_with_mock_backend_flags_low_confidence_and_columns() {
        let backend = MockBackend {
            spans: vec![
                span("Bill", 20, 10, 0.50),
                span("to:", 116, 10, 0.50),
                span("Jane", 20, 36, 0.50),
                span("Doe", 116, 36, 0.50),
                span("Email:", 360, 10, 0.50),
                span("alice@example.invalid", 360, 36, 0.50),
            ],
        };
        let tmp = tempfile::tempdir().expect("tempdir");
        let input = tmp.path().join("input.png");
        fs::write(&input, b"\x89PNG\r\n\x1A\nnot-real-image").expect("write input");

        let bundle = Pipeline::new()
            .with_low_confidence_threshold(0.65)
            .clean_with_ocr_backend(&input, tmp.path(), &backend)
            .expect("clean succeeds");

        assert_eq!(bundle.report.bundle_version, 2);
        assert_eq!(bundle.report.pages.len(), 1);
        let page = &bundle.report.pages[0];
        assert_eq!(page.ocr_backend.as_deref(), Some("mock-ocr"));
        assert_eq!(page.column_count, 2);
        assert_eq!(page.confidence, Some(0.5));
        assert!(page.low_confidence);
        assert!(
            bundle.clean_markdown.contains(":Email_"),
            "{}",
            bundle.clean_markdown
        );
        assert!(
            !bundle.clean_markdown.contains("alice@example.invalid"),
            "{}",
            bundle.clean_markdown
        );
    }

    #[test]
    fn clean_with_mock_backend_preserves_table_cell_context() {
        let backend = MockBackend {
            spans: vec![
                span("Field", 20, 10, 0.92),
                span("Value", 160, 10, 0.92),
                span("Bill", 20, 40, 0.92),
                span("Jane", 160, 40, 0.92),
                span("Email", 20, 70, 0.92),
                span("alice@example.invalid", 160, 70, 0.92),
            ],
        };
        let tmp = tempfile::tempdir().expect("tempdir");
        let input = tmp.path().join("input.png");
        fs::write(&input, b"\x89PNG\r\n\x1A\nnot-real-image").expect("write input");

        let bundle = Pipeline::new()
            .clean_with_ocr_backend(&input, tmp.path(), &backend)
            .expect("clean succeeds");

        assert_eq!(bundle.report.pages[0].column_count, 1);
        assert!(
            bundle.clean_markdown.contains("Field\nValue\n\nBill\nJane"),
            "{}",
            bundle.clean_markdown
        );
        assert!(
            bundle.clean_markdown.contains(":Email_"),
            "{}",
            bundle.clean_markdown
        );
        assert!(
            !bundle.clean_markdown.contains("alice@example.invalid"),
            "{}",
            bundle.clean_markdown
        );
    }

    #[cfg(feature = "pdf-input")]
    #[test]
    fn clean_preprocesses_rotated_image_before_backend_ocr() {
        use image::{GrayImage, ImageFormat as EncodedImageFormat, Luma};

        #[derive(Debug)]
        struct OrientationSensitiveBackend;

        impl OcrBackend for OrientationSensitiveBackend {
            fn name(&self) -> &str {
                "orientation-sensitive"
            }

            fn recognize(
                &self,
                image: ImageInput,
                _hints: OcrHints,
            ) -> Result<Vec<OcrSpan>, OcrError> {
                let decoded = image::load_from_memory(&image.bytes)
                    .map_err(|err| OcrError::Internal(err.to_string()))?;
                if decoded.width() <= decoded.height() {
                    return Ok(Vec::new());
                }
                Ok(vec![span("alice@example.invalid", 20, 20, 0.91)])
            }
        }

        let mut image = GrayImage::from_pixel(120, 80, Luma([255]));
        for y in 38..42 {
            for x in 16..104 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        let sideways = image::imageops::rotate90(&image);
        let mut bytes = Vec::new();
        sideways
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                EncodedImageFormat::Png,
            )
            .expect("encode png");
        let tmp = tempfile::tempdir().expect("tempdir");
        let input = tmp.path().join("input.png");
        fs::write(&input, bytes).expect("write input");

        let bundle = Pipeline::new()
            .clean_with_ocr_backend(&input, tmp.path(), &OrientationSensitiveBackend)
            .expect("clean succeeds");

        assert!(
            bundle.clean_markdown.contains(":Email_"),
            "{}",
            bundle.clean_markdown
        );
        assert!(
            !bundle.clean_markdown.contains("alice@example.invalid"),
            "{}",
            bundle.clean_markdown
        );
    }

    #[cfg(feature = "pdf-input")]
    #[test]
    fn clean_deskews_image_before_backend_ocr() {
        use image::{GrayImage, ImageFormat as EncodedImageFormat, Luma};
        use imageproc::geometric_transformations::{rotate_about_center, Interpolation};

        fn horizontal_score(bytes: &[u8]) -> Result<u64, OcrError> {
            let decoded = image::load_from_memory(bytes)
                .map_err(|err| OcrError::Internal(err.to_string()))?
                .to_luma8();
            let mut score = 0u64;
            for y in 0..decoded.height() {
                let mut dark = 0u64;
                for x in 0..decoded.width() {
                    if decoded.get_pixel(x, y).0[0] < 200 {
                        dark += 1;
                    }
                }
                score = score.saturating_add(dark.saturating_mul(dark));
            }
            Ok(score)
        }

        #[derive(Debug)]
        struct DeskewSensitiveBackend {
            minimum_score: u64,
        }

        impl OcrBackend for DeskewSensitiveBackend {
            fn name(&self) -> &str {
                "deskew-sensitive"
            }

            fn recognize(
                &self,
                image: ImageInput,
                _hints: OcrHints,
            ) -> Result<Vec<OcrSpan>, OcrError> {
                if horizontal_score(&image.bytes)? < self.minimum_score {
                    return Ok(Vec::new());
                }
                Ok(vec![span("alice@example.invalid", 20, 20, 0.91)])
            }
        }

        let mut image = GrayImage::from_pixel(120, 80, Luma([255]));
        for y in 38..42 {
            for x in 16..104 {
                image.put_pixel(x, y, Luma([0]));
            }
        }
        let skewed = rotate_about_center(
            &image,
            4.0_f32.to_radians(),
            Interpolation::Nearest,
            Luma([255]),
        );
        let mut bytes = Vec::new();
        skewed
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                EncodedImageFormat::Png,
            )
            .expect("encode png");
        let raw_score = horizontal_score(&bytes).expect("raw score");
        let backend = DeskewSensitiveBackend {
            minimum_score: raw_score + 1_000,
        };
        assert!(
            backend
                .recognize(
                    ImageInput {
                        bytes: bytes.clone(),
                        format: ImageFormat::Png,
                        dpi: None
                    },
                    OcrHints::default()
                )
                .expect("raw recognize")
                .is_empty(),
            "raw skewed payload should miss before preprocessing"
        );
        let tmp = tempfile::tempdir().expect("tempdir");
        let input = tmp.path().join("input.png");
        fs::write(&input, bytes).expect("write input");

        let bundle = Pipeline::new()
            .clean_with_ocr_backend(&input, tmp.path(), &backend)
            .expect("clean succeeds");

        assert!(
            bundle.clean_markdown.contains(":Email_"),
            "{}",
            bundle.clean_markdown
        );
        assert!(
            !bundle.clean_markdown.contains("alice@example.invalid"),
            "{}",
            bundle.clean_markdown
        );
    }

    #[test]
    fn format_clean_markdown_appends_trailing_newline() {
        let md = format_clean_markdown("hello", InputKind::Png);
        assert!(md.ends_with('\n'));
        assert!(md.contains("Source kind: `png`"));
        assert!(md.contains("hello"));
    }
}
