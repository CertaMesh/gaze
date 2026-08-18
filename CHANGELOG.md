# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`gaze_proxy::ProxyConfig::with_dictionaries` installs one immutable
  dictionary source for every proxy detection pass.** Omitting the builder keeps
  the existing empty-bundle default. `ProxyConfig` was already
  `#[non_exhaustive]` and the stored field is private, so this addition does not
  break external struct construction.

- **Scheme- and `www.`-anchored URL detection at the deterministic rule floor**
  (todo #2254). The new `url.anchored` recognizer in the embedded `core` bundle
  tokenizes `http://`, `https://`, and `www.`-prefixed URLs as
  `custom:url`, covering the whole span rather than a fragment. It is
  `safety_tier = "safe_default"` with `locales = ["global"]`, so it is active for
  every adopter of the default bundle, not only for configurations that
  auto-activate locale-gated recognizers.

  Measured against the EN/DE benchmark holdout: those two anchors carry 232 of
  276 gold URL spans and 6,385 of 7,209 leaked URL bytes (88.6%), while matching
  nothing at all across the 1,024 documents of the committed A4 negative corpus.
  Before this recognizer the deterministic rule floor was completely blind to
  URLs — 0 of 276 spans covered and 0 overlapped.

  **Bare-host URLs are deliberately out of scope.** Shapes with no scheme and no
  `www.` prefix (`example.invalid/orders`) are the remaining 47 spans / 887 bytes
  / 12.3% of the bucket, and 97 of the 1,024 committed negative documents contain
  bare-host shapes, so no bare-host rule can clear the negative gate.

  **Documentation, repository, and example URLs are tokenized.** This is
  intentional: 16 of the 232 gold spans the rule covers are themselves
  reference-host shaped, so the corpus treats a reference URL inside a
  data-owner document as PII to protect. Tokens stay restorable through the
  manifest, so an over-tokenized public URL is a recoverable ergonomics cost
  (axis 5) while an under-tokenized private one is a leak (axis 1).

- **Cue-anchored bearer-credential detection at the deterministic rule floor**
  (todo #2318). The new `security_token.anchored` recognizer is one two-arm,
  `safe_default`, global rule in the embedded `core` bundle. It protects
  structurally typed AWS access-key/JWT shapes and high-entropy values adjacent
  to explicit English or German credential cues, emitting the reversible
  `custom:security_token` class.

  A fresh full EN/DE comparison removed 3,065 leaked SECURITYTOKEN bytes at the
  rule floor and 2,985 with pass2 NER. Each deterministic cell added 17
  false-positive bytes, ratios of about 180:1 and 176:1, while the rule matched
  0 of all 1,024 committed A4 negative documents. Cue anchoring is supported by
  the corpus asymmetry: 88.1% of SECURITYTOKEN spans have `token` or `secret`
  in the preceding 64 characters, versus only 1.8% cue context for URL.

  Arm 2 requires at least one unambiguous delimiter between cue and value.
  Whitespace and `:`, `=`, or `#` qualify; `_` and a directly abutting `-` do
  not. The holdout has 0 of 193 cue-context spans with no delimiter and 0 using
  direct hyphen alone, so the requirement costs no measured gold coverage.
  This prevents the safe default from splitting cue-prefixed snake_case
  identifiers such as tokenization helper names. A4 does not contain this
  identifier class; post-fix dogfooding across project documentation and Rust
  source produced zero SECURITYTOKEN detections.

  **The shipped no-policy `CorePipelineConfig` default now tokenizes
  credential-shaped strings in ordinary adopter text.** Git tokens in logs,
  API keys in documentation, and similar cue-anchored secrets are protected
  and remain restorable through the manifest. This is a deliberate,
  recoverable axis-5 ergonomics cost in service of axis-1 reliability. The CLI's
  separate no-policy stub path is unchanged.

- **Corpus-informed government-ID recognizers at the deterministic rule floor**
  (todos #2318 follow-on, #2923). Four cue-anchored `safe_default` recognizers
  join the embedded `core` bundle: `ssn.de_cue` (German social-insurance cues
  such as `Sozialversicherungsnummer` and `SV-Nummer` before dashed, dotted, or
  9 to 11 digit values, class `custom:ssn`), `tax_number.cue_anchored`
  (`custom:tax_number`), `driver_license.cue_anchored`
  (`custom:driver_license`), and `national_id.cue_anchored`
  (`custom:national_id`). Every chosen shape was picked by measured sweep against
  looser drafts and matches 0 of the 1,024 committed A4 negative documents.

  Locale basis follows the mixed model from #414: `ssn.de_cue` is a
  format-basis sibling of `ssn.us` (same class, same national identifier
  shapes, German cue vocabulary, DACH provenance) and is an explicit addition
  to the ratified format-basis promotion set; the three bilingual cue-anchored
  recognizers are document-basis `global`, like `security_token.anchored`. All
  four therefore fire under every locale chain, including `--locale=global`.
  `ssn.us` keeps its pattern, locales, and basis unchanged; the only edit to it
  is the symmetric `cooperates_with` metadata line. Cross-class overlaps between
  the numeric shapes are resolved by the new `government-id` collision family
  (`ssn` 10 beats `tax-number` 20 beats `national-id` 30; lower wins).

  The [shipped scorecard](docs/reference/benchmarks/v0.12-government-id-scorecard.md)
  measures the deterministic cells at 3,194 fewer leaked bytes each and 9 more
  false-positive bytes: rule floor adds 1 false-positive document, while pass2
  adds none. The full-stack Kiji `resolve` cell removes 3,093 leaked bytes and
  613 false-positive bytes with no change in false-positive documents.
  **Disclosed regression:** the Kiji cell loses 6 covered `PASSWORD` bytes, the
  downstream safety-net interaction tracked as todo #2491 (mechanism #2420),
  not a resolver decision and unaffected by collision precedence.

  `tax_number.cue_anchored` deliberately requires a three-digit lead and
  internal separators: it cedes the checksummed 2-3-3-3 Steuer-ID shape to
  `steuer_id.de` and excludes A4's bare-digit invalid identifiers, so its
  measured 16.8% coverage is a precision choice, not a detection deficiency.
  `national_id.cue_anchored` ships with a known, bounded gap: 77 bare German
  NATIONALID spans carry no cue and are structurally unreachable by any
  cue-anchored rule.

### Changed

- **One documented safety-net default across the library and the CLI** (audit
  7201 S01-F1, solo todo #2949). `Pipeline::clean_with_safety_net` and
  `clean_with_safety_net_detect_context` — the policy-less convenience entry
  points — previously hard-coded `SafetyNetMode::Strict` + `Redact`, which
  contradicted `SafetyNetPolicy::default()` (`Resolve` + `Redact`, the shipped
  production default and the CLI default since v0.8.1). They now use
  `SafetyNetPolicy::default()`. **Adopters calling these entry points with a
  registered safety net now get enforcement instead of observation**: suspects
  are tokenized reversibly and any residual takes the `Redact` fallback, rather
  than being reported and shipped. This strengthens axis 1 and preserves
  axis 2 (the promoted spans stay restorable). For the previous behaviour pass
  an explicit policy to `clean_with_safety_net_policy_detect_context`, or use
  `Pipeline::scan_safety_nets` for a report-only pass. In-tree pipelines that
  register no safety net are unaffected.

- **The three structured-document walkers are one `walk_structured` with a
  `LeafOp`** (audit 7201 S01-F2, solo todo #2950). The pseudonymize,
  clean-and-scan, and scan-only traversals of `RawDocument::Structured` were
  three near-identical recursive copies that had already drifted. They are now
  one function parameterized by `LeafOp { Pseudonymize, CleanAndScan, ScanOnly }`,
  with every intentional difference — empty-string skipping, whether scalar
  leaves are scanned, whether the document is rebuilt, and the root field-path
  prefix — declared once on the op and documented there. Behaviour is unchanged
  on all three paths, including the pre-existing divergence where
  `scan_safety_nets_structured` reports bare-key field paths (`profile.email`)
  and `clean_with_safety_net*` reports JSONPath-style ones (`$.profile.email`),
  which is preserved deliberately and tracked as solo todo #2958.

- **`SafetyNetMode` x `SafetyNetFallback` is lowered once to a total
  `SafetyNetDecision`** (audit 7201 S01-F1, solo todo #2949). The two public
  fields spell twelve pairs; the runtime has six behaviours, and seven pairs
  previously differed only in a field nothing read. `SafetyNetPolicy::decision()`
  is now the single, total lowering to
  `SafetyNetDecision { Observe { strict }, Redact, Resolve { on_residual } }`,
  and every pipeline arm, the skip-gating optimizer, and the CLI boundary read
  the decision instead of re-deriving the lattice from the pair. `SafetyNetDecision`
  is exported from `gaze`. The public `mode` and `fallback` fields are unchanged.
  The full twelve-pair behaviour table is pinned by
  `safety_net_policy_lowering_covers_all_twelve_representable_pairs`.

- **`gaze clean` no longer warns that `--safety-net-fallback` is "ignored when
  `--safety-net-mode` is terminal"** (audit 7201 S01-F1, solo todo #2949). The
  lowering documents which pairs consult the fallback, so the runtime warning is
  redundant. Relatedly, the tolerant-deprecation warning now fires only where a
  tolerant disposition is reachable — `--safety-net-mode tolerant`, or a tolerant
  fallback under `--safety-net-mode resolve`. It no longer fires for
  `--safety-net-mode redact --safety-net-fallback tolerant`, where the fallback
  is never consulted. The `GAZE_ALLOW_TOLERANT` gate is unchanged and still
  rejects a tolerant flag in any position.

- **Persistent owner-side corpus index schema v2: each document is stored
  once, postings are derived on load, and re-ingest upserts** (audit 7201
  S17-F1, todo #2936). `gaze_token_bridge::persistent::FileCorpusIndexStore`
  previously wrote one record per distinct fingerprint, each carrying a full
  copy of the document (snippet plus every entity's raw value), so a document
  with E entities was duplicated E times on disk, and re-ingesting a `doc_id`
  without clearing the domain left the stale copy first in scan order (the
  stale snippet won). Raw document snippets and entity values are now persisted
  exactly once per `(domain_id, doc_id)`; the fingerprint-to-document postings
  map is rebuilt on load and never written to disk; `insert_hit` replaces an
  existing `(domain_id, doc_id)` outright, and `clear_domain` + `save` leaves
  no document bytes in the sealed payload.

  **Breaking for existing local index files.** `SCHEMA_VERSION` is now `2`.
  Loading a v1 `index.json` fails closed with the typed
  `unsupported owner-side index schema 1; supported 2` error (the version is
  checked before the payload shape, so there is no partial or reinterpreted
  load), and a v2 payload carrying duplicate document keys is rejected the
  same way. Rebuild local indexes by re-running `gaze index ingest <dir>`.
  The `entities: N` metric printed by `gaze index ingest` keeps its meaning
  (indexed document/fingerprint pairs). `FileCorpusIndexStore::hit_count_for_domain`
  was removed (it had no callers). The AEAD/key layer and the sealed-file
  envelope are unchanged.
- **Audit-row enums now own one canonical string form** (audit S05-F2, solo todo
  #2935). `Action`, `ConflictTier`, `DocumentKind`, and `FallbackReason` expose
  matching `as_str` / `from_canonical_str` methods, and SQLite plus CLI consumers
  delegate to them instead of maintaining panic-prone copies. `FallbackReason`
  JSON now serializes as snake_case to match SQLite; every former PascalCase
  spelling remains accepted as a serde alias.

- **`gaze_assembly::build_pipeline` derives its `NoRecognizers` guard from
  actual registration and uses one locale predicate** (audit 7201 S10-F1,
  todo #2928). The guard now fails closed when zero recognizers were
  registered, instead of re-deriving eligibility from policy and rulepack
  metadata. Two behavioural consequences for adopters:
  - Rulepack recognizers with an **empty locale list** (a pack that omits both
    `default_locales` and per-recognizer `locales`) now register and run under
    every document locale chain, matching the detect-time
    `LocaleChain::intersects` semantics that already treated an empty list as
    matching. Previously assembly silently dropped them (a missed detection).
    Bundled rulepacks are unaffected (every bundled recognizer declares
    locales).
  - An `anchored_match`-only rulepack whose optional builtin cue bucket
    (`forward_markers`, `agent_recipient_cues`, `footer_cues`) is not present
    under the active locale chain now fails with `NoRecognizers` instead of
    building a zero-recognizer pipeline that preserved every byte.
  NER model loading now runs before the guard; error precedence is unchanged
  for reachable configurations (a configured `model_dir` that fails to load
  still surfaces as `NerLoad`).
- **Locale-gated auto-activation is derived from the loaded rulepacks**
  (audit 7201 S10-F2, todo #2929). The `auto_activate_locale_gated` locale set
  (`core-extended` compatibility alias) is now computed by the new public
  `gaze_assembly::locale_gated_activation_locales(&[Rulepack])` — the union of
  `locales` over enabled, document-basis `safety_tier = "locale_gated"`
  recognizers, minus `global`, ordered compatibility-first
  (`en-US, de-DE, de-AT, de-CH`) then by canonical tag — instead of a literal
  `[en-US, de-DE, de-AT, de-CH]` list that was triplicated across
  `CorePipelineConfig`, `gaze clean`, and `gaze daemon`. For the bundled `core`
  recognizers the derived set equals the old literal, so shipped behaviour is
  unchanged (the compatibility chain stays `global, en-US, de-DE, de-AT,
  de-CH`). Behavioural widening for adopters: a path rulepack whose
  document-basis locale-gated recognizer declares another locale (for example
  `es-ES`) now auto-activates under the alias without an explicit `--locale`
  or policy locale; previously it silently never activated. Any future bundled
  locale-gated recognizer joins the activation set automatically. The
  `--locale`/policy locale override precedence is unchanged.
- `[bundle-tokenization-drift]` snapshot for bundle `core` regenerated: the new
  `url.anchored` recognizer adds one `custom:url` detection to the drift corpus
  (11 -> 12 detections). No existing detection changed class, span, or shape.
- `[bundle-tokenization-drift]` snapshot for bundle `core` regenerated for the
  mixed locale-basis rulepack version bump (`0.5.1` -> `0.5.2`). Detection count
  remains 12; no detection changed class, span, or shape.
- **Bundled identifier recognizers now use explicit mixed locale semantics**
  (todo #2417). Rulepacks gain additive
  `locale_basis = "document" | "format"` metadata. External and adopter
  rulepacks that omit it retain the legacy `document` default. Bundled
  recognizers declare it explicitly.

  The A4-clean format recognizers `aadhaar.in`, `bsn.nl`, `cnpj.br`, `cpf.br`,
  `nhs.uk`, `nino.uk`, `nir.fr`, `pan.in`, `phone.national.us`, `ssn.us`,
  `steuer_id.de`, `vat.de`, and `vat.es` now run once regardless of the
  document locale. Their `locales` values record identifier-format provenance,
  and their candidates join document-basis candidates before the unchanged
  conflict resolver runs. Linguistic `name.*` recognizers remain
  document-basis. `phone.national.de`, `postal.de`, and `postal.us` remain
  temporarily document-gated pending precision hardening; postal promotion
  depends on todo #2424.

  **Breaking behavior:** `--locale=global` and narrow locale chains no longer
  suppress the format-basis identifiers. Adopters relying on that suppression
  can receive new tokens and changed snapshots. To restore the old output,
  disable the affected recognizer outright, for example by selecting an
  adopter rulepack copy with `enabled = false`; locale mismatch is no longer a
  suppression mechanism.

  This deliberately trades axis 5 (snapshot compatibility and configuration
  convenience) for axis 1 (never leak a foreign-format identifier merely
  because the surrounding document uses another locale). The known remaining
  rule-coverage debt is 27 target spans: 14 DE national-phone and 13 postal
  spans. The proxy transport debt is now closed by #2411: direct/codec primary
  and residual passes receive the shared `ProxyConfig::locale_chain`; #2403
  previously fixed the legacy path.

- **A provably corrupt clean-text manifest now hard-errors in every safety-net
  fallback mode, including `Tolerant`** (#403). The safety-net RESOLVE path checks
  that the manifest describes a monotonic, gap-preserving clean/raw alignment
  before it makes any manifest-derived decision. A manifest that fails that check
  contradicts the document it describes, so it fails closed as
  `SafetyNetError::InvalidOutput` with a `manifest-integrity:` message prefix
  rather than taking the configured fallback — `Tolerant` no longer returns
  `Ok(())` and `Redact` does not attempt redaction. This is deliberate: redaction
  derives its deletion spans from manifest coordinates, so a manifest that
  misdescribes the document makes the redactor delete the wrong bytes and can
  leave the flagged PII in place. That is a leak, not a degraded-but-safe
  document, and `Tolerant` was contracted to tolerate residual suspects, never an
  internally inconsistent manifest. Axis 1 (never leak) over axis 5 (adopter
  ergonomics).

  **Not reachable from the public surface today.** `redact_text_with_manifest`
  emits an `EmittedTokenSpan` for every replacing action — `Tokenize`, `Redact`,
  `Generalize`, `FormatPreserve` — so every clean/raw divergence is described by a
  manifest entry and a pipeline-produced manifest satisfies the check by
  construction. The checks are defense in depth against a future change to the
  primary pass, not a response to a reachable failure. No new `Error` or
  `FallbackReason` variant; `gaze-types` is unchanged.

### Fixed

- **The `mcp-tier-isolation` gate now actually fails when the agent/operator
  tier boundary is violated** (audit 7359 §6-F1, solo todo #2993). **The tier
  partition itself was, and remains, enforced by rustc:** the operator surface
  sits behind `#[cfg(feature = "operator-tier")]` in
  `crates/gaze-mcp-core/src/{lib.rs,tools/mod.rs}`, and no agent-tier build has
  ever linked it. What was missing was any check that this keeps holding. The
  gate's agent-tier assertion was `assert!(true)`, deferring the real guarantee
  to the `dylint-gate`, whose `dylint.toml` carries only `gaze_audit` rules and
  nothing about tiers — so un-gating the operator restore surface and running
  `cargo run -p xtask -- mcp-tier-isolation` exited 0. **No adopter was exposed
  by this; the alarm on the boundary was, until now, the only thing that was
  not real.**

  The vacuous assertion is replaced by `trybuild` compile-fail fixtures in
  `crates/gaze-mcp-core/tests/ui/tier/`, one per gated surface (`tools::export`,
  `tools::restore`, `tools::restore_strict`, and the `operator_tools`
  re-export). Each is compiled as an external crate against the same agent-tier
  feature graph the test binary was built with, so rustc — not another gate's
  configuration — is the enforcer. Removing any `cfg` gate makes the matching
  fixture compile, which fails the gate and names the surface. The gate
  additionally requires each feature graph to report its tier tests as passing,
  since `cargo test` exits 0 for zero tests, and the fixtures are enrolled in
  the `trybuild-fixture-hygiene` inventory so deleting one is also a failure.
  Each graph further declares whether it is driven with `cargo test` or
  `cargo check`, and the gate refuses a `cargo test` graph that names no test,
  so an empty expectation cannot silently mean both "nothing executes here" and
  "nobody filled this in".

- **Structured documents no longer accept a safety-net enforcement request and
  silently perform observation** (audit 7201 S01-F2, solo todo #2950). The
  structured arm of `clean_with_safety_net_policy_detect_context` cleaned each
  field, ran the nets over the result, and returned `Ok` — with no enforcement
  stage anywhere on the path. A caller passing `SafetyNetMode::Redact` or
  `SafetyNetMode::Resolve` with a `RawDocument::Structured` therefore got
  observer-only behaviour and a success return, with the suspect bytes still in
  the document. It now fails closed with the new
  `Error::UnsupportedSafetyNetModeForStructured { mode }` before any field is
  tokenized. **Adopters passing structured documents with an enforcing mode now
  get an error** — the intended correction; pass `SafetyNetMode::Strict` (or
  `Tolerant`) for the observer behaviour they were actually receiving, or use
  `Pipeline::scan_safety_nets_structured`. Note that the policy-less
  `clean_with_safety_net*` entry points default to `Resolve`, so structured
  documents must go through `clean_with_safety_net_policy_detect_context`.
  Text documents are unaffected. Axis 1.

- **Safety-net `resolve` fallback acted on the primary report instead of the
  residual one, destroying tokens and shipping residual PII under the default
  policy** (audit 7201 S01-F1, solo todos #2949 and #2956). When the resolve
  pass converged and the post-resolution re-run flagged a residual suspect, the
  fallback was handed the *primary* `LeakReport`, which by then described
  pre-resolve coordinates.

  With a **non-empty** primary report — the broadly reachable case, since any
  deterministic safety net produces one — the redactor was pointed at stale
  pre-resolve spans. Those spans had since become part of a token the resolve
  pass minted, so the fallback deleted the token, dropped its manifest entry,
  and left the actual residual in the document: an axis-1 leak and an axis-2
  restore break in the same operation, returned as `Ok`.

  With an **empty** primary report the fallback had nothing to act on at all, so
  the residual shipped and no fallback audit row was written. (Reaching that
  shape requires a backend whose verdict differs across byte-identical text,
  since a converged resolve leaves the document unchanged.)

  Only the `strict` fallback was safe, because it rejects the document without
  consulting the report. The fallback now receives the report that produced the
  reason, and acts only on the suspects in it that are not already protected by
  a live token — a protected suspect is audited as a `Preserve` no-op instead of
  being redacted, which also closes a gap where such suspects were left out of
  the audit entirely. Pinned by
  `resolve_fallback_does_not_redact_stale_pre_resolve_spans`,
  `resolve_fallback_redacts_the_residual_report_not_the_stale_primary_report`,
  `resolve_fallback_redacts_the_residual_without_deleting_protected_live_tokens`
  (both fallback reasons), and the twelve-pair lowering table; the first three
  are required by name in the `safety-net-sanity` gate.

- **A residual found only by the post-resolution re-run was invisible in the
  returned `LeakReport`** (solo todo #2959). The report handed back to the
  caller is the first pass's, so a boundary that decides on it — the CLI's
  tolerant-mode deprecation warning, or an adopter's "did anything leak?" check
  — was told nothing was found while the residual shipped under `tolerant` or
  was destroyed one-way under `redact`. The suspects the fallback acted on are
  now merged into the returned report. A converged resolve merges nothing, so a
  deterministic net that re-reports the same suspect does not produce
  duplicates.

- **Fallback audit rows now state what happened to the suspect's bytes**
  (audit 7201 S01-F1, solo todo #2949). `fallback_action` was renamed to
  `fallback_row_action` and documented as the row's claim about the bytes:
  `Action::Redact` only when the residual span is actually deleted,
  `Action::Preserve` when it is left in place (shipped under `tolerant`,
  rejected under `strict`). With the residual-report fix above, a
  `decided_by: Fallback` row now also names the residual suspect that drove it
  rather than a stale primary-pass suspect.
- **A detached `gaze proxy start --policy prod.toml` now runs the policy instead
  of the bundled `core` pipeline** (solo todo #2965). `start` persisted the
  policy into its daemon config and then spawned the serving child with only
  `--bind` and `--session-ttl`, so the detached daemon resolved
  `build_pipeline(None, "core")`: no policy rules, no custom recognizers, no
  dictionaries, and no policy locale tier. The configured `--rulepack` and all
  three `--upstream-*` overrides were dropped the same way, which is why
  `gaze proxy status` could print upstreams the running daemon never used. The
  child's argument list is now derived from the daemon config as a whole, so the
  daemonized proxy resolves the same pipeline as `gaze clean`. **The previous
  entry for `gaze proxy` (solo todo #2937, PR #437) covered `gaze proxy serve`
  only**; adopters running the daemon were unaffected by that fix. `restart`
  carried the same defect and is fixed by the same change.

- **`gaze proxy start` now fails when the daemon dies during startup instead of
  reporting success** (found by the red test for #2965). The liveness probe used
  `kill(pid, 0)`, which cannot distinguish a running child from one that exited
  and has not been reaped, so a child that failed closed on an unloadable policy
  was reported as `gaze-proxy started` with exit code 0. `start` now reports the
  new `ProxyError::DaemonExitedEarly`, naming the child's exit code and the
  stderr log to read, and removes the empty pidfile that would otherwise fail
  every later start as stale. A startup failure slower than the 250 ms probe
  window is still reported as started; that daemon is dead rather than serving
  unprotected.

- **`custom:family:*` policy classes now preserve the collision-family namespace**
  (audit S05-F1, solo todo #2934). `PiiClass::from_policy_name` previously
  normalized the reserved `family:` separator and hyphenated family name, so a
  protective class rule could silently miss a family-level ambiguity token and
  preserve the original value. `PiiClass::family` and `as_family_name` now model
  that namespace once, and policy and rulepack parsing share the same
  non-normalizing path. The enum and manifest wire shape are unchanged.
- **`gaze proxy` now resolves the same rulepacks, dictionaries, and
  auto-activated locales as `gaze clean` for the same policy** (audit 7201
  S11-F2, solo todo #2937). Previously the proxy assembled a narrower pipeline
  that skipped dictionary values and locale-gated auto-activation.
- **Proxy request protection and fail-closed residual validation now read the
  same configured `DictionaryBundle`.** This covers direct/codec JSON and SSE
  response validation plus the legacy primary and residual request passes; the
  residual can no longer know fewer dictionary terms than the primary pass.
- **`gaze-proxy` direct/codec primary and residual passes now use the resolved
  locale chain instead of a pinned Global chain** (solo todo #2411). This
  closes the direct/codec half after #2403 fixed the legacy path, and keeps both
  passes aligned with the same configured dictionaries and document locales.

- **The ORT NER backend now hands the BIO decoder the document text, not its
  provenance label** (audit S07-F1, solo todo #2902). `OrtBackend::detect` passed
  the constant `"ner/ort"` where `merge_bio_span_results` expected the string
  the tokenizer offsets index into, so in production (a) joiner bridging read
  the bytes between tokens from `"ner/ort"` and was dead — hyphenated or dotted
  names such as `Anne-Marie` / `john.doe` decoded as two spans — and (b) any
  input of 7 bytes or fewer had its spans boundary-checked against `"ner/ort"`,
  so short structured field values such as `Anna`, `Alice`, or `Berlin` (the
  tool-call JSON shape axis 3 is built for) lost their NER span entirely. Both
  are recall defects (axis 1). The decoder now receives the real input, the
  decode step is a model-free `decode_logits` seam with red-first tests, and the
  Kiji safety-net decoders (`ort`/`tract`/`candle`) were checked and already
  passed the real text.

  The heuristic `enforce_source_boundaries` flag (which guessed whether the
  argument was text by comparing span ends to its length) and the
  `is_token_boundary_match` suppression it gated were removed rather than
  silently activated by the corrected argument: that suppression never ran in
  production and turning it on is a measured decision (solo todo #2904).
  Production output changes only by adding spans the decoder was designed to
  emit; no span the previous decoder emitted is dropped.

  **API:** `NerDetector::merge_bio_spans` now takes `document_text` and
  `provenance` as separate parameters (was one `source` used for both);
  `NerDetector::merge_bio_span_results`' last parameter is renamed
  `document_text` (same position, same type). Callers that passed a provenance
  string as `source` must pass the tokenizer input instead.

- **Safety-net RESOLVE no longer returns `Ok` on a result it cannot verify**
  (#403). The mode's only post-condition used to be a follow-up net scan, so any
  net whose second pass disagreed with its first — an ML net, a cached net, a
  sampled net — could leave a raw PII fragment in the clean text and emit a
  permanently unrestorable manifest while the call succeeded. Resolutions are now
  planned against the unmutated clean text and rejected as a typed
  `FallbackReason` when they overlap each other, when a suspect's coverage claim
  contradicts the manifest, or when a residual suspect survives the follow-up
  pass; suspects lying wholly inside a live token are audited as reversible
  no-ops instead of destroying the token. `SafetyNetFallback::Redact` keeps its
  existing redact-and-deliver meaning for every fallback reason.

- **Kiji safety net: location and organization suspects were swapped** (PR
  #425, todo #2312, defect todo #2925). Since v0.9.0-rc.1 the in-process Kiji
  DistilBERT decoders (`ort`, `tract`, `candle`) mapped classifier ids 3–6 as
  `B-LOC, I-LOC, B-ORG, I-ORG` while the pinned model actually emits
  `B-ORG, I-ORG, B-LOC, I-LOC`, and the Python subprocess runner used a third,
  MISC-first order. Every real place was therefore reported as `organization`
  (mapped to the `Name` safety-net class) and every real organisation as
  `location`: the `LeakSuspect` class, the class/family of tokens minted by
  RESOLVE mode, `ClassMismatch`/fallback decisions, and the `raw_label` /
  `mapped_class` columns of `safety_net_log` audit rows were all wrong for
  those suspects. Span positions and scores were unaffected. All backends and
  the runner now share one label registry pinned to the upstream
  `onnx-community/distilbert-NER-ONNX` `config.json` (`3a19fe9`), verified by
  decoder-parity tests, and every backend fails the whole request closed with
  a typed `SafetyNetError::InvalidOutput` on a malformed classifier width,
  offset/logit length mismatch, non-finite logits, or missing output tensor
  instead of silently mapping to `O` or returning no spans. The bundle ships no
  `id2label` artifact, so the registry cannot be re-checked at bundle load; the
  SHA-256 bundle pin plus the parity tests are the guard.

## [0.12.0] - 2026-07-06

### Added

- **Policy-authoring docs now cover the `\b`-next-to-symbol regex pitfall**
  (#361). `\b` adjacent to a non-word character (`€`, `$`, `£`, punctuation)
  never forms a boundary against whitespace, so currency-style patterns like
  `\b(...€|$...)\b` silently fail to match — a fail-open leak in redaction
  policies. Investigated as a suspected 0.5.x → 0.11.x regression and ruled
  out: outputs are byte-identical across versions (standard Rust `regex`
  semantics, engine unchanged). `docs/reference/policy.md` documents the
  failure modes and the explicit-boundary-group rewrite.
- **`gaze clean` now warns when a collision-family fallback class would silently
  leak** (#360). When an active recognizer can emit a `custom:family:<family>`
  class (for example `custom:family:payment-card-or-iban` for an anchor-less
  IBAN) and the policy leaves that class to a non-protective default, the CLI
  prints a `warning:` to stderr naming the exact rule to add. Rust adopters get
  the same list from the new `gaze_assembly::uncovered_collision_family_classes`.

### Changed

- **CI now runs `bundle-tokenization-drift --verify-ack`** (#360). Any change to
  a committed bundle tokenization snapshot — including emitted class renames —
  must carry a `// drift-ack:` comment and a CHANGELOG entry to merge. The v0.8
  `custom:iban` → `custom:family:payment-card-or-iban` snapshot change merged
  without either, which is how the rename reached adopters undocumented.

### Fixed

- **Documented the collision-family fallback class contract so `preserve`-default
  policies stop leaking IBANs** (#360). A policy keyed on `custom:iban` with a
  `preserve` default matched no rule for the anchor-less
  `custom:family:payment-card-or-iban` class and silently preserved the IBAN.
  The class names are now documented as pinnable contract in
  [`docs/reference/policy.md`](docs/reference/policy.md), and setting a covering
  `[[rule]]` (or loading `locale-en`/`locale-de` for the precise `custom:iban`
  class) closes the leak. Restores axis-1 protection for the default `core`-only
  configuration.

## [0.11.3] - 2026-07-02

v0.11.2 crates were never published to crates.io; v0.11.3 supersedes it for
crates.io users.

### Added

- **Restore and manifest property suites now cover round-trip and invariant
  behavior** (#354), raising confidence that reversible pseudonymization stays
  stable across generated inputs.
- **Remote CI now runs the release-critical gates** (#356): MSRV,
  `cargo-deny`, and the `xtask` gate suite.
- **Restore token regexes now use a session cache** (#356), reducing repeated
  parsing work without changing strict-restore behavior.

### Changed

- **The unused daemonization dependency was removed** (#352), narrowing the
  dependency graph shipped to adopters.

### Fixed

- **Email boundary leak fixes, SafetyNet fail-closed behavior, and locale
  fallback fixes were ported to main** (#353), restoring axis-1 protections that
  were absent from v0.10.0 through v0.11.2.
- **Strict restore no longer treats Unicode-digit token ordinals as valid**
  (#355), avoiding a false-positive restore path for non-ASCII token numbers.
- **The crates.io publish workflow now packages the workspace as a unit before
  publishing.** Per-crate pre-flight packaging resolved unpublished internal
  dependencies against the registry and caused the v0.11.2 multi-crate publish
  failure.

### Security

- **The pdfium CI download is pinned and SHA-256 verified** (#352), replacing an
  unverified network fetch in the document test setup.

## [0.11.2] - 2026-06-23

### Added

- **`gaze setup` provides the one-command onboarding path.** The CLI now installs
  and SHA-verifies the pinned NER model, writes a working policy, and runs a
  doctor check so OPF model setup is verified or fails closed.
- **Recognizer coverage expanded for outbound DLP workflows.** EU VAT IDs,
  ISO-length-gated IBANs, and spaced international E.164 phone numbers are now
  detected by default recognizers.

### Changed

- **Owner-side TokenBridge indexes are encrypted at rest.** Index files now use
  ChaCha20-Poly1305 bound to a per-index id, with `GAZE_INDEX_KEY` and optional
  `os-keychain` support, closing the plaintext PII and projection-key material
  exposure from `0.11.1`.
- **`gaze index ingest` defaults residual safety-net hits to redact.** The new
  `--on-residual redact|strict` mode keeps real-document ingestion usable while
  preserving the never-leak contract; operators can still opt into strict
  fail-closed behavior.
- **The README now leads with the one-command quickstart and the correct product
  framing:** deterministic reversible PII pseudonymization and outbound DLP, not
  guardrails, prompt-injection defense, or content-safety filtering.

### Fixed

- **`gaze index` now surfaces real error detail.** Failures that previously
  collapsed into an opaque `PolicyConfig` error now preserve the actionable
  underlying error.
- **Detection NER now loads the Kiji bundle.** The loader accepts optional
  `config.json` metadata and conditionally supplies `token_type_ids`, matching
  the shipped Kiji model bundle.
- **Proxy and structural recognizer hardening.** OpenAI proxy PII surfaces were
  tightened, email structural TLD matching was corrected, and the new phone and
  IBAN recognizers avoid the known false-negative shapes fixed in this release.

## [0.11.1] - 2026-06-20

### Added

- **`gaze-token-bridge` is now published to crates.io.** The crate remains
  experimental and focused on gated index-search; output backstop verification
  is still pending.

### Fixed

- **`gaze-cli` index installs now resolve from crates.io.** The optional
  `gaze-token-bridge` dependency now carries a publishable version so
  `gaze-cli` can publish and install with its `index` feature.

## [0.11.0] - 2026-06-20

### Added

- **`gaze-mcp-bridge`: optional policy-gated MCP bridge.** Gaze can now sit as
  an MCP server toward the agent and an MCP client toward downstream MCP
  servers, with restore-on-egress / redact-on-ingress handling, fail-closed
  errors, and default-deny policy behavior.
- **`gaze-token-bridge`: owner-side gated search over redacted corpora.** This
  experimental crate is not published to crates.io. It keeps search
  authorization and translation owner-side; output never-leak backstop
  verification is still pending.
- **`scan_folder` example for `gaze`.** The new bring-your-own-data redaction
  demo shows how to scan a local folder through the core runtime.

### Changed

- **Documentation now follows a Diátaxis × feature information architecture.**
  The docs were reorganized around task, reference, explanation, and tutorial
  needs while staying anchored to product features.
- **GDPR adopter guidance was substantially expanded.** The new material covers
  per-party identifiability, Article 25, Chapter V transfers, enterprise
  security expectations, and DPO-grade DPIA support.
- **Top-level repository layout was decluttered.** Examples, benches, and assets
  moved into crate-scoped docs, and the internal lint crate moved from `xtask/`
  to `lint/`.
- **Release notes are now sourced from `CHANGELOG.md` plus GitHub generated
  notes.** The committed `dist/release-notes` artifact was removed.

## [0.10.1] - 2026-06-04

### Fixed

- **fix(gaze-952): stop over-redacting camelCase command/argv identifiers + narrow lowerCamel suppression** (PR #302). This patch keeps command and argv-shaped identifiers from being treated as PII while preserving the tighter lowerCamel safety-net suppression.

## [0.10.0] - 2026-06-01

### Changed

- **BREAKING (`gaze-types`, custom recognizer authors): `Recognizer::detect`
  is now fallible** (P0 #908, PR #293). The trait method signature changed from
  the infallible

  ```rust
  fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate>;
  ```

  to

  ```rust
  fn detect(&self, input: &str, ctx: &DetectContext<'_>)
      -> Result<Vec<Candidate>, gaze_types::DetectError>;
  ```

  A recognizer backend can no longer represent "scan failed" as an empty
  candidate list — the only way a leak could previously slip through. The
  shared `DetectError` type lives in `gaze-types` (`DetectError::Backend {
  recognizer_id, message }`). `RecognizerRegistry` aggregation propagates the
  error, and the pipeline surfaces it as the new
  `gaze::pipeline::Error::RecognizerDetect(DetectError)` variant.

  **Migration for custom `Recognizer` impls:** wrap your existing return value
  in `Ok(...)`, and map any backend/runtime failure to
  `DetectError::backend(self.id(), <message>)` instead of swallowing it and
  returning an empty `Vec`. Infallible recognizers (pure regex/dictionary
  logic that cannot fail) simply return `Ok(candidates)`. See the fail-closed
  design contract in
  [`docs/architecture/p0-908-ner-failclosed.md`](docs/architecture/p0-908-ner-failclosed.md).

### Fixed

- **Byte-exact restore for adjacent and path-like tokens** (P0 #923, PR #295).
  Restore no longer inserts stray whitespace between adjacent spans, so
  path-like and back-to-back token sequences round-trip byte-for-byte. (Axis 2
  reversibility.)
- **Recognizer spans respect token boundaries** (P0 #923, PR #295). A
  recognizer no longer matches a substring inside a larger token — e.g.
  `Artist` is not tokenized inside `Artistfy`. A single-token common word such
  as `Workspace` is no longer promoted to an `Organization` span. (Axis 4
  determinism, fewer false-positive leaks of surrounding context.)

### Security

- **NER detection fails closed on backend error** (P0 #908, PR #293).
  Previously a NER backend runtime failure mapped to an empty detection set,
  silently passing raw text through unredacted — a critical PII-leak path. The
  failure now propagates as `DetectError::Backend` and the pipeline aborts
  outbound redaction (`Error::RecognizerDetect`) rather than emitting partially
  cleaned output. (Axis 1 never-leak.)
- **Long NER inputs are chunked into bounded, overlapping tokenizer-token
  windows** (P0 #908, PR #293). Inputs longer than the model's 512-token
  ceiling are scanned in overlapping WordPiece-token windows (480-token payload
  budget, 30-token overlap) so a long document can no longer slip past the
  model unscanned. The overlap is a documented security invariant —
  `overlap_tokens >= longest detectable entity + margin` — not a throughput
  knob; spans are remapped to original byte offsets before de-duplication.
  Contract:
  [`docs/architecture/p0-908-ner-failclosed.md`](docs/architecture/p0-908-ner-failclosed.md).
- **Release pre-flight now scrubs public text for local path/PII leaks** (PR #294).
  The new `scrub-public-text` gate scans release-facing docs and notes before
  publication, making accidental workspace-path or fixture leaks a release
  blocker rather than a post-release cleanup. (Axis 1 never-leak, Axis 4 trust.)

## [0.9.1] - 2026-05-29

v0.9.1 is a reliability and adopter-trust patch. The headline is an Axis-1
never-leak fix: NER `detect()` now fails **closed**. Previously a detector-backend
error returned an empty detection set, silently passing the raw text through
unredacted — a critical leak path that let PII reach an LLM outside the manifest
contract. The backend error now propagates as a typed failure instead of an empty
result, and inputs longer than the NER window (>512 tokens) are chunked so long
documents can no longer slip past the model unscanned. Manifest restore semantics
and the signed snapshot wire format are unchanged from v0.9.0.

### Added

- **Accessibility-aware CLI output gate** (PR #287): the CLI honours `NO_COLOR`
  and `CLICOLOR_FORCE` and performs TTY detection; informational output is never
  conveyed by colour alone. (Axis 5 ergonomics.)

### Changed

- **Daemon-mode docs reframed as stdio server.** `gaze daemon` is now documented
  as a long-lived stdio server in the LSP / MCP / language-server-protocol
  tradition rather than a Unix daemon in the strict sense. The subcommand verb is
  unchanged through v0.9.x; a `gaze serve` canonical alias is planned for v0.10
  (todo #486). External adopter feedback prompted the reframe. (Axis 4 trust,
  Axis 5 ergonomics.)
- **`gaze document clean` bundle layout splits into agent + owner paths** (axis 1
  enforcement). `Bundle::write` now requires distinct `AgentBundleDir` and
  `OwnerBundleDir` newtypes; the writer rejects equal or nested paths with typed
  `DocumentError::BundleLayoutInvalid`. The CLI gains `--agent-out` + `--owner-out`
  and retains `--out` as a shorthand that auto-creates `<PATH>/agent` +
  `<PATH>/owner` subdirs.
- **CI: DCO sign-off is now enforced on pull requests** (PR #288), and the Rust
  toolchain is pinned to 1.96.0 for reproducible trybuild output (PR #289).
  Contributor-facing; no adopter API change. (Axis 4 trust.)

### Fixed

- **Axis-1 bundle leak risk** (closes todo #489): `gaze document clean` previously
  wrote `manifest.json` next to `clean.md` in a single caller-selected `out_dir`,
  with no runtime enforcement of the agent / owner partition. Adopters following
  the README who uploaded the bundle directory to an LLM workspace leaked
  restorable manifest material. The new split-path bundle layout enforces the
  agent / owner partition at type and path-validation level. Original two-directory
  `manifest.bin` signed-envelope binding (the v0.7.0 architectural spec in
  `docs/architecture/document-extension.md`) remains a v0.11+ follow-up.

### Security

- **NER fail-closed never-leak fix** (PR #290): a recognizer/NER-backend error no
  longer returns an empty detection set that passes raw text through unredacted;
  the error now propagates and inputs exceeding the NER window (>512 tokens) are
  chunked. Any byte of PII reaching an LLM outside the manifest contract is a
  critical defect — this closes a detector-error bypass of the redaction pipeline.
  (Axis 1 reliability.)

## [0.9.0] - 2026-05-16

v0.9.0 is the performance-wave final release: Kiji int8 ORT warm p50 lands at
1.849ms in the committed model leaderboard snapshot, int8 preserves F1 recall
with a 0.000 delta across the safety-net matrix, opt-in pipeline skip/capitals
gates reduce the synthetic numeric bench from 300 SafetyNet calls to 0, and the
documented prefix-cache run reduces detector bytes by 52.7% and latency by
50.8%. `gaze daemon` removes full binary fork + model-load overhead for repeated
calls, and `tract` provides the new opt-in static-binary path. Manifest restore
semantics and the signed snapshot wire format are unchanged from v0.8.1.

Measured on: Apple M5 Max / macOS 26.5 hosts in the committed v0.9 snapshots
and final rc revalidation. Methodology, runnable commands, fixture SHAs, and
model pins: [`docs/benchmarks.md`](docs/benchmarks.md).

### Added

- **In-process Kiji ORT backend** (PR #250 `4b8db66`): Kiji DistilBERT now runs
  inside the process instead of through the Python subprocess path; the final
  public latency claims are tied to the committed benchmark snapshots and
  [`docs/benchmarks.md`](docs/benchmarks.md). (Axis 1 reliability, Axis 5 ergonomics.)
- **Kiji int8 dynamic quantization** (PR #256 `0a35f8e`): shipped the quantized
  Kiji bundle with 1.849ms warm p50 in
  `crates/gaze-recognizers/benches/ner_models_snapshot.json` and a 0.000 F1
  recall delta in
  `crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json`.
  Measured on: Apple M5 Max / macOS 26.5; see
  [`docs/benchmarks.md`](docs/benchmarks.md). (Axis 1 reliability, Axis 4 trust.)
- **`gaze daemon` JSONL stdio mode** (PR #255 `cddbea4`): a persistent,
  multi-session CLI daemon removes per-call binary fork and model-load overhead
  while preserving the same reversible session contract. (Axis 3 agentic-first,
  Axis 5 ergonomics.)
- **Tier 4 pipeline gating and prefix cache** (PR #252 `67374cb`): opt-in skip
  gates, capitals heuristic, prefix cache, and length bucketing reduce avoidable
  SafetyNet work; the synthetic numeric bench drops SafetyNet calls from 300 to
  0, and the documented prefix-cache run reduces detector bytes by 52.7% and
  latency by 50.8%. Measured on: Apple M5 Max / macOS 26.5; see
  [`docs/benchmarks.md`](docs/benchmarks.md) and
  `crates/gaze/benches/tier4_pipeline_gating.rs`. (Axis 1 reliability,
  Axis 5 ergonomics.)
- **Runtime comparison benchmark** (PR #257 `0fd7c8e`): published ORT vs tract
  vs candle results; recommendation is ORT by default and tract for opt-in
  static binaries. (Axis 4 trust.)
- **Tiny-model leaderboard** (PR #258 `3fd3859`): validates Kiji int8 as the
  v0.9 default against the smaller model candidates. (Axis 4 trust, Axis 5 ergonomics.)
- **End-to-end pipeline benchmark** (PR #244 `1bf78f3`): added detection +
  performance measurement for the full Gaze pipeline. (Axis 4 trust.)
- **Python Kiji runner reference wrapper** (PR #236 `c5e623f`): added a reference
  subprocess runner to bridge the older benchmark path and the new in-process
  runtime work. (Axis 4 trust.)
- **CLI plaintext JSON `entries` field** (PR #232 `ac579cd`): `gaze clean
  --format=json` now emits top-level `entries` mirroring session snapshot entries
  while preserving the signed `session_blob`; empty detections emit `entries: []`.
  (Axis 5 ergonomics.)
- **OPF checkpoint trust pin and benchmark cells** (PR #240 `e681361`, PR #241
  `430ba7f`): the OpenAI Privacy Filter backend pins the local checkpoint bundle
  SHA256 and required artifact inventory, and the benchmark snapshot now includes
  measured OPF direct-detector cells. (Axis 4 trust.)
- **Observer-residual safety-net cells and latency snapshot** (PR #248 `7cab754`):
  Kiji and OPF share a direct/observer scorer, observer-residual cells are
  populated across locale buckets, and latency snapshots cover the 150-fixture
  direct-mode corpus. (Axis 4 trust.)
- **Multi-NER leaderboard** (PR #245 `4a4338d`): published
  `docs/research/v0.9-ner-model-leaderboard.md`; Kiji selected as the v0.9
  default per shipped class-map measurement. (Axis 4 trust, Axis 5 ergonomics.)
- **Audit NER-provenance schema migration** (PR #238 `b030038`): `gaze-audit`
  adds eleven nullable provenance columns to `redaction_log` for future NER
  attribution; existing rows read back with `NULL` provenance fields. (Axis 4 trust,
  Axis 5 ergonomics.)
- **Locale-aware Pass-3 SafetyNet dispatcher** (PR #226 `98fa572`, PR #242
  `4b28ebc`): `Pipeline::with_safety_net_registry` routes SafetyNet work through
  `LocaleAwareModelRegistry`; the CLI gains registry and backend-locale override
  flags, and audit rows carry the resolved backend id. (Axis 1 reliability,
  Axis 5 ergonomics.)
- **Metrics single source of truth** (PR #235 `71c8e6c`): added `docs/metrics.md`
  to catalog observable surfaces. (Axis 4 trust.)

### Changed

- **Workspace version pin** `0.8.1` -> `0.9.0` across all ten published
  crates.
- **Synthetic email fixtures** (PR #233 `e48901b`) now use the IANA-reserved
  `@example.invalid` domain instead of reachable `@example.com` examples across
  crates and document fixtures. (Axis 4 trust.)
- **`SqliteLogger` leak-suspect writes now have one canonical verb** (PR #234
  `77f91b9`): call `LeakSuspectLogger::log_leak_suspect`; SQLite schema and
  write behavior are unchanged. (Axis 5 ergonomics.)
- **Pre-1.0 API naming cleanup** (PR #237 `d6a48af`): `redact_text` /
  `redact_with_context` names now use `pseudonymize_*` terminology to match
  reversible pseudonymization. (Axis 4 trust naming.)
- **Safety-net benchmark snapshot schema v2** (PR #230 `1bc60f5`): internal
  benchmark artifacts move from single-backend Kiji fields to a backend x locale
  x mode matrix with mode-independent strict-span leak-rate entries. (Axis 4 trust.)
- **Kiji and OPF benchmark docs** (PR #231 `eba3c03`, PR #245 `4a4338d`, PR #257
  `0fd7c8e`): consolidated v0.9 research docs around pinned artifacts,
  per-locale metrics, observer-residual caveats, runtime tradeoffs, and the Kiji
  int8 default recommendation. (Axis 4 trust.)
- **README and docs scope corrections** (PR #228 `051bbc9`, PR #229 `4bdcaa8`,
  PR #246 `5ff0b1b`, PR #251 `dbc6bdc`, PR #254 `930e38e`): clarified the
  ghostwriter flow, replaced the Mermaid walkthrough with an ASCII flow, removed
  private demo references, and qualified proxy scope as API-key path only.

### Removed

- **Private demo-repo README link** (PR #251 `dbc6bdc`): removed the stale link
  from public docs. (Axis 5 ergonomics.)

### Fixed

- **CI proxy smoke SIGPIPE** (PR #227 `31e00cc`): proxy status capture now uses
  command substitution so smoke tests do not trip SIGPIPE.
- **Ubuntu CI disk-full failure** (PR #249 `d412800`): frees preinstalled bloat
  before the workspace build on `ubuntu-latest`.
- **v0.9.1 follow-up fixups pulled into rc.1** (PR #253 `23d9ddd`): typed
  snapshot accessor, OPF artifact-hash gate, silent-drop telemetry, and
  locale-aware benchmark re-run.

### Release validation notes

- Final validation re-ran the coverage-loop recall pass, the Kiji int8
  direct/observer scorer, the ORT int8 benchmark, and
  `cargo run -p xtask -- ci-feature-matrix` on `origin/main` commit `79ba82f`
  (`v0.9.0-rc.1`) before promoting the release notes. Measured on: Apple M5 Max
  / macOS 26.5; see [`docs/benchmarks.md`](docs/benchmarks.md) and
  `docs/research/v0.9.0-rc1-combined-revalidation.md`.
- The PR #240 checkpoint-pinning caveat remains scoped to benchmark
  reproducibility: the old release checksum URL returned the expected pre-tag
  404, so the validation used the local Kiji cache after checksum verification.
  Kiji int8 observer-residual macro recall held at `0.666667`; the one-shot
  Python scorer's p99 row was an outlier while median and p95 remained inside
  the drift band.
- Reversibility is unchanged: manifest restore semantics and the signed snapshot
  wire format remain compatible with v0.8.1.
- ORT is the recommended runtime default; use the opt-in `tract` path when a
  static binary is the deployment constraint.

## [0.8.1] - 2026-05-15

Reversibility-first SafetyNet defaults, layout-report v2 with vector-PDF + multi-column + table-cell + deskew handling, an `OcrBackend` trait for plug-in OCR drivers, model-SHA integrity for the Kiji backend, and the default release binary now baking `--features proxy`. Schema-level: `BundleReport.bundle_version` bumps `1 → 2` (additive); `gaze-audit` rows gain a typed `fallback_triggered: Option<FallbackReason>` column and `decided_by` gains `Redact`/`Resolve`/`Fallback` variants.

### Added

- **SafetyNet `resolve` + `redact` + `fallback` modes** (PR #223 `167acca`): suspect spans flagged by Pass-3 SafetyNet are now promoted to custom-recognizer matches and rejoin conflict resolution before any irreversible side-effect. On promotion failure the typed fallback path emits a `:Redact_` token and records a `FallbackReason` in the audit log. Closed-enum variants: `FallbackReason::{OverlapConflict, ValidatorVeto, AnchorMissing, ResidualSuspect}`. New `decided_by` variants: `Redact`, `Resolve`, `Fallback`. CLI gains `--safety-net-mode {resolve|strict|tolerant}` and `--safety-net-fallback {redact|none}`. Tolerant remains dev-only behind `GAZE_ALLOW_TOLERANT=1`. (Axis 1 reliability, Axis 2 reversibility, Axis 4 trust.)
- **gaze-document layout report v2** (PR #219 `6acf77e`, PR #222 `9714b41`): `BundleReport.bundle_version` bumps `1 → 2`. New per-page fields: `ocr_source`, `ocr_backend`, `confidence`, `low_confidence`, `column_count`, `page_index`. New top-level field: `low_confidence_threshold`. Vector-PDF text-extraction fallback when PDFs have selectable text; multi-column segmentation in the post-processor; per-page confidence + low-confidence flagging against the threshold; table-cell preservation in markdown output; rotation/deskew preprocessing before OCR. v1 bundles continue to parse on read; emission is always v2. (Axis 1 reliability, Axis 4 trust.)
- **`OcrBackend` trait** (PR #218 `b9f3407`): single trait, single impl (`TesseractBackend`). `gaze-document` now exposes one OCR contract that second-party backends (ocrs, Apple Vision, PaddleOCR) can slot into cleanly. Trait is object-safe; covered by `tests/ocr_backend.rs`. (Axis 4 trust, Axis 5 ergonomics.)
- **Kiji model-SHA integrity** (PR #221 `07cf93d`): `KijiDistilbertSafetyNet` backend now verifies the DistilBERT bundle SHA256 at init and fails closed via `SafetyNetError::ModelIntegrityMismatch { expected, actual }` on mismatch. Direct-vs-observer benchmark harness shipped under `gaze-recognizers/benches/`; published metric fields stay `null` until populated on a machine with the pinned local Kiji runtime (Axis 4 — no uncited benchmark numbers). (Axis 1 reliability, Axis 4 trust.)
- **Safety-net architecture contract** (PR #216 `1cf6732`): `docs/architecture/safety-nets.md` now documents the resolve/redact/fallback semantics, the typed `FallbackReason` set, and how SafetyNet promotion interacts with `ConflictTier`. Companion adopter-facing doc updated in PR #217 `d55af13`.

### Changed

- **`--safety-net-mode resolve` is the new default** (PR #217 `d55af13`, PR #223 `167acca`), replacing `strict`. Reversibility-first; falls back to `redact` on resolve failure. Strict mode remains available for hard-fail deployments via `--safety-net-mode strict`. (Axis 1, Axis 2.)
- **Default release binary now bakes `--features proxy`** (PR #220 `fc00c26`): the published `gaze-v0.8.1-*.tar.gz` artifacts include `gaze proxy {serve,start,stop,status,logs,restart}` out of the box. Adopters who build from source unchanged.
- **Marketing-pass README** (PR #215 `ad22121`): adopter-focused copy refresh; no behavior change.
- **Workspace version pin** `0.8.0 → 0.8.1` across all ten crates.

### Removed

- **Legacy `OcrAdapter` shims** (PR #224 `89aaa4e`): the deprecated v0.7.1 adapter surface is gone. Adopters who plug in custom OCR now implement `OcrBackend` directly. Magic-byte validation (`detect_image_format`) is now mandatory at the `clean_with_ocr_backend` boundary — bare-byte payloads fail closed with `DocumentError::UnsupportedInput`.

### Fixed

- **Table-cell mock-backend test missing PNG magic bytes**: `bundle::tests::clean_with_mock_backend_preserves_table_cell_context` failed on `89aaa4e` after the magic-byte gate landed in PR #224. Test fixture now prepends `\x89PNG\r\n\x1A\n` to the synthetic payload. CI was red on main HEAD; this commit makes it green.

### Migration notes

- If your downstream tooling reads SafeBundle JSON: handle the `bundle_version=2` field. v1 reads work; v2 emission is non-optional.
- If you query the audit log: the new `fallback_triggered` column is nullable on existing rows; the new `decided_by` variants are closed-enum and discriminated.
- If your pipeline expected `--safety-net-mode strict` as default: pass the flag explicitly.
- If you embedded custom OCR via `OcrAdapter`: port to `OcrBackend` (object-safe, same shape).
- v0.7.x → v0.8.x → v0.8.1 multi-hop adopters: read [UPGRADE.md](./UPGRADE.md) before bumping; v0.8.0 already flipped several defaults.

## [0.8.0] - 2026-05-14

Bundle unification + versioned recognizer lineage + Kiji-style defense-in-depth + ten checksum-backed and locale-gated national-ID recognizers across five new locale packs, plus the new `gaze-proxy` crate that puts a PII chokepoint in front of OpenAI / Anthropic / Gemini API traffic. The workspace publish count rises from nine to ten.

### Added

- **Versioned recognizer lineage** (v0.8 Tier 1, PR #203 `3c95304`): `Candidate.recognizer_version_id` + `RedactionEntry.recognizer_id` + `recognizer_version_id` (all `Option<String>`, additive). Audit boundary in `pipeline.rs` now propagates lineage instead of dropping at `source`. SQLite schema gains nullable `recognizer_id` / `recognizer_version_id` columns; pre-migration rows tagged `legacy_unversioned`. NER recognizer emissions versioned as `ner.<model>.<vN>` from artifact config metadata (`ner.unknown.v0` fallback). `docs/architecture/locale-chain.md` gains a coverage matrix listing every bundled recognizer × supported locales × ValidatorKind. (Axis 4 trust/auditable, Axis 5 ergonomics.)
- **`SafetyTier` enum on rulepack recognizers** (v0.8 Tier 1.5, PR #201 `8ab9daf`): `SafeDefault`, `LocaleGated`, `OptIn` with `#[non_exhaustive]`. Closed-enum activation gate replaces the dual-bundle activation model. (Axis 1 reliability, Axis 4 trust.)
- **`KijiDistilbertSafetyNet` backend** (v0.8 Tier 2.5, PR #202 `0cd9ccc`): new `--safety-net-backend kiji-distilbert` flag (default remains `openai-filter` for compat). Pass-3 SafetyNet device with pinned-artifact contract identical to existing OpenAI filter (SHA256SUMS hard-fail on missing). New `CliError::SafetyNetArtifactMissing { backend, path }` typed variant. `scripts/fetch/fetch-kiji-safetynet-model.sh` mirror of existing NER fetcher. (Axis 1 defense-in-depth.)
- **Seven checksum-backed locale validators** (v0.8 Tier 2, PR #207 `16c1fd5`): Aadhaar Verhoeff (IN), French NIR MOD-97 variant (FR), German Steuer-ID MOD 11,10 (DE), Dutch BSN MOD-11 (NL), Brazilian CPF + CNPJ MOD-11 (BR), and UK NHS number MOD-11 (UK). New `ValidatorKind` variants are closed-enum and fail-closed on parse. Five new locale packs ship alongside: `locale-fr`, `locale-nl`, `locale-br`, `locale-in`, `locale-uk`. Every entity ships at `safety_tier = "safe_default"`, so adopters in BR/FR/NL/IN/UK get coverage out of the box once their locale is set. (Axis 1 reliability, Axis 3 agentic-first.)
- **Three locale-gated regex recognizers** (v0.8 Tier 3, PR #208 `7348690`): US SSN, UK NINO, and Indian PAN. All ship at `safety_tier = "locale_gated"` — no bare 9-digit / 10-character shapes activate without explicit locale + cue context. PAN extends the existing `locale-in` pack from Tier 2 in place. (Axis 1 reliability, Axis 4 trust.)
- **Corpus rework v2 implementation** (PR #205 `aa9c5fc`): the 61 stochastic status-quo templates + the `fixture_variants` mechanism are replaced with 150 deliberate scenarios. Each scenario declares its expected emissions including `recognizer_version_id` from day one. `fake` crate added as an xtask-only dev dependency; seed pinned in a documented `COVERAGE_CORPUS_SEED` constant. `baseline.json` fully re-snapped. (Axis 4 trust.)
- **`UPGRADE.md`** (PR #206 `492573d`): per-minor migration guide complementing `CHANGELOG.md`, with v0.7.x → v0.8.0 TL;DR + backfill summaries for v0.4 → v0.7.
- **`docs/research/v0.8-kiji-class-gap.md`** (PR #210 `eba350a`): coverage map of all 26 Kiji PII classes against gaze's recognizers — 6 beat-via-Tier-2, 1 beat-via-Tier-3, 16 observer-only-via-Tier-2.5, 3 parity, 0 deferred.
- **`docs/research/v0.8-kiji-benchmark.md`** (PR #209 `b875381`): two-mode (direct-detector + observer-residual) benchmark methodology headlining strict span leak rate, with a rule-floor snapshot pinned to corpus + Gaze tag. Kiji direct-detector + observer-residual cells deferred (no pinned model SHA yet — tracked as v0.8.x follow-up).
- **`ARCHITECTURE.md`** (PR #211 `fd130ac`): 14.8 KiB repo-root architecture overview of how the ten workspace crates fit together, with eight numbered Key Design Decisions and a one-diagram view of the redact/restore path.
- **`gaze-proxy` crate at v0.8.0** (PR #212 `503d0f9`): new published workspace crate. Multi-provider HTTP proxy with an adapter/driver pattern that serves OpenAI's `/v1/chat/completions`, Anthropic's `/v1/messages`, and Gemini's `/v1beta/models/*:{generateContent,streamGenerateContent}` without translation. SSE streaming and tool-call argument reconstruction wired through `gaze::Pipeline` (chunk-split PII spans inside `tool_calls.function.arguments` are accumulated and redacted before leaving the proxy). Daemon-mode subcommands `gaze proxy {serve,start,stop,status,logs,restart}` plus opt-in `install-launchd` / `install-systemd-user` installers. Feature-gated on `gaze-cli` as `--features proxy`, off by default. (Axis 3 agentic-first, Axis 5 ergonomics.)

### Changed

- **Unified `core` + `core-extended` bundled rulepacks** into single `core` bundle (v0.8 Tier 1.5, PR #201). Each recognizer now declares a `safety_tier` field; no-policy activation gates on `SafeDefault` only. Adopter behavior preserved through alias path described under Deprecated.
- **Workspace version pin** `0.8.0-rc.1` → `0.8.0` across ten crates (was nine; `gaze-proxy` joins this release).
- **[bundle-tokenization-drift] `baseline.json`** fully re-snapped against the corpus rework v2 scenarios.

### Deprecated

- **`--rulepack-bundled core-extended`** is deprecated (v0.8 Tier 1.5, PR #201). Aliases to `--rulepack-bundled core` with auto-activation of `LocaleGated` recognizers + a `tracing::warn!` deprecation notice. Scheduled removal in v0.10.0. Adopters who relied on the v0.4.5 PR #58 no-policy surprise activation for `phone.national.*` / `postal.*` should pass `--locale=de-DE` or `--locale=en-US` explicitly.

### Migration notes

- Existing `RedactionEntry` JSON consumers see no shape change — new fields use `#[serde(skip_serializing_if = "Option::is_none")]` and emit nothing when None.
- Existing SQLite audit DBs are migrated forward: pre-migration redaction rows get `recognizer_id = "legacy_unversioned"`, `recognizer_version_id = NULL`. Migration is idempotent.
- Existing `policy.toml` files unchanged. No new required fields.
- **`gaze-proxy` is opt-in** behind `--features proxy` on `gaze-cli`; existing adopters are unaffected unless they invoke `gaze proxy serve` or `gaze proxy start`.
- All Tier 2 + Tier 3 entity adds are additive. Adopters in BR / FR / NL / IN / UK / US-only / UK-only see no behavior change unless they enable their locale via `--locale=<bcp47>`.

## [0.7.2] - 2026-05-13

Dogfooding-driven point release. Both findings surfaced during a Pulseflow
adopter demo (`EmpireTwo/business:dogfooding/pulseflow-demo-2026-05-13`) and
strengthen the trust + adopter-ergonomics axes of the north star.

### Added

- **Policy schema versioning** (F#6, PR #192 `e698e35`): new top-level
  `schema_version` field in `policy.toml`. The loader gates the `major.minor`
  prefix against the supported version and fails closed with a dedicated
  `{"error":"PolicySchemaUnsupported","exit":2,"found":"...","supported":"0.1"}`
  CLI envelope. Existing 0.6.x/0.7.x policies continue to load via a soft
  default. Public-surface additions: `gaze::SUPPORTED_POLICY_SCHEMA_MAJOR_MINOR`,
  `gaze::DEFAULT_POLICY_SCHEMA_VERSION`, `gaze::PolicyError::PolicySchemaUnsupported`,
  `gaze_cli::CliError::PolicySchemaUnsupported`. (Axis 4 trust, Axis 5 ergonomics.)

### Changed

- **PolicyConfig error envelope now carries detail** (F#5, PR #191 `6ec7afd`):
  every `gaze-cli` `map_err(|_| PolicyConfig)` site now threads the underlying
  loader cause through `CliError::PolicyConfigDetail`. JSON shape stays additive
  — `{"error":"PolicyConfig","exit":2}` unchanged, optional `detail` field now
  populated at every site EXCEPT the bare clap parse fallback (intentional —
  argv noise must not leak through `detail`). (Axis 4 trust, Axis 5 ergonomics.)

## [0.7.1] - 2026-05-12

### Added

- New OSS crate `gaze-document` for document → safe-bundle generation.
  Ships with PNG/JPG/PDF input support via Tesseract subprocess OCR (single-page
  PDF rasterization via pdfium). Output is a `SafeBundle` containing redacted
  Markdown + a restorable `gaze::Manifest` + an OCR/PII report. New `gaze document
  clean <input> --out <dir>` subcommand on `gaze-cli` (opt-in via `--features
  document`).
- Validator-veto pre-resolver phase for validator-backed recognizer failures.
  Invalid candidates are rejected before conflict resolution, then logged as
  loser-only audit rows with `decided_by: ValidatorVeto` and typed
  `validator_fail_reason` metadata. See
  `docs/architecture/validator-veto.md`.
- Collision-family metadata and `FamilyPolicyTable` for cross-class recognizer
  rivalries. Bundled `core-extended` now declares PAN-vs-IBAN and phone-family
  metadata, `ConflictTier::CollisionPolicy` is audit-serializable, adopter
  custom recognizers can declare non-reserved collision families, and
  `xtask family-policy-table-coherence` validates bundled declarations.
- Mandatory-anchor resolution for collision-family recognizers. Bundled locale
  cue blocks under `[locale.cues.<key>]` can keep structural candidates on their
  precise variant; missing anchors emit a family-level
  `PiiClass::Custom("family:<name>")` token plus `AmbiguityReason::NoAnchor`.
  `xtask locale-cue-bundle-coherence` validates bundled cue coverage.
- Ambiguity side-channel and bundled audit migration for v0.7.x collision
  handling. `RedactionEntry` can carry `ValidatorFailReason` and
  `AmbiguityRecord`; `SqliteLogger` migrates `validator_fail_reason`,
  `ambiguity_record`, `collision_family`, and `collision_variant`; CLI audit
  queries can filter ambiguity and collision metadata. See
  `docs/architecture/ambiguity-side-channel.md`.
- `gaze-document` opt-in `mcp` feature exposes `gaze_read_file` and
  `gaze_read_text` Tool impls that route document ingestion through
  `PiiEnvelope::dispatch`. Returns `{ clean_markdown, manifest_id,
  file_metadata }`.
- `gaze-cli` opt-in `mcp` feature exposes `gaze mcp install --client=<name>`,
  `gaze mcp doctor`, and `gaze mcp serve` subcommands. Install writes client
  JSON with the absolute `current_exe()` path and an idempotent marker-fenced
  skill section in `AGENTS.md`. Ships claude-code, claude-desktop, and cursor
  at launch.

### Changed

- [bundle-tokenization-drift] `core-extended` no-policy snapshot refreshed for
  mandatory-anchor fallback on structural IBAN candidates without loaded locale
  cue bundles.

### Deprecated

### Removed

### Fixed

### Security

## [0.7.0] - 2026-05-11

### Added

- `gaze-mcp-core` — transport-free PII chokepoint runtime: `Tool` trait, sealed `ToolCtx`, `ToolRegistry`, `PiiEnvelope::dispatch`, `Frontend`/`DispatchHost`, `ManifestStore`, `AuthHook`, and `SessionIdPolicy`. Public tool structs (`CleanTool`, `TokenizeFieldTool`, `SafetyNetCheckTool`, `ExportSessionTokensTool`, `RestoreTool`, `RestoreStrictTool`) all use `#[non_exhaustive]` with `pub fn new()` constructors per pre-1.0 SemVer policy. (#162)
- `gaze-mcp-rmcp` — rmcp transport sink that binds `gaze-mcp-core`'s transport-free runtime to the rmcp protocol surface. (#174)
- `gaze_pii::Session::export_with_extension(DocumentExtension) -> Result<SensitiveSnapshot>` — opt-in document mode for OCR/PDF/transcript bundles. (#177)
- `gaze_types::DocumentExtension` (signed-envelope-bound integrity hashes for `<base>-agent/` files). (#177)
- `gaze_types::TextOrigin`, `CodecAuditRow`, `CodecCapabilitySet`, `ExtractionDensityPolicy`. (#177)
- `docs/architecture/document-extension.md` — bundle contract + two-dir layout reference. (#177)
- Coverage feedback loop Phase 0+1: xtask `coverage-corpus` + 5-fixture integration test skeleton. (#176)
- Coverage feedback loop (Phase 2-5): full synthetic corpus plus info-only trend gate. (#178)
- CC-8 token-shape shadow guard at policy + rulepack regex paths fails closed on patterns that match emitted token samples. `PolicyError::TokenShapeShadow` + `RulepackError::TokenShapeShadow`. (#162)
- `gaze-audit` columns `snapshot_scheme` (TEXT NOT NULL), `snapshot_alg` (TEXT NOT NULL), `snapshot_key_version` (INTEGER NULL) on the audit row. Pre-existing rows migrate with `"gaze.snapshot.v1.sha256-salted"` / `"SHA-256"` / NULL defaults. Plumbed through `AuditLogRow`, `AuditFilter`, and `build_audit_query_sql`. (#179)
- `Ipv4Parse`, `Ipv6Parse`, and `EthEip55` validator kinds for parser-backed
  IP address validation and EIP-55 Ethereum address checksums. Closes #440.
- `eth.address` in `core-extended`, emitting `custom:eth_address` for
  EIP-55-valid Ethereum addresses.
- New dependency: `sha3 = "0.10"` for Keccak-256 checksum validation.

### Changed

- `gaze_pii::Session` snapshot reference now binds the final emitted byte sequence rather than an earlier semantic object. Operator-bypass mutations post-snapshot are detectable. Pre-existing audit rows continue to verify under the v1 scheme tag. (#179)
- Snapshot envelope: text-only `Session::export()` stays v3; document-extended `Session::export_with_extension()` emits v4. v0.6.x readers fail closed on v4. (#177)
- Workspace bumped 0.6.6 → 0.7.0.
- [bundle-tokenization-drift] `eth.address` and parser-backed IP validator fixtures refreshed `core` and `core-extended` no-policy snapshots.

### Fixed

- `gaze_pii::default_policy` falls back to `Tokenize` (axis-1 fail-closed). (#175)

### Deprecated

### Removed

### Security

## [0.6.6] - 2026-05-09

### Fixed

- Each crates.io page for `gaze-pii`, `gaze-types`, `gaze-audit`, `gaze-recognizers`, `gaze-assembly`, and `gaze-cli` now renders its own per-crate README. Previously, the v0.6.5 placeholder publish mirrored the project root README to all 8 placeholder stubs, so adopters landing on `crates.io/crates/gaze-types` saw the umbrella project README instead of the gaze-types-specific content.

### Changed

- Real workspace crates publish to crates.io at v0.6.6 via the trusted-publisher OIDC workflow. Placeholder stubs at v0.6.5 remain as version history.

### Notes

- `gaze-mcp-core` and `gaze-mcp-rmcp` stay at v0.6.5 placeholder content until their feature branches merge in v0.7. The v0.7.0 release publishes both as real crates with their own per-crate READMEs.
- No code changes vs v0.6.5. Detection contracts, audit-sink isolation, and recognizer behavior are identical.

## [0.6.5] - 2026-05-09

### Added

- `SECURITY.md` — vulnerability disclosure policy with scoped in/out-of-scope
  criteria for the chokepoint runtime, audit-sink isolation, and recognizer
  fail-open regressions.
- `CODE_OF_CONDUCT.md` — Contributor Covenant 2.1.
- `.github/workflows/publish-crates.yml` — crates.io trusted-publisher OIDC
  workflow, with no long-lived token, for workspace publishes on tag push or
  manual dry-run dispatch.
- README badges for crates.io, license, docs.rs, tests, and GitHub stars, plus an
  "Available on crates.io" section listing all published workspace crate names.
- `.github/workflows/test.yml` — fmt + clippy + workspace test suite on PRs
  and main push.
- Placeholder publishes on crates.io at 0.6.5 for `gaze-pii`, `gaze-types`,
  `gaze-audit`, `gaze-recognizers`, `gaze-assembly`, `gaze-cli`,
  `gaze-mcp-core`, and `gaze-mcp-rmcp` to reserve namespace ahead of the v0.7
  real publish. Each placeholder mirrors the canonical project README and
  declares the same internal dependency topology the real workspace will
  publish.

### Changed

- README rewrite: tighter lede, copy-paste build-from-source install snippet
  until v0.7, token format example matched to runtime output, license section,
  and no v0.7 roadmap language in install instructions.
- Repo description changed from "GDPR-compliant debugging proxy between AI
  agents and production data" to "Reversible PII pseudonymization runtime for
  agentic LLM workflows."
- Adopter attribution in CHANGELOG and the `gaze-recognizers` NER module docs
  now uses neutral "an adopter" phrasing instead of named individuals.
- Repository visibility changed from private to public.

### Notes

- No code changes in this release. Detection contracts, audit-sink isolation,
  and recognizer behavior are identical to v0.6.4. Adopters pinned to `^0.6.4`
  resolve to v0.6.5 with no behavioral diff.
- v0.7.0 is the next functional release; it introduces `gaze-mcp-core`
  (chokepoint runtime) and `gaze-mcp-rmcp` (rmcp transport adapter) as full
  implementations.

## [0.6.4] - 2026-04-30

### Added

- `phone.national.de` rulepack class extended with DE 3-digit and 4-digit
  area-code metro alternations. Closes #420.

### Changed

- Removed bogus `891` ONK from the 3-digit alternation; BNetzA
  Vorwahlverzeichnis source URL and as-of date are pinned in test fixture
  comments.
- Test fixtures use synthetic non-reachable subscriber shapes
  (zero-exchange-code per `CONTRIBUTING.md:42`) instead of real-looking
  BNetzA-assigned numbers.
- Pre-push hook gains a docs-only fast-path for allowlisted documentation
  paths, from PR #120 by external contributor @naoray.

### Fixed

- IBAN-shape mod-97-failing input now has test coverage documenting the
  class-misattribution behavior while preserving manifest restore and avoiding
  leaks.

## [0.6.3] - 2026-04-30

### Added

- `phone.national.de`: 10-digit metropolitan landline coverage (Berlin 030,
  Hamburg 040, Frankfurt 069, Munich 089). Previously only matched 11+ digit
  national-significant-numbers, leaking common metro landlines. Closes #414.

### Fixed

- `phone.national.us`: consuming-boundary mirror with `phone.national.de`
  rejects identifier-attached numbers like `Order_15551234567`,
  `Customer+12025550100`. Closes #415.
- `phone.structural`: cross-recognizer leak — applied consuming-boundary class
  so global E.164 candidate respects same identifier-attached rejection as
  national recognizers. Previously `Customer+12025550100` leaked through
  `phone.structural` even after `phone.national.us` rejected it.
- DE phone regex no longer over-matches formatted IBAN tails like
  `DE89 3704 0044 0532 0130 00`.

## [0.6.2] - 2026-04-30

### Fixed

- `ip.v6` recognizer: RFC 4291 §2.2 IPv4-embedded form support
  (`x:x:x:x:x:x:d.d.d.d`, including IPv4-mapped `::ffff:d.d.d.d` and
  IPv4-compatible `::d.d.d.d`). Previously, inputs like
  `::ffff:192.0.2.128` partially tokenized as `::ffff:192`, leaking the
  embedded IPv4 octets. Closes #419.

## [0.6.1] — 2026-04-30

### Added

- `gaze clean --openai-filter-device {auto|cpu|cuda|mps}` selects the
  Pass-3 OpenAI SafetyNet subprocess device. The default `auto` preserves
  v0.6.0 behavior (closes #362).

### Changed

- `phone.national.de` now matches German national phone numbers across
  hyphen, space, slash, and dot separator variants, including `0171-...`,
  `0171 ...`, `0171/...`, and `+49-171-...` shapes (closes #316, refs #92).

### Fixed

- `xtask cargo-metadata-audit-isolation` now fails loud on unknown feature
  names instead of silently ignoring them, with an explicit cross-platform
  allowlist for known optional cargo metadata features (closes #340, closes
  #350).
- Default-feature CLI builds no longer warn on dead OpenAI device-selection
  helper code when `safety-net-openai` is disabled.

## [0.6.0] — 2026-04-29

### Added

- Tracked `.githooks/pre-push` runs full local gate matrix (cargo fmt + tests + xtask gates) before allowing push. Doc-only pushes fast-path. `GAZE_PREPUSH_FAST=1` skips xtask gates when CI is healthy. One-time setup: `git config core.hooksPath .githooks` per clone.
- **v0.6 GH #24 anchored_match recognizer kind:** cue-anchored
  `Name` detection now covers email forward headers, agent reply preambles, and
  auto-footers through deterministic structural rules. The default `core`
  bundle adds `name.forward_marker`, `name.agent_recipient`, and
  `name.auto_footer` with structural audit source labels such as
  `structural.agent_recipient`.
- **v0.6 locale cue buckets:** `locale-de` now ships `forward_markers`,
  `agent_recipient_cues`, and `footer_cues` with German cues plus English
  safety duplicates; `locale-en` ships English-only cue buckets. The synthesis
  matrix and 12-fixture false-positive budget are locked in tests for GH#24.

### Changed

- **Trait method signature changed.** Custom `RedactionLogger` impls must update
  their return type from `gaze::Result<()>` to
  `Result<(), gaze_types::RedactionLogError>`. Import-path source-compat is
  preserved via the permanent `gaze::RedactionLogger` re-export; the canonical
  trait home is `gaze_types::RedactionLogger`.
- **v0.6 RedactionLogger home moved to `gaze-types` (closes #252):**
  `gaze-types` now owns `RedactionLogger` and the closed
  `RedactionLogError` sink-error set. `gaze-audit::SqliteLogger` implements
  the trait directly, and `gaze` converts sink failures at the pipeline
  boundary through `gaze::Error::RedactionLog`.
- **v0.6 closes #114 — generic locale-bucket placeholder syntax adopted in
  bundled `core` rulepack:** the shipped `email.header.name` recognizer now uses
  the canonical `{locale.email_headers}` placeholder. The legacy
  `{locale_email_headers}` underscore alias still parses for back-compat (one
  more rev cycle, scheduled to drop in v0.7) — adopter rulepacks should migrate
  to the dotted form. No detection or token-shape change.
- **v0.6 adopter migration for GH#24:** v0.5.1 adopters can load
  `["core", "locale-de"]` under `[locale].active = ["de-DE"]` to tokenize the
  prompt/header/footer leak shapes reported by adopters without changing existing custom
  recognizers. Mixed German/English templates can load
  `["core", "locale-de", "locale-en"]`; per-tenant cue additions should live in
  custom locale/rulepack data.
- **v0.6 known limits documented:** `anchored_match` still fires inside
  markdown code fences and URLs in v0.6; RegionHint-based `CodeBlock` / `Url`
  exclusion is deferred to v0.7. The docs also call out deferred Subject/Re
  anchors, unanchored scheduling prose, the current `person_name`-only
  `name_shape`, and global rather than per-region NER thresholding.
- **v0.6 audit source-label coverage:** audit-row metadata tests now lock in
  `AUDIT_RESTRICTED_COLUMNS` including `source`, so persisted audit queries can
  explain structural `anchored_match` emissions without adding a
  `recognizer_id` column. References GH #24.
- **v0.6 audit source-label normalization:** `name.auto_footer` now emits the
  structural source label `structural.footer`, matching the `footer_cues`
  bucket wording used by the bundled locale rulepacks. References PR #84 NIT
  #289.

### Deprecated

### Removed

- **BREAKING — audit-sink imports.** Replace `use gaze::SqliteLogger;` with
  `use gaze_audit::SqliteLogger;`. Same for `gaze::AuditFilter`,
  `gaze::AuditLogRow`, `gaze::build_audit_query_sql`, and
  `gaze::AUDIT_RESTRICTED_COLUMNS`. The v0.5 `gaze = { features = ["audit"] }`
  shim is removed.
- **v0.6 audit feature shim removed from `gaze` (closes #315):** removed the
  `audit` feature, the optional normal `gaze-audit` dependency, the cfg-gated
  `gaze::{SqliteLogger, AuditFilter, AuditLogRow, build_audit_query_sql,
  AUDIT_RESTRICTED_COLUMNS}` re-exports, the cargo-deny `gaze.audit` feature
  ban, and the xtask `"gaze audit feature sanity"` cargo-metadata graph.

### Fixed

### Security

### Pass-3 SafetyNet (PR #91 — ships in v0.6.0 alongside the audit-shim drop and the v0.6 anchored_match work)

#### Added

- **Pass-3 observer-only SafetyNet rollup (PR #91):** new
  privacy backend that audits Gaze's clean output for PII the deterministic
  pipeline missed, without ever mutating the manifest, the clean text, or
  the restore path. The shipped backend is the official OpenAI Privacy
  Filter (`opf`) subprocess adapter. North-star fit is explicit: A1 (never
  leak) holds because the upstream `text` and `placeholder` JSON fields
  are stripped at the adapter boundary and never cross into Gaze; A2
  (reversibility) is preserved because the contract is observer-only and
  the manifest is immutable from a backend's perspective; A3
  (agentic-first) is supported by per-field structured-document traversal
  that emits field-pathed suspects for agent tool-call JSON; A4
  (auditable + deterministic) is preserved by the closed `SafetyNetError`
  variant set, the typed `LeakKind` classification (`Uncovered` /
  `PartialBleed` / `ClassMismatch`), and the optional `safety_net_log`
  SQLite table.
- **`gaze-types` SafetyNet trait surface (Phase 1):** new public
  `SafetyNet`, `SafetyNetContext`, `LeakSuspect`, `LeakKind`,
  `LeakReport`, `LeakReportTelemetry`, `SafetyNetPiiClass`,
  `OpenAiPrivateLabel`, and `SafetyNetError` types. The contract is
  byte-free: `SafetyNetContext` is `Copy`, holds borrowed references, and
  exposes only manifest, locale chain, document kind, optional opaque
  session id, and optional structured field path.
- **`Pipeline::clean_with_safety_net_detect_context` (Phase 2):** new
  pipeline entry point that runs deterministic clean, builds the manifest,
  and dispatches per-field structured traversal to registered safety
  nets. Returns `(CleanDocument, LeakReport)`. Locale-skip telemetry is
  recorded per field when the session-level locale chain does not match
  the backend's `supported_locales`.
- **`OpenAiFilterSafetyNet` adapter (Phase 4) at
  `crates/gaze-recognizers/src/safety_net/openai_filter`:** subprocess
  adapter for the official `openai/privacy-filter` `opf` CLI, invoked as
  `opf --format json --output-mode typed`. Adopters bring their own
  pinned upstream Git revision or release. PII-bearing `text` and
  `placeholder` JSON fields are deserialized through a private
  `PrivatePiiString` whose `Drop` clears the buffer and whose `Debug`
  writes `<private-opf-field>`; spans are projected to `RawSpan`
  (start, end, label, score) before any code outside the adapter sees
  them.
- **Subprocess deadline + resource isolation (closes #320, refs #321,
  closes #322):** single deadline covers stdin write, stdout read,
  stderr read, and child wait. Timeout fires `SIGKILL` and reaps the
  process, returns `SafetyNetError::Runtime { message: "opf subprocess
  timed out and was killed" }`, which the CLI maps to exit `3` with
  variant `Timeout`. Stdout/stderr readers are bounded (4 MiB / 256 B).
  Initialization failures are cached in a
  `OnceLock<Result<Arc<...>, Arc<...>>>` so deterministic problems do
  not retry on every clean.
- **Stderr discipline:** default `Stdio::null()`. Opt-in
  `with_stderr_diagnostics(true)` captures up to 256 bytes, replaces
  non-printable bytes with spaces, and sanitizes whitespace-separated
  tokens that contain `@` or seven or more ASCII digits to `<redacted>`
  so backend logs cannot leak emails or phone shapes.
- **Checkpoint perms verification:** `--openai-filter-checkpoint` must
  exist before the subprocess spawns. Files and directories must be
  owned by the current uid, must not be symlinks, and must not be
  group/world writable; directories must be mode `0700`. Missing
  checkpoints produce sanitized `WeightsMissing { path:
  "<missing:<filename>>" }`.
- **`gaze-cli` SafetyNet surface (Phase 6):** new flags `--safety-net`,
  `--openai-filter-command`, `--openai-filter-checkpoint`,
  `--openai-filter-operating-point`, `--safety-net-timeout-ms`,
  `--safety-net-input-limit-bytes`, `--safety-net-mode` (strict |
  tolerant). `clean` JSON output gains a `leak_report` block carrying
  typed stats. Strict mode exits `3` on `Uncovered` / `PartialBleed`
  suspects with variant `SuspectedLeak`; tolerant mode emits a stderr
  `{"warning":"SafetyNet",...}` event and exits `0`. `ClassMismatch`
  always warns and never fails strict mode.
- **Exhaustive `SafetyNetError` -> `CliError::SafetyNetFailure` mapping:**
  stable variant strings (`Unavailable`, `WeightsMissing`,
  `ModelUnavailable`, `InputTooLarge`, `Timeout`, `Runtime`,
  `InvalidOutput`, `SuspectedLeak`) so adopters can branch on the
  failure shape without parsing free-form text.
- **`safety_net_log` audit table (Phase 5, `gaze-audit`):** new table
  on the existing audit DB stores metadata-only suspect rows plus
  `LocaleSkipped` telemetry events. Restricted columns lock that no raw
  upstream payload (text or placeholder bytes) is persisted; the
  `safety_net_log_does_not_persist_suspect_or_placeholder_bytes` test
  pins the invariant. `gaze audit safety-net query --audit-db <path>`
  reads filtered rows back from a read-only connection.
- **`safety-net-sanity` xtask gate (Phase 7):** new behavioral gate
  batched across `gaze`, `gaze-cli`, `gaze-recognizers`, and
  `gaze-audit` that asserts manifest diff invariants, strict/tolerant
  CLI behavior, subprocess boundary safety, and `safety_net_log` schema.
  Enforced by `.githooks/pre-push` through
  `cargo run -p xtask -- safety-net-sanity`.
- **`class-map-override-safety` extension (Phase 7):** the existing gate
  now asserts that `all_official_labels_map_exactly_to_gaze_classes`
  runs and passes, so the closed OPF label allowlist cannot drift
  silently.
- **`ci-feature-matrix` extension (Phase 7):** the matrix now enrolls
  the `safety-net` and `safety-net-openai` feature combos so the gated
  code paths are covered by the local pre-push gate.
- **MockSafetyNet test helper (Phase 3):** `gaze-recognizers` exports
  a `test-support`-gated `MockSafetyNet` so adopter tests can drive
  manifest diffing without spawning a subprocess.
- **Documentation (Phase 8):** new
  [`docs/architecture/safety-nets.md`](docs/architecture/safety-nets.md)
  covers the trait shape, observer-only contract, OPF adapter boundary,
  stderr discipline, structured-doc traversal, replay hash, audit
  table, and CI gate. `crates/gaze-cli/README.md` documents every
  flag, the exit-code map, the latency budget, and synthetic examples
  using only approved fixtures (RFC 6761 `*.invalid` domains, NANPA
  `555-01xx` phones, Ofcom drama ranges). `docs/policy.md` notes that
  SafetyNet activation is CLI / programmatic only and lists the
  requirements any future TOML surface must satisfy (locale gating,
  fail-closed load, default strict mode, CLI override precedence).

#### Changed

- **deny.toml feature scope (Phase 0):** safety-net dependency bans for
  `reqwest`, `hyper`, `tokio`, and `ureq` are scoped to the
  `safety-net-*` feature graphs. The `cargo-metadata-audit-isolation`
  xtask gate is the authoritative enforcer; `cargo-deny` remains a
  belt-and-suspenders check for feature policy.

#### Notes for adopters

- SafetyNet code paths are gated off by default. Build with
  `--features safety-net-openai` on `gaze-cli` (or `gaze-recognizers`
  for programmatic use) to opt in. Existing clean / restore consumers
  see no dependency-graph change.
- Bring-your-own-binary plus bring-your-own-weights: install `opf`
  from a pinned upstream Git revision or release. The adapter does
  not download or update the checkpoint. Pin the install path with
  `GAZE_OPENAI_FILTER_OPF=<path>` or `--openai-filter-command=<path>`.
- Strict mode is the default. Tolerant mode
  (`--safety-net-mode=tolerant`) preserves exit `0` for runs that
  report suspects, but always writes a stderr warning event so
  monitoring can pick it up.
- Activation is **CLI / programmatic only**, not `policy.toml`, in this
  Pass-3 rollup. See `docs/policy.md` for the requirements any future
  TOML surface must satisfy.
- This rollup ships in **v0.6.0** alongside the audit-shim drop
  and the v0.6 `anchored_match` recognizer work. Adopters
  upgrading from v0.5.x see one combined release: switch
  `gaze::SqliteLogger` imports to `gaze_audit::SqliteLogger`, then opt
  into SafetyNet at their own pace via the `safety-net-openai` feature.

#### Deferred to a post-v0.6.0 release

The following SafetyNet items are intentionally out of scope for v0.6.0
and are tracked for a later release:

- **Live-model nightly workflow** with a non-empty synthetic corpus to
  detect FP-rate drift between checkpoint upgrades.
- **Native `ort` backend** with a `weights.rs` SHA-pinned scaffolding
  module that removes the subprocess hop. The `OpenAiFilterBackend`
  trait shape was designed so the same adapter API serves both
  subprocess and in-process implementations.
- **Fetch / download command** (`gaze safety-net fetch`) that pulls a
  pinned `opf` build into a private cache directory and verifies the
  checksum offline. Closes the "first-run requires manual install" gap.
- **Long-lived subprocess / daemon mode** to amortize subprocess
  startup cost when latency budgets tighten.
- **False-positive adjudication dashboard** on top of `gaze audit
  safety-net query` and `audit export` so reviewers can triage
  suspects across runs.

See
[`docs/architecture/safety-nets.md` "Future work"](docs/architecture/safety-nets.md#future-work-deferred-to-a-post-v060-release)
for the same list with its design notes.

## [0.5.2] - 2026-04-29

### Added

- **NER adopter assets (GH issue #90 items 1+4):** promoted the
  Davlan mBERT label contract and canonical NER policy snippet to
  `crates/gaze-recognizers/assets/ner/` for framework adapters and adopters. `crates/gaze-recognizers/assets/ner/README.md`
  documents the BIO tag to Gaze class schema, the `"drop"` sentinel, and the
  future `gaze model fetch <name>` / `gaze policy snippet ner` manifest path.

### Changed

- **Pinned default NER artifact source (GH issue #90 item 2):**
  `scripts/fetch/fetch-ner-model.sh` now installs the pre-quantized int8 ONNX artifact
  from `onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` at commit
  `cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8`, copies the Gaze-authored
  `labels.json` contract, and verifies all installed bytes against the
  repository-root `SHA256SUMS`.
- **Policy docs for NER adopters:** `docs/policy.md` now cites the canonical
  `crates/gaze-recognizers/assets/ner/` contracts, documents `[ner].locale` as a single BCP47 string,
  and calls out Rust-regex inline flags such as `(?i)` in
  `[[policy.custom_recognizers]].pattern`.

## [0.5.1] - 2026-04-29

### Fixed

- **Bundled rulepack version sync:** corrective patch - bundled `core`, `core-extended`, `locale-de`, and `locale-en` rulepacks now report `rulepack_version = "0.5.1"`, restoring the v0.4.6 CHANGELOG contract that bundled rulepacks track `gaze-recognizers`. v0.5.0 release-prep missed the embedded TOMLs; this patch corrects that.

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
- **v0.5 Phase D — `dylint-gate` xtask command (PR #76):** verifies the `lint/dylint/ui` fixture corpus has exactly 18 enabled fixtures, rejects `*_disabled.rs`, and runs `cargo dylint --workspace --all` when `cargo-dylint` is installed (skips with a clear message locally when absent; CI installs it explicitly).

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
- **`audit_metadata_only` xtask gate (PR #59):** compile-time enforcement that restore-path code does not import audit metadata symbols. Walker covers file scope `use`, nested `mod`, function/impl/trait-default/const/static block-statement `use`, glob imports, aliased crates, `extern crate`, and `#[path]`-resolved external modules. Known limitations (fully-qualified path references, `include!`, let-else diverge, macro-emit) documented in `docs/architecture/xtask.md`; v0.5 architectural pivot to dylint-based name-resolution lint scheduled.
- **`--session` audit filter (PR #57):** opaque session-scope filter for `gaze audit query` / `gaze audit export` (NOT raw `session_hex`).
- **DE + US national phone recognizers (PR #58):** parser-backed E.164 region-aware validators (`phonenumber` crate) for German and US national phone numbers. Cooperate with structural phone recognizer; gated behind `phone-parser` Cargo feature.
- **ClassMapOverrideSafety extension (PR #55 / S4):** further hardening of class-map override safety gate.
- **Rulepack version bump validation (PR #56 / S5):** rulepack version bump audit + drift-prevention rule.
- **`gaze-assembly` crate restructure (PR #61 / S6):** `lib.rs` split into focused modules by responsibility.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.5`.
- **`core-extended` no-policy locale activation (PR #58):** the bundled `core-extended` rulepack now activates `phone.national.de`, `phone.national.us`, `postal.us`, and `postal.de` recognizers when invoked without a policy via `--rulepack-bundled core-extended`. Previously these required an explicit `--locale` or policy-supplied locale. Adopters using the bundle without a policy will see additional tokenization for German/US national phone numbers AND bare 5-digit numeric strings (matching the postal recognizers). To restore prior behavior, supply an explicit `--locale=global` or pass a policy with narrower locale gating.

### Fixed

- No standalone `fix(...)` commits landed between `v0.4.4` and `v0.4.5`; the bundle is release plumbing plus S1-S6 feature, hardening, and documentation work.

### Documentation

- README catch-up for v0.4.2-v0.4.4 (PR #60).
- README Requirements section with per-OS support matrix (PR #62).
- Org transfer URL sweep from the original org to the then-current org (PR #63).
- New `docs/architecture/xtask.md` documenting `audit_metadata_only` gate coverage, known limitations, and v0.5 roadmap.
- New `v0.5-dylint-audit-gate.md` research stub (now hosted in [PIInuts/business:research/](https://github.com/PIInuts/business/blob/main/research/v0.5-dylint-audit-gate.md)).

## [0.4.4] - 2026-04-26

### Added

- **S1 ClassMapOverrideSafety xtask gate** (#51): the previously scaffolded gate is now active. The behavioral test runner invokes `t20_context_class_map_overrides_policy_dict_class` and `t20a_class_map_override_fails_closed_when_action_rule_uncovered` through `cargo test`, while `.github/workflows/class-map-override-safety.yml` runs the gate on PRs and pushes to `main`. An adversarial in-PR self-test programmatically verifies the gate fails non-zero when a listed test is missing or renamed, following the meta-Potemkin guard captured in drawer `gaze_architecture_12b32d53`.
- **S2 audit schema v2** (#53): `RedactionEntry` now includes `created_at: i64` epoch milliseconds, with an on-open SQLite `ALTER TABLE` migration so legacy DBs without `created_at` remain queryable through a NULL default. `gaze audit query` and `gaze audit export` now accept `--from <iso8601>` and `--to <iso8601>` filters, JSONL export includes `created_at`, and ISO 8601 parse failures emit typed `CliError::PolicyConfig` messages with the offending input quoted. Time-filtered queries omit NULL `created_at` legacy rows by SQL semantics; unfiltered queries still include them. Fixture coverage covers both v0.4.3-shaped and v0.4.4-shaped SQLite DBs.
- **S3a phonenumber-backed `E164Phone` validator** (#52): the `phonenumber` crate is available behind the optional `phone-parser` feature, default-on for `gaze-cli` and opt-in for raw library users. `ValidatorKind::E164Phone` extends the existing `phone.structural` recognizer in `core-extended.toml`, preserving valid E.164 matches such as `+4915550112233` while rejecting regex-passing but unassigned shapes such as `+99999999`. Builds without `phone-parser` reject the `e164_phone` validator at rulepack load time with `RulepackError::UnsupportedValidator`, preserving axis-1 fail-closed behavior rather than silently dropping phone detection at runtime. Audit notes live in [`PIInuts/business:research/v0.4.4-phonenumber-audit.md`](https://github.com/PIInuts/business/blob/main/research/v0.4.4-phonenumber-audit.md).
- **S4 Date posture memo** (#50): [`PIInuts/business:research/v0.4.4-date-posture.md`](https://github.com/PIInuts/business/blob/main/research/v0.4.4-date-posture.md) locks Gaze's Date-as-PII stance. Dates are not PII by default, never ship in default `core` or `core-extended` bundles, and future v0.4.5+ implementation scope is limited to DOB-only structured contexts. General-prose dates require context classification research for v0.5+, and the GH #5 token-spam tradeoff is resolved as no-default-on. The negative corpus covers version strings, IPs, file paths, ID-shaped numerics, year-only strings, and build or CI metadata.

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
- DOB-scoped Date recognizer, per the S4 memo and only if an adopter provides a concrete DOB leak fixture.
- S3b national phone recognizers for DE and US, deferred from v0.4.4 due to scope budget.
- ClassMapOverrideSafety coverage for other class-rule paths.
- Audit retention and auto-purge, now unblocked by the v0.4.4 `created_at` foundation.

### Deferred to v0.5

- Open-key `PiiClass` refactor.
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

- Adopter-reported gap closed: locale-aware email-header recognizer (`Von:` / `An:` plus English defaults) tokenizes header display names and restores them round-trip. See GH #24.
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
on an unmet dependency. Adopter target is Apple Silicon, so dropping x86_64
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
  to avoid blocking adopters on the adapter retarget.
- **Homebrew SHAs are placeholders** until the workflow publishes the
  darwin binaries; follow-up commit fills them.

[Unreleased]: https://github.com/EmpireTwo/gaze/compare/v0.6.4...HEAD
[0.6.4]: https://github.com/EmpireTwo/gaze/compare/v0.6.3...v0.6.4
[0.6.3]: https://github.com/EmpireTwo/gaze/compare/v0.6.2...v0.6.3
[0.6.2]: https://github.com/EmpireTwo/gaze/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/EmpireTwo/gaze/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/EmpireTwo/gaze/compare/v0.5.1...v0.6.0
[0.4.6]: https://github.com/EmpireTwo/gaze/compare/v0.4.5...v0.4.6
[0.4.5]: https://github.com/EmpireTwo/gaze/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/EmpireTwo/gaze/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/EmpireTwo/gaze/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/EmpireTwo/gaze/compare/v0.4.0-rc.1...v0.4.2
[0.4.0-rc.1]: https://github.com/EmpireTwo/gaze/releases/tag/v0.4.0-rc.1
[v0.3.1]: https://github.com/EmpireTwo/gaze/releases/tag/v0.3.1
[0.3.0]: https://github.com/EmpireTwo/gaze/releases/tag/v0.3.0
[0.3.0-rc.2]: https://github.com/EmpireTwo/gaze/releases/tag/v0.3.0-rc.2
[0.3.0-rc.1]: https://github.com/EmpireTwo/gaze/releases/tag/v0.3.0-rc.1
