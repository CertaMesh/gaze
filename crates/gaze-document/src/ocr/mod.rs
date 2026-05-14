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
        let text = spans_to_text(spans);
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
        Self {
            text,
            mean_confidence,
            word_count: conf_count,
            lang,
        }
    }
}

fn spans_to_text(spans: &[OcrSpan]) -> String {
    let mut ordered = spans.to_vec();
    ordered.sort_by_key(|span| (span.bbox.y, span.bbox.x));

    let mut lines: Vec<Vec<OcrSpan>> = Vec::new();

    for span in ordered {
        if span.text.is_empty() {
            continue;
        }
        let belongs_to_current_line = lines
            .last()
            .and_then(|line| line.first())
            .map(|first| span.bbox.y.abs_diff(first.bbox.y) <= span.bbox.h.max(first.bbox.h))
            .unwrap_or(false);
        if belongs_to_current_line {
            if let Some(line) = lines.last_mut() {
                line.push(span);
            }
        } else {
            lines.push(vec![span]);
        }
    }

    lines
        .into_iter()
        .map(|mut line| {
            line.sort_by_key(|span| span.bbox.x);
            line.into_iter()
                .map(|span| span.text)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
