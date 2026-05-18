# Document Extension Architecture

`DocumentExtension` is the v0.7.x upstream hook for `gaze-document`. It lets a
document bundle bind document metadata into the same signed owner-only
`SensitiveSnapshot` that already restores tokens.

## Boundary

Only the owner-side snapshot envelope may contain reversible PII. Agent-facing
files must be safe to upload to an LLM workspace as a unit.

```text
agent_out/
  clean.md
  report.json

owner_out/
  manifest.json
```

`agent_out` is the agent-shippable directory. `owner_out/manifest.json` is
owner-only restore material. The original `manifest.bin` signed-envelope binding
remains a v0.11+ Design B follow-up; v0.10 Design A keeps the shipped JSON
manifest and enforces the partition with `AgentBundleDir` / `OwnerBundleDir`
newtypes plus path validation.

## File Shapes

### clean.md

`clean.md` is UTF-8 clean text containing Gaze tokens only. It has no
frontmatter, comments, or duplicated metadata. Byte spans are relative to this
normalized file.

### manifest.json

`manifest.json` is the shipped `gaze::Manifest` restore mapping. It is the only
v0.10 bundle file that can carry reversible PII, so it stays in `owner_out`.
Moving the owner restore material to the signed snapshot envelope
(`Session::export_with_extension` -> `manifest.bin`) is deferred to v0.11+.

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
let extension = DocumentExtension::builder(1)
    .clean_md_sha256([1; 32])
    .layout_json_sha256([2; 32])
    .report_json_sha256([3; 32])
    .page_count(1)
    .audit_session_id(session.audit_session_id())
    .build()?;

let manifest_bin = session.export_with_extension(extension)?.into_bytes();
```

`Session::export()` remains unchanged for text-only adopters. `Session::import`
continues to restore both plain v3 and document-extended v4 snapshots.

## Shipped in v0.7.1

`gaze-document` now ships the OSS document-ingestion path with PNG/JPG/PDF
input, Tesseract OCR, optional PDF rasterization, `write_bundle` runtime
separation, and a versioned `BundleReport` with `bundle_version = 2`. The
signed `DocumentExtension` envelope shown above is still the intended Design B
integrity upgrade, not the v0.10 on-disk owner manifest.
