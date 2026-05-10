# Document Extension Architecture

`DocumentExtension` is the v0.7.0 upstream hook for `gaze-document`. It lets a
document bundle bind document metadata into the same signed owner-only
`SensitiveSnapshot` that already restores tokens.

## Boundary

Only the owner-side snapshot envelope may contain reversible PII. Agent-facing
files must be safe to upload to an LLM workspace as a unit.

```text
<base>-agent/
  clean.md
  layout.json
  preview-redacted.png
  report.json

<base>-owner/
  manifest.bin
```

`<base>-agent/` is the agent-shippable directory. `<base>-owner/manifest.bin` is
owner-only restore material produced by `Session::export_with_extension`.

> v0.7.0 Phase 1 followup: runtime enforcement of `<base>-agent/` vs
> `<base>-owner/` separation lands in `Bundle::write` (v0.7.0 Phase 1 PR,
> tracked by Solo todo 751). No raw byte may land in `<base>-agent/` except via
> tokenization.

## File Shapes

### clean.md

`clean.md` is UTF-8 clean text containing Gaze tokens only. It has no
frontmatter, comments, or duplicated metadata. Byte spans are relative to this
normalized file.

### manifest.bin

`manifest.bin` is the existing signed snapshot envelope with an optional
`DocumentExtension` field inside the payload. It is the only v0.7.0 bundle file
that can carry reversible PII, so it stays in `<base>-owner/`.

### layout.json

`layout.json` carries geometry, reading order, coordinate-space metadata, and
pointers into `clean.md`. It must not include raw OCR text, source filenames,
PDF metadata, EXIF fields, or codec stderr/stdout.

### preview-redacted.png

`preview-redacted.png` is an advisory redacted preview with boxes burned into
pixels. Its metadata is not authoritative; the signed snapshot extension is the
integrity root.

### report.json

`report.json` is metadata-only: status, codec provenance, capability flags,
counts, warning codes, and safety-net stats. It must not contain raw PII or
token restore values.

## Versioning

`DocumentExtension::schema_version` is a single bundle-level `u16`. It versions
the bundle contract as one unit. Sub-files do not carry independent schema
versions because spans and integrity data cross file boundaries.

## Rust Hook

```rust
use gaze::{DocumentExtension, Scope, Session};

let session = Session::new(Scope::Conversation("doc-1".to_string()))?;
let extension = DocumentExtension::new(1);

let manifest_bin = session.export_with_extension(extension)?.into_bytes();
```

`Session::export()` remains unchanged for text-only adopters. `Session::import`
continues to restore both plain v3 and document-extended v4 snapshots.

## v0.7.1 Adapter Path

The `gaze-document` crate, real codec registry, Tesseract adapter, PDFium
adapter, and owner-side `manifest.index.json` are v0.7.1+ work. v0.7.0 only
locks the upstream value contracts and session export hook needed by those
adapters.
