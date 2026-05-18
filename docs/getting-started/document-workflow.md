# Gaze Document Workflow

This page is an adopter setup guide for `gaze document clean`, the OSS document
ingestion path. For the extension contract, see
[`docs/architecture/document-extension.md`](../architecture/document-extension.md).

## When To Use

Use `gaze document clean` when the input is a PNG, JPG, or PDF and you need a
bundle that is safe to hand to an agent workspace:

```text
source document -> OCR / PDF text extraction -> Gaze redact -> SafeBundle
```

The output directory contains:

```text
clean.md
manifest.json
report.json
```

`clean.md` is the tokenized Markdown. `manifest.json` is the restorable
`gaze::Manifest`. `report.json` carries OCR, layout, and PII-count provenance.

## Prerequisites

- A `gaze` binary built with the `document` feature.
- `tesseract` on PATH for OCR.
- The pdfium runtime when PDF input is used.

Install from the repository:

```sh
cargo install --path crates/gaze-cli --features document
```

Install Tesseract with your platform package manager:

```sh
brew install tesseract
sudo apt-get install tesseract-ocr
```

For PDFs, install a pdfium shared library and make it visible to the runtime
with your platform's dynamic-library path.

## Spawn The Verb

Run OCR plus Gaze redaction:

```sh
gaze document clean ./invoice.pdf --out ./safe-bundle/
```

The source document can be a synthetic fixture such as an invoice PDF containing
`alice@example.invalid`. The output directory is created if it does not exist.

Successful stdout is a one-line JSON summary; the bundle files are written to
`--out`.

```text
safe-bundle/
  clean.md
  manifest.json
  report.json
```

## SafeBundle Anatomy

`clean.md` contains Markdown with PII replaced by reversible Gaze tokens. It is
the file to provide to the agent or model-facing workflow.

`manifest.json` contains the `gaze::Manifest` needed for restore. Keep it on the
owner side with the same controls used for other restore material.

`report.json` serializes `BundleReport`. New emissions use
`bundle_version = 2`; older v1 reports still deserialize, but v2 is the current
layout-report shape.

Important per-page fields:

- `page_index`: zero-based page index.
- `ocr_source`: `vector_pdf` for selectable PDF text or `ocr` for raster OCR.
- `ocr_backend`: backend name when OCR produced the page.
- `confidence`: normalized page confidence in `0.0..=1.0`.
- `low_confidence`: true when confidence is below the configured threshold.
- `column_count`: detected text column count.

Important top-level field:

- `low_confidence_threshold`: threshold used to set each page's
  `low_confidence` value. The default is `0.65`.

## Layout Report V2 Features

- Vector-PDF fallback: selectable text is extracted directly when the PDF page
  provides it.
- Multi-column segmentation: OCR spans are reordered into conservative reading
  order and report the detected column count.
- Table-cell preservation: table-like grids keep cell boundaries inline instead
  of being split as prose columns.
- Deskew preprocessing: raster input is normalized before OCR so Tesseract sees
  a more stable page image.

## `OcrBackend` Trait

`OcrBackend` is the narrow second-party extension point for OCR drivers:

```rust
pub trait OcrBackend: Send + Sync {
    fn name(&self) -> &str;
    fn recognize(&self, image: ImageInput, hints: OcrHints) -> Result<Vec<OcrSpan>, OcrError>;
}
```

The default backend is `TesseractBackend`. Alternative drivers receive finalized
image bytes and return flat spans with bounding boxes and optional confidence.
Magic-byte validation is mandatory before bytes are accepted as PNG, JPEG, or
TIFF image input; unsupported payloads fail closed before OCR.

## Restore Round-Trip

After an agent or model works with `clean.md`, pass the owner-retained
`manifest.json` plus the model output to the standard Gaze restore path. The
manifest is the authority for rehydration; do not ask the model to infer
original values from tokens.

The exact restore API depends on the embedding surface. CLI users should follow
the restore contract in
[`crates/gaze-cli/README.md#restore`](../../crates/gaze-cli/README.md#restore).

## Five-Axis Pitch

- Reliability: OCR output is normalized before redaction, and low-confidence
  pages are surfaced for downstream routing.
- Reversibility: `manifest.json` carries the same restore contract as the rest
  of Gaze.
- Agentic-first: `clean.md` is safe Markdown for agent workspaces; the owner
  keeps restore material.
- Trust: `report.json` records OCR source, backend, confidence, layout, and PII
  counts without raw PII.
- Adopter ergonomics: one CLI verb turns PNG, JPG, or PDF input into a
  three-file SafeBundle.

## Next Steps

- [`docs/architecture/document-extension.md`](../architecture/document-extension.md)
  — document extension contract and bundle boundary.
- [`crates/gaze-document/README.md`](../../crates/gaze-document/README.md) —
  runtime requirements, crate API, and feature flags.
