# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- v0.4.1 Bundle P1 foundation: `gaze-assembly` library entrypoint, `xtask` scaffold, and the `symmetric_potemkin_gate` workflow.
- `token.family` now threads from recognizers into session snapshot entries while preserving the existing emitted token grammar.
- Locale-aware regex `pattern_template` lowering for `{locale_email_headers}` with English and German defaults.
- `capture_groups = [...]` regex span narrowing with first-non-empty semantics.
- `NerRecognizer` public export plus `[ner] threshold` policy knob using min-aggregated span confidence.
- Core `email.header.name` recognizer for RFC822-style header display names, including German `Von:` / `An:` forms.
- Strict rulepack composition validation: same-class recognizer pairs now require explicit `cooperates_with` declarations.
- `Context::fields_typed() -> ContextFieldsRef<'_>` borrowed accessor for context-field consumers.
- `gaze clean --audit-db=<path>` persists the metadata-only SQLite redaction log for pipe-mode invocations.
- `gaze clean` now exposes three-surfaces CLI overrides for existing policy runtime knobs: `--session-scope`, `--ner-model-dir`, `--ner-locale`, `--rulepack-bundled`, and `--rulepack-path`.
- Opt-in `core-extended` bundled rulepack with Phase 1 shape-only recognizers for E.164 phone numbers, IPv4/IPv6 addresses, and `de-DE`/`en-US` postal codes.
- v0.5 design doc for open-key `PiiClass` and decision-deferred crate-shape Option B sketch.
- Three-surfaces parity audit table for every `policy.toml` field, classifying runtime knobs with CLI/TOML/default coverage and policy-document fields that intentionally remain TOML-only.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.1`.
- Snapshot envelope version bumped from 2 to 3; v0.4.1 imports v2 snapshots with default `counter` family, while v0.4.0 rejects v3 snapshots instead of silently collapsing family metadata.
- Dictionary recognizer audit sources now include per-term traceability as `dictionary:{name}[#term_index]`.

### Fixed

- Markus adopter dogfood gap closed: locale-aware email-header recognizer (`Von:` / `An:` plus English defaults) tokenizes header display names and restores them round-trip. See GH #24.
- `[ner] threshold` knob un-deferred from v0.4.2 so adopters can tune the NER confidence floor for prompt-preamble PII.
- Template lowering now preserves regex quantifiers such as `{0,3}` and keeps locale-header alternation non-capturing, so capture-group span narrowing remains stable.

## [0.4.0-rc.1] - 2026-04-24

### Added

- **F3 Rulepack schema** - TOML-defined recognizer bundles with closed validator/normalizer kind registry. Fail-closed on unknown matchers (Dictionary now wired; NER deferred to v0.5).
- **F4 Locale infrastructure** - 4-tier chain (CLI > policy > rulepack defaults > system default). Per-recognizer locale gating via `locales = [...]`. Strict opaque-tag matching.
- **F2-full Resolver** - class-priority > rule-priority > score > span-length > recognizer-id with multi-overlap fixed-point iteration.
- **F5 `.invalid` domain swap** - FPE email shape now uses `email{N}.{session_hex}@gaze-fake.invalid`. Legacy `example.test` Pass 2 trap arm retained for v0.3 manifest restore compatibility.
- **F6 Dictionary detector** - Aho-Corasick-backed recognizer registered through the new Recognizer trait. Adopter-tunable via `[[policy.custom_recognizers]]` or `--context-json` (standalone).
- **Typed Context envelope** - `--context-json` carries tenant fields/dictionaries/class_map through `DetectContext` into per-recognizer detection (no longer parsed-and-dropped).
- **F7.5 Byte-range-skip** - Pass 1 substitution spans tracked; Pass 2 trap scan skips matches fully contained in spans. Closes Pass 1->Pass 2 cascade false-positive (adopter raw values matching trap arms no longer rejected in strict mode).
- **Audit symmetry** - `RedactionEntry.decided_by` ConflictTier enum + merge-loser entries.
- **Schema-drift gating** - `RulepackError::UnsupportedFieldInB1` rejects `token.family`, `token.format`, `context.hotwords`, `context.boost`, `context.window` if set to non-default until consumers ship in v0.4.1.

### Changed

- **Pipeline**: legacy `Detector` trait path removed. All detection routes through `RecognizerRegistry`.
- **Policy surface**: legacy top-level `[[detector]]` rejected with `LegacyDetectorUnsupported` error; migrate to `[[policy.custom_recognizers]]`.
- **Locale tag matching**: `LocaleTag::Other(_)` now strict-equals (no longer universal fallback).

### Fixed

- NER label-map BIO-prefix resolution (already shipped in v0.3.1; folded into rc series for completeness).
- Cascade false-positive on adopter tenant identifiers (`Order_42`, `Song_42`, `User_7`) under strict mode (PR #22).

### Known limits - please test in dogfood

- **GH #24**: NER context-sensitivity gap - names in prompt boilerplate / RFC822 email headers may pass through default davlan-hrl. Workarounds + roadmap in issue #24.
- **token.family / token.format**: parsed + gated; runtime consumers planned for v0.4.1.
- **context.hotwords / boost / window**: parsed + gated; runtime consumers planned for v0.4.1.
- **Per-term traceability** in dictionary detection log: `dictionary:{name}` only; `[#term_index]` extension planned for v0.4.1.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>

## [v0.3.1] — 2026-04-24

### Fixed

- **NER silent no-op with BIO-prefixed labels.json.** `LabelMap::resolve`
  now accepts both BIO-prefixed (`B-PER`, `B-LOC`) and bare (`PER`, `LOC`)
  label keys. Previously, bundles shipping BIO-prefixed labels (the standard
  Davlan/HuggingFace format) produced zero detections silently. Adopters on
  aarch64-apple-darwin were particularly affected. (#19)
- Spec-drift: `[session]` policy.toml key now authoritative over
  `--session-ttl`. (#16)
- Spec-drift: Broken `[ner] model_dir` exits `PolicyConfig` (exit code 2)
  instead of silently degrading. (#16)
- Spec-drift: `kind = "column"` policy rules rejected by `gaze clean` CLI
  load. (#16)

### Added

- `tracing::info!("ner detector registered, N backends")` on NER bootstrap -
  adopters can now confirm whether [ner] block is being picked up. (#19)
- `tracing::warn!` on zero-overlap (NER inference ran but emitted 0 entities
  for input class) - surfaces silent detection failures. (#19)

### Changed

- README hero copy + project north star documentation refresh. (#15)
- Roadmap documentation for v0.4 / v0.4.1 / v0.5. (docs-only,
  offsite-readable)

## [0.3.0] — 2026-04-24

### Changed

- **Counter-family tokens now wrap in angle brackets.** `<{session_hex}:Email_1>`,
  `<{session_hex}:Name_1>`, `<{session_hex}:Custom:order_id_1>`. Format-preserving email tokens
  (`email1.{session_hex}@gaze-fake.invalid`) stay bare — angle brackets defeat the
  format-preserving purpose.

### Added

- **`crate::token_shape` module** exposing `pattern()` +
  `contains_token()`. Centralizes the token grammar the CLI's Pass 2
  hallucination detector uses. Drift-gate fixture forces compile
  errors if `PiiClass` grows without grammar updates.
- **Exhaustive Pass 1 + Pass 2 regex for wrapped tokens.** Pass 1 uses
  a delimiter-sensitive match (angle brackets serve as explicit
  delimiters); Pass 2 whitelists via `contains_token()`.
- **`docs/policy.md`** — user-facing `policy.toml` authoring guide.

### Fixed

- PR #10 follow-up — `Custom:` namespace round-trip + hallucination
  tests.
- **Homebrew formula SHA placeholders replaced** with the real
  `gaze-aarch64-apple-darwin` digest
  (`baa7edb79d84fea5d74377f82877c5069d861381a9f6012aa55af2264a8287f4`)
  once the tag-triggered release workflow published the binary. Closes
  the rc.1 "Known gaps" entry — `brew install Naoray/gaze/gaze` now
  resolves without the cask fallback.

## [0.3.0-rc.2] — 2026-04-23

Same contents as rc.1 — only the release workflow matrix changed
(x86_64-apple-darwin dropped). rc.1 was tagged but its workflow never
published a release: the `macos-13` Intel runner pool could not
allocate a runner for the x86_64 build, leaving the release job blocked
on an unmet dependency. Markus is on Apple Silicon, so dropping x86_64
for rc unblocks the adapter retarget immediately; Intel + Linux return
in a later rc when runner strategy is worked out.

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

[Unreleased]: https://github.com/Naoray/gaze/compare/v0.4.0-rc.1...HEAD
[0.4.0-rc.1]: https://github.com/Naoray/gaze/releases/tag/v0.4.0-rc.1
[v0.3.1]: https://github.com/Naoray/gaze/releases/tag/v0.3.1
[0.3.0]: https://github.com/Naoray/gaze/releases/tag/v0.3.0
[0.3.0-rc.2]: https://github.com/Naoray/gaze/releases/tag/v0.3.0-rc.2
[0.3.0-rc.1]: https://github.com/Naoray/gaze/releases/tag/v0.3.0-rc.1
