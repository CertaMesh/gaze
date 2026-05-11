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

use crate::ocr::OcrResult;

#[cfg(feature = "ocr-tesseract")]
use std::collections::BTreeMap;
#[cfg(feature = "ocr-tesseract")]
use std::fs;
#[cfg(feature = "ocr-tesseract")]
use std::path::Path;

#[cfg(feature = "ocr-tesseract")]
use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, LocaleTag, Pipeline, RawDocument, Scope, Session,
};
#[cfg(feature = "ocr-tesseract")]
use gaze_recognizers::RegexDetector;
#[cfg(feature = "ocr-tesseract")]
use gaze_types::{EmittedTokenSpan, PiiClass};

#[cfg(feature = "ocr-tesseract")]
use crate::extract::InputKind;
#[cfg(feature = "ocr-tesseract")]
use crate::DocumentError;

/// Versioned `report.json` schema tag (bump on breaking shape changes).
pub const BUNDLE_VERSION: u32 = 1;

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
        }
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
    let kind = InputKind::detect(input)?;
    let absolute_input = absolutize(input);
    let absolute_out = absolutize(out_dir);

    fs::create_dir_all(out_dir)
        .map_err(|err| DocumentError::OutputDir(absolute_out.clone(), err))?;

    let (ocr_result, pdf_page_count, pdf_page_index) = run_ocr(input, kind)?;
    let pipeline = build_document_pipeline()?;
    let session = Session::new(Scope::Ephemeral).map_err(|err| pipeline_err("session", err))?;
    let locale_chain = [LocaleTag::Global];
    let (clean_doc, spans, _leak_report) = pipeline
        .clean_with_safety_net(
            &session,
            RawDocument::Text(ocr_result.text.clone()),
            &locale_chain,
        )
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
        &ocr_result,
        clean_text.chars().count(),
        pii_token_count,
        counts,
        pdf_page_count,
        pdf_page_index,
    );

    let clean_markdown = format_clean_markdown(&clean_text, kind);
    write_bundle(out_dir, &clean_markdown, &manifest, &report)?;

    Ok(SafeBundle::new(
        clean_markdown,
        manifest,
        LayoutSummary::single_page(),
        None,
        report,
        absolute_input,
        absolute_out,
    ))
}

#[cfg(feature = "ocr-tesseract")]
fn run_ocr(
    input: &Path,
    kind: InputKind,
) -> Result<(OcrResult, Option<i32>, Option<i32>), DocumentError> {
    use crate::ocr::TesseractOcr;
    let ocr = TesseractOcr::new();
    match kind {
        InputKind::Png | InputKind::Jpeg => {
            let result = ocr.extract_from_file(input)?;
            Ok((result, None, None))
        }
        InputKind::Pdf => {
            #[cfg(feature = "pdf-input")]
            {
                use crate::extract::pdf::{rasterize_first_page, PdfRasterConfig};
                let raster = rasterize_first_page(input, PdfRasterConfig::new())?;
                let result = ocr.extract_from_bytes(&raster.png_bytes, "png")?;
                Ok((result, Some(raster.page_count), Some(raster.page_index)))
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
fn build_document_pipeline() -> Result<Pipeline, DocumentError> {
    let email = RegexDetector::emails().map_err(|err| pipeline_err("email-regex", err))?;
    // Conservative phone pattern: optional `+CC`, area, exchange, line, with
    // common separators. Synthetic fixture uses `+1-555-0142`-style numbers.
    let phone = RegexDetector::new(
        r"\+?\d{1,3}[-.\s]\(?\d{3}\)?[-.\s]?\d{3,4}[-.\s]?\d{0,4}",
        PiiClass::custom("phone"),
    )
    .map_err(|err| pipeline_err("phone-regex", err))?;
    Pipeline::builder()
        .detector(email)
        .detector(phone)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(PiiClass::custom("phone"), Action::Tokenize))
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
fn format_clean_markdown(text: &str, kind: InputKind) -> String {
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
fn kind_label(kind: InputKind) -> &'static str {
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
            vec![ClassCount::new("email", 2), ClassCount::new("custom:phone", 1)],
            None,
            None,
        );
        let json = serde_json::to_value(&report).expect("serialize");
        assert_eq!(json["bundle_version"], BUNDLE_VERSION);
        assert_eq!(json["input_kind"], "png");
        assert_eq!(json["pii_token_count"], 3);
    }

    #[test]
    fn format_clean_markdown_appends_trailing_newline() {
        let md = format_clean_markdown("hello", InputKind::Png);
        assert!(md.ends_with('\n'));
        assert!(md.contains("Source kind: `png`"));
        assert!(md.contains("hello"));
    }
}
