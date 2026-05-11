//! OCR adapter contract surface and concrete backends.
//!
//! The [`OcrAdapter`] trait stays open for future backends (cloud OCR, Apple
//! Vision, etc.). The shipping backend in v0.0.x is the Tesseract subprocess
//! adapter under [`tesseract`].
//!
//! The trait itself remains fail-loud-default via [`PendingOcrAdapter`] so an
//! adopter who builds a trait object without picking a backend gets a typed
//! `NotImplemented` error instead of silent zero-detection output (Axis 1
//! fail-closed).

use crate::DocumentError;

#[cfg(feature = "ocr-tesseract")]
#[cfg_attr(docsrs, doc(cfg(feature = "ocr-tesseract")))]
pub mod tesseract;

#[cfg(feature = "ocr-tesseract")]
#[cfg_attr(docsrs, doc(cfg(feature = "ocr-tesseract")))]
pub use tesseract::TesseractOcr;

/// Result of an OCR pass: full text + a structured confidence summary.
///
/// Backend-agnostic — concrete adapters (e.g., [`tesseract::TesseractOcr`])
/// produce values of this shape so the rest of the pipeline does not
/// hard-code one OCR backend's surface.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct OcrResult {
    /// Extracted plain text, with original line breaks preserved.
    pub text: String,
    /// Mean per-word OCR confidence in `[0.0, 100.0]`. `None` when the page
    /// contained no recognizable words.
    pub mean_confidence: Option<f32>,
    /// Number of words emitted with confidence `>= 0`.
    pub word_count: usize,
    /// OCR language tag used (e.g., `eng`).
    pub lang: String,
}

impl OcrResult {
    /// Build an OCR result from raw fields.
    pub fn new(
        text: String,
        mean_confidence: Option<f32>,
        word_count: usize,
        lang: String,
    ) -> Self {
        Self {
            text,
            mean_confidence,
            word_count,
            lang,
        }
    }
}

/// Adapter contract for OCR backends.
///
/// Implementations live behind feature flags (e.g., `ocr-tesseract`) and
/// must round-trip page coordinates so the eventual `layout::ReadingOrder`
/// pass can stitch the result without losing source spans.
pub trait OcrAdapter {
    /// Extract textual content from `_bytes` (raw image / page payload).
    ///
    /// # Errors
    /// Implementations return [`DocumentError`] on backend failure.
    fn extract_text(&self, bytes: &[u8]) -> Result<String, DocumentError>;
}

/// Reserved fail-loud adapter.
///
/// Used as an explicit "no backend wired" sentinel. Every call returns
/// [`DocumentError::NotImplemented`] so accidental wiring is caught at the
/// call site (Axis 1 fail-closed).
#[non_exhaustive]
pub struct PendingOcrAdapter {
    _private: (),
}

impl PendingOcrAdapter {
    /// Build the fail-loud adapter.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for PendingOcrAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl OcrAdapter for PendingOcrAdapter {
    fn extract_text(&self, _bytes: &[u8]) -> Result<String, DocumentError> {
        Err(DocumentError::NotImplemented(
            "PendingOcrAdapter::extract_text (wire a concrete OCR backend)",
        ))
    }
}
