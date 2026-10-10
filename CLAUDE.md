# CLAUDE.md — Gaze

See [AGENTS.md](AGENTS.md) for canonical project rules + the Gaze north star. This file adds only Claude-Code-specific addenda.

Detection and benchmark changes must pass the
[benchmark gain gate](AGENTS.md#benchmark-gain-gate).

## Project north star

Apply the [north star and five axes](AGENTS.md#project-north-star) to every
design, implementation and review. Justify any weakened axis in the PR body;
correctness axes 1–4 beat performance.

### The five axes

[Canonical definitions](AGENTS.md#the-five-axes).

### How to apply

Use those definitions for every decision.

## Local gates

The repo no longer ships a tracked pre-push hook. Run gates manually before
pushing if you want defense in depth:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- ci-feature-matrix
```

PR-triggered CI catches doc-test and rustdoc warnings, workspace tests, MSRV
checks, cargo-deny, and the active xtask gate roster on every relevant PR.

## v0.9 architecture primer

See the current [crate map](CONTRIBUTING.md#workspace-shape). Always test with
`cargo test --workspace --all-features`, including safety nets and
`gaze-recognizers/phone-parser`.

`gaze-audit` owns `SqliteLogger`, `AuditFilter`, `AuditLogRow`,
`build_audit_query_sql` and `AUDIT_RESTRICTED_COLUMNS`. `gaze-document` depends
on `gaze`, `gaze-recognizers` and `gaze-types`; its CLI needs `document`.
The browser-MITM project is separate (formerly in-tree `debug-proxy`).

- Detection is recognizer-native. Every detector runs through `gaze::RecognizerRegistry` with a
  typed `DetectContext` envelope. New detection features should land as a `Recognizer` impl in
  `gaze-recognizers`, not as a bespoke pipeline hook.
- Audit-log path: Rust adopters import `use gaze_audit::SqliteLogger;` directly. `gaze` no longer
  carries `rusqlite` in any feature graph. `gaze::SqliteLogger` and the core `audit` shim are
  removed. `cargo run -p xtask -- cargo-metadata-audit-isolation` plus a `cargo deny` ban rule keep
  the protected default, `--no-default-features`, and safety-net graphs clean.
- Ambiguity side-channel: `RedactionEntry` can carry optional `ValidatorFailReason` and
  `AmbiguityRecord` metadata. `SqliteLogger` persists the bundled four-column migration
  (`validator_fail_reason`, `ambiguity_record`, `collision_family`, `collision_variant`) and CLI
  audit queries can filter ambiguity/collision metadata. Contract:
  [`docs/explanation/detection/ambiguity-side-channel.md`](docs/explanation/detection/ambiguity-side-channel.md).
- Validator-veto: validator-backed recognizer failures are rejected before conflict resolution and
  logged as loser-only audit rows with `decided_by: ValidatorVeto` plus typed
  `validator_fail_reason`. Existing `ConflictTier::Validator` remains the same-class containment
  tie-breaker. Contract:
  [`docs/explanation/detection/validator-veto.md`](docs/explanation/detection/validator-veto.md).
- Repeat-value sweep: after resolve and before the safety net, uncovered copies of rule-found values
  (this document's winners + session manifest entries with a non-learned evidence tier) join the
  pool and it is resolved again; NER/safety-net values never propagate. `session_blob` is v6
  (per-entry evidence tier). Contract:
  [`docs/explanation/detection/manifest-sweep.md`](docs/explanation/detection/manifest-sweep.md).
- Collision-family policy: cross-class recognizer rivalries declare `[recognizers.collision]` or
  `[policy.custom_recognizers.collision]` metadata and compile into `FamilyPolicyTable`.
  Validator-veto runs first; family policy then decides same-family different-variant overlaps with
  `ConflictTier::CollisionPolicy`. Precedence ties emit a family-level
  `PiiClass::Custom("family:<name>")` token plus `AmbiguityRecord::PrecedenceTie`. Contract:
  [`docs/explanation/detection/collision-family.md`](docs/explanation/detection/collision-family.md).
- Mandatory-anchor resolution: collision-family recognizers can declare `mandatory_anchor = "<key>"`; locale packs provide `[locale.cues.<key>]`. Missing anchors fail closed to one
  family-level `PiiClass::Custom("family:<name>")` token, `ConflictTier::AnchoredContext`, and
  `AmbiguityReason::NoAnchor`. Contract:
  [`docs/explanation/detection/anchor-resolution.md`](docs/explanation/detection/anchor-resolution.md).
- MCP CLI surface: `gaze mcp install` writes supported client config for `gaze mcp serve`, `gaze mcp doctor` checks runtime dependencies and client wiring, and `gaze mcp serve` starts the stdio
  server exposing agent-tier document tools. Requires the `gaze-cli` `mcp` feature; document tools
  also require the `document` feature.
- gaze-proxy: API-key chokepoint for OpenAI, Anthropic, and Gemini SDK base-URL paths. Web-tier
  subscription clients, certificate/PAC/Electron integration, and transparent MITM are out of scope
  here and belong in a separate browser-MITM project.
- Daemon mode: `gaze daemon` is a JSONL stdio protocol with a per-`session_id` manifest registry,
  LRU + idle eviction, SIGTERM-graceful shutdown, and audit `provenance_stage = "daemon"`.
- Pipeline optimization config: `PipelineOptimizationConfig` is opt-in and default-off. Skip-gating
  applies only to observer-only modes; capitals heuristic is locale-configured; prefix reuse is
  disabled; every input is rescanned; length bucketing is explicit.
- DistilBERT safety net removed: both backends, int8 precision, its Cargo feature and the
  `runtime-tract` / `runtime-candle` features are gone. No net runs without a policy; `gaze setup`
  enables Nym, and OPF (`--safety-net openai-filter`) stays opt-in. `gaze setup` installs the pinned
  Davlan mBERT NER bundle (`gaze_model_setup::install_ner_bundle`, verified by
  `gaze_recognizers::verify_davlan_ner_bundle`). `gaze index ingest` requires that pinned bundle via
  `--ner-model-dir` / `GAZE_NER_MODEL_DIR` and otherwise fails closed with `IndexNerModelMissing`.
- Audit-sink protected-path enforcer (canonical): the `gaze_module_isolation` Dylint lint resolved
  via `LateContext::qpath_res`. Pinned toolchain: `nightly-2025-09-18`, `clippy_utils@20ce69b9...`,
  `dylint_linting`/`dylint_testing` 5.0. 18 UI fixtures cover all known bypass classes including
  macro call-site hygiene, `#[path]`, `include!`, type positions, trait bounds, and `extern crate`.
  The legacy `audit-metadata-only` syn walker is removed.
- Nym-small safety net: `safety_net::nym` runs `Wismut/nym-pii-multilingual-small` v3 int8 in
  process via `ort` behind `safety-net-nym`; SHA-pinned bundle (`NYM_SMALL_INT8_BUNDLE_SHA256`)
  installed by `gaze setup --safety-net nym`; op-B allowlist/thresholds in
  `gaze_types::nym::NymOperatingPoint`, overridable by `[safety_net.nym]` (configures, never
  activates); word-aligned via the shared `gaze_types::is_inside_word`; 512/64 chunking with a
  coverage error; `raw_label = "LABEL>=THRESHOLD"`; bench arm `full-stack-nym-resolve`. The `gaze setup` policy enables Nym; a policy without `[safety_net]` runs no net.
- GLiNER DOB judge: `[dob_judge]` scores rule-found, unclaimed dates with the SHA-pinned
  `gliner-multi-pii-dob-int8` bundle (`GLINER_DOB_BUNDLE_SHA256`); fail-closed on a missing or
  corrupt bundle. `gaze setup --dob-judge` installs and enables it; plain `gaze setup` does not
  until the 352 MB bundle is shrunk, although its own benchmark arm gained under v2 and v1 with zero
  FP bytes (+664 MiB peak RSS; `docs/reference/benchmarks/mechanism-arms.json`). Every mechanism
  gets such an arm via `scripts/bench/mechanism_arms.py` (a policy delta file plus a `record` run).
- Pass-3 SafetyNet: observer-only post-clean check that runs against already-tokenized output
  without mutating the manifest or restore path. Activation paths are CLI flags (see
  [`crates/gaze-cli/README.md`](crates/gaze-cli/README.md#safety-net)) and the programmatic
  `Pipeline::with_safety_net` builder. Architecture contract:
  [`docs/explanation/safety-net/safety-nets.md`](docs/explanation/safety-net/safety-nets.md). The
  OpenAI-filter subprocess device is selectable via `gaze clean --openai-filter-device {auto|cpu|cuda|mps}`.
- Cue-anchored Name detection: `anchored_match` recognizer kind and the `forward_markers` /
  `agent_recipient_cues` / `footer_cues` locale buckets in `locale-de` and `locale-en`. Composes
  with the `core` bundle without custom recognizers.
- Policy surface: `[policy.rulepacks]` (bundled + path) + `[[policy.custom_recognizers]]`. Top-level
  `[[detector]]` is rejected. When editing `crates/gaze/src/policy.rs`, cross-check
  [docs/reference/policy.md](docs/reference/policy.md) and the `gaze-cli` integration suite in
  `crates/gaze-cli/tests/cli_pipe.rs`.
- Locale chain is 4-tier (CLI > policy > rulepack default > system default) with strict
  `LocaleTag::Other(_)` matching. `locale_basis = "document"` recognizers gate on `locales = [...]`;
  `locale_basis = "format"` treats locales as format provenance and runs once regardless of the
  document chain. External omission defaults to `document`; bundled recognizers declare the basis
  explicitly.
- Conflict resolution: class-priority > rule-priority > score > span-length > recognizer-id with
  multi-overlap fixed point. Losers are logged with `decided_by: ConflictTier` in the redaction log.
- Validator/normalizer enums: the complete closed sets, feature gates, rulepack spellings,
  ownership, and exact fail-closed wiring stage live in the drift-gated [redaction classes and
  recognizers reference](docs/reference/redaction-classes.md#closed-validator-and-normalizer-sets).
  Do not duplicate the variant list here.
- Active xtask gates: `symmetric-potemkin`, `class-map-override-safety`,
  `recognizer-composition-validator`, `no-tenant-knowledge`, `bundle-tokenization-drift`,
  `family-policy-table-coherence`, `locale-cue-bundle-coherence`, `fixture-citation-lint`,
  `ci-feature-matrix`, `cargo-metadata-audit-isolation`, `readme-version-check`, `dylint-gate`
  (canonical audit-sink isolation), `safety-net-sanity`, `tokenbridge-no-raw-index`,
  `tokenbridge-encrypted-index`. All gates must invoke at least one behavioral test;
  symbol-or-string-presence-only checks are recursive-Potemkin and forbidden.
- `core-extended` no-policy bundled activation: the authoritative compatibility locale chain and
  complete active-recognizer list live in [Shipped default
  activation](docs/reference/redaction-classes.md#shipped-default-activation). Adopters relying on
  no national phone tokenization or no bare 5-digit numeric tokenization must pass `--locale=global`
  or supply a policy with narrower locale gating.
- Postal rules use `custom:postal_code`. Numeric rules are document-gated;
  alphanumeric rules are format-basis and always run. Keep these constraints:

  | Rule | Required behavior |
  |---|---|
  | `postal.de`, `postal.us` | Bare `\d{5}` only under `de-DE`, `en-US`. |
  | `postal.ca`, `postal.gb`, `postal.ie` | Accept NBSP/NARROW NBSP. Eircode identifiers need a letter; all-digit `ORDER A12 3456` is a reference. CA/GB reject preceding `#` using `capture_groups = [1]`; `\b` alone allows `#D3D3D3`. |
  | `postal.at_ch` | Document basis, `de-AT`/`de-CH`. `\d{4}` only after a postal cue or before a city-shaped token; `capture_groups = [1, 2]`. Reject `#` and bare `St.`. Known FP: capitalized German nouns (`1500 Euro`, 28 holdout tokens). |
  | `postal.au` | Requires `en-AU`. Protect state + four-digit code together, using Australia Post state ranges and a preceding capitalized word of 3+ letters or terminal field boundary; `capture_groups = [1, 2]`. Exclude listed EN/DE function words, but other capitals/German nouns qualify. Terminal CSV/table cells can match years. |
  | `postal.cued_four_digit` | Default chains; four digits only after explicit postal labels across locales. Unlabelled street-and-town values stay raw to avoid years/amounts. |

  Review positives and negatives outside the corpus when adding always-on
  alphanumeric rules. A4 has no `#RRGGBB` or `L99 9999`; 0/1024 proves neither.
  Bare four-digit matching is 19% precise on holdout and hits 62.5% of negatives:
  keep its anchors. Locale fall-through is per span: `[de-AT, de-DE]` protects
  both code types (`postal_at_ch.rs`). Strict same-class containment lets joined
  AU tokens win in both shipped locale orders; AT/CH still covers rejected AU
  codes or inactive `en-AU`. The no-policy `core-extended` chain runs AT/CH and
  auto-activates `en-AU` on every document; broad setup includes `en-AU` too.
- Format-basis bundled identifiers: `phone.national.us`, the alphanumeric postal rules `postal.ca` /
  `postal.gb` / `postal.ie`, and other format-basis recognizers run for every locale, including
  under `core`; `--locale=global` cannot suppress them. Adopters needing the old token stream must
  disable the relevant recognizer outright.
- Labelled-identifier value boundary: bundled tax, driver-licence and identity-card rules set
  `match.complete_labelled_value`; the shared scanner protects the whole adjacent run and writes a
  typed, value-free `labelled_value_scan_reason` when it stops or exceeds four groups/40 bytes. See
  [labelled identifiers](docs/explanation/detection/labelled-identifiers.md); limits never justify
  exposing a suffix.
- Nym view contract: Nym scans both the stable token view and a byte-aligned neutral view, then
  projects neutral findings onto exposed clean-text gaps. Other safety nets retain one stable scan.
  See [Nym's neutral token view](docs/explanation/safety-net/nym-neutral-view.md).
- Rulepack fields parsed but gated: `token.format`, `context.hotwords`, `context.boost`,
  `context.window`. `token.family` was un-gated in v0.4.2.
