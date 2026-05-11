# gaze-document

Experimental document ingestion + safe-bundle generation for the Gaze runtime.

This crate is a scaffold (todo 728). The public surface is intentionally
fail-loud — every trait and constructor returns `unimplemented!()` or an
explicit `Err(...)` until concrete adapters land in follow-up PRs.

See the parent repository `README.md` and `CHANGELOG.md` for status.

## Status

- **Stability:** experimental, pre-0.1.
- **Publish:** `publish = false`. Not yet on crates.io.
- **Scope:** PDF / image / scanned-document ingestion → clean Markdown + a
  signed safe-bundle that pairs with `gaze` (`gaze-pii`) tokenization.

## License

Apache-2.0
