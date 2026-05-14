//! Direct OCR backend contract tests.

#![cfg(feature = "ocr-tesseract")]

use std::path::PathBuf;

use gaze_document::{ImageFormat, ImageInput, OcrBackend, OcrError, OcrHints, TesseractBackend};

fn testdata_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn ocr_backend_is_object_safe() {
    let _backend: Box<dyn OcrBackend> = Box::new(TesseractBackend::new());
}

#[test]
fn tesseract_backend_recognizes_synthetic_image_spans() {
    let input = testdata_dir().join("synthetic_image.png");
    let bytes = std::fs::read(&input).expect("fixture readable");
    let backend = TesseractBackend::new();

    let spans = match backend.recognize(
        ImageInput {
            bytes,
            format: ImageFormat::Png,
            dpi: None,
        },
        OcrHints::default(),
    ) {
        Ok(spans) => spans,
        Err(OcrError::InitFailed(reason)) => {
            eprintln!("SKIP: tesseract not installed: {reason}");
            return;
        }
        Err(err) => panic!("tesseract recognize failed: {err}"),
    };

    assert!(!spans.is_empty(), "expected at least one OCR span");
    let recognized = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        recognized.contains("Bill"),
        "expected OCR spans to include fixture text, got: {recognized}"
    );
}
