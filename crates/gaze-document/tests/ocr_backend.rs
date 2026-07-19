//! Direct OCR backend contract tests.

#![cfg(feature = "ocr-tesseract")]

use std::path::PathBuf;

#[cfg(target_os = "macos")]
use gaze_document::DocumentError;
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

#[cfg(target_os = "macos")]
#[test]
fn tesseract_backend_canonicalizes_logical_tmp_input() {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    let task_dir = tempfile::Builder::new()
        .prefix("gaze-tesseract-boundary-")
        .tempdir_in("/private/tmp")
        .expect("task-local physical tempdir");
    let physical_input = task_dir.path().join("synthetic-image.png");
    std::fs::copy(testdata_dir().join("synthetic_image.png"), &physical_input)
        .expect("copy synthetic fixture");

    let logical_input = Path::new("/").join(
        physical_input
            .strip_prefix("/private")
            .expect("physical temp path starts with /private"),
    );
    assert!(logical_input.starts_with("/tmp"));
    assert!(logical_input.exists());

    let fake_tesseract = task_dir.path().join("fake-tesseract");
    let mut fake = std::fs::File::create(&fake_tesseract).expect("create fake tesseract");
    fake.write_all(
        br#"#!/bin/sh
case "$1" in
  /tmp|/tmp/*) exit 64 ;;
  /private/tmp/*) ;;
  *) exit 65 ;;
esac
[ -r "$1" ] || exit 66
printf 'level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n5\t1\t1\t1\t1\t1\t0\t0\t10\t10\t99\tSynthetic\n'
"#,
    )
    .expect("write fake tesseract");
    fake.flush().expect("flush fake tesseract");
    std::fs::set_permissions(&fake_tesseract, std::fs::Permissions::from_mode(0o700))
        .expect("make fake tesseract executable");

    let mut backend = TesseractBackend::new();
    backend.binary = Some(fake_tesseract);

    let result = backend
        .extract_from_file(&logical_input)
        .expect("logical /tmp input is normalized at the Tesseract boundary");
    assert_eq!(result.text, "Synthetic");
    assert_eq!(result.word_count, 1);

    std::fs::set_permissions(&physical_input, std::fs::Permissions::from_mode(0o000))
        .expect("make synthetic input unreadable");
    let unreadable = backend
        .extract_from_file(&logical_input)
        .expect_err("unreadable OCR input must fail closed");
    assert!(
        matches!(
            unreadable,
            DocumentError::TesseractFailed { status: 66, .. }
        ),
        "unexpected unreadable-input error: {unreadable}"
    );
    std::fs::set_permissions(&physical_input, std::fs::Permissions::from_mode(0o600))
        .expect("restore synthetic input permissions");

    std::fs::remove_file(&physical_input).expect("remove synthetic input");
    let missing = backend
        .extract_from_file(&logical_input)
        .expect_err("missing OCR input must fail closed");
    assert!(
        matches!(missing, DocumentError::Io(_)),
        "unexpected missing-input error: {missing}"
    );
}
