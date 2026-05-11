//! Single-page PDF rasterization via [`pdfium-render`](https://crates.io/crates/pdfium-render).
//!
//! ## Runtime dependency
//!
//! `pdfium-render` dynamically loads the pdfium shared library at runtime.
//! Adopters must have `libpdfium` reachable to the process (system library,
//! `LD_LIBRARY_PATH` / `DYLD_LIBRARY_PATH`, or alongside the executable).
//!
//! Per-OS install guidance is surfaced in [`DocumentError::PdfiumNotFound`]
//! whenever binding fails.
//!
//! ## Scope (v0.0.x)
//!
//! * Only page index `0` is rasterized. Multi-page PDFs are accepted but the
//!   first page wins. Multi-page support is incremental on top.
//! * Target resolution: 150 DPI, configurable via [`PdfRasterConfig`].

use std::io::Cursor;
use std::path::Path;

use image::ImageFormat;
use pdfium_render::prelude::{PdfRenderConfig, Pdfium, PdfiumError};

use crate::DocumentError;

/// Configuration for one PDF rasterization pass.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct PdfRasterConfig {
    /// Target image width in pixels (height auto-scales).
    pub width_px: u32,
    /// Target image height in pixels (height auto-scales when 0).
    pub height_px: u32,
    /// Zero-based page index to rasterize.
    pub page_index: i32,
}

impl PdfRasterConfig {
    /// Default config: 1240×1754 (≈150 DPI A4) on page 0.
    pub fn new() -> Self {
        Self {
            width_px: 1240,
            height_px: 1754,
            page_index: 0,
        }
    }
}

impl Default for PdfRasterConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of rasterizing a PDF page.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct RasterizedPage {
    /// PNG-encoded image bytes.
    pub png_bytes: Vec<u8>,
    /// Page index that was rasterized.
    pub page_index: i32,
    /// Total page count in the source document.
    pub page_count: i32,
    /// Width in pixels of the rasterized page.
    pub width_px: u32,
    /// Height in pixels of the rasterized page.
    pub height_px: u32,
}

impl RasterizedPage {
    /// Build a [`RasterizedPage`] from already-encoded fields.
    pub fn new(
        png_bytes: Vec<u8>,
        page_index: i32,
        page_count: i32,
        width_px: u32,
        height_px: u32,
    ) -> Self {
        Self {
            png_bytes,
            page_index,
            page_count,
            width_px,
            height_px,
        }
    }
}

/// Rasterize a single page of a PDF on disk to PNG bytes.
///
/// # Errors
///
/// * [`DocumentError::PdfiumNotFound`] — pdfium dynamic library could not be
///   located. Payload carries per-OS install guidance.
/// * [`DocumentError::PdfRasterFailed`] — pdfium reported an error while
///   opening or rendering the document.
pub fn rasterize_first_page(
    path: &Path,
    config: PdfRasterConfig,
) -> Result<RasterizedPage, DocumentError> {
    let bindings = Pdfium::bind_to_system_library().map_err(|err| {
        DocumentError::PdfiumNotFound(format!("{}. {}", err, pdfium_install_hint()))
    })?;
    let pdfium = Pdfium::new(bindings);
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(map_pdfium_error)?;
    let pages = document.pages();
    let page_count = pages.len();
    if page_count == 0 {
        return Err(DocumentError::PdfRasterFailed(
            "input PDF contains zero pages".to_string(),
        ));
    }

    if config.page_index < 0 || config.page_index >= page_count {
        return Err(DocumentError::PdfRasterFailed(format!(
            "requested page index {} but document has {} page(s)",
            config.page_index, page_count
        )));
    }

    let page = pages
        .get(config.page_index)
        .map_err(map_pdfium_error)?;
    let mut render_config = PdfRenderConfig::new().set_target_width(config.width_px as i32);
    if config.height_px > 0 {
        render_config = render_config.set_maximum_height(config.height_px as i32);
    }
    let bitmap = page
        .render_with_config(&render_config)
        .map_err(map_pdfium_error)?;
    let dynamic_image = bitmap.as_image().map_err(map_pdfium_error)?;
    let (width, height) = (dynamic_image.width(), dynamic_image.height());

    let mut buf = Cursor::new(Vec::with_capacity(64 * 1024));
    dynamic_image
        .write_to(&mut buf, ImageFormat::Png)
        .map_err(|err| DocumentError::PdfRasterFailed(format!("png encode failed: {err}")))?;

    Ok(RasterizedPage {
        png_bytes: buf.into_inner(),
        page_index: config.page_index,
        page_count,
        width_px: width,
        height_px: height,
    })
}

fn map_pdfium_error(err: PdfiumError) -> DocumentError {
    DocumentError::PdfRasterFailed(err.to_string())
}

fn pdfium_install_hint() -> String {
    if cfg!(target_os = "macos") {
        "Download the pdfium dynamic library from https://github.com/bblanchon/pdfium-binaries \
         and place `libpdfium.dylib` on DYLD_LIBRARY_PATH, in /usr/local/lib, or next to your binary."
            .to_string()
    } else if cfg!(target_os = "linux") {
        "Download the pdfium dynamic library from https://github.com/bblanchon/pdfium-binaries \
         and place `libpdfium.so` on LD_LIBRARY_PATH, in /usr/local/lib, or next to your binary."
            .to_string()
    } else if cfg!(target_os = "windows") {
        "Download the pdfium dynamic library from https://github.com/bblanchon/pdfium-binaries \
         and place `pdfium.dll` on PATH or next to your executable."
            .to_string()
    } else {
        "Download the pdfium dynamic library from https://github.com/bblanchon/pdfium-binaries.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_config_defaults_to_first_page_150_dpi() {
        let cfg = PdfRasterConfig::new();
        assert_eq!(cfg.page_index, 0);
        assert_eq!(cfg.width_px, 1240);
        assert_eq!(cfg.height_px, 1754);
    }

    #[test]
    fn install_hint_is_non_empty() {
        assert!(!pdfium_install_hint().is_empty());
    }
}
