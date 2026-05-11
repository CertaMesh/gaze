//! Synthetic-fixture generator for the gaze-document e2e suite.
//!
//! Run with:
//!
//! ```ignore
//! cargo run -p gaze-document --example gen_fixtures
//! ```
//!
//! Writes `testdata/synthetic_image.png` + `testdata/synthetic_doc.pdf` plus
//! their `*.expected.json` companions. The fixtures contain only synthetic
//! PII (origin tag: `synthetic-rust-generator`) — never real personal data.
//!
//! The generator is intentionally a separate `[[example]]` so it does not
//! pull `imageproc` / `ab_glyph` into the shipped library graph; both are
//! `[dev-dependencies]` only.

use std::fs;
use std::path::PathBuf;

use ab_glyph::{FontVec, PxScale};
use image::{Rgb, RgbImage};
use imageproc::drawing::draw_text_mut;

const LINES: &[&str] = &[
    "Bill to: Jane Doe",
    "Email: jane.doe@example.com",
    "Phone: +1-555-0142",
];

const FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let testdata = manifest_dir.join("testdata");
    fs::create_dir_all(&testdata)?;

    let font_bytes = load_font()?;
    let font = FontVec::try_from_vec(font_bytes)?;
    let scale = PxScale::from(28.0);

    let mut image = RgbImage::from_pixel(800, 240, Rgb([255, 255, 255]));
    for (idx, line) in LINES.iter().enumerate() {
        let y = 30 + (idx as i32) * 56;
        draw_text_mut(&mut image, Rgb([0, 0, 0]), 30, y, scale, &font, line);
    }
    image.save(testdata.join("synthetic_image.png"))?;
    fs::write(
        testdata.join("synthetic_image.expected.json"),
        expected_json("png"),
    )?;

    fs::write(testdata.join("synthetic_doc.pdf"), build_pdf_bytes())?;
    fs::write(
        testdata.join("synthetic_doc.expected.json"),
        expected_json("pdf"),
    )?;

    eprintln!("wrote fixtures to {}", testdata.display());
    Ok(())
}

fn load_font() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    for path in FONT_CANDIDATES {
        if let Ok(bytes) = fs::read(path) {
            return Ok(bytes);
        }
    }
    Err("no system font found in candidate list; install DejaVuSans or Arial".into())
}

fn expected_json(kind: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "origin": "synthetic-rust-generator",
        "kind": kind,
        "expected_classes": ["email", "custom:phone"],
        "must_not_contain_substrings": ["jane.doe@example.com", "555-0142"]
    }))
    .expect("expected fixture json")
}

/// Minimal hand-written single-page PDF with three text lines.
///
/// Uses standard Helvetica (built into every PDF reader) so we don't have to
/// embed a font. The PDF is small, deterministic, and renderable by pdfium.
fn build_pdf_bytes() -> Vec<u8> {
    let content_stream = b"BT\n/F1 16 Tf\n50 720 Td\n(Bill to: Jane Doe) Tj\n0 -28 Td\n(Email: jane.doe@example.com) Tj\n0 -28 Td\n(Phone: +1-555-0142) Tj\nET\n";

    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    let mut content_obj = format!("<< /Length {} >>\nstream\n", content_stream.len()).into_bytes();
    content_obj.extend_from_slice(content_stream);
    content_obj.extend_from_slice(b"endstream");
    objects.push(content_obj);

    let mut out = Vec::with_capacity(1024);
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (idx, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        let header = format!("{} 0 obj\n", idx + 1);
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = out.len();
    out.extend_from_slice(b"xref\n");
    let count = objects.len() + 1;
    out.extend_from_slice(format!("0 {count}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
        )
        .as_bytes(),
    );
    out
}
