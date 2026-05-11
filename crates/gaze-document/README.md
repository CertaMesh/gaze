# gaze-document

Reversible PII pseudonymization for **documents** — image + single-page PDF →
clean Markdown + a restorable `gaze::Manifest` + an OCR/PII report. Powers
the `gaze document clean` CLI verb on top of the same `gaze-pii` runtime
that handles streaming and structured inputs.

The crate inherits the project's [north star](../../CLAUDE.md): zero PII
leaks from agent to data owner, deterministic detection, and a manifest
contract that always restores. OCR is a subprocess call to the standard
`tesseract` binary so adopters never need a native build toolchain.

## Install

### Library

```toml
[dependencies]
gaze-document = "0.0.1"
```

### CLI

```bash
cargo install gaze-cli --features document
```

The `document` feature is opt-in on `gaze-cli` so the default install stays
free of OCR / PDF dependencies.

## Runtime requirements

### Tesseract

`gaze-document` shells out to the `tesseract` CLI (Tesseract 4 or 5).

| Platform     | Install                                              |
|--------------|------------------------------------------------------|
| macOS        | `brew install tesseract`                             |
| Debian/Ubuntu| `sudo apt-get install tesseract-ocr`                 |
| Fedora       | `sudo dnf install tesseract`                         |
| Arch         | `sudo pacman -S tesseract`                           |
| Windows      | `winget install --id UB-Mannheim.TesseractOCR`       |

If the binary is missing, `clean()` returns
`DocumentError::TesseractNotFound` with a per-OS install hint in the
message — fail-loud by design (Axis 1 reliability).

### pdfium (only for PDF input)

PDF rasterization uses [`pdfium-render`](https://crates.io/crates/pdfium-render),
which loads the **pdfium** shared library at runtime. Prebuilt binaries
for every major OS / arch are published by
[`bblanchon/pdfium-binaries`](https://github.com/bblanchon/pdfium-binaries):

| Platform      | What to do                                                                |
|---------------|---------------------------------------------------------------------------|
| macOS (arm64) | Download `pdfium-mac-arm64.tgz`; place `lib/libpdfium.dylib` on `DYLD_LIBRARY_PATH` or in `/usr/local/lib`. |
| macOS (x64)   | Download `pdfium-mac-x64.tgz`; same placement.                            |
| Linux (x64)   | Download `pdfium-linux-x64.tgz`; place `lib/libpdfium.so` on `LD_LIBRARY_PATH` or in `/usr/local/lib`. |
| Windows       | Download `pdfium-win-x64.zip`; place `pdfium.dll` on `PATH` or next to the binary. |

Image-only workflows (PNG / JPG) do **not** need pdfium.

## Quickstart (library)

```rust,no_run
use std::path::Path;

let bundle = gaze_document::clean(
    Path::new("invoice.pdf"),
    Path::new("./safe-out"),
)?;

// Tokenized Markdown safe to hand to an LLM.
let _ = &bundle.clean_markdown;

// Restorable manifest — pair with a `gaze::Session` to round-trip.
let _ = &bundle.manifest;

// Provenance: OCR confidence + PII counts.
println!(
    "tokens={} confidence={:?}",
    bundle.report.pii_token_count,
    bundle.report.ocr_mean_confidence,
);
# Ok::<(), gaze_document::DocumentError>(())
```

## Quickstart (CLI)

```bash
gaze document clean ./invoice.pdf --out ./safe/
```

Writes:

```
safe/
  clean.md        # OCR text with PII replaced by reversible tokens
  manifest.json   # gaze::Manifest — restorable, canonical
  report.json     # BundleReport — OCR + PII counts + provenance
```

Stdout carries a one-line JSON summary so callers can pipe it.

## Bundle on-disk shapes

* **`clean.md`** — Markdown with a short header (`# gaze-document safe
  bundle`) plus the OCR text after token substitution.
* **`manifest.json`** — serialized `gaze::Manifest` (re-exported from
  `gaze-types`). Compatible with `gaze restore` and the rest of the
  `gaze` runtime.
* **`report.json`** — `BundleReport`. Schema versioned via
  `bundle_version: u32 = 1`; field set is `#[non_exhaustive]` so additive
  fields are SemVer-safe. Includes OCR confidence, per-class PII counts,
  PDF metadata, and the source kind.

## Feature flags

| Feature           | Default | What it enables                                      |
|-------------------|---------|------------------------------------------------------|
| `ocr-tesseract`   | yes     | Tesseract subprocess OCR backend + `clean()` entry.  |
| `pdf-input`       | yes     | `pdfium-render` PDF rasterization (single page).     |
| `extract-docling` | no      | Reserved — future Docling layout adapter.            |
| `render-image`    | no      | Reserved — future redacted-preview renderer.         |

The `extract-docling` and `render-image` features are intentionally empty
in v0.0.x so adopters can pin against the eventual flag names early.

## License

Apache-2.0
