# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0-rc.1] — 2026-04-23

First release candidate of the standalone `gaze` CLI. Ships the
subprocess contract that language-specific adapters (e.g.
`gaze-laravel`) target. Library API surface continues to evolve in
parallel — the CLI protocol is the stable seam.

### Added

- **Standalone `gaze` CLI with pipe-mode subcommands.** `gaze clean`
  consumes plaintext on stdin and emits `{text, session_blob}`;
  `gaze restore` consumes `{text, session_blob}` and emits the
  rehydrated original. Adapters shell out rather than linking the
  library.
- **Two-pass restore.** First pass matches exact tokens via
  `Session::tokens()`; second pass runs a shape validator over the
  surviving text to catch reformatted token placeholders. Addresses
  the counselors-review finding that single-pass restore silently
  skipped renders.
- **Session TTL enforcement.** Snapshots carry `issued_at` and
  `Session::import` rejects blobs past the configured TTL with a
  `BlobExpired` error (CLI exit bucket 3). Prevents stale blobs from
  leaking tokens across restarts.
- **Policy TOML loader.** `Policy::load` parses a user-supplied
  `policy.toml`; `Pipeline::from_policy` builds the detection engine
  from it. `gaze --policy path/to/policy.toml` wires the file into the
  CLI.
- **Typed `CliError` variants with exit buckets and stderr JSON
  protocol.** `UnknownToken`, `Tamper`, `VersionByte`, `EmptyInput`,
  `InvalidEncoding`, `BlobExpired`, `MaxBytes`, plus a panic hook that
  funnels unexpected failures into the same structured protocol.
- **`--max-bytes` input size cap.** Rejects oversize input with a
  structured error instead of allocating unbounded buffers.
- **`--session-ttl` flag.** Overrides the default blob lifetime per
  invocation.
- **`--format=json` flag.** Stats output (`{detections, runtime_ms,
  ...}`) for adapter observability.
- **Pipe-mode integration suite.** Roundtrip, canary, `UnknownToken`,
  tamper, version-byte, argv, panic, and stats coverage.
- **Homebrew formula skeleton** at `dist/homebrew/gaze.rb`. SHAs
  filled post-release.
- **GitHub Actions release workflow** at `.github/workflows/release.yml`.
  Tag-triggered macOS builds (darwin-arm64 + darwin-x86_64).

### Changed

- **Workspace refocus: ghostwriter crate removed.** v0.2's
  language-specific `ghostwriter` crate was deleted in favour of the
  channel-agnostic `gaze` CLI. Adapters now consume the subprocess
  contract instead of linking a Rust library.
- **Custom class namespace fix.** Custom-class tokens are emitted as
  `Custom:{name}_N` rather than colliding with built-in class names.
- **`stats.detections` counter excludes `Preserve`.** Preserve-action
  hits are not real detections; they no longer inflate the count.
  Dead `Structured` dispatch branch dropped.

### Fixed

- Session snapshot payload carries an `issued_at` timestamp — previous
  layout had no basis for TTL enforcement.

### Known gaps (deferred)

- **Linux x86_64 binary not built.** The `ort` (ONNX runtime)
  dependency needs bundled system libraries; folded into a later rc
  to avoid blocking Markus on the adapter retarget.
- **Homebrew SHAs are placeholders** until the workflow publishes the
  darwin binaries; follow-up commit fills them.

[Unreleased]: https://github.com/Naoray/gaze/compare/v0.3.0-rc.1...HEAD
[0.3.0-rc.1]: https://github.com/Naoray/gaze/releases/tag/v0.3.0-rc.1
