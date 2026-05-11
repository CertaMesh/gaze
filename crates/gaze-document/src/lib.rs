//! Document ingestion + safe-bundle generation for the Gaze runtime.
//!
//! `gaze-document` turns a single image (PNG / JPG) or single-page PDF into a
//! [`SafeBundle`]: tokenized Markdown, a restorable [`gaze::Manifest`], and a
//! structured OCR + PII [`BundleReport`]. PII detection flows through the
//! standard [`gaze::Pipeline`] so the manifest stays canonical and reversible
//! (Axis 2 reversibility).
//!
//! # Quickstart
//!
//! ```no_run
//! use std::path::Path;
//!
//! let bundle = gaze_document::clean(
//!     Path::new("invoice.pdf"),
//!     Path::new("./safe-out"),
//! )?;
//! assert!(!bundle.clean_markdown.is_empty());
//! # Ok::<(), gaze_document::DocumentError>(())
//! ```
//!
//! # Runtime requirements
//!
//! * `tesseract` binary on `PATH` (Tesseract 4.x or 5.x).
//! * For PDF input: a pdfium dynamic library available to the process. See
//!   the crate README for per-OS install instructions.
//!
//! # Feature flags
//!
//! | Flag             | Default | What it enables                                            |
//! |------------------|---------|------------------------------------------------------------|
//! | `ocr-tesseract`  | yes     | Tesseract subprocess OCR backend.                          |
//! | `pdf-input`      | yes     | Single-page PDF rasterization via `pdfium-render`.         |
//! | `serde`          | yes     | `Serialize` / `Deserialize` for [`BundleReport`].          |
//! | `extract-docling`| no      | Reserved — future Docling layout adapter (no impl yet).    |
//! | `render-image`   | no      | Reserved — future redacted-preview renderer (no impl yet). |

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod bundle;
pub mod extract;
pub mod layout;
pub mod ocr;
pub mod render;

#[cfg(feature = "ocr-tesseract")]
#[cfg_attr(docsrs, doc(cfg(feature = "ocr-tesseract")))]
pub use bundle::clean;
pub use bundle::{BundleReport, ClassCount, LayoutSummary, SafeBundle, BUNDLE_VERSION};
pub use layout::ReadingOrder;
pub use ocr::OcrAdapter;
pub use render::Renderer;

/// Crate-level error type for `gaze-document`.
///
/// Fail-closed by construction (Axis 1 reliability): every variant describes
/// a specific, recoverable surface so adopters can branch on cause without
/// matching opaque strings.
#[non_exhaustive]
#[derive(Debug)]
pub enum DocumentError {
    /// Tesseract CLI is not on `PATH`. The payload is a per-OS install hint.
    TesseractNotFound(String),
    /// Tesseract returned a non-zero exit status. The payload carries the
    /// captured stderr (truncated) so adopters can surface it.
    TesseractFailed {
        /// Exit status reported by the OS.
        status: i32,
        /// Captured stderr (truncated to keep error payloads bounded).
        stderr: String,
    },
    /// pdfium dynamic library could not be loaded. The payload is a per-OS
    /// install hint.
    PdfiumNotFound(String),
    /// pdfium reported an error while parsing or rasterizing a PDF.
    PdfRasterFailed(String),
    /// Input file format is not supported by the current build (e.g. PDF
    /// input without the `pdf-input` feature).
    UnsupportedInput {
        /// Path that was rejected.
        path: std::path::PathBuf,
        /// Reason the input was rejected.
        reason: &'static str,
    },
    /// An I/O error while reading the input or writing the bundle.
    Io(std::io::Error),
    /// The bundle output directory could not be prepared.
    OutputDir(std::path::PathBuf, std::io::Error),
    /// `gaze::Pipeline` construction or invocation failed.
    Pipeline(String),
    /// `serde_json` serialization of the bundle report or manifest failed.
    Serde(serde_json::Error),
    /// The requested operation is part of the public contract but has no
    /// implementation yet. Returned by reserved stubs (Renderer trait,
    /// ReadingOrder) until follow-up PRs land.
    NotImplemented(&'static str),
}

impl core::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TesseractNotFound(hint) => write!(
                f,
                "gaze-document: tesseract binary not found on PATH. {hint}"
            ),
            Self::TesseractFailed { status, stderr } => write!(
                f,
                "gaze-document: tesseract exited with status {status}: {stderr}"
            ),
            Self::PdfiumNotFound(hint) => write!(
                f,
                "gaze-document: pdfium dynamic library not found. {hint}"
            ),
            Self::PdfRasterFailed(detail) => {
                write!(f, "gaze-document: pdf rasterization failed: {detail}")
            }
            Self::UnsupportedInput { path, reason } => write!(
                f,
                "gaze-document: unsupported input `{}`: {reason}",
                path.display()
            ),
            Self::Io(err) => write!(f, "gaze-document: io error: {err}"),
            Self::OutputDir(path, err) => write!(
                f,
                "gaze-document: cannot prepare output dir `{}`: {err}",
                path.display()
            ),
            Self::Pipeline(detail) => write!(f, "gaze-document: pipeline error: {detail}"),
            Self::Serde(err) => write!(f, "gaze-document: serialize error: {err}"),
            Self::NotImplemented(what) => {
                write!(f, "gaze-document: {what} is not yet implemented")
            }
        }
    }
}

impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) | Self::OutputDir(_, err) => Some(err),
            Self::Serde(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DocumentError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for DocumentError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serde(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_compiles_and_error_renders() {
        let err = DocumentError::NotImplemented("smoke");
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[test]
    fn tesseract_not_found_error_includes_hint() {
        let err = DocumentError::TesseractNotFound(
            "Install via `brew install tesseract`.".into(),
        );
        let msg = err.to_string();
        assert!(msg.contains("tesseract"));
        assert!(msg.contains("brew install"));
    }
}
