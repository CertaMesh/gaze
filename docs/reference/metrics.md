# Gaze metrics catalog

Audit columns, conflict labels, SafetyNet fields, counters, document reports,
MCP context and exit codes. Each family links to its source and behavior contract.

## How to read this catalog

| Stability | Reader contract |
|---|---|
| Closed enum | Use the documented current strings for alerts. |
| `#[non_exhaustive]` | Handle future variants and fields; minor releases may add them. |
| Free string | Group/display as opaque text; do not switch on it in alerts. |
| Internal-only | No public guarantee; may change without notice. |

Tables name storage or wire fields.

### Versioning posture

For Rust `#[non_exhaustive]` enums, include a wildcard arm. For structs, use
named patterns with `..`. Do not treat free strings as closed enums.

Update this catalog and the changelog when a field changes.

## Table of contents

1. [Audit-row fields (`gaze-audit`)](#1-audit-row-fields-gaze-audit)
2. [Conflict-resolution tiers (`gaze`)](#2-conflict-resolution-tiers-gaze)
3. [SafetyNet metrics (`gaze-recognizers`)](#3-safetynet-metrics-gaze-recognizers)
4. [Recognizer surface (`gaze-recognizers` + `gaze`)](#4-recognizer-surface-gaze-recognizers--gaze)
5. [Pipeline observability (`gaze` `Pipeline`)](#5-pipeline-observability-gaze-pipeline)
6. [SafeBundle / `BundleReport` (`gaze-document`)](#6-safebundle--bundlereport-gaze-document)
7. [MCP chokepoint observability (`gaze-mcp-core`)](#7-mcp-chokepoint-observability-gaze-mcp-core)
8. [CLI exit codes (`gaze-cli`)](#8-cli-exit-codes-gaze-cli)
9. [Restore telemetry](#restore-telemetry)
10. [See also](#see-also)

## 1. Audit-row fields (`gaze-audit`)

Source contract: [`RedactionEntry`](../../crates/gaze-types/src/lib.rs). Storage:
[`SqliteLogger`](../../crates/gaze-audit/src/sqlite.rs). Query surface:
[`AuditLogRow`](../../crates/gaze-audit/src/query.rs) +
[`AuditFilter`](../../crates/gaze-audit/src/query.rs) +
[`AUDIT_RESTRICTED_COLUMNS`](../../crates/gaze-audit/src/query.rs). Companion deep-dives:
[`docs/explanation/detection/ambiguity-side-channel.md`](../explanation/detection/ambiguity-side-channel.md),
[`docs/explanation/detection/validator-veto.md`](../explanation/detection/validator-veto.md),
[`docs/explanation/detection/collision-family.md`](../explanation/detection/collision-family.md),
[`docs/explanation/safety-net/safety-net-modes.md`](../explanation/safety-net/safety-net-modes.md).

### 1.1 `redaction_log` columns

Every row emitted to a [`RedactionLogger`](../../crates/gaze-types/src/lib.rs)
(canonical trait at `crates/gaze-types/src/lib.rs:2001`) maps 1:1 to a column
in the `redaction_log` SQLite table when persisted via `SqliteLogger`. The
schema is `CREATE TABLE`-on-first-use + idempotent `ALTER TABLE` migrations;
see `crates/gaze-audit/src/sqlite.rs:133-308` for the migration block.

| Column | Type | What | Stability |
|---|---|---|---|
| `source` | `TEXT NOT NULL` | Detector or recognizer source identifier emitting the row. | Free string |
| `recognizer_id` | `TEXT NULL` | Stable semantic recognizer identifier, e.g. `email.global`. Rows written before v0.7 are backfilled as `legacy_unversioned`. | Free string |
| `recognizer_version_id` | `TEXT NULL` | Versioned recognizer artifact/rule identifier for audit lineage. | Free string |
| `class` | `TEXT NOT NULL` | Canonical PII class string: `email`, `name`, `location`, `organization`, or `custom:<name>`. Built-in serialization: `PiiClass::to_canonical_str` (`gaze-types/src/lib.rs:177`). | Closed enum (built-ins) + free string (`custom:*`) |
| `action` | `TEXT NOT NULL` | Policy action applied. Closed serialization at `gaze-types/src/lib.rs:1849`: `tokenize` / `redact` / `format_preserve` / `generalize` / `preserve`. | `#[non_exhaustive]` enum |
| `field_name` | `TEXT NULL` | Structured-field name when the row came from a `RawDocument::Structured` document. | Free string |
| `document_kind` | `TEXT NOT NULL` | Source document kind. Closed: `structured` / `text` (`gaze-types/src/lib.rs:1859`). | `#[non_exhaustive]` enum |
| `conflict_loser` | `INTEGER NOT NULL` | `1` when the row records a candidate that *lost* conflict resolution (paired with a separate winner row); `0` otherwise. | Closed (boolean) |
| `decided_by` | `TEXT NOT NULL DEFAULT 'none'` | Conflict tier that decided the outcome. See §2 for the full enum. | `#[non_exhaustive]` enum |
| `created_at` | `INTEGER NULL` | Epoch-millisecond timestamp. | Numeric (i64 ms) |
| `session_id` | `TEXT NULL` | Audit session identifier. | Free string |
| `snapshot_scheme` | `TEXT NOT NULL DEFAULT 'gaze.snapshot.v1.sha256-salted'` | Snapshot-reference scheme name. Constant [`DEFAULT_SNAPSHOT_SCHEME`](../../crates/gaze-audit/src/query.rs) at `gaze-audit/src/query.rs:3`. | Closed string |
| `snapshot_alg` | `TEXT NOT NULL DEFAULT 'SHA-256'` | Hash algorithm used by the snapshot scheme. Constant `DEFAULT_SNAPSHOT_ALG` at `gaze-audit/src/query.rs:4`. | Closed string |
| `snapshot_key_version` | `INTEGER NULL` | Key-rotation generation for the snapshot scheme. | Numeric (i64) |
| `validator_fail_reason` | `TEXT NULL` | Closed validator-failure reason when `ConflictTier::ValidatorVeto` rejected the candidate. JSON-serialized [`ValidatorFailReason`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:336`. | `#[non_exhaustive]` enum (see §1.2) |
| `ambiguity_record` | `TEXT NULL` | JSON-encoded [`AmbiguityRecord`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:290` for family-level fallbacks. | `#[non_exhaustive]` struct |
| `collision_family` | `TEXT NULL` | Collision-family name that influenced this decision. | Free string (bundled families reserved in [`RESERVED_BUNDLED_FAMILIES`](../../crates/gaze-types/src/lib.rs) in `gaze-types`) |
| `collision_variant` | `TEXT NULL` | Variant name within `collision_family`. | Free string |
| `fallback_triggered` | `TEXT NULL` | Safety-net fallback reason when fallback policy handled the row. JSON-serialized [`FallbackReason`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:1737`. | `#[non_exhaustive]` enum (see §1.2) |

The full list is also exported as the constant
[`AUDIT_RESTRICTED_COLUMNS`](../../crates/gaze-audit/src/query.rs), which `audit export` selects from. New
columns added to the schema must be added to this constant; the Dylint
`gaze_module_isolation` gate ensures the clean path cannot route raw values
into the audit path.

### 1.2 Closed-enum value sets

These string columns serialize closed Rust enums. Adopters can pattern-match
on the *current* variants but must accept that the enum is `#[non_exhaustive]`
in Rust and additive variants can ship in any minor release.

`decided_by` (`ConflictTier`) — see §2 for the full catalog.

`validator_fail_reason` (`ValidatorFailReason`) — serialization (snake_case via serde):

| Value | When |
|---|---|
| `luhn_failed` | Luhn checksum rejected the candidate. |
| `iban_mod97_failed` | IBAN MOD-97 validation failed. |
| `email_rfc_rejected` (alias `email_rfc_failed`) | Basic email-shape validation rejected the candidate. |
| `phone_e164_rejected` (alias `e164_phone_failed`) | E.164 phone validation failed (feature `phone-parser`). |
| `phone_national_region_mismatch` | National phone parser accepted the number but region validation failed. |
| `ipv4_parse_failed` | IPv4 parser rejected the candidate. |
| `ipv6_parse_failed` | IPv6 parser rejected the candidate. |
| `ipv4_documentation_range` | Bundled IPv4 rule excluded an RFC 5737 documentation address. |
| `ipv6_documentation_range` | Bundled IPv6 rule excluded an RFC 3849 or embedded RFC 5737 documentation address. |
| `eth_eip55_checksum_failed` | EIP-55 Ethereum checksum validation failed. |
| `aadhaar_verhoeff_failed` | Aadhaar Verhoeff checksum failed. |
| `fr_nir_mod97_failed` | French NIR MOD-97 key failed. |
| `de_steuer_id_mod1110_failed` | German Steuer-ID MOD 11,10 checksum failed. |
| `bsn_mod11_failed` | Dutch BSN MOD-11 checksum failed. |
| `cpf_mod11_failed` | Brazilian CPF MOD-11 checksum failed. |
| `cnpj_mod11_failed` | Brazilian CNPJ MOD-11 checksum failed. |
| `uk_nhs_mod11_failed` | UK NHS number MOD-11 checksum failed. |
| `ipv4_loopback_range` | Bundled IPv4 rule excluded a loopback address (127.0.0.0/8). |
| `ipv6_loopback_range` | Bundled IPv6 rule excluded `::1` or an IPv4-mapped or IPv4-compatible loopback address. |

`fallback_triggered` (`FallbackReason`) — serialization:

| Value | When |
|---|---|
| `overlap_conflict` | Suspect overlapped an emitted token in a way resolve could not promote. |
| `validator_veto` | A validator rejected the promoted candidate. |
| `anchor_missing` | A mandatory anchor was missing for the promoted candidate. |
| `residual_suspect` | A follow-up SafetyNet pass still observed a suspect. |

`ambiguity_record.reason` (`AmbiguityReason`) — serialization:

| Value | When |
|---|---|
| `no_anchor` | Span matched a multi-recognizer family and no anchor cue resolved it. |
| `validator_indeterminate` | Multiple validator-stage recognizers remained viable. |
| `multi_family_match` | Recognizers across two or more distinct families matched. |
| `precedence_tie` | Multiple variants tied on precedence with no discriminator. |

### 1.3 `safety_net_log` columns (SafetyNet observer rows)

`SqliteLogger` writes a second table for SafetyNet suspect telemetry. Source:
[`LeakSuspectRow`](../../crates/gaze-audit/src/query.rs); CREATE at `gaze-audit/src/sqlite.rs:160-178`;
restricted-columns allowlist
[`SAFETY_NET_RESTRICTED_COLUMNS`](../../crates/gaze-audit/src/query.rs).

| Column | Type | What | Stability |
|---|---|---|---|
| `id` | `INTEGER PRIMARY KEY` | Auto-increment row id. | Numeric |
| `safety_net_id` | `TEXT NOT NULL` | Backend identifier (`opf`, `nym-small-int8`, ...). | Free string (registered by adopter) |
| `raw_label` | `TEXT NOT NULL` | Raw backend label after validation/mapping. Never source text. | Free string (backend-defined) |
| `mapped_class` | `TEXT NOT NULL` | Mapped Gaze `PiiClass` canonical string. | Closed enum + `custom:*` |
| `leak_kind` | `TEXT NOT NULL` | One of `uncovered` / `partial_bleed` / `class_mismatch`. See [`LeakKind`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:1170`. | `#[non_exhaustive]` enum |
| `span_len` | `INTEGER NOT NULL` | Length of the suspect span in bytes. | Numeric (i64) |
| `document_kind` | `TEXT NOT NULL` | `structured` / `text`. | `#[non_exhaustive]` enum |
| `field_path` | `TEXT NULL` | Optional structured-document field path (e.g. `$.user.email`). | Free string |
| `score` | `REAL NULL` | Optional backend confidence in `0.0..=1.0`. | Numeric (f64) |
| `created_at` | `INTEGER NOT NULL` | Epoch-millisecond timestamp. | Numeric (i64 ms) |
| `session_id` | `TEXT NULL` | Audit session identifier. | Free string |
| `pipeline_class` | `TEXT NULL` | Class the deterministic pipeline emitted for the overlapping token (set when `leak_kind = class_mismatch`). | Closed enum + `custom:*` |
| `safety_net_replay_hash` | `TEXT NULL` | Optional replay hash for deterministic backend replays. | Free string |
| `backend_id` | `TEXT NULL` | Backend-supplied identifier; redundant with `safety_net_id` but distinct field. | Free string |
| `backend_version` | `TEXT NULL` | Backend version string (e.g. ONNX model SHA prefix). | Free string |
| `decoding_params_hash` | `TEXT NULL` | Hash of canonical decoding parameters for replay determinism. | Hex string |
| `telemetry_kind` | `TEXT NULL` | Set for non-suspect telemetry rows (`locale_skipped`). See [`LeakReportTelemetry`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:1190`. | `#[non_exhaustive]` enum |

### 1.4 `AuditFilter` query dimensions

The query surface exposed to the CLI is [`AuditFilter`](../../crates/gaze-audit/src/query.rs)
at `crates/gaze-audit/src/query.rs:13`. Every field maps to a `WHERE`
predicate; unset fields select-all. SQL built in
[`build_audit_query_sql`](../../crates/gaze-audit/src/query.rs).

| Filter field | Targets column | Type |
|---|---|---|
| `class` | `class` | String equality |
| `source` | `source` | String equality |
| `action` | `action` | String equality |
| `document_kind` | `document_kind` | String equality |
| `raw_label` | `raw_label` (safety-net only) | String equality |
| `field_path` | `field_path` (safety-net only) | String equality |
| `from_epoch_ms` | `created_at >= ?` | i64 ms |
| `to_epoch_ms` | `created_at <= ?` | i64 ms |
| `session_id` | `session_id` | String equality |
| `snapshot_scheme` | `snapshot_scheme` | String equality |
| `snapshot_alg` | `snapshot_alg` | String equality |
| `snapshot_key_version` | `snapshot_key_version` | i64 equality |
| `has_ambiguity` | `ambiguity_record IS [NOT] NULL` | bool |
| `ambiguity_reason` | JSON match on `ambiguity_record` | String |
| `collision_family` | `collision_family` | String equality |
| `collision_variant` | `collision_variant` | String equality |
| `recognizer_id` | `recognizer_id` | String equality |
| `recognizer_version_id` | `recognizer_version_id` | String equality |

### 1.5 Schema migration history

`SqliteLogger::new` detects columns with `PRAGMA table_info` and adds missing
ones with idempotent `ALTER TABLE` operations. It uses no schema-version PRAGMA.
Opening an old database with a newer writer adds columns; mixed-version callers
must allow that.

## 2. Conflict-resolution tiers (`gaze`)

Source: [`ConflictTier`](../../crates/gaze-types/src/lib.rs). Serialization (audit-string form). Resolver: `crates/gaze/src/resolver.rs`.
Companion deep-dives:
[`docs/explanation/detection/validator-veto.md`](../explanation/detection/validator-veto.md),
[`docs/explanation/detection/collision-family.md`](../explanation/detection/collision-family.md),
[`docs/explanation/detection/anchor-resolution.md`](../explanation/detection/anchor-resolution.md),
[`docs/explanation/safety-net/safety-net-modes.md`](../explanation/safety-net/safety-net-modes.md).

### 2.1 Conflict tier labels

See [Full conflict-resolution order](redaction-classes.md#full-conflict-resolution-order)
for validator veto, family policy, anchors, containment and generic arbitration.
Additional audit labels:

| Label | Meaning |
|---|---|
| `Merged` | Adjacent same-class candidates combined. |
| `ProtectionOverride` | Residual inside preserved bytes claimed by a protected class; no winner/loser row. |
| `Redact` | Safety-net suspect replaced one-way; original bytes lost. |
| `Resolve` | Restorable suspect token in `safety_net` family; resolver does not rerun. |
| `Fallback` | Fallback policy handled a failed primary mode; see `fallback_triggered`. |

### 2.2 `decided_by` audit-string values

Canonical strings produced by `ConflictTier::as_str`. These are what land in
the `decided_by` column.

| String | Variant |
|---|---|
| `none` | `ConflictTier::None` (no conflict resolved this row) |
| `class_priority` | `ConflictTier::ClassPriority` |
| `rule_priority` | `ConflictTier::RulePriority` |
| `score` | `ConflictTier::Score` |
| `span_length` | `ConflictTier::SpanLength` |
| `validator` | `ConflictTier::Validator` |
| `validator_veto` | `ConflictTier::ValidatorVeto` |
| `collision_policy` | `ConflictTier::CollisionPolicy` |
| `anchored_context` | `ConflictTier::AnchoredContext` |
| `structured_containment` | `ConflictTier::StructuredContainment` |
| `containment_precedence` | `ConflictTier::ContainmentPrecedence` |
| `protection_override` | `ConflictTier::ProtectionOverride` |
| `recognizer_id` | `ConflictTier::RecognizerId` |
| `merged` | `ConflictTier::Merged` |
| `redact` | `ConflictTier::Redact` |
| `resolve` | `ConflictTier::Resolve` |
| `fallback` | `ConflictTier::Fallback` |

`ConflictTier` is `#[non_exhaustive]`; future tiers will land here without
a major-version bump.

### 2.3 Per-conflict audit row fields

When a conflict resolves, the *winner* gets one row with
`conflict_loser = 0` and the loser(s) get one row each with
`conflict_loser = 1`. Both rows carry the same `decided_by` value. Side-
channel metadata fields (§1.2) attach to the row that materially carries
the metadata — typically the loser for `validator_fail_reason`, the
fallback row for `fallback_triggered`, and the winner for
`ambiguity_record` (family-level fallback emission).

## 3. SafetyNet metrics (`gaze-recognizers`)

Source: trait [`SafetyNet`](../../crates/gaze-types/src/lib.rs); context
[`SafetyNetContext`](../../crates/gaze-types/src/lib.rs); error [`SafetyNetError`](../../crates/gaze-types/src/lib.rs)
at `gaze-types/src/lib.rs:1623`. Implementations under
`crates/gaze-recognizers/src/safety_net/`. Benchmark harness:
`crates/gaze-recognizers/benches/safety_net_matrix.rs`; pinned snapshot:
`crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json`. Deep-dives:
[`docs/explanation/safety-net/safety-nets.md`](../explanation/safety-net/safety-nets.md),
[`docs/explanation/safety-net/safety-net-modes.md`](../explanation/safety-net/safety-net-modes.md),
[`docs/reference/benchmarks/README.md`](benchmarks/README.md#safety-net-matrix).

### 3.1 Per-suspect metrics (`LeakSuspect`)

Each suspect reported by a SafetyNet backend carries these fields. Source:
[`LeakSuspect`](../../crates/gaze-types/src/lib.rs). The exhaustive surface is also visible
in the `safety_net_log` table (§1.3).

| Field | What | Stability |
|---|---|---|
| `span: Range<usize>` | Byte span in clean text. | Stable |
| `class: PiiClass` | Backend-mapped Gaze class. | Closed enum + `custom:*` |
| `safety_net_id: String` | Backend identifier. | Free string (adopter-registered) |
| `score: Option<f32>` | Optional backend confidence in `0.0..=1.0`. | Numeric |
| `kind: LeakKind` | `Uncovered` / `PartialBleed { uncovered }` / `ClassMismatch { pipeline_class, safety_net_class }`. | `#[non_exhaustive]` enum |
| `raw_label: String` | Backend label post-validation, never source text. | Free string (backend-defined) |
| `field_path: Option<String>` | Structured-document field path. | Free string |

### 3.2 `LeakReportStats` (per-call aggregate)

Source: [`LeakReportStats`](../../crates/gaze-types/src/lib.rs). Emitted under
`leak_report.stats` in `gaze clean` JSON output
(`crates/gaze-cli/src/pipeline/run.rs:685-707`).

| Field | What | Surface |
|---|---|---|
| `suspect_count` | Total suspects in this report. | `leak_report.stats.suspect_count` (CLI JSON) |
| `uncovered_count` | Count of `LeakKind::Uncovered` suspects. | `leak_report.stats.uncovered_count` |
| `partial_bleed_count` | Count of `LeakKind::PartialBleed { .. }` suspects. | `leak_report.stats.partial_bleed_count` |
| `class_mismatch_count` | Count of `LeakKind::ClassMismatch { .. }` suspects. | `leak_report.stats.class_mismatch_count` |
| `locale_skipped_count` | Number of `LocaleSkipped` telemetry events. | `leak_report.stats.locale_skipped_count` |

> Adopter contract. Exit code `0` paired with `leak_report.stats.suspect_count = 0`
> is the "no leaks" contract documented in [`crates/gaze-cli/README.md`](../../crates/gaze-cli/README.md).

### 3.3 Benchmark-matrix snapshot fields

The pinned snapshot at
[`crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json`](../../crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json)
records per-cell metrics produced by
`cargo bench -p gaze-recognizers --features safety-net-openai --bench safety_net_matrix`.
The schema is version 2; cells are keyed by `backend × locale × mode`
(`{openai_privacy_filter} × {Global, EnUs, DeDe} × {direct_detector, observer_residual}`).
See [`docs/reference/benchmarks/README.md`](benchmarks/README.md#matrix-shape).

Top-level (mode-independent):

| Field | What | Stability |
|---|---|---|
| `strict_span_leak_rate` | Per-(backend × locale) nullable fail-closed leak rate. End-to-end metric, not detector P/R. | `null` until pinned; numeric in `0.0..=1.0` when populated |

Per cell (`direct_detector` mode):

| Field | What | Stability |
|---|---|---|
| `precision` | Class-averaged precision. | Nullable f64 |
| `recall` | Class-averaged recall. | Nullable f64 |
| `f1` | Class-averaged F1. | Nullable f64 |
| per-class metrics | P / R / F1 per `PiiClass`. | Nullable f64 |

Per cell (`observer_residual` mode):

In addition to the `direct_detector` fields:

| Field | What | Stability |
|---|---|---|
| `observer_residual_recall` | Recall measured against the rule-floor residual (suspects the deterministic pipeline missed). | Nullable f64 |
| `agreement_with_rule_floor` | Fraction of safety-net spans that overlap a rule-floor emitted token of the same class. | Nullable f64 |
| `expansion_fraction` | Fraction of safety-net spans that extend an overlapping rule-floor span. | Nullable f64 |
| `contradiction_fraction` | Fraction of safety-net spans that contradict a rule-floor span (class mismatch). | Nullable f64 |
| `novel_tp_over_rule_floor` | True-positive safety-net spans with no rule-floor coverage, normalized by rule-floor TP. | Nullable f64 |

Result cells are `null` until pinned local backend commands and model
directories are available. Publishing numeric safety-net claims without
those pins violates the Axis 4 trust contract
([benchmark contract](benchmarks/README.md#safety-net-matrix)).

### 3.4 Mode + fallback observability

Mode and fallback selection drives the `decided_by` + `fallback_triggered`
columns (§1.2, §2). The CLI surface emits a single-line stderr warning per
suspect class when running `--safety-net-mode tolerant` or when the
fallback hop fires; see `crates/gaze-cli/src/pipeline/run.rs:792-820`.
Tolerant mode is gated behind `GAZE_ALLOW_TOLERANT=1` for production
deployments (see [`safety-net-modes.md`](../explanation/safety-net/safety-net-modes.md#why-tolerant-is-not-a-production-mode)).

No public warning counter exists. CLI warnings follow nonzero `stats.*_count` buckets.

### 3.5 Backend integrity pins

The benchmark document declares backend pins (OpenAI Privacy Filter source
commit and checkpoint bundle SHA).
These pins are part of the Axis 4 evidence trail. Pin values are tracked in
[`docs/reference/benchmarks/README.md`](benchmarks/README.md#backend-integrity-pins)
and enforced via the `safety-net-sanity` xtask gate plus the `safety_net_matrix`
bench pin assertions.

## 4. Recognizer surface (`gaze-recognizers` + `gaze`)

Source: trait [`Recognizer`](../../crates/gaze-types/src/lib.rs); registry
[`RecognizerRegistry`](../../crates/gaze/src/registry.rs); candidate
[`Candidate`](../../crates/gaze-types/src/lib.rs); detect context
[`DetectContext`](../../crates/gaze-types/src/lib.rs).

### 4.1 Per-recognizer fields

Every recognizer exposes the same metadata surface via the trait. The
registry indexes by `id` and uses these fields for conflict resolution and
locale gating.

| Method | What | Stability |
|---|---|---|
| `id() -> &str` | Stable recognizer identifier (e.g. `email.global`, `phone.national.de`). | Free string (per-recognizer-stable) |
| `supported_class() -> &PiiClass` | PII class emitted by this recognizer. | Closed enum + `custom:*` |
| `token_family() -> &str` | Token-family label used for output token shape. | Free string |
| `validator_kind() -> Option<ValidatorKind>` | Validator declared by this recognizer; pre-resolver validator-veto runs on it. Default `None`. | Closed enum ([`ValidatorKind`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:399`) |
| `locales() -> &[LocaleTag]` | Locale tags where this recognizer activates. Empty / unset defaults to `[Global]`. | `#[non_exhaustive]` enum ([`LocaleTag`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:2067`) |

Versioned-rule lineage attaches to *candidates*, not recognizers:

| Candidate field | What | Surface |
|---|---|---|
| `Candidate::recognizer_id` | Stable id propagated to the audit row. | `redaction_log.recognizer_id` |
| `Candidate::recognizer_version_id` | Optional versioned artifact id. | `redaction_log.recognizer_version_id` |
| `Candidate::source` | Free-string source label (per-row pseudo-id). | `redaction_log.source` |
| `Candidate::priority` | Rule/recognizer priority used by `ConflictTier::RulePriority`. | Internal (resolver-only); not audited directly |
| `Candidate::score` | Confidence in `0.0..=1.0`. Used by `ConflictTier::Score`. | Internal |
| `Candidate::canonical_form` | Optional canonical form for validators / merge logic. | Internal |
| `Candidate::token_family` | Output token-family label. | Internal |
| `Candidate::merged_sources` | Sources merged into this candidate by `ConflictTier::Merged`. | Internal |
| `Candidate::decided_by` | Last `ConflictTier` to touch this candidate. | `redaction_log.decided_by` (via `RedactionEntry`) |

### 4.2 Collision-family membership

Recognizers participating in a collision family expose membership via
[`CollisionMembership`](../../crates/gaze-types/src/lib.rs).

| Field | What | Surface |
|---|---|---|
| `family: String` | Cross-class family name. | `redaction_log.collision_family` |
| `variant: String` | Variant name within the family. | `redaction_log.collision_variant` |
| `precedence: u32` | Lower values win on overlap. | Internal (resolver-only) |
| `mandatory_anchor: Option<String>` | Anchor cue key required for this variant; missing anchor triggers `ConflictTier::AnchoredContext`. | `ambiguity_record.reason = no_anchor` |

Bundled family names reserved against adopter custom recognizers:
[`RESERVED_BUNDLED_FAMILIES`](../../crates/gaze-types/src/lib.rs).

### 4.3 Rulepack-load and registry-build observability

`gaze-cli` emits structured-log diagnostics when a rulepack loads;
unsupported validators / normalizers fail closed at load with typed errors:
[`ValidatorKindParseError`](../../crates/gaze-types/src/lib.rs), `RulepackError::UnsupportedValidator`,
`RulepackError::UnsupportedNormalizer`. These do not surface as audit-row
columns — they surface as CLI exit code `2` (config-level error; see §8).

Registry counters (recognizers registered, validators registered) are
internal-only as of v0.8; they live on
`RecognizerRegistry` but are not exported on a public counter surface.

### 4.4 Locale-chain resolution

`LocaleChain` is the 4-tier resolution surface (CLI > policy > rulepack
default > system default); see
[`docs/explanation/policy/locale-chain.md`](../explanation/policy/locale-chain.md). The
recognizer-side gate is `Recognizer::locales()` intersected with the active
`LocaleChain` via [`LocaleChain::intersects`](../../crates/gaze-types/src/lib.rs)
at `crates/gaze-types/src/lib.rs:2193`. When a recognizer skips because of
locale gating in a SafetyNet pass, a `LeakReportTelemetry::LocaleSkipped`
event is emitted and surfaces as `safety_net_log.telemetry_kind = locale_skipped`.

## 5. Pipeline observability (`gaze` `Pipeline`)

Source: [`Pipeline`](../../crates/gaze/src/pipeline.rs); result struct
[`SafetyNetResult`](../../crates/gaze/src/pipeline.rs). Companion deep-dives:
[`ARCHITECTURE.md`](../../ARCHITECTURE.md) (top-level pipeline ASCII),
[`docs/explanation/safety-net/safety-nets.md`](../explanation/safety-net/safety-nets.md) §"Trait shape".

### 5.1 Per-pass counters

The pipeline runs in up to three passes. Source: structured
recognizer registry (Passes 1 + 2) and the safety-net loop (Pass 3) inside
`Pipeline::redact_*` and `Pipeline::clean_with_safety_net_*`.

| Pass | What runs | Counter surface | Stability |
|---|---|---|---|
| Pass 1 | Regex + dictionary recognizers from the registry. | Not exposed as a counter; visible by `Candidate::source` / `recognizer_id` on every emitted token. | Internal |
| Pass 2 | NER recognizers (optional feature `ner`); produces `Candidate { class=Name|Location|Organization, score, span }`. | Same as Pass 1; recognizers carry `id()` like `ner.davlan-mbert`. | Internal |
| Pass 3 | SafetyNet observer, post-tokenization. | `SafetyNetResult { nets_run, report }` returned by `Pipeline::clean_with_safety_net*`. `nets_run = N` registered nets; `report` is the aggregated [`LeakReport`](../../crates/gaze-types/src/lib.rs). | `#[non_exhaustive]` struct |

### 5.2 Per-call output

`Pipeline::clean_with_safety_net_detect_context` returns
`(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)`:

| Return field | What | Surface | Stability |
|---|---|---|---|
| `CleanDocument` | Tokenized text or structured doc. | `clean` JSON field in `gaze clean` output. | `#[non_exhaustive]` enum |
| `Vec<EmittedTokenSpan>` | Per-token (clean-span, raw-span, class, origin) tuples used by `Manifest` (§3) and restore. | Inside `Manifest`; manifest JSON in adopter restore paths. | `#[non_exhaustive]` struct |
| `LeakReport` | SafetyNet report; see §3.2. | `leak_report` JSON field. | `#[non_exhaustive]` struct |

#### `EmittedTokenOrigin` — whole selection vs residual fragment (v0.15)

`EmittedTokenSpan` carries `origin: EmittedTokenOrigin`, a `#[non_exhaustive]`
enum with two variants:

| Variant | Wire form | Means |
|---|---|---|
| `Whole` (default) | omitted | The replacement covers a whole selection chosen by conflict resolution. |
| `ResidualFragment` | `"origin":"residual_fragment"` | The replacement covers raw bytes that admitted originals evidenced but no selection kept (see [Residual coverage](redaction-classes.md#residual-coverage)). |

A fragment is a real protected byte range but it is not an entity: it can be
a single space, quote, or letter carved out of the middle of one. Consumers that
index, canonicalize, or count entities must branch on `origin` first.
`EmittedTokenSpan::new` yields `Whole` and keeps its signature;
`EmittedTokenSpan::residual_fragment` is the new constructor.

Serialization compatibility, both directions. `Whole` is the serde default
and is skipped on output, so JSON written for an existing whole span is
byte-identical to what v0.14 produced, and JSON written before the field existed
deserializes as `Whole`
(`emitted_token_origin_tests` in `crates/gaze-types/src/lib.rs` pins both with a
byte-exact snapshot).

Known limit for unmigrated readers. There is no `deny_unknown_fields`, so a
reader built before v0.15 silently ignores `"origin":"residual_fragment"` and
treats the fragment as a whole span. Absence of the key therefore means "whole"
both for genuinely old JSON and for new fragment JSON read by an old parser, and
no version field distinguishes them: `BundleReport.bundle_version` is unchanged
and the `gaze daemon` JSONL protocol carries no version. A consumer that counts
entities must be rebuilt against v0.15, not merely re-pointed. (An *unknown
future* origin value fails closed — it errors rather than being misread.)

### 5.3 Per-token / manifest counts

`Manifest::spans.len()` counts replacements, not distinct values. For whole
selections filter `span.origin.is_whole()`. `BundleReport.pii_tokens_by_class`
provides class counts; generic pipeline callers group `EmittedTokenSpan.class`.

### 5.4 Per-pass timing

Public pipeline results expose no per-pass timings. Recognizer benches measure
backends separately.

### 5.5 Session-id propagation

`SafetyNetContext::session_id` (`gaze-types/src/lib.rs:994`) carries the
audit session identifier into Pass 3; the field appears on
`redaction_log.session_id` and `safety_net_log.session_id` (§1.1, §1.3) and
is filtered via `AuditFilter::session_id`. There is no `Pipeline`-level
session counter; sessions are created and bound externally by `gaze::Session`.

## 6. SafeBundle / `BundleReport` (`gaze-document`)

Source: [`BundleReport`](../../crates/gaze-document/src/bundle/mod.rs); per-page record
[`PageReport`](../../crates/gaze-document/src/bundle/mod.rs); class count
[`ClassCount`](../../crates/gaze-document/src/bundle/mod.rs); ocr-source
[`OcrSource`](../../crates/gaze-document/src/bundle/mod.rs). Deep-dive:
[`docs/explanation/document/document-extension.md`](../explanation/document/document-extension.md).

### 6.1 Schema versioning

| Field | What | Versions |
|---|---|---|
| `bundle_version: u32` | Top-level schema version. | `1` in v0.7.1, `2` in v0.8. v1 bundles continue to parse on read; emission is always v2 in v0.8+. |

Field set is `#[non_exhaustive]` — adopters reading `report.json` must
stay forward-compatible. Adopter-write contract is `bundle_version` first, all other
fields readable on a best-effort basis.

### 6.2 Top-level `BundleReport` fields

| Field | What | Surface | Stability |
|---|---|---|---|
| `bundle_version: u32` | Schema version. | `report.json` `bundle_version` | Numeric |
| `input_kind: String` | Detected input kind. | `report.json` `input_kind` | Free string |
| `ocr_mean_confidence: Option<f32>` | Mean Tesseract word confidence (0..100). | `report.json` `ocr_mean_confidence` | Numeric |
| `ocr_word_count: usize` | Number of OCR words with non-negative confidence. | `report.json` `ocr_word_count` | Numeric |
| `ocr_lang: String` | Tesseract language code. | `report.json` `ocr_lang` | Free string |
| `clean_char_count: usize` | Character count of tokenized markdown. | `report.json` `clean_char_count` | Numeric |
| `pii_token_count: u32` | Total PII tokens across all classes. | `report.json` `pii_token_count` | Numeric |
| `pii_tokens_by_class: Vec<ClassCount>` | Per-class token counts. `ClassCount { class: String, count: u32 }`. | `report.json` `pii_tokens_by_class[]` | Free string + numeric |
| `pdf_page_count: Option<i32>` | PDF page count (`None` for image inputs). | `report.json` `pdf_page_count` | Numeric |
| `pdf_page_index: Option<i32>` | PDF page index rasterized (`None` for image inputs). | `report.json` `pdf_page_index` | Numeric |
| `pages: Vec<PageReport>` | Per-page extraction + confidence + layout provenance. | `report.json` `pages[]` | `#[non_exhaustive]` struct |
| `low_confidence_threshold: f32` | Threshold used to set `PageReport.low_confidence`. | `report.json` `low_confidence_threshold` | Numeric |

### 6.3 Per-page `PageReport` fields

| Field | What | Stability |
|---|---|---|
| `page_index: i32` | Zero-based page index. | Numeric |
| `ocr_source: OcrSource` | `vector_pdf` / `ocr` (closed enum at `crates/gaze-document/src/bundle/mod.rs:127`). | Closed enum (rename-stable) |
| `ocr_backend: Option<String>` | OCR backend name when `ocr_source = ocr`. | Free string |
| `confidence: Option<f32>` | Aggregated page confidence in `0.0..=1.0`. `None` for vector-PDF text. | Numeric |
| `low_confidence: bool` | True when `confidence < low_confidence_threshold`. | Boolean |
| `column_count: u32` | Detected text column count (`1` = single-column). | Numeric |
| `ocr_word_count: usize` | OCR words with confidence for this page. | Numeric |
| `ocr_mean_confidence: Option<f32>` | Legacy percent-scale mean confidence. | Numeric |

> Adopter note. The `pages[]` array shipped in v0.8 alongside the
> `bundle_version` bump. Downstream tooling reading v1 bundles will not see
> a `pages` field; downstream tooling reading v2 bundles must handle it.

### 6.4 `DocumentExtension` (signed snapshot envelope)

Owner-only signed bundle integrity envelope. Source:
[`DocumentExtension`](../../crates/gaze-types/src/lib.rs). Surfaces inside the session snapshot —
never inside agent-facing artifacts.

| Field | What | Stability |
|---|---|---|
| `schema_version: u16` | Bundle-level schema shared by clean/layout/preview/report/manifest. | Numeric |
| `clean_md_sha256: [u8; 32]` | SHA-256 of `clean.md` NFC bytes. | Bytes |
| `layout_json_sha256: [u8; 32]` | SHA-256 of canonical `layout.json` bytes. | Bytes |
| `report_json_sha256: [u8; 32]` | SHA-256 of canonical `report.json` bytes. | Bytes |
| `preview_png_sha256: Option<[u8; 32]>` | SHA-256 of `preview-redacted.png` when present. | Bytes |
| `page_count: u32` | Page count for the source document. | Numeric |
| `audit_session_id: String` | Audit session id mirrored from the writing session. | Free string |
| `clean_spans: Vec<EmittedTokenSpan>` | Signed clean.md byte spans for every emitted token. | `#[non_exhaustive]` struct |
| `codec_audit: Vec<CodecAuditRow>` | Per-decode codec audit rows (codec id, version, MIME, capabilities, text origin). | `#[non_exhaustive]` struct |

### 6.5 `CodecAuditRow`

Per-decode metadata-only row. Source:
[`CodecAuditRow`](../../crates/gaze-types/src/lib.rs). Adopters embedding alternate OCR /
codec backends populate one row per decode.

Notable fields: `codec_id` (e.g. `gaze.codec.tesseract`), `codec_version`,
`accepted_mime`, `advertised` / `delivered` capability bitsets, `text_origin`
(closed enum: `ocr` / `embedded_text` / `transcript` / `hybrid`), `codec_output_schema_version`, `options_hash_hex`,
`engine_provenance`, `extraction_density_policy`. Stable since v0.7.

## 7. MCP chokepoint observability (`gaze-mcp-core`)

Source: [`ToolCtx`](../../crates/gaze-mcp-core/src/ctx.rs); `ManifestStore`; `AuthHook`. Deep-dive:
[`docs/explanation/mcp/mcp-runtime.md`](../explanation/mcp/mcp-runtime.md).

### 7.1 `ToolCtx` fields

Sealed tool-invocation context — every tool body receives exactly one
`ToolCtx<'a>` for the dispatch frame. Constructor is `pub(crate)`; the
fields enumerate the audit-correlation surface.

| Field / accessor | What | Surface | Stability |
|---|---|---|---|
| `call_id: Ulid` | Stable ULID per dispatch. Reused as `CallHandle`. | `redaction_log.session_id` per row when persisted via the audit logger bridge in `gaze-cli`; also flows into rmcp protocol fields. | Closed (ULID) |
| `tool_name: &'a str` | Name of the tool being dispatched. | Borrowed in `BeginCallContext.tool_name`; adopter-owned column in manifest store. | Free string |
| `principal_id: &'a str` | Principal stable id (post-`AuthHook`). | `BeginCallContext.principal_id`. | Free string |
| `session().audit_session_id() -> &'a str` | Audit-correlation session id. | `redaction_log.session_id`; `safety_net_log.session_id`. | Free string |
| `redacted_args: &serde_json::Value` | Post-redaction JSON args; safe to inspect / re-emit. | `BeginCallContext.redacted_args`. | Adopter-owned JSON |
| `resources().pipeline()` / `.session()` / `.manifest()` / `.locale_chain()` | Borrowed backend handles for tool bodies that need Gaze internals. | Internal (sealed) | Stable since v0.7 |

### 7.2 `ManifestStore` lifecycle counters

`ManifestStore` is an adopter-implemented trait
(`gaze-mcp-core/src/manifest.rs:175`). The dispatcher invokes the three
methods in fixed order:

| Method | What | When | Failure mode | Stability |
|---|---|---|---|---|
| `begin_call(BeginCallContext<'_>) -> Result<CallHandle, ManifestError>` | Opens a manifest entry. Must be idempotent on `call_id` collisions. | After auth + input redaction, before tool body runs. | `ManifestError::DuplicateCallId` on collision; `Backend` for adopter errors; `Validation` for malformed payloads. | `#[non_exhaustive]` trait |
| `finish_call(CallHandle, SnapshotRef) -> Result<(), ManifestError>` | Finalizes the entry on success. | After tool body returns and response is redacted; before chokepoint returns to transport. | `UnknownHandle` if `begin_call` was not run. | `#[non_exhaustive]` trait |
| `fail_call(CallHandle, FailureReason) -> Result<(), ManifestError>` | Finalizes the entry on failure. | When auth / tool body / response redaction returned an error. | Same as `finish_call`. | `#[non_exhaustive]` trait |

`FailureReason` variants (closed via `#[non_exhaustive]`; serialization via
serde at `gaze-mcp-core/src/manifest.rs:73`):

| Variant | What |
|---|---|
| `ToolError { class, message }` | Tool implementation returned a typed error. |
| `AuthDenied { reason }` | `AuthHook` rejected the call. |
| `RedactionFailed { message }` | Response redaction itself errored (defense in depth). |
| `Other { message }` | Catch-all for adopter-supplied failure modes. |

`SnapshotRef` (`gaze-mcp-core/src/manifest.rs:109`) carries
`{ locator, sha256_hex, byte_len }` — the response bytes themselves never
inline into the manifest row.

### 7.3 `AuthHook` decision audit

[`AuthHook`](../../crates/gaze-mcp-core/src/auth.rs) gates every tool dispatch.

| Method | What | Returns | Audit linkage |
|---|---|---|---|
| `authorize_agent(...)` | Agent-tier authorization. | `Result<Principal, AuthError>` | Failure → `ManifestStore::fail_call(_, FailureReason::AuthDenied { reason })` |
| `authorize_operator(...)` | Operator-tier authorization. | `Result<Principal, AuthError>` | Same as above. |

`Principal` (`gaze-mcp-core/src/auth.rs:24`) carries
`{ id: String, roles: Vec<String> }` — `id` becomes `ToolCtx.principal_id`.

### 7.4 Session-id format policy

[`SessionIdPolicy`](../../crates/gaze-mcp-core/src/session_id.rs) validates transport-supplied
session ids. Closed enum
[`SessionIdFormat`](../../crates/gaze-mcp-core/src/session_id.rs) declares accepted formats with
per-format `effective_entropy_bits()`. Validation failures fail closed at
the transport boundary before `ManifestStore::begin_call` runs.

## 8. CLI exit codes (`gaze-cli`)

Source: [`CliError`](../../crates/gaze-cli/src/error.rs); `exit_code()`. Stderr emission is one JSON line per
error: `{"error":"<Variant>","exit":<N>, ...}`.

| Exit code | When | `error` field | Stability |
|---|---|---|---|
| `0` | Success. With SafetyNet active: `leak_report.stats.suspect_count = 0` is the "no leaks" contract. | n/a | Stable |
| `1` | Stdin parse / empty input / input-too-large / invalid-encoding. | `StdinParse` / `EmptyInput` / `InputTooLarge` / `InvalidEncoding` | Closed |
| `2` | Config-level error: policy malformed, unsupported policy schema, ISO-8601 parse failure on `audit purge`, pinned SafetyNet artifact missing. | `PolicyConfig` / `PolicySchemaUnsupported` / `AuditPurgeIso8601` / `SafetyNetArtifactMissing` | Closed |
| `3` | Runtime fail-closed: SafetyNet config rejected, SafetyNet runtime suspect (strict mode), pipeline error, unknown token at restore, invalid signature, invalid blob version, blob expired. The dedicated "audit-logger fatal" path also exits 3 via `std::process::exit(3)` (`crates/gaze-cli/src/logger.rs:12`). | `SafetyNetConfig` / `SafetyNet` / `Pipeline` / `UnknownToken` / `InvalidSignature` / `InvalidBlobVersion` / `BlobExpired` | Closed |
| `4` | I/O or policy-file-open error. | `Io` / `PolicyOpen` | Closed |
| `5` | Document subcommand error (feature `document` only). | `Document` | Closed |
| `6` | MCP subcommand error (feature `mcp` only). | `Mcp` | Closed |
| `7` | Proxy subcommand error (feature `proxy` only). | `Proxy` | Closed |

> Adopter contract. Exit code is the primary CI / agent signal; the
> stderr JSON line is the structured detail. `CliError` is private to
> `gaze-cli` but the exit-code → behavior contract above is part of the
> adopter-facing surface and is documented in
> [`crates/gaze-cli/README.md`](../../crates/gaze-cli/README.md).

## Restore telemetry

Source: [`RestoreTelemetry`](../../crates/gaze-types/src/lib.rs) and the shared
[`Session` restore assessment](../../crates/gaze/src/session.rs). Available through
pipeline restore telemetry and CLI `restore --telemetry`; audit persistence is
selected with `--audit-db`. The counter split and `trap_shape_count` are unreleased.

| Field | Meaning | Audit column |
|---|---|---|
| `unknown_token_count` | Canonical placeholders absent from the active map, plus incomplete prefixed wrappers, outside authorized output ranges. This alone drives Strict/Lenient decisions. | `restore_unknown_token_count` |
| `manifest_bypass_count` | Broad bare identifier matches outside authorized output ranges. Audit-only lexical suspicion, not proof of PII bypass. | `restore_manifest_bypass_count` |
| `trap_shape_count` | All unprefixed trap matches in restored text, including authorized output. Additive JSON field with a serde default of zero. | `restore_trap_shape_count`, nullable for old rows |
| `fresh_pii_detected_count` | Fresh-PII scan findings. Zero in the token-shape assessment, which does not execute that detector. | `restore_fresh_pii_count` |

`restore_policy` retains `strict` and `lenient`. With no unknown canonical placeholders or incomplete prefixed wrappers,
`restore_decision` is `success`, even when trap/bypass counts are positive.
Otherwise Strict reports `failed` and Lenient reports `partial`. These spellings
are unchanged, including the exact observer-facing string `success`. Audit uses
`restore_policy` and `restore_decision` with the same values.

`phase_execution_mask` (audit: `restore_phase_mask`) records manifest lookup
(bit 0), unknown-token scan (bit 1), and trap/manifest-bypass scan (bit 2).
The shared assessment sets these three bits, mask `7`. It does not set the
fresh-PII scan bit (bit 3). Structural restore events remain a separate opt-in
path; the lexical trap scan does not imply that structural detection ran.

Authorized ranges are output byte ranges produced by manifest substitutions.
Only fully contained matches are exempt from unknown/bypass counts. The ranges
and matched strings are owner-side data and are never audit telemetry.

Previously `unknown_token_count` and `manifest_bypass_count` were aliases of one
post-restore count. Historical rows retain that old meaning and remain unchanged.
Some historical `failed` decisions become `success` on new runs because bare or
authorized token-like literals no longer block. This is a restore-semantics change,
not evidence of better detection. Byte-exact restoration and detection metrics
remain independent. New telemetry readers accept missing `trap_shape_count`;
strict older JSON readers that reject unknown fields may require an update.
Audit queries project missing columns as NULL; writers add the nullable column
without backfilling historical rows. Rust callers constructing `AuditLogRow`
literals must supply the new optional field. Snapshot payload versions are unchanged.

## See also

- [`docs/explanation/detection/ambiguity-side-channel.md`](../explanation/detection/ambiguity-side-channel.md) — `ambiguity_record`, `validator_fail_reason`, `collision_*` schema.
- [`docs/explanation/detection/validator-veto.md`](../explanation/detection/validator-veto.md) — `ConflictTier::ValidatorVeto` semantics.
- [`docs/explanation/detection/collision-family.md`](../explanation/detection/collision-family.md) — `CollisionMembership`, `ConflictTier::CollisionPolicy`.
- [`docs/explanation/detection/anchor-resolution.md`](../explanation/detection/anchor-resolution.md) — `mandatory_anchor`, `ConflictTier::AnchoredContext`.
- [`docs/explanation/safety-net/safety-nets.md`](../explanation/safety-net/safety-nets.md) — Pass-3 observer contract, trait shape, manifest invariants.
- [`docs/explanation/safety-net/safety-net-modes.md`](../explanation/safety-net/safety-net-modes.md) — `resolve` / `redact` / `fallback` modes and `decided_by` extensions.
- [`docs/reference/benchmarks/README.md`](benchmarks/README.md#safety-net-matrix) — `safety_net_matrix` snapshot pins + matrix shape.
- [`docs/explanation/mcp/mcp-runtime.md`](../explanation/mcp/mcp-runtime.md) — `ToolCtx` seal, dispatch ordering, manifest persistence.
- [`docs/explanation/document/document-extension.md`](../explanation/document/document-extension.md) — signed snapshot envelope.
- [`docs/explanation/policy/locale-chain.md`](../explanation/policy/locale-chain.md) — 4-tier locale resolution.
- [`docs/explanation/detection/feedback-loop.md`](../explanation/detection/feedback-loop.md) — synthetic, deterministic regression harness for recognizer coverage.
