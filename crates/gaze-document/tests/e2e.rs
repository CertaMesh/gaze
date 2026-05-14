//! End-to-end fixture tests for the `gaze-document` MVP.
//!
//! Runs `gaze_document::clean` against both committed synthetic fixtures
//! (`testdata/synthetic_image.png`, `testdata/synthetic_doc.pdf`) and
//! asserts:
//!
//! * `clean.md` exists, contains the canonical tokenized substrings, and
//!   carries **none** of the original PII literals.
//! * `manifest.json` deserializes back into a `gaze::Manifest` and contains
//!   at least one `Email` + one `Custom("phone")` span.
//! * `report.json` deserializes into a `BundleReport` shape with
//!   `bundle_version = 2`, per-page provenance, and non-zero PII counts.
//!
//! ## Skip conditions
//!
//! * Tesseract missing on PATH → reported as a skip (not a failure) so the
//!   test stays runnable on dev machines without the binary.
//! * pdfium dynamic library missing → PDF case skipped with the same logic.
//! * CI is expected to provide both via apt / Homebrew (see CI workflow).

#![cfg(feature = "ocr-tesseract")]

use std::path::{Path, PathBuf};

use gaze::Manifest;
use gaze_document::{BundleReport, DocumentError, OcrSource, BUNDLE_VERSION};
use gaze_types::PiiClass;

const ORIGINAL_EMAIL: &str = "jane.doe@example.com";
const ORIGINAL_EMAIL_DOMAIN: &str = "@example.com";
const ORIGINAL_EMAIL_LOCAL: &str = "jane.doe";
const ORIGINAL_NAME: &str = "Jane Doe";
const ORIGINAL_PHONE_TAIL: &str = "555-0142";

fn testdata_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

fn skip_reason(err: &DocumentError) -> Option<&'static str> {
    match err {
        DocumentError::TesseractNotFound(_) => Some("tesseract not installed"),
        DocumentError::PdfiumNotFound(_) => Some("pdfium dynamic library not installed"),
        _ => None,
    }
}

fn assert_clean_bundle(input: &Path, expect_pdf_fields: bool) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = match gaze_document::clean(input, tmp.path()) {
        Ok(b) => b,
        Err(err) => {
            if let Some(why) = skip_reason(&err) {
                eprintln!("SKIP: {why}: {err}");
                return;
            }
            panic!("gaze_document::clean failed: {err}");
        }
    };

    let clean_md_path = tmp.path().join("clean.md");
    let manifest_path = tmp.path().join("manifest.json");
    let report_path = tmp.path().join("report.json");
    assert!(clean_md_path.exists(), "clean.md missing");
    assert!(manifest_path.exists(), "manifest.json missing");
    assert!(report_path.exists(), "report.json missing");

    let clean_text = std::fs::read_to_string(&clean_md_path).expect("clean.md readable");
    // Belt-and-braces negative substring checks. Each one independently
    // detects a different leak shape that could survive a brittle
    // detector (e.g. OCR-corrupted email with spurious whitespace, or
    // partial-domain leftover after a narrow regex match).
    assert!(
        !clean_text.contains(ORIGINAL_EMAIL),
        "clean.md leaked the original email substring:\n{clean_text}"
    );
    assert!(
        !clean_text.contains(ORIGINAL_EMAIL_DOMAIN),
        "clean.md leaked the email domain substring (OCR-artifact survival):\n{clean_text}"
    );
    assert!(
        !clean_text.contains(ORIGINAL_EMAIL_LOCAL),
        "clean.md leaked the email local-part substring:\n{clean_text}"
    );
    assert!(
        !clean_text.contains(ORIGINAL_PHONE_TAIL),
        "clean.md leaked the original phone tail substring:\n{clean_text}"
    );
    assert!(
        !clean_text.contains(ORIGINAL_NAME),
        "clean.md leaked the original recipient name substring:\n{clean_text}"
    );
    assert!(
        clean_text.contains(":Email_"),
        "expected a tokenized Email reference in clean.md, got:\n{clean_text}"
    );
    assert!(
        clean_text.contains(":Custom:phone_"),
        "expected a tokenized phone reference in clean.md, got:\n{clean_text}"
    );
    assert!(
        clean_text.contains(":Name_"),
        "expected a tokenized Name reference in clean.md, got:\n{clean_text}"
    );

    let manifest_bytes = std::fs::read(&manifest_path).expect("manifest bytes");
    let manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).expect("manifest deserializes");
    assert!(
        manifest
            .spans
            .iter()
            .any(|s| matches!(s.class, PiiClass::Email)),
        "manifest missing Email span"
    );
    assert!(
        manifest
            .spans
            .iter()
            .any(|s| matches!(&s.class, PiiClass::Custom(name) if name == "phone")),
        "manifest missing Custom(phone) span"
    );
    assert!(
        manifest
            .spans
            .iter()
            .any(|s| matches!(s.class, PiiClass::Name)),
        "manifest missing Name span"
    );

    let report_bytes = std::fs::read(&report_path).expect("report bytes");
    let report: BundleReport = serde_json::from_slice(&report_bytes).expect("report deserializes");
    assert_eq!(report.bundle_version, BUNDLE_VERSION);
    assert!(report.pii_token_count >= 2);
    assert!(report.clean_char_count > 0);
    assert_eq!(report.low_confidence_threshold, 0.65);
    assert_eq!(report.pages.len(), 1);
    assert_eq!(report.pages[0].page_index, 0);
    assert!(report.pages[0].column_count >= 1);
    if report.pages[0].ocr_source == OcrSource::Ocr {
        assert!(report.ocr_word_count > 0, "OCR returned zero words");
        assert_eq!(report.pages[0].ocr_backend.as_deref(), Some("tesseract"));
    } else {
        assert_eq!(report.pages[0].ocr_source, OcrSource::VectorPdf);
        assert!(report.pages[0].confidence.is_none());
        assert!(!report.pages[0].low_confidence);
    }

    if expect_pdf_fields {
        assert_eq!(report.input_kind, "pdf");
        assert_eq!(report.pdf_page_count, Some(1));
        assert_eq!(report.pdf_page_index, Some(0));
        assert_eq!(report.pages[0].ocr_source, OcrSource::VectorPdf);
    } else {
        assert!(matches!(report.input_kind.as_str(), "png" | "jpeg"));
        assert!(report.pdf_page_count.is_none());
        assert_eq!(report.pages[0].ocr_source, OcrSource::Ocr);
    }

    // Sanity: the in-memory bundle matches the written report.
    assert_eq!(bundle.report.bundle_version, BUNDLE_VERSION);
    assert_eq!(bundle.report.pii_token_count, report.pii_token_count);
}

#[test]
fn synthetic_image_png_clean_emits_safe_bundle() {
    let input = testdata_dir().join("synthetic_image.png");
    assert!(input.exists(), "missing fixture: {}", input.display());
    assert_clean_bundle(&input, false);
}

#[cfg(feature = "pdf-input")]
#[test]
fn synthetic_doc_pdf_clean_emits_safe_bundle() {
    let input = testdata_dir().join("synthetic_doc.pdf");
    assert!(input.exists(), "missing fixture: {}", input.display());
    assert_clean_bundle(&input, true);
}
