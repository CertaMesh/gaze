# gaze-document

[![Crates.io](https://img.shields.io/crates/v/gaze-document.svg)](https://crates.io/crates/gaze-document)
[![docs.rs](https://docs.rs/gaze-document/badge.svg)](https://docs.rs/gaze-document)
[![License](https://img.shields.io/crates/l/gaze-document.svg)](https://github.com/EmpireTwo/gaze#license)

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
gaze-document = "0.9.1"
```

### CLI

```bash
cargo install gaze-cli --version 0.9.1 --features document
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
    gaze_document::AgentBundleDir::new("./agent-bundle")?,
    gaze_document::OwnerBundleDir::new("./owner-vault")?,
)?;

// Tokenized Markdown safe to hand to an LLM.
let _ = &bundle.clean_markdown;

// Restorable manifest — pair with a `gaze::Session` to round-trip.
let _ = &bundle.manifest;

// Provenance: per-page extraction confidence + PII counts.
println!(
    "tokens={} first_page_confidence={:?}",
    bundle.report.pii_token_count,
    bundle.report.pages.first().and_then(|page| page.confidence),
);
# Ok::<(), gaze_document::DocumentError>(())
```

## Quickstart (CLI)

```bash
# Convenience shorthand: --out creates agent/ + owner/ subdirs
gaze document clean ./invoice.pdf --out ./safe/

# Explicit: caller controls both paths
gaze document clean ./invoice.pdf --agent-out ./agent-bundle/ --owner-out ./owner-vault/
```

Writes:

```
agent/
  clean.md        # OCR text with PII replaced by reversible tokens
  report.json     # BundleReport — OCR + PII counts + provenance
owner/
  manifest.json   # gaze::Manifest — restorable, canonical
```

Stdout carries a one-line JSON summary so callers can pipe it.

`manifest.json` carries restorable PII mapping material. It belongs in an
owner-only path; uploading it alongside `clean.md` to an LLM workspace defeats
pseudonymization. The split layout makes that axis-1 boundary a runtime
contract instead of caller discipline.

## Bundle on-disk shapes

* **`agent/clean.md`** — Markdown with a short header (`# gaze-document safe
  bundle`) plus the OCR text after token substitution.
* **`owner/manifest.json`** — serialized `gaze::Manifest` (re-exported from
  `gaze-types`). Compatible with `gaze restore` and the rest of the
  `gaze` runtime.
* **`agent/report.json`** — `BundleReport`. Schema versioned via
  `bundle_version: u32 = 2`; field set is `#[non_exhaustive]` so additive
  fields are SemVer-safe. Includes per-page extraction source
  (`vector_pdf` or `ocr`), OCR backend, normalized confidence,
  low-confidence flag, column count, per-class PII counts, PDF metadata,
  and the source kind. Existing v1 reports still deserialize; new emission
  is always v2. Full field-by-field catalog with stability per field:
  [`docs/metrics.md`](../../docs/metrics.md#6-safebundle--bundlereport-gaze-document).

## OCR brittleness + normalization

OCR is a lossy stage. Tesseract — like every engine — sometimes inserts
spurious whitespace between adjacent glyphs that share kerning. The most
common artifact in practice (and the most dangerous for axis-1
reliability) is a single space inserted next to the `@` of an email:

```text
jane.doe@example.invalid   →   "jane.doe @example.invalid"
```

The corrupted form is still unmistakably an email to a human or LLM but
slips past strict `\S+@\S+` recognizers. To keep the bundle safe to hand
to a model, `gaze-document` applies a narrow normalization pass between
the OCR adapter and the redact pipeline.

### Normalization rules

The full rule set is documented in source at
`crates/gaze-document/src/ocr/normalize.rs`. Today there is exactly one
rule:

* **Email separator repair.** Collapse intra-line horizontal whitespace
  immediately adjacent to `@` when both sides are non-whitespace.
  Pattern: `(\S)[ \t]*@[ \t]*(\S)` → `$1@$2`. Newline-adjacent `@`
  remains untouched.

Additional rules will land here as additional artifact classes are
discovered. Every rule lives next to the others in
`ocr::normalize`, doc-commented with its trigger, scope, and a worked
example.

### Brittleness limit

`gaze-document` assumes **mostly-clean OCR** — text where most glyphs
are recognized, line breaks are preserved, and only the documented
narrow artifacts (currently: whitespace around `@`) intrude on PII
shapes. Bundles produced from low-DPI rasterization, heavy noise, or
non-Latin scripts without the right `--lang` setting may still leak.
Two mitigations land at the test boundary so future drift fails loudly:

* The `tests/e2e.rs` fixtures assert with belt-and-braces negative
  substring checks (`!contains("@example.invalid")`, `!contains("Jane Doe")`,
  `!contains("555-0142")`) **in addition** to the positive `:Email_`,
  `:Name_`, `:Custom:phone_` token assertions.
* `BundleReport.pages[].confidence` and `pages[].low_confidence` are always
  surfaced to adopters. The default threshold is `0.65`, configurable with
  `gaze_document::Pipeline::with_low_confidence_threshold()`, so downstream
  gates can route low-confidence pages for human review.

If you observe a new artifact class slipping through, file an issue
with the OCR output and the expected normalization shape; the fix
belongs in `ocr::normalize` alongside the existing rules.

## MCP feature

Enable `mcp` to register two agent-tier tools with `gaze-mcp-core`:
`gaze_read_text` for already-extracted text and `gaze_read_file` for PNG,
JPG, or PDF paths. Hosts still call them through `PiiEnvelope::dispatch`,
so args, responses, manifest rows, and auth stay on the MCP chokepoint.

```rust,no_run
use std::sync::Arc;

use gaze_document::mcp::{self, GazeReadOpts};
use gaze_mcp_core::ToolRegistry;
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};

let mut registry = ToolRegistry::new();
mcp::register_tools(&mut registry, GazeReadOpts::default())?;

let frontend = RmcpFrontend::stdio(Arc::new(
    FixedPrincipalResolver::agent("local-stdio"),
));
# Ok::<(), gaze_mcp_core::ToolRegistryError>(())
```

Both tools return a JSON object:

```json
{
  "clean_markdown": "# gaze-document safe text\n\n...",
  "manifest_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  "file_metadata": {
    "source_kind": "text",
    "ocr_mean_confidence": null,
    "bundle_version": 2,
    "page_count": null
  }
}
```

`gaze_read_file` defaults to a 25 MiB input cap. Override it with
`GazeReadFile::with_max_file_size(bytes)` or `GazeReadOpts`.
`gaze-cli` provides `gaze mcp install`, `gaze mcp doctor`, and `gaze mcp serve`;
this crate only provides the opt-in tool implementations.

## Feature flags

| Feature           | Default | What it enables                                      |
|-------------------|---------|------------------------------------------------------|
| `ocr-tesseract`   | yes     | Tesseract subprocess OCR backend + `clean()` entry.  |
| `pdf-input`       | yes     | `pdfium-render` PDF text extraction + raster OCR fallback. |
| `mcp`             | no      | `gaze_read_file` + `gaze_read_text` Tool impls.      |
| `extract-docling` | no      | Reserved — future Docling layout adapter.            |
| `render-image`    | no      | Reserved — future redacted-preview renderer.         |

The `extract-docling` and `render-image` features are intentionally empty
in v0.9.1 so adopters can pin against the eventual flag names early.

## License

Dual-licensed under either of [Apache-2.0](https://github.com/EmpireTwo/gaze/blob/main/LICENSE-APACHE) or [MIT](https://github.com/EmpireTwo/gaze/blob/main/LICENSE-MIT), at your option.
