//! Input extraction backends.
//!
//! Each submodule turns a specific input format (PDF, future: Word, HTML)
//! into a PNG image ready for OCR.

use crate::DocumentError;

#[cfg(feature = "pdf-input")]
#[cfg_attr(docsrs, doc(cfg(feature = "pdf-input")))]
pub mod pdf;

/// Source kind detected for an input path.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// PNG image input. Passed straight to the OCR backend.
    Png,
    /// JPEG image input. Passed straight to the OCR backend.
    Jpeg,
    /// PDF input. Rasterized to PNG before OCR (single-page only in v0.0.x).
    Pdf,
}

impl InputKind {
    /// Detects [`InputKind`] from a file path's extension.
    ///
    /// Returns [`DocumentError::UnsupportedInput`] when the extension is
    /// missing or not in the supported set.
    pub fn detect(path: &std::path::Path) -> Result<Self, DocumentError> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase());
        match ext.as_deref() {
            Some("png") => Ok(Self::Png),
            Some("jpg" | "jpeg") => Ok(Self::Jpeg),
            Some("pdf") => Ok(Self::Pdf),
            _ => Err(DocumentError::UnsupportedInput {
                path: path.to_path_buf(),
                reason: "extension must be one of: png, jpg, jpeg, pdf",
            }),
        }
    }

    /// File extension used when writing a temp copy of this input kind.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Pdf => "pdf",
        }
    }
}
