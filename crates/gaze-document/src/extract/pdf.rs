//! PDF text extraction and rasterization via [`pdfium-render`](https://crates.io/crates/pdfium-render).
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
//! * Selectable text is extracted directly, per page, before OCR is attempted.
//! * Pages with no selectable text are rasterized at 150 DPI by default.

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

/// Per-page PDF extraction payload.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum PdfPagePayload {
    /// Page had selectable text and did not require OCR.
    VectorText {
        /// Extracted text in PDF text order.
        text: String,
        /// Zero-based page index.
        page_index: i32,
        /// Total page count in the source document.
        page_count: i32,
    },
    /// Page had no selectable text and was rasterized for OCR.
    Raster(RasterizedPage),
}

impl PdfPagePayload {
    /// Zero-based page index.
    pub fn page_index(&self) -> i32 {
        match self {
            Self::VectorText { page_index, .. } => *page_index,
            Self::Raster(page) => page.page_index,
        }
    }

    /// Total page count in the source document.
    pub fn page_count(&self) -> i32 {
        match self {
            Self::VectorText { page_count, .. } => *page_count,
            Self::Raster(page) => page.page_count,
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

    let page = pages.get(config.page_index).map_err(map_pdfium_error)?;
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

/// Extract every PDF page, routing selectable-text pages directly and
/// rasterizing image-only pages for OCR.
///
/// # Errors
///
/// Returns [`DocumentError`] when pdfium cannot open the document or a page
/// cannot be rasterized.
pub fn extract_pages(
    path: &Path,
    config: PdfRasterConfig,
) -> Result<Vec<PdfPagePayload>, DocumentError> {
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

    let mut out = Vec::with_capacity(page_count as usize);
    for page_index in 0..page_count {
        let page = pages.get(page_index).map_err(map_pdfium_error)?;
        let text = page
            .text()
            .ok()
            .map(|page_text| normalize_pdf_text(&page_text.all()))
            .unwrap_or_default();
        if !text.trim().is_empty() {
            out.push(PdfPagePayload::VectorText {
                text,
                page_index,
                page_count,
            });
            continue;
        }

        out.push(PdfPagePayload::Raster(render_page(
            &page, page_index, page_count, config,
        )?));
    }

    Ok(out)
}

fn render_page(
    page: &pdfium_render::prelude::PdfPage<'_>,
    page_index: i32,
    page_count: i32,
    config: PdfRasterConfig,
) -> Result<RasterizedPage, DocumentError> {
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
        page_index,
        page_count,
        width_px: width,
        height_px: height,
    })
}

fn normalize_pdf_text(text: &str) -> String {
    text.replace('\0', "")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
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
        "Download the pdfium dynamic library from https://github.com/bblanchon/pdfium-binaries."
            .to_string()
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

    #[test]
    fn normalize_pdf_text_removes_nuls_and_outer_blank_space() {
        assert_eq!(normalize_pdf_text(" hello\0 \n\n"), "hello");
    }
}
