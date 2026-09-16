# Upgrading Gaze

This file is a per-minor migration guide for adopters of the `gaze-pii`
workspace (the published cargo name; the library is imported as `gaze`).
Pair it with [CHANGELOG.md](CHANGELOG.md): CHANGELOG records what changed,
UPGRADE.md tells you what *you* need to do.

## How this file is organized

- One H2 section per `MAJOR.MINOR` release in **reverse-chronological** order.
- Each section opens with **TL;DR** (the one or two actions an adopter
  cannot skip), then drills into details.
- "Additive" entries are no-action and noted for awareness only.
- "Action required" entries are the ones a human upgrade reviewer should
  read in full.

## Pre-1.0 promise

Gaze is pre-1.0. Per the [SemVer pre-1.0 contract][semver-pre1] minor bumps
*may* introduce breaking changes; we minimize them. Every breaking surface
in this file is also a breaking entry in CHANGELOG.md, gated by closed
non-exhaustive enums + typed errors so that downstream code only breaks
at compile time, never silently at runtime.

The five north-star axes — **reliability, reversibility, agentic-first,
trust, ergonomics** — bound every upgrade. Reversibility means: if an
upgrade ever changes a manifest's restore round-trip, that is a bug, not
a migration step. Manifests written by an older minor restore on the new
minor unless this file explicitly says otherwise. (No such exception
exists today.)

[semver-pre1]: https://semver.org/spec/v2.0.0.html#spec-item-4

---

## v0.14.x → v0.15.0

### TL;DR

1. **Your token stream changes even if you change nothing.** Residual coverage
   is on by default, and three new postal recognizers run at every locale.
   Re-baseline any test or consumer that counts spans or asserts exact output.
2. Handle the result of `PiiClass::custom` and add `max_sessions` to Rust
   `SessionCfg` literals — the two source breaks.
3. Budget full-input and configured-net scanning latency; handle session-capacity
   errors and new safety-net and proxy denials. Safety-net fallback documents run
   one extra model pass and can complete carrying a finding in their leak report.
4. If you run `gaze-proxy`, upgrade for the fallback-deletion leak fix. See
   [the CHANGELOG Security section](CHANGELOG.md).
5. **Measured clean p95 regressed 28.3% (195.86 ms → 251.36 ms).** Size capacity
   against that before rolling out if you are p95-sensitive.

### Residual coverage is on by default (action required for counting consumers)

Every pipeline built through `Pipeline::builder()` now protects raw bytes that
admitted originals evidenced but conflict resolution did not keep. One
recognized value can therefore contribute more than one replacement.

- **A manifest span count is a count of replacements, not of distinct
  recognized values.** The same applies to `gaze-document`
  `BundleReport::pii_token_count`, `pii_tokens_by_class`, and
  `ClassCount::count`. `bundle_version` is unchanged.
- **Restore is unaffected.** Round-trip behavior is the same.
- **Bytes that no original evidenced remain uncovered.** Residual coverage
  widens protection over evidenced bytes; it is not a completeness claim.
- `EmittedTokenSpan` gained `origin: EmittedTokenOrigin` (`Whole` |
  `ResidualFragment`). `Whole` is the default and is omitted on the wire, so
  existing whole-span JSON is byte-identical and pre-v0.15 JSON reads back as
  `Whole`. `EmittedTokenSpan::new` keeps its signature.

**Unmigrated readers are the real risk.** There is no `deny_unknown_fields` and
no version field, so a consumer built before v0.15 silently ignores the new key
and counts a residual fragment as a whole entity. Rebuild entity-counting
consumers against v0.15.

In `gaze-token-bridge`, a residual fragment is handled **by location, not by
identity**: it gets a class-derived placeholder in the stored snippet and
produces no `CanonicalEntity`, no `IndexEntity`, and no posting. Fragment raw
bytes no longer reach the persistent index. The cost is stated plainly: a
fragment is **protected but unsearchable** — not retrievable by value or
fingerprint, and absent from `hit.entities`. Whole entities remain searchable
exactly as before, so nothing you can do today gets narrower. See
[Residual coverage](docs/reference/redaction-classes.md#residual-coverage).

### New postal recognizers run at every locale (review your token stream)

`postal.ca`, `postal.gb`, and `postal.ie` are new, and they are
`locale_basis = "format"`. Format basis treats `locales` as provenance, not as a
gate, so these rules run for every document locale **including
`--locale=global`**. Narrowing the locale chain does not suppress them.

If you must not tokenize Canadian, UK, or Irish postal codes, disable the
recognizer outright. The measured precision cost on the 1,886-document holdout
is one false positive — an uppercase UK-postcode-shaped token in lowercase
prose — and zero across the 1,024 committed negative documents. The numeric
`postal.de` and `postal.us` rules are unchanged and keep their document-locale
gates.

### Custom class construction (action required)

`PiiClass::custom(name)` now returns `Result<PiiClass, EmptyCustomClassName>`.
At least one ASCII letter or digit must survive normalization. Propagate or
handle invalid runtime input in a fallible caller:

```rust
use gaze::{EmptyCustomClassName, PiiClass};

fn make_class(name: &str) -> Result<PiiClass, EmptyCustomClassName> {
    let class = PiiClass::custom(name)?;
    Ok(class)
}
```

An explicit `expect` is appropriate only for a known-valid literal. Constructing
`PiiClass::Custom` directly does not bypass live/staged tokenization validation.
The token bridge now treats surrounding and repeated custom-entity whitespace
consistently across its normalization paths.

### Bridge session capacity (action required for Rust configuration)

Add `max_sessions: 1000` to `gaze_mcp_bridge::config::SessionCfg` literals, or
choose a positive deployment-specific capacity. TOML omission defaults to 1,000:

```toml
[session]
mode = "ephemeral"
max_sessions = 1000
```

Zero is rejected by every construction boundary. Existing sessions remain
accessible at capacity. Ephemeral mode rejects new sessions instead of evicting
restoration mappings. In file mode, an inactive eviction candidate must be
exclusively owned and persisted successfully before admission commits. Retained
strong or weak handles, persistence errors, and cancellation preserve the
canonical cached session and can prevent admission. Release handles promptly
and handle `BridgeError::LimitExceeded` and persistence failures.

**Limitation:** `max_sessions` bounds cached sessions only. The separate per-ID
file-lock registry remains unbounded; it is not a total-memory limit.

### Prefix reuse disabled (latency and audit action required)

`enable_prefix_cache()` and `PipelineOptimizationConfig::with_prefix_cache(true)`
remain source-compatible but no longer skip detection or retain raw prefixes.
Every input is fully rescanned under its current field, locale, dictionaries,
recognizers, and rules. Both transactional prefix-cache modes use that same path.

Budget full-scan latency on growing inputs. Update audit consumers to expect
actual recognizer/rule rows instead of `prefix_cache` provenance. Token mappings
and manifest restoration retain their normal behavior. Correctness takes
priority over the removed optimization. See the
[safety rationale](docs/explanation/pipeline/tier4-pipeline-gating.md).

### Proxy residual checks now fail closed (upgrade required if you run the proxy)

Shipped v0.14.0 had a leak: after a safety-net **fallback deletion**, both
`gaze-proxy` residual checks decided on surviving manifest entries alone. A
fallback deletion emits no manifest entry, so a deleted net-only span looked
like "no PII found" while the caller still held the original raw bytes, on the
request path and on both response paths. Both boundaries now state the invariant
their own contract needs, and both only add rejections.

**What you have to do:** upgrade, and expect requests and responses that were
previously forwarded to be rejected instead. That is the fix working. No
configuration change is required.

### Safety-net and resolve behavior (review integration assumptions)

Resolve with a `Redact` fallback now scans the final text and manifest before
returning success. Remaining unprotected or malformed suspects and net errors
reject; verified live-token hits remain allowed. This adds one inference after
fallback, without another mutation or retry. Successful fallback deletion
remains one-way; a final scan does not certify exact restoration.

Two changes make more documents stay **reversible** before any one-way deletion
runs, which is a widening and needs no action: resolve now plans every gap in a
truthful `PartialBleed` report instead of requiring the first named gap to be
the only one, and it applies one additional complete reversible batch when a
successful first resolve is followed by actionable raw gaps.

Terminal validation after a fallback deletion uses deletion-aware bounds.
Supported primary `Redact` and `Generalize` replacements produce manifest
entries without live tokens, so output that was falsely rejected for lacking an
owning token is accepted again.

**A fallback document now gets one reversible round before it can be denied,
and this is a widening.** Under `SafetyNetMode::Resolve` with
`SafetyNetFallback::Redact`, the terminal scan after a fallback deletion used to
deny the document on *any* unprotected suspect. That scan is the fourth full
model pass over a string the deletion rewrote, so it routinely reported a 1–5
byte sub-word span the three earlier passes had read and accepted. The terminal
report now gets one reversible round (tokenize, never delete) and one bounded
deletion of a suspect that *contains* a deletion seam, then a typed admission.
Denials are named: a suspect covering bytes the fallback's own audit rows say it
removed, a second seam-manufactured shape, a suspect naming no real range, or a
round the resolver refuses. Everything else is merged into the returned
`LeakReport` and the document completes carrying it.

**Nothing that completed under v0.14 starts denying** — admission is strictly
wider. Two costs you do have to budget for:

- A fallback document that reports anything at the terminal scan now runs **one
  extra model pass**.
- A fresh finding that appears only *after* that round **ships raw** in the
  output with an honest report, because both bounds are spent. Measured on the
  v0.15 production corpus, that is **2 bytes in one document, overlapping 0
  gold**, out of 42 bytes across 16 spans that previously denied. Treat those as
  measurements on that corpus, seed, and model bundle, not as a bound for your
  documents — and a shipped byte that overlaps no gold is not proof it is not
  PII, only that the benchmark does not count it.

Audit consumers: the extra round emits `decided_by: resolve`, `action: tokenize`
with the fallback reason attached, which is what distinguishes it from the
second batch's rows. The protection trace projects it as an ordinary
`("safety_net", "resolve", "tokenize")`; there are no new wire keys.

Clean-to-raw mapping now understands deletions: `map_clean_span_to_raw` and
`validate_clean_manifest` reconstruct the document layout from the deletion
ledger's raw coordinates and reconcile it against the manifest, so a manifest
that disagrees with its own ledger fails closed instead of mapping onto the
wrong bytes. A document with no deletions takes the unchanged affine path
(#599).

### Agent surfaces and audit (review consumers)

**Handle new configured-net denials and inference cost.** Direct Anthropic and
legacy proxy request surfaces run configured-net admission after primary
pseudonymization and before provider I/O, including complete reconstructed
surfaces and codec validation views. Nets use actual session token ownership and
restore boundaries. Token-contained reflags, including class disagreements, are
allowed; raw gaps, malformed suspects, registry failures, and net errors reject.
Requests previously forwarded can now fail, including text preserved by primary
policy. Budget the extra inference and handle errors without bypassing
admission. Selected registry backends run across the locale chain; observer skip
optimizations do not suppress admission.

**Coverage and state limits remain.** No model is required globally; an absent
net, a custom net skipped for locale coverage, or a detector miss still limits
coverage. Existing strict protection retains its primary and locale
requirements. Primary Preserve/Redact actions and public legacy clean defaults
do not change. Direct failures abandon staged mappings before commit/send.
Legacy mappings are already published and remain live on failure; admission uses
an immutable snapshot, without a whole-request rollback or serialization
guarantee. Core live session mappings can likewise remain after a failed
fallback; caller-owned staging must be discarded rather than committed after
failure. See the
[proxy admission contract](crates/gaze-proxy/README.md#configured-safety-nets-at-request-admission).

Proxy integrations must accept rebuilt safe response headers and guards across
content blocks and structured Responses text. Agent responses remain separate
from owner/operator restoration surfaces. Audit integrations should retain
terminal MCP journal context, deciding ingress rules, and JSONL restore fields.
The MCP journal preserves context, not durable duplicate protection after
completion or restart.

**Additive, no migration:** a tool may now register
`RequestMode::UntrustedInvocation` and read unchanged untrusted execution
arguments through `dispatch_request` and `ToolCtx::invocation_args()`; the
envelope audits a constant metadata-only omission record instead of the
arguments. The default `dispatch` contract is unchanged, and a descriptor
written for the other mode is rejected before authorization or audit. Hosts must
not log those arguments.

`gaze_proxy::serve_with_listener` is additive. Embedders can hand off an owned
listener; its address must match configuration, except that a configured port of
zero resolves to the actual port. Existing `serve` remains available. Dashboard
pairing now waits for an explicit child-ready response before startup or
rotation returns success; no dashboard configuration migration is required.

### Policy and restoration (review integration assumptions)

Keep policy `schema_version = "0.1.0"`; the policy schema does not follow crate
version 0.15.0. Unsupported two-digit minor schemas now fail closed instead of
accidentally matching a prefix. Inline comments no longer suppress strict
overlap validation. Production integrations still need an explicit policy even
though CLI path-rulepack tokenization now works without one.

Use complete-text restoration through the existing manifest/session APIs.
Known bare session tokens can now restore after leading ASCII or Unicode word
characters. Matching is single-pass over original input; inserted values are
not scanned again as tokens. Family-namespace tokens restore in prose and keep
resolver provenance. Trailing word boundaries and family-hyphen ambiguity remain
guarded: separate a family token from a following hyphen with whitespace.
Restoration guarantees only reconstruction authorized by the supplied manifest,
not arbitrary suffix handling or universal unknown-suffix rejection.

### Subprocess diagnostics and remaining limits

Verbose stderr no longer fails otherwise valid inference. Diagnostics remain
opt-in, retain at most a bounded sanitized prefix, and discard the rest.
Stdout limits, invalid responses, I/O errors, and deadlines still fail closed.
Diagnostic redaction is heuristic and cannot guarantee arbitrary logged PII is
removed. Keep diagnostics off unless the operator accepts that limitation.

Unix and Windows adapters cancel pipe workers and reap the direct child on
failure. Descendant processes are not killed; cleanup is cooperative rather
than a hard real-time guarantee. Other platforms return `ModelUnavailable`
before spawning. See [subprocess behavior](docs/explanation/safety-net/safety-nets.md).

NER chunk planning borrows the existing tokenizer when truncation is already
disabled, instead of cloning it and its vocabulary on every call. This is a cost
change only; configured truncation keeps its original path.

### Budget for the measured latency regression

This release ships its own benchmark, measured like-for-like against v0.14.0 on
an identical scored-document digest. Protection and reversibility improved —
surviving PII bytes **25,179 → 22,491 (−10.68%)**, exact restoration **78.42% →
96.53%**, and zero documents failed closed. **Latency got worse: clean p95
195.86 ms → 251.36 ms (+28.3%)** on the shipped default arm.

That is the cost of full input rescanning plus the extra configured-net and
terminal model passes this release adds, and it is the number to size capacity
against. If your deployment is p95-sensitive, measure before rolling out; the
correctness changes are not individually opt-out.

**22,491 labeled PII bytes still survive on that corpus.** A valid manifest
alone does not prove detection completeness or a successful round trip. Full
provenance is in
[docs/reference/benchmarks/README.md](docs/reference/benchmarks/README.md).

---

## v0.9.x → v0.10.0

Status: **unreleased.**

### TL;DR

1. **Document bundles now split agent and owner outputs.** `gaze document clean`
   requires either `--agent-out` + `--owner-out` or the `--out` shorthand that
   creates `<PATH>/agent` + `<PATH>/owner`.

### gaze document clean — bundle layout split (axis 1)

Previous behavior: `gaze document clean --out <PATH>` wrote `clean.md`,
`manifest.json`, and `report.json` into a single directory. Uploading
that directory to an LLM workspace leaked restorable manifest material —
an axis-1 violation that depended on caller discipline rather than
runtime enforcement.

New behavior: `gaze document clean` requires `--agent-out` + `--owner-out`
or the `--out` shorthand that auto-creates `<PATH>/agent` + `<PATH>/owner`
subdirs. `clean.md` and `report.json` land in the agent path; `manifest.json`
lands in the owner path. The writer rejects equal or nested agent/owner
paths with a typed `DocumentError::BundleLayoutInvalid`.

Migration:

- If you used `--out <PATH>` and you intend `<PATH>` to remain agent-shippable,
  switch to `--agent-out <PATH> --owner-out <SOMEWHERE_ELSE>`.
- If you can accept the agent/ + owner/ subdir split, keep `--out <PATH>` —
  the shorthand now creates both subdirs for you.
- Downstream tooling that read files from `<PATH>` must move manifest reads
  to `<PATH>/owner/manifest.json` (or the explicit owner path).

---

## v0.7.x → v0.8.0

Status: **shipped.** v0.8.0 is published to crates.io; the workspace
now includes ten published crates (the new `gaze-proxy` joins
`gaze-types`, `gaze-recognizers`, `gaze-audit`, `gaze-pii`,
`gaze-assembly`, `gaze-mcp-core`, `gaze-mcp-rmcp`, `gaze-document`,
and `gaze-cli`).

### TL;DR

1. **Bundle unification.** If your CLI invocation or `policy.toml`
   references `core-extended`, switch to `core` and pass an explicit
   `--locale` (or `policy.locale`). `core-extended` is now a deprecation
   alias that warns at runtime. See "Tier 1.5".
2. **Audit-row schema.** If you persist `gaze-audit` SQLite rows, the
   `recognizer_id` and `recognizer_version_id` columns are now populated.
   Forward-compatible: pre-v0.8 rows stay readable, new rows carry
   `_vN`-suffixed lineage. See "Tier 1".
3. **Custom recognizers** in `[[policy.custom_recognizers]]` may now
   declare an optional `safety_tier`. When omitted, the loader defaults
   to `safe_default` — your existing policy files keep working without
   edits.

Everything else in v0.8.0 is additive (new entities, new locales, new
opt-in SafetyNet backend).

### Tier 1 — Versioned recognizer-IDs (additive)

PR [#203](https://github.com/CertaMesh/gaze/pull/203) (`3c95304`).

- `RedactionEntry` now carries both `recognizer_id` (semantic slug used
  for registry/collision lookup, unchanged shape) and
  `recognizer_version_id` (audit-facing, suffixed with `_vN`).
- `gaze-audit`'s SQLite schema gains nullable `recognizer_id` +
  `recognizer_version_id` columns. The schema migrates forward without
  rewriting existing rows; legacy rows carry a `legacy_unversioned`
  marker.
- The NER recognizer's bare `"ner"` slug is now extended with the loaded
  model id (e.g. `ner.distilbert.v1`).

**Action required:** none. If you query the audit table directly, your
existing SQL keeps working. If you want to consume the new columns, they
are nullable so a simple `SELECT recognizer_id, recognizer_version_id
FROM gaze_audit_log` is forward-safe.

### Tier 1.5 — Bundled rulepack unification (action required for some)

PR [#201](https://github.com/CertaMesh/gaze/pull/201) (`8ab9daf`).

The two embedded rulepacks (`core` with 6 recognizers, `core-extended`
with 10) have been collapsed into **one unified `core` bundle**. Each
recognizer now declares a closed-enum `safety_tier` that machine-encodes
its activation contract:

| Tier            | Activation rule                                                 |
| --------------- | --------------------------------------------------------------- |
| `safe_default`  | Active whenever the bundle is loaded.                           |
| `locale_gated`  | Active only when the resolved locale matches `recognizer.locales`. |
| `opt_in`        | Active only when explicitly named under `[[policy.custom_recognizers]]` or future opt-in surface. |

The pre-v0.8 PR #58 no-policy surprise activation (where
`--rulepack-bundled core-extended` silently turned on
`phone.national.{de,us}` + `postal.{de,us}`) is gone. Those recognizers
are now `locale_gated` and require an explicit `--locale=de-DE` or
`--locale=en-US`.

**Action required**

- **If your CLI scripts pass `--rulepack-bundled core-extended`**, they
  keep working in v0.8.x: the flag aliases to `--rulepack-bundled core`
  and emits a deprecation warning. The alias will be removed in a future
  major (target v0.10.0). Update at your convenience.
- **If your scripts rely on bare 5-digit postal or German/US national
  phone tokenization without passing a locale**, you will see those
  spans pass through untokenized. Add the matching locale flag (or
  `policy.locale` field) to restore behavior. The deprecation warning
  on `core-extended` calls this out at runtime.
- **If your `[[policy.custom_recognizers]]` blocks need explicit tier
  declarations**, set `safety_tier = "safe_default"` (or the tier you
  want) on each entry. When omitted, the loader defaults to
  `safe_default` so existing policies load unchanged.

**No action required**

- Manifest contracts are unchanged. Tokens emitted by v0.7.x deserialize
  + restore on v0.8.x.
- Adopters who already passed `--locale` were unaffected by PR #58
  surprise activation and are unaffected by this change.

### Tier 2 — Checksum-backed locale parity (additive)

In flight at tag time as `v0.8/tier2-validator-locales`. When merged, the
release notes for v0.8.0 will replace this paragraph with the merged PR
number(s) and the entity table below.

| Entity     | Locale | Validator        | `ValidatorKind`         |
| ---------- | ------ | ---------------- | ----------------------- |
| Aadhaar    | IN     | Verhoeff         | `AadhaarVerhoeff`       |
| NIR        | FR     | MOD-97 variant   | `FrNirMod97`            |
| Steuer-ID  | DE     | MOD 11,10        | `DeSteuerIdMod1110`     |
| BSN        | NL     | MOD-11           | `BsnMod11`              |
| CPF        | BR     | MOD-11           | `CpfMod11`              |
| CNPJ       | BR     | MOD-11           | `CnpjMod11`             |
| NHS number | UK     | MOD-11           | `UkNhsMod11`            |

All seven ship with `safety_tier = "safe_default"` (activated whenever
the `core` bundle is loaded). New locale packs ship at `locale-fr`,
`locale-nl`, `locale-br`, `locale-in`, `locale-uk`.

**Action required:** none — every entity is additive, gated by locale
unless your policy enables it globally. Adopters in BR / FR / NL / IN /
UK get out-of-box coverage; everyone else sees no behavior change.

### Tier 2.5 — Kiji DistilBERT SafetyNet backend (opt-in)

PR [#202](https://github.com/CertaMesh/gaze/pull/202) (`0cd9ccc`).

A second Pass-3 SafetyNet observer is available alongside the existing
OpenAI Privacy Filter. Subprocess contract is identical to
`OpenAiFilterSafetyNet` — read clean text on stdin, emit JSON spans on
stdout, never mutate the manifest. New CLI flags:

- `--safety-net-backend {openai-filter|kiji-distilbert}`
- `--kiji-distilbert-command <path>`
- `--kiji-distilbert-model-dir <dir>`

Fetcher: `scripts/fetch/fetch-kiji-safetynet-model.sh`. Pinned-artifact
contract: model dir must carry `SHA256SUMS`, `labels.json`,
`model.onnx`, `tokenizer.json` with `0o700` directory + `0o600` file
permissions on Unix. Missing artifacts fail closed with typed
`CliError::SafetyNetArtifactMissing` (exit `2`) before the subprocess
spawns.

Setup walkthrough: [`docs/how-to/safety-net/set-up-kiji-safetynet.md`](docs/how-to/safety-net/set-up-kiji-safetynet.md).

**Action required:** none. The backend is opt-in. If you do not select
it, your current SafetyNet configuration (OpenAI Privacy Filter or
none) is unchanged.

### Tier 3 — Regex-only locale recognizers (additive)

PR [#208](https://github.com/CertaMesh/gaze/pull/208).

Adds US SSN, UK NINO, and Indian PAN as `safety_tier = "locale_gated"`
recognizers — they fire only when the resolved locale matches. No
validator math; regex shape plus cue context only.

| Entity     | Locale | Cue examples                              | ValidatorKind |
| ---------- | ------ | ----------------------------------------- | ------------- |
| US SSN     | US     | `SSN`, `Social Security Number`, `SS#`    | None          |
| UK NINO    | UK     | `NINO`, `NI Number`, `National Insurance` | None          |
| Indian PAN | IN     | `PAN`, `Permanent Account Number`, `पैन`  | None          |

**Action required:** none — pure additive coverage when the relevant
locale is set.

### Depending on v0.8.0

The workspace is published to crates.io. Pin by version:

```toml
[dependencies]
gaze-pii = "0.8.0"
```

The exact crate name is `gaze-pii` (cargo package); the library imports
as `gaze` (e.g. `use gaze::Pipeline;`).

### Schema-version field on `policy.toml`

Shipped in v0.7.2 (PR #192) but worth re-stating because v0.8.0 is the
first minor where the field is *exercised by new content*:

```toml
schema_version = "0.1"
```

The loader checks the `major.minor` prefix against the supported version
and fails closed with
`{"error":"PolicySchemaUnsupported","exit":2,"found":"...","supported":"0.1"}`.
Existing policies without the field continue to load via a soft default;
add it explicitly to lock yourself onto a known schema.

---

## v0.6.x → v0.7.0

Highlights only — backfill in detail if adopter friction surfaces.

- **New crate `gaze-document`** for OSS document → SafeBundle ingestion
  (PNG/JPG/PDF → Tesseract OCR → redact → `clean.md` + `manifest.json`
  + `report.json`). Opt-in via `gaze-cli`'s `document` feature.
- **MCP runtime split.** `gaze-mcp-core` (transport-free) +
  `gaze-mcp-rmcp` (rmcp transport sink) replace the prior in-tree MCP
  surface. Opt-in via `gaze-cli`'s `mcp` feature.
- **Validator-veto pre-resolver** rejects invalid candidates before
  conflict resolution, logs loser-only audit rows with
  `decided_by: ValidatorVeto`. See
  [`docs/explanation/detection/validator-veto.md`](docs/explanation/detection/validator-veto.md).
- **Collision-family metadata + `FamilyPolicyTable`** for cross-class
  recognizer rivalries (PAN-vs-IBAN, phone family). See
  [`docs/explanation/detection/collision-family.md`](docs/explanation/detection/collision-family.md).
- **Mandatory-anchor resolution** keeps structural candidates on their
  precise variant when a `[locale.cues.<key>]` cue is in scope, else
  emits a family-level fallback token. See
  [`docs/explanation/detection/anchor-resolution.md`](docs/explanation/detection/anchor-resolution.md).
- **`PiiClass::Custom("eth_address")`** for EIP-55 Ethereum addresses;
  new `Ipv4Parse`/`Ipv6Parse`/`EthEip55` validator kinds.
- **`gaze_pii::default_policy` falls back to `Tokenize`** (axis-1
  fail-closed). Adopters who relied on the previous default-allow path
  must declare per-class policy explicitly.

**Action required**

- The `Tokenize` default change may surface previously-allowed classes
  as tokens. Review your `[policy.classes]` block and set explicit
  policies for any class you want to allow through.
- The MCP runtime split changes the import path: replace any
  `gaze::mcp::*` imports with `gaze_mcp_core::*` or `gaze_mcp_rmcp::*`.

---

## v0.5.x → v0.6.0

- `KijiDistilbertSafetyNet`'s predecessor — the OpenAI Privacy Filter
  Pass-3 SafetyNet — landed as an observer-only backend. Manifests are
  not mutated by Pass-3; restore round-trip is unaffected.
- Cue-anchored Name detection (`anchored_match` recognizer kind +
  `forward_markers` / `agent_recipient_cues` / `footer_cues` locale
  buckets). Adopters using `locale-de` or `locale-en` get this for
  free.
- `gaze` no longer carries `rusqlite` in any feature graph. Adopters
  who want SQLite audit logging now depend on `gaze-audit` directly:

  ```rust
  use gaze_audit::SqliteLogger;
  ```

  The one-minor `audit` feature shim on `gaze` (introduced in v0.5
  Phase C) is gone. `gaze::SqliteLogger` no longer compiles.

---

## v0.4.x → v0.5.0

- New crate `gaze-types` for shared value contracts (serde-only, no
  ML/sql deps). Adopters who want the contract surface without
  `ort` / `tokenizers` / `ndarray` should depend on `gaze-types`
  directly.
- The `RedactionLogger` trait moved into `gaze-types`. `gaze`
  re-exports it for source compatibility.
- Audit-sink protected-path enforcement switched from the legacy
  syn-walker to a Dylint resolver-based gate
  (`xtask dylint-gate`).

---

## Reversibility statement (every upgrade)

If an upgrade ever causes a manifest written by an older minor to fail
restore on a newer minor, that is a bug. Open an issue tagged
`reversibility-regression` and we will treat it as a critical defect
against north-star axis 2. There is no migration step that asks you to
re-tokenize stored manifests.

# v0.9.0

## Perf wave

v0.9.0 is a performance and deployment release: in-process Kiji ORT
removes the Python subprocess boundary for adopters who select it, int8 dynamic
quantization adds a separately SHA-pinned smaller/faster model path, `gaze
daemon` keeps multi-session state behind a JSONL stdio process boundary,
pipeline skip-gating/capitals/prefix-cache/length-bucketing optimizations are
available behind explicit opt-in flags, and `tract`/`candle` feature gates give
static-binary deployments alternatives to the default `ort` runtime. Public
benchmark claims are documented in [`docs/reference/benchmarks/README.md`](docs/reference/benchmarks/README.md):
Kiji int8 ORT warm p50 is 1.849ms in the committed model leaderboard snapshot,
and the safety-net matrix records a 0.000 F1 delta versus fp32 Kiji.

Measured on: Apple M5 Max / macOS 26.5 hosts in the committed v0.9 snapshots
and final rc revalidation.

## New CLI flags (opt-in)

- `--kiji-backend {subprocess|ort}` (default `subprocess`): selects Kiji DistilBERT runtime.
- `--kiji-distilbert-precision {fp32|int8}` (default `fp32`): selects precision for ORT path.
- Pipeline-optimization flags wired through CLI: skip-class-gating, capitals-heuristic-gate, prefix-cache, length-bucketing (opt-in default-off).

## New subcommand

- `gaze daemon --policy <path> [--idle-timeout <secs>]` — long-lived JSONL stdio session manager. Protocol: `{session_id, text}` request, `{session_id, clean_text, manifest, tokens}` response. SIGTERM-graceful, multi-session-isolated.

## New opt-in features (Cargo)

- `gaze-recognizers` features: `runtime-tract`, `runtime-candle` — alternative ONNX runtimes for static-binary deployments.

## Reversibility

Manifest restore semantics + signed snapshot wire format unchanged from v0.8.1.

# v0.8.1

v0.8.1 made SafetyNet `resolve` the default mode, added Kiji DistilBERT bundle
SHA verification, and introduced the `LocaleAwareModel` registry groundwork in
`gaze-recognizers`. The public default `--safety-net-mode` flipped from
`strict` to `resolve`; adopters who require strict hard-fail semantics must opt
back in explicitly with `--safety-net-mode=strict`.
# v0.8.0

## gaze-proxy

The new off-by-default `proxy` feature adds `gaze-proxy` and `gaze proxy`
subcommands for multi-provider LLM SDK base-URL swaps. OpenAI, Anthropic, and
Gemini ship as separate provider adapters from day one. The proxy uses native
provider wire shapes and does not transcode between providers.

Daemon UX is available through:

```bash
gaze proxy serve
gaze proxy start
gaze proxy status
gaze proxy logs --follow
gaze proxy stop
gaze proxy restart
```

Pidfiles are stored in platform local-data directories and stale pidfiles are
cleaned after process liveness checks.
