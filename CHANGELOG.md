# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

### Changed

- [bundle-tokenization-drift] v0.5.1 rulepack_version sync refreshed `core` and `core-extended` no-policy snapshots; only the `rulepack_version` field changed.

### Deprecated

### Removed

### Fixed

### Security

## [0.5.1] - 2026-04-29

### Fixed

- **Bundled rulepack version sync (todo #267):** corrective patch - bundled `core`, `core-extended`, `locale-de`, and `locale-en` rulepacks now report `rulepack_version = "0.5.1"`, restoring the v0.4.6 CHANGELOG contract that bundled rulepacks track `gaze-recognizers`. v0.5.0 release-prep missed the embedded TOMLs; this patch corrects that.

### Changed

- Version bump 0.5.0 -> 0.5.1 across `gaze`, `gaze-types`, `gaze-recognizers`, `gaze-audit`, `gaze-cli`, and `gaze-assembly`.
- [bundle-tokenization-drift] v0.5.1 rulepack_version sync refreshed `core` and `core-extended` no-policy snapshots; only the `rulepack_version` field changed.

## [0.5.0] - 2026-04-27

### Added

- **v0.5 Phase B — `gaze-types` crate (PR #74, commit `4675b79`):** new shared-contract crate hosts `Recognizer`, `Detection`, `PiiClass`, `Action`, `RedactionEntry`, `LocaleTag` / `LocaleChain` / `LocaleError`, `RawDocument`, `CleanDocument`, `DictionaryBundle`, and the token-related value types. Adopters now get a serde-only contract crate without `ort`/`tokenizers`/`ndarray` ML deps in their dependency tree. `gaze` re-exports the contracts under their previous paths for source-compatibility.
- **v0.5 Phase B — `bundled-recognizers` feature gate (PR #74):** `gaze` no longer pulls `ort`/`tokenizers`/`ndarray`/`onig` in `--no-default-features` builds. Default features remain unchanged, so existing CLI / library consumers see no behavior change.
- **v0.5 Phase B — `DictionaryBundleExt` extension trait (PR #74):** `bundle.from_context(&ctx)` now requires `use gaze::DictionaryBundleExt;` (or import from `gaze-types`). The split keeps `gaze-types::DictionaryBundle` a pure value type while preserving the convenience constructor for `gaze` callers.
- **v0.5 Phase B — `DictionaryEntry::try_new` validated construction (PR #74):** empty term lists and non-ASCII case-insensitive entries fail closed at construction time rather than reaching the recognizer registry. `DictionaryEntry::new` is replaced by the validated `try_new`.
- **v0.5 Phase C — `gaze-audit` crate (PR #75, commit `64b6394`):** new passive-sink crate hosts `SqliteLogger`, `AuditFilter`, `AuditLogRow`, `build_audit_query_sql`, and `AUDIT_RESTRICTED_COLUMNS`. `gaze` no longer carries `rusqlite` in its default or `--no-default-features` graphs.
- **v0.5 Phase C — `audit` feature shim on `gaze` (PR #75):** one-minor migration window. `gaze = { features = ["audit"] }` re-exports `gaze::SqliteLogger` and the audit-query symbols by adding `gaze-audit` as a normal dependency. Scheduled to be removed in v0.6 (decision drawer `gaze_decisions_6c60bce3b9f8ed7a4de538d8`).
- **v0.5 Phase C — `cargo-metadata-audit-isolation` xtask gate (PR #75):** parses `cargo metadata --format-version=1` and fails closed if any non-audit-responsible workspace member has a normal-dependency path to `gaze-audit` in default or `--no-default-features` graphs. The audit-responsible allowlist is documented in source; `gaze-cli` is the only allowed consumer because its `audit` subcommands run against the passive sink directly.
- **v0.5 Phase C — `cargo deny` audit-feature ban (PR #75):** denies enabling `gaze`'s `audit` feature outside the dedicated compatibility tests, blocking accidental reintroduction of `gaze-audit` into the protected default graph.
- **v0.5 Phase D — `gaze_module_isolation` Dylint lint (PR #76, commit `3e367d1`):** Dylint late-HIR lint replaces the syn-walker `audit-metadata-only` gate. Resolution runs through `LateContext::qpath_res` against rustc's name resolver, not text matching. `check_item`, `check_expr`, `check_ty`, trait references, struct fields, and macro emission are covered. 18 UI fixtures cover all known bypass classes including macro call-site hygiene, `#[path]` modules, `include!`, type positions, trait bounds, and `extern crate gaze_audit`. Pinned toolchain: `nightly-2025-09-18`, `clippy_utils@20ce69b9...`, `dylint_linting`/`dylint_testing` 5.0. New `dylint` GitHub Actions workflow runs the gate on every push to `main` and PR.
- **v0.5 Phase D — `dylint-gate` xtask command (PR #76):** verifies the `xtask/dylint/ui` fixture corpus has exactly 18 enabled fixtures, rejects `*_disabled.rs`, and runs `cargo dylint --workspace --all` when `cargo-dylint` is installed (skips with a clear message locally when absent; CI installs it explicitly).

### Changed

- **v0.5 Phase B / C audit-sink refactor:** `gaze` core no longer carries `rusqlite` in default or `--no-default-features` builds. Library callers that previously imported `gaze::SqliteLogger` should switch to `use gaze_audit::SqliteLogger;` (preferred), or temporarily enable `gaze`'s `audit` feature for the one-minor migration window.
- [bundle-tokenization-drift] Release aggregation refreshed `core` and `core-extended` no-policy snapshots for the v0.4.6 bundled rulepack version bump.

### Removed

- **v0.5 Phase E — legacy `audit-metadata-only` syn walker (PR #77, commit `f4fde12`):** decommissioned. The Dylint gate added in Phase D is now the canonical audit-sink protected-path enforcer. Phase E removed: the inline syn-walker source from `crates/xtask`, the `RESTORE_AUDIT_FORBIDDEN_SYMBOLS` constant, the adversarial walker tests in `crates/xtask/tests/adversarial_audit_metadata_only.rs`, and the `.github/workflows/audit-metadata-only.yml` workflow. Net: `-942` lines of legacy walker code, tests, and workflow.

### Migration notes (adopters)

- `use gaze::SqliteLogger;` → `use gaze_audit::SqliteLogger;` (preferred). One-minor compatibility option: `gaze = { features = ["audit"] }` re-exports the original path; the shim is scheduled to drop in v0.6.
- `bundle.from_context(&ctx)` now requires `use gaze::DictionaryBundleExt;` (or `use gaze_types::DictionaryBundleExt;`). The trait is the explicit migration seam introduced when `DictionaryBundle` moved into `gaze-types`.
- `DictionaryEntry::new(...)` → `DictionaryEntry::try_new(...)?` if the call site cannot statically guarantee a non-empty term list and ASCII case-insensitive entries.
- Workspace tests that reference `gaze::SqliteLogger` via the dev-dependency path should run with `cargo test --workspace --all-features`; the `--all-features` flag enables the `audit` shim that those compatibility tests rely on.

## [0.4.6] - 2026-04-26

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.6`.
- Bundled rulepack versions now track `gaze-recognizers` at `0.4.6`.
- **Bundle-tokenization drift gate:** no-policy `core` and `core-extended` bundled outputs now have checked-in baselines; intentional drift requires an explicit source ACK and changelog marker before release.
- **Fixture-citation lint:** synthetic fixture policy is now enforced by `xtask`, tightening the no-real-PII discipline for examples and tests.
- **Rulepack-derived bundle classes:** bundled class listings are derived from rulepacks instead of hand-maintained metadata, reducing release drift for adopter-facing bundle docs and checks.
- **DE national-phone recall broaden:** `core-extended` recognizes additional documented synthetic German national-phone mobile shapes while preserving parser-backed validation.
- **CI/no-feature matrix:** `xtask ci-feature-matrix` guards the no-default-feature phone parser path so unsupported parser validators continue to fail closed.
- **Homebrew tap decision:** README install guidance remains release-asset first until a public tap exists and the release process publishes to it.

## [0.4.5] - 2026-04-26

### Added

- **Audit retention manual purge (PR #59):** `gaze audit purge --before <iso8601> [--dry-run | --count]` deletes redaction-log rows older than the cutoff. Calendar-aware ISO 8601 validation rejects malformed dates fail-closed with typed `AuditPurgeIso8601` error. Restricted DELETE clause; no policy-level retention default; no background auto-purge.
- **`audit_metadata_only` xtask gate (PR #59):** compile-time enforcement that restore-path code does not import audit metadata symbols. Walker covers file scope `use`, nested `mod`, function/impl/trait-default/const/static block-statement `use`, glob imports, aliased crates, `extern crate`, and `#[path]`-resolved external modules. Known limitations (fully-qualified path references, `include!`, let-else diverge, macro-emit) documented in `docs/architecture/xtask.md`; v0.5 architectural pivot to dylint-based name-resolution lint scheduled (todo #181).
- **`--session` audit filter (PR #57):** opaque session-scope filter for `gaze audit query` / `gaze audit export` (NOT raw `session_hex`).
- **DE + US national phone recognizers (PR #58):** parser-backed E.164 region-aware validators (`phonenumber` crate) for German and US national phone numbers. Cooperate with structural phone recognizer; gated behind `phone-parser` Cargo feature.
- **ClassMapOverrideSafety extension (PR #55 / S4):** further hardening of class-map override safety gate.
- **Rulepack version bump validation (PR #56 / S5):** rulepack version bump audit + drift-prevention rule.
- **`gaze-assembly` crate restructure (PR #61 / S6):** `lib.rs` split into focused modules by responsibility.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.5`.
- **`core-extended` no-policy locale activation (PR #58):** the bundled `core-extended` rulepack now activates `phone.national.de`, `phone.national.us`, `postal.us`, and `postal.de` recognizers when invoked without a policy via `--rulepack-bundled core-extended`. Previously these required an explicit `--locale` or policy-supplied locale. Adopters using the bundle without a policy will see additional tokenization for German/US national phone numbers AND bare 5-digit numeric strings (matching the postal recognizers). To restore prior behavior, supply an explicit `--locale=global` or pass a policy with narrower locale gating. (todo #171)

### Fixed

- No standalone `fix(...)` commits landed between `v0.4.4` and `v0.4.5`; the bundle is release plumbing plus S1-S6 feature, hardening, and documentation work.

### Documentation

- README catch-up for v0.4.2-v0.4.4 (PR #60).
- README Requirements section with per-OS support matrix (PR #62).
- Org transfer URL sweep `Naoray/gaze` -> `piinuts/gaze` (PR #63).
- New `docs/architecture/xtask.md` documenting `audit_metadata_only` gate coverage, known limitations, and v0.5 roadmap.
- New `docs/research/v0.5-dylint-audit-gate.md` stub (todo #181).

## [0.4.4] - 2026-04-26

### Added

- **S1 ClassMapOverrideSafety xtask gate** (#51): the previously scaffolded gate is now active. The behavioral test runner invokes `t20_context_class_map_overrides_policy_dict_class` and `t20a_class_map_override_fails_closed_when_action_rule_uncovered` through `cargo test`, while `.github/workflows/class-map-override-safety.yml` runs the gate on PRs and pushes to `main`. An adversarial in-PR self-test programmatically verifies the gate fails non-zero when a listed test is missing or renamed, following the meta-Potemkin guard captured in drawer `gaze_architecture_12b32d53`. Closes todo #132.
- **S2 audit schema v2** (#53): `RedactionEntry` now includes `created_at: i64` epoch milliseconds, with an on-open SQLite `ALTER TABLE` migration so legacy DBs without `created_at` remain queryable through a NULL default. `gaze audit query` and `gaze audit export` now accept `--from <iso8601>` and `--to <iso8601>` filters, JSONL export includes `created_at`, and ISO 8601 parse failures emit typed `CliError::PolicyConfig` messages with the offending input quoted. Time-filtered queries omit NULL `created_at` legacy rows by SQL semantics; unfiltered queries still include them. Fixture coverage covers both v0.4.3-shaped and v0.4.4-shaped SQLite DBs.
- **S3a phonenumber-backed `E164Phone` validator** (#52): the `phonenumber` crate is available behind the optional `phone-parser` feature, default-on for `gaze-cli` and opt-in for raw library users. `ValidatorKind::E164Phone` extends the existing `phone.structural` recognizer in `core-extended.toml`, preserving valid E.164 matches such as `+4915550112233` while rejecting regex-passing but unassigned shapes such as `+99999999`. Builds without `phone-parser` reject the `e164_phone` validator at rulepack load time with `RulepackError::UnsupportedValidator`, preserving axis-1 fail-closed behavior rather than silently dropping phone detection at runtime. Audit notes live in `docs/research/v0.4.4-phonenumber-audit.md`.
- **S4 Date posture memo** (#50): `docs/research/v0.4.4-date-posture.md` locks Gaze's Date-as-PII stance. Dates are not PII by default, never ship in default `core` or `core-extended` bundles, and future v0.4.5+ implementation scope is limited to DOB-only structured contexts. General-prose dates require context classification research for v0.5+, and the GH #5 token-spam tradeoff is resolved as no-default-on. The negative corpus covers version strings, IPs, file paths, ID-shaped numerics, year-only strings, and build or CI metadata.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.4`.
- ClassMapOverrideSafety is no longer a scaffold; `cargo run -p xtask -- class-map-override-safety` now executes its named tests and returns a meaningful exit code.
- The audit query path continues to open SQLite read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY`, carrying forward the v0.4.3 S4 hardening.

### Notes for adopters

- The Linux x86_64 binary requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer), the same constraint as v0.4.2 and v0.4.3.
- Phone validation is feature-gated. `gaze-cli` enables `phone-parser` by default; raw library users opt in with `gaze-recognizers = { features = ["phone-parser"] }` when they need parser-backed E.164 validation. Without that feature, `e164_phone` is rejected at rulepack load time.
- Audit time filters accept ISO 8601 timestamps through `--from` and `--to`. Legacy audit DBs without `created_at` are still queryable, but time-filtered queries exclude their NULL timestamp rows by SQL semantics.

### Deferred to v0.4.5

- `--session` audit filtering, deferred from v0.4.4 until the session identifier storage type design is locked.
- DOB-scoped Date recognizer, per the S4 memo and only if Markus or another adopter provides a concrete DOB leak fixture.
- S3b national phone recognizers for DE and US, deferred from v0.4.4 due to scope budget.
- ClassMapOverrideSafety coverage for other class-rule paths.
- Audit retention and auto-purge, now unblocked by the v0.4.4 `created_at` foundation.

### Deferred to v0.5

- Open-key `PiiClass` refactor, per scratchpad 256 LOCK 2.
- Crate-shape Option B: extract `gaze-types` and collapse `gaze-assembly`.

## [0.4.3] - 2026-04-26

### Added

- **S1 ValidatorKind substrate** (#47): three new validators in `crates/gaze-recognizers/src/regex.rs`: `Luhn` for Mod 10 checksums, `IbanMod97` for ISO 7064 mod-97 validation, and `IbanCanonical` for uppercase-plus-whitespace-stripped normalization.
- **S2 core-extended Phase 2** (#48): two validator-backed recognizers in `core-extended.toml`:
  - `iban.structural` matches IBANs with optional whitespace, applies the `iban_mod97` validator plus `iban_canonical` normalizer, and emits class `custom:iban`.
  - `card.structural` matches broad credit-card shapes with optional space or hyphen separators, applies the `luhn` validator, and emits class `custom:credit_card`.
  - Default `[[rule]]` entries now ship in the rulepack so `--rulepack-bundled core,core-extended` tokenizes these classes out of the box, following the CLI shipping divergence pattern captured in drawer `gaze_architecture_c6eefa4b`.
  - The bundled `core-extended` rulepack version is now `0.4.3`.
- **S3 xtask `no_tenant_knowledge` gate** (#46): production-code lint scanner rejects tenant-pattern strings (`order_id`, `Order_42`, `Song_42`, `User_7`) in `crates/{gaze,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. Allow markers (`// allow(tenant-fixture)`) hard-fail in production scope and remain valid only in `tests/`, `benches/`, `docs/`, and `CONTRIBUTING.md`. CI runs the gate through `.github/workflows/no-tenant-knowledge.yml`, and an adversarial in-PR self-test verifies the scanner actually scans rather than printing success.
- **S4 `gaze audit query/export` CLI** (#45): the existing `commands/audit.rs` stub is now wired into full read-only metadata export from audit SQLite. Filters include `--class`, `--source`, `--action`, and `--document-kind`; JSONL is the default output. A restricted column set defends against extra-column leaks, with cross-version SQLite fixture coverage for current and legacy schemas.
- Tenant numeric ID negative fixtures (`Subscriber_*`, `Order_*`, `Customer_*`, `0815 12345`) are explicitly proven not to fire as IBAN or credit-card matches.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.3`.
- `--audit-db` queries now open the SQLite database read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY` for defense in depth, so the audit CLI cannot write to the DB even if compromised.

### Deferred to v0.4.4

- `--session` and `--from`/`--to` audit filters need a session column and `created_at` schema migration.
- Date recognizer needs an explicit policy-posture brainstorm, including the GH #5 tradeoff considerations.
- National phone patterns need parser-backed per-locale validation because of collision risk with tenant numeric IDs.
- Open-key `PiiClass` refactor plus crate-shape Option B remain targeted for v0.5.

### Notes for adopters

- The Linux x86_64 binary requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer), the same constraint as v0.4.2.
- Phase 2 validator-backed recognizers are opt-in via the `core-extended` rulepack; adopters using only `core` get no behavior change.

## [0.4.2] - 2026-04-25

### Added

- **S4 Linux release artifact:** release CI now publishes `gaze-x86_64-unknown-linux-gnu` from a native `ubuntu-24.04` runner, alongside `gaze-aarch64-apple-darwin`, with `.sha256` files for both artifacts. The Linux artifact requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer); older distros should build from source.
- Release artifact smoke now executes the packaged binary for `--version`, `alice@example.invalid` clean/restore reversibility, S1 runtime knob help flags (`--session-scope`, NER, and rulepack surfaces), and `core-extended` bundled rulepack loading with neutral non-real fixture data.
- v0.4.1 Bundle P1 foundation: `gaze-assembly` library entrypoint, `xtask` scaffold, and the `symmetric_potemkin_gate` workflow.
- `token.family` now threads from recognizers into session snapshot entries while preserving the existing emitted token grammar.
- Locale-aware regex `pattern_template` lowering for `{locale_email_headers}` with English and German defaults.
- `capture_groups = [...]` regex span narrowing with first-non-empty semantics.
- `NerRecognizer` public export plus `[ner] threshold` policy knob using min-aggregated span confidence.
- Core `email.header.name` recognizer for RFC822-style header display names, including German `Von:` / `An:` forms.
- Strict rulepack composition validation: same-class recognizer pairs now require explicit `cooperates_with` declarations.
- `Context::fields_typed() -> ContextFieldsRef<'_>` borrowed accessor for context-field consumers.
- `gaze clean --audit-db=<path>` persists the metadata-only SQLite redaction log for pipe-mode invocations.
- **S1 three-surfaces backfill:** `gaze clean` now exposes CLI overrides for existing policy runtime knobs: `--session-scope`, `--ner-model-dir`, `--ner-locale`, `--rulepack-bundled`, and `--rulepack-path`.
- **S2 core-extended rulepack:** opt-in bundled rulepack with Phase 1 shape-only recognizers for E.164 phone numbers, IPv4/IPv6 addresses, and `de-DE`/`en-US` postal codes.
- **S5 v0.5 design:** design doc for open-key `PiiClass` and decision-deferred crate-shape Option B sketch.
- **P3.5 #100 parity audit:** three-surfaces parity audit table for every `policy.toml` field, classifying runtime knobs with CLI/TOML/default coverage and policy-document fields that intentionally remain TOML-only.
- **P3.5 #114 generic placeholder vocab:** rulepack locale `pattern_template` placeholders now support generic `{locale.<bucket>}` expansion from adopter-defined `[locale.<bucket>] names = [...]` tables.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.2`.
- **P3.5 #115 CLI split:** split `gaze-cli/src/main.rs` into focused `commands`, `pipeline`, `restore`, `io`, `error`, and `logger` modules with responsibility-based names and no CLI behavior change.
- Snapshot envelope version bumped from 2 to 3; v0.4.1 imports v2 snapshots with default `counter` family, while v0.4.0 rejects v3 snapshots instead of silently collapsing family metadata.
- Dictionary recognizer audit sources now include per-term traceability as `dictionary:{name}[#term_index]`.
- **S3 fixture sweep:** renamed tenant-pattern test and benchmark strings to neutral placeholders, with `CONTRIBUTING.md` documenting tenant class naming policy.
- `{locale_email_headers}` remains supported as a v0.4.2 compatibility alias for `{locale.email_headers}` and is deprecated for removal in the v0.5 cycle.
- **P3.5 #116 NER split:** split the NER recognizer implementation into focused `ner/` submodules without changing public exports or runtime behavior.

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

[Unreleased]: https://github.com/piinuts/gaze/compare/v0.4.6...HEAD
[0.4.6]: https://github.com/piinuts/gaze/compare/v0.4.5...v0.4.6
[0.4.5]: https://github.com/piinuts/gaze/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/piinuts/gaze/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/piinuts/gaze/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/piinuts/gaze/compare/v0.4.0-rc.1...v0.4.2
[0.4.0-rc.1]: https://github.com/piinuts/gaze/releases/tag/v0.4.0-rc.1
[v0.3.1]: https://github.com/piinuts/gaze/releases/tag/v0.3.1
[0.3.0]: https://github.com/piinuts/gaze/releases/tag/v0.3.0
[0.3.0-rc.2]: https://github.com/piinuts/gaze/releases/tag/v0.3.0-rc.2
[0.3.0-rc.1]: https://github.com/piinuts/gaze/releases/tag/v0.3.0-rc.1
