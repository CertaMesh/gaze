//! Safe-bundle contract surface.
//!
//! A `SafeBundle` is the post-ingestion artifact: clean Markdown, the
//! reversible Gaze [`gaze::Manifest`], a layout summary, an optional preview
//! PNG, and a structured report. Concrete builders ship in follow-up PRs;
//! this module currently exposes only type contracts.

use crate::DocumentError;

/// Post-ingestion artifact paired with a Gaze [`gaze::Manifest`].
///
/// Field set is reserved (`#[non_exhaustive]`) until the bundle contract
/// stabilizes. See scratchpads 1537 and 1561 in the orchestration project
/// for the locked plan.
#[non_exhaustive]
pub struct SafeBundle {
    /// Tokenized Markdown safe to hand to an LLM.
    pub clean_markdown: String,
    /// Reversible manifest from the Gaze pipeline.
    pub manifest: gaze::Manifest,
    /// Opaque layout summary (page count, reading-order hints, etc.).
    pub layout: LayoutSummary,
    /// Optional rasterized preview of the source document.
    pub preview_png: Option<Vec<u8>>,
    /// Per-bundle audit + provenance report.
    pub report: BundleReport,
}

impl SafeBundle {
    /// Reserved constructor. Fails loud until concrete bundle builders land.
    ///
    /// # Errors
    /// Always returns [`DocumentError::NotImplemented`] in the scaffold.
    pub fn new(
        _clean_markdown: String,
        _manifest: gaze::Manifest,
        _layout: LayoutSummary,
        _preview_png: Option<Vec<u8>>,
        _report: BundleReport,
    ) -> Result<Self, DocumentError> {
        Err(DocumentError::NotImplemented("SafeBundle::new"))
    }
}

/// Opaque layout summary placeholder.
///
/// Concrete shape lands with the layout-extraction PR. For now it is an
/// empty newtype reserved on the public surface.
#[non_exhaustive]
#[derive(Default)]
pub struct LayoutSummary {
    _private: (),
}

impl LayoutSummary {
    /// Reserved constructor. Returns an empty summary; concrete fields land later.
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }
}

/// Opaque bundle audit + provenance report placeholder.
///
/// Concrete shape lands alongside the OCR/extract adapters.
#[non_exhaustive]
#[derive(Default)]
pub struct BundleReport {
    _private: (),
}

impl BundleReport {
    /// Reserved constructor. Returns an empty report; concrete fields land later.
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }
}
