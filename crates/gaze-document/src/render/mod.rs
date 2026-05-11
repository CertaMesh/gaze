//! Renderer contract surface.
//!
//! Concrete renderers (preview PNG, redacted PDF, etc.) land behind feature
//! flags (e.g., `render-image`) in follow-up PRs. This module currently
//! exposes only the trait shape and a fail-loud `Pending` renderer.

use crate::DocumentError;

/// Adapter contract for output renderers.
///
/// Renderers consume a [`crate::SafeBundle`] and produce a binary payload
/// (preview PNG, redacted PDF, etc.). Implementations must respect the
/// manifest contract — every byte they emit must be either tokenized or
/// non-PII per the bundle's [`gaze::Manifest`].
pub trait Renderer {
    /// Render the bundle into a byte payload (PNG, PDF, ...).
    ///
    /// # Errors
    /// Implementations return [`DocumentError`] on backend failure.
    fn render(&self, bundle: &crate::SafeBundle) -> Result<Vec<u8>, DocumentError>;
}

/// Reserved fail-loud renderer.
///
/// Acts as the default until a concrete renderer ships. Every call
/// returns [`DocumentError::NotImplemented`] so accidental wiring is
/// caught at the call site (Axis-1 fail-closed).
#[non_exhaustive]
#[derive(Default)]
pub struct PendingRenderer {
    _private: (),
}

impl PendingRenderer {
    /// Build the fail-loud renderer. Always succeeds; calls error.
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Renderer for PendingRenderer {
    fn render(&self, _bundle: &crate::SafeBundle) -> Result<Vec<u8>, DocumentError> {
        Err(DocumentError::NotImplemented("PendingRenderer::render"))
    }
}
