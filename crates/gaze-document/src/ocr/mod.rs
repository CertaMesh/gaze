//! OCR adapter contract surface.
//!
//! Concrete backends (Tesseract, etc.) land behind feature flags in
//! follow-up PRs. This module currently exposes only the trait shape and a
//! fail-loud `Pending` adapter used in tests + as a forward-compat stub.

use crate::DocumentError;

/// Adapter contract for OCR backends.
///
/// Implementations live behind feature flags (e.g., `ocr-tesseract`) and
/// must round-trip page coordinates so the eventual `layout::ReadingOrder`
/// pass can stitch the result without losing source spans.
pub trait OcrAdapter {
    /// Extract textual content from `_bytes` (raw image / page payload).
    ///
    /// # Errors
    /// Implementations return [`DocumentError`] on backend failure. The
    /// scaffold trait has no default body; impls are required.
    fn extract_text(&self, _bytes: &[u8]) -> Result<String, DocumentError>;
}

/// Reserved fail-loud adapter.
///
/// Acts as the default until a concrete OCR backend is wired in. Every
/// call returns [`DocumentError::NotImplemented`] so accidental wiring is
/// caught at the call site (Axis-1 fail-closed).
#[non_exhaustive]
pub struct PendingOcrAdapter {
    _private: (),
}

impl PendingOcrAdapter {
    /// Build the fail-loud adapter.
    ///
    /// # Errors
    /// Always returns [`DocumentError::NotImplemented`] in the scaffold.
    pub fn new() -> Result<Self, DocumentError> {
        Err(DocumentError::NotImplemented("PendingOcrAdapter::new"))
    }
}

impl OcrAdapter for PendingOcrAdapter {
    fn extract_text(&self, _bytes: &[u8]) -> Result<String, DocumentError> {
        Err(DocumentError::NotImplemented(
            "PendingOcrAdapter::extract_text",
        ))
    }
}
