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

PR [#203](https://github.com/EmpireTwo/gaze/pull/203) (`3c95304`).

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

PR [#201](https://github.com/EmpireTwo/gaze/pull/201) (`8ab9daf`).

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

PR [#202](https://github.com/EmpireTwo/gaze/pull/202) (`0cd9ccc`).

A second Pass-3 SafetyNet observer is available alongside the existing
OpenAI Privacy Filter. Subprocess contract is identical to
`OpenAiFilterSafetyNet` — read clean text on stdin, emit JSON spans on
stdout, never mutate the manifest. New CLI flags:

- `--safety-net-backend {openai-filter|kiji-distilbert}`
- `--kiji-distilbert-command <path>`
- `--kiji-distilbert-model-dir <dir>`

Fetcher: `scripts/fetch-kiji-safetynet-model.sh`. Pinned-artifact
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

PR [#208](https://github.com/EmpireTwo/gaze/pull/208).

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
benchmark claims are documented in [`docs/reference/benchmarks/index.md`](docs/reference/benchmarks/index.md):
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
