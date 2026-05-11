//! Renderer contract surface.
//!
//! Concrete renderers (redacted preview PNG, redacted PDF) land behind the
//! `render-image` feature in a follow-up PR. This module reserves the trait
//! shape and a fail-loud default so adopters can wire trait objects without
//! risking silent zero-output rendering (Axis 1 fail-closed).

use crate::DocumentError;

/// Adapter contract for output renderers.
///
/// Renderers consume a [`crate::SafeBundle`] and produce a binary payload.
/// Implementations must respect the manifest contract — every byte they
/// emit must be either tokenized or non-PII per the bundle's
/// [`gaze::Manifest`].
pub trait Renderer {
    /// Render the bundle into a byte payload (PNG, PDF, ...).
    ///
    /// # Errors
    /// Implementations return [`DocumentError`] on backend failure.
    fn render(&self, bundle: &crate::SafeBundle) -> Result<Vec<u8>, DocumentError>;
}

/// Reserved fail-loud renderer.
///
/// Every call returns [`DocumentError::NotImplemented`] so accidental
/// wiring is caught at the call site.
#[non_exhaustive]
pub struct PendingRenderer {
    _private: (),
}

impl PendingRenderer {
    /// Build the fail-loud renderer.
    ///
    /// # Errors
    /// Always returns [`DocumentError::NotImplemented`]. The placeholder
    /// exists so adopters wiring a trait object without a concrete
    /// renderer fail at construction time rather than producing silent
    /// zero-output (Axis 1 fail-closed).
    pub fn new() -> Result<Self, DocumentError> {
        Err(DocumentError::NotImplemented(
            "PendingRenderer::new (wire a concrete Renderer impl)",
        ))
    }
}

impl Renderer for PendingRenderer {
    fn render(&self, _bundle: &crate::SafeBundle) -> Result<Vec<u8>, DocumentError> {
        Err(DocumentError::NotImplemented(
            "PendingRenderer::render (wire a concrete Renderer impl)",
        ))
    }
}
