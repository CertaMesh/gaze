//! OCR backend contract surface and concrete backends.
//!
//! The [`OcrBackend`] trait is intentionally narrow: finalized image bytes in,
//! flat OCR spans out. Preprocessing, multi-page orchestration, and layout
//! reconstruction stay above or below this module so backend plurality can
//! arrive later without widening the trust boundary.

mod normalize;

use crate::DocumentError;

pub(crate) use normalize::normalize_ocr_artifacts;

#[cfg(feature = "ocr-tesseract")]
#[cfg_attr(docsrs, doc(cfg(feature = "ocr-tesseract")))]
pub mod tesseract;

#[cfg(feature = "ocr-tesseract")]
#[cfg_attr(docsrs, doc(cfg(feature = "ocr-tesseract")))]
pub use tesseract::TesseractBackend;

/// Backward-compatible alias for the Tesseract subprocess OCR backend.
#[cfg(feature = "ocr-tesseract")]
#[cfg_attr(docsrs, doc(cfg(feature = "ocr-tesseract")))]
#[deprecated(note = "use TesseractBackend")]
pub type TesseractOcr = TesseractBackend;

/// Raster image format handed to an OCR backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// PNG image bytes.
    Png,
    /// JPEG image bytes.
    Jpeg,
    /// TIFF image bytes.
    Tiff,
}

impl ImageFormat {
    /// File extension used for temporary subprocess handoff.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tiff",
        }
    }
}

/// Finalized image payload for one OCR pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInput {
    /// Encoded image bytes.
    pub bytes: Vec<u8>,
    /// Encoded image format.
    pub format: ImageFormat,
    /// Optional source DPI, when known by the orchestration layer.
    pub dpi: Option<u32>,
}

/// Backend language tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageTag(String);

impl LanguageTag {
    /// Build a language tag from a backend-specific language code.
    pub fn new(tag: impl Into<String>) -> Self {
        Self(tag.into())
    }

    /// Borrow the language tag as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for LanguageTag {
    fn default() -> Self {
        Self::new("eng")
    }
}

/// OCR backend hints. Backends may downgrade hints they cannot support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrHints {
    /// Preferred OCR languages.
    pub languages: Vec<LanguageTag>,
}

impl OcrHints {
    /// Build hints with the default English Tesseract-compatible tag.
    pub fn english() -> Self {
        Self {
            languages: vec![LanguageTag::default()],
        }
    }

    /// Return the first requested language, falling back to English.
    pub fn primary_language(&self) -> &str {
        self.languages
            .first()
            .map(LanguageTag::as_str)
            .unwrap_or("eng")
    }
}

impl Default for OcrHints {
    fn default() -> Self {
        Self::english()
    }
}

/// Bounding box in image pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BBox {
    /// Left coordinate.
    pub x: u32,
    /// Top coordinate.
    pub y: u32,
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
}

/// One OCR text span emitted by a backend.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrSpan {
    /// Recognized text for this span.
    pub text: String,
    /// Span bounding box in image pixel coordinates.
    pub bbox: BBox,
    /// Backend confidence normalized to `0.0..=1.0`.
    pub confidence: Option<f32>,
}

/// Closed OCR backend error surface.
#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    /// Backend initialization failed.
    #[error("backend init failed: {0}")]
    InitFailed(String),
    /// Recognition failed after backend initialization.
    #[error("recognize failed: {0}")]
    RecognizeFailed(String),
    /// Image format is unsupported by this backend.
    #[error("unsupported image format: {0:?}")]
    UnsupportedFormat(ImageFormat),
    /// Backend hit an internal invariant or I/O failure.
    #[error("backend internal error: {0}")]
    Internal(String),
}

/// Narrow OCR backend contract.
pub trait OcrBackend: Send + Sync {
    /// Stable backend name used in diagnostics.
    fn name(&self) -> &str;

    /// Recognize flat spans from one finalized image.
    fn recognize(&self, image: ImageInput, hints: OcrHints) -> Result<Vec<OcrSpan>, OcrError>;
}

/// Legacy text-only adapter contract kept for source compatibility.
///
/// New code should use [`OcrBackend`] so backend output remains traceable to
/// bounding boxes and confidence values.
pub trait OcrAdapter {
    /// Extract textual content from image bytes.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError`] when backend recognition fails.
    fn extract_text(&self, bytes: &[u8]) -> Result<String, DocumentError>;
}

/// Reserved fail-loud legacy adapter.
///
/// New code should wire a concrete [`OcrBackend`]. This placeholder remains so
/// older callers fail closed instead of receiving silent empty OCR output.
#[non_exhaustive]
pub struct PendingOcrAdapter {
    _private: (),
}

impl PendingOcrAdapter {
    /// Build the fail-loud adapter.
    ///
    /// # Errors
    ///
    /// Always returns [`DocumentError::NotImplemented`].
    pub fn new() -> Result<Self, DocumentError> {
        Err(DocumentError::NotImplemented(
            "PendingOcrAdapter::new (wire a concrete OCR backend)",
        ))
    }
}

impl OcrAdapter for PendingOcrAdapter {
    fn extract_text(&self, _bytes: &[u8]) -> Result<String, DocumentError> {
        Err(DocumentError::NotImplemented(
            "PendingOcrAdapter::extract_text (wire a concrete OCR backend)",
        ))
    }
}

/// Result of an OCR pass: full text + a structured confidence summary.
///
/// Backend-agnostic — concrete adapters (e.g., [`tesseract::TesseractBackend`])
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

    /// Build an OCR result from flat spans using pixel y-position to recover
    /// a conservative reading order.
    pub fn from_spans(spans: &[OcrSpan], lang: String) -> Self {
        Self::from_spans_with_column_detection(spans, lang, false).0
    }

    /// Build an OCR result from flat spans using the crate layout
    /// post-processor. Returns the result plus the detected column count.
    pub(crate) fn from_spans_with_column_detection(
        spans: &[OcrSpan],
        lang: String,
        column_detection: bool,
    ) -> (Self, u32) {
        let ordered = crate::postprocess::order_spans(spans, column_detection);
        let mut conf_sum = 0.0f64;
        let mut conf_count = 0usize;
        for span in spans {
            if let Some(confidence) = span.confidence {
                conf_sum += (confidence * 100.0) as f64;
                conf_count += 1;
            }
        }
        let mean_confidence = if conf_count == 0 {
            None
        } else {
            Some((conf_sum / conf_count as f64) as f32)
        };
        (
            Self {
                text: ordered.text,
                mean_confidence,
                word_count: conf_count,
                lang,
            },
            ordered.column_count,
        )
    }

    /// Mean confidence normalized to `0.0..=1.0`.
    pub(crate) fn mean_confidence_unit(&self) -> Option<f32> {
        self.mean_confidence.map(|confidence| {
            if confidence > 1.0 {
                (confidence / 100.0).clamp(0.0, 1.0)
            } else {
                confidence.clamp(0.0, 1.0)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_confidence_unit_normalizes_legacy_percent_value() {
        let result = OcrResult::new("body".to_string(), Some(91.0), 1, "eng".to_string());
        assert_eq!(result.mean_confidence_unit(), Some(0.91));
    }

    #[test]
    fn from_spans_reports_detected_columns() {
        let spans = vec![
            OcrSpan {
                text: "A1".to_string(),
                bbox: BBox {
                    x: 10,
                    y: 10,
                    w: 30,
                    h: 10,
                },
                confidence: Some(0.8),
            },
            OcrSpan {
                text: "B1".to_string(),
                bbox: BBox {
                    x: 280,
                    y: 10,
                    w: 30,
                    h: 10,
                },
                confidence: Some(0.8),
            },
            OcrSpan {
                text: "A2".to_string(),
                bbox: BBox {
                    x: 10,
                    y: 30,
                    w: 30,
                    h: 10,
                },
                confidence: Some(0.8),
            },
            OcrSpan {
                text: "B2".to_string(),
                bbox: BBox {
                    x: 280,
                    y: 30,
                    w: 30,
                    h: 10,
                },
                confidence: Some(0.8),
            },
        ];

        let (result, columns) =
            OcrResult::from_spans_with_column_detection(&spans, "eng".to_string(), true);

        assert_eq!(columns, 2);
        assert_eq!(result.text, "A1\nA2\n\nB1\nB2");
        assert_eq!(result.mean_confidence_unit(), Some(0.8));
    }
}
