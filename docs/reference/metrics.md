# Gaze metrics catalog

> **SSOT (Single Source of Truth)** for every observable surface Gaze exposes
> to adopters: audit-log columns, conflict tiers, SafetyNet benchmark snapshot
> fields, recognizer registry surfaces, pipeline counters, SafeBundle JSON
> fields, MCP chokepoint context, and CLI exit codes.
>
> **North-star fit:** axis 4 (trust-by-evidence — every emitted token must
> trace to a typed metric) and axis 5 (adopter ergonomics — SRE / compliance
> teams need one place to wire queries, dashboards, and alerts). See
> [`AGENTS.md`](../../AGENTS.md) for the five axes and [`ARCHITECTURE.md`](../../ARCHITECTURE.md)
> for the crate map.
>
> **Status guarantees.** Every metric in this catalog declares one of:
> - **Closed enum** — string set is closed and exhaustively listed in code;
>   safe to switch on in alert rules.
> - **`#[non_exhaustive]` enum** — adopters must match with a wildcard for
>   forward compatibility; the *current* variant set is listed but additive
>   changes can land in any minor release.
> - **Free string** — no API guarantee on the value space; do not pattern-match
>   in alert rules; safe for grouping and display only.
> - **Internal-only** — not exported on a public surface; subject to change
>   without notice.
>
> **Where metrics surface.** Each row points to the on-disk / on-wire surface
> the adopter actually reads: a column in the `redaction_log` SQLite table,
> a JSON field in `gaze clean` stdout, a JSON field in `report.json`, etc.
> Internal-only metrics are flagged as such — they exist in source for
> traceability but are not part of the public contract.

This document is intentionally a *catalog* — it lists, points to source, and
declares stability. The *behavior* of each metric is documented in the
architecture deep-dives linked per family. If you find a metric in code that
is not in this catalog, file a follow-up todo against the metrics-SSOT track.

## Table of contents

1. [Audit-row fields (`gaze-audit`)](#1-audit-row-fields-gaze-audit)
2. [Conflict-resolution tiers (`gaze`)](#2-conflict-resolution-tiers-gaze)
3. [SafetyNet metrics (`gaze-recognizers`)](#3-safetynet-metrics-gaze-recognizers)
4. [Recognizer surface (`gaze-recognizers` + `gaze`)](#4-recognizer-surface-gaze-recognizers--gaze)
5. [Pipeline observability (`gaze` `Pipeline`)](#5-pipeline-observability-gaze-pipeline)
6. [SafeBundle / `BundleReport` (`gaze-document`)](#6-safebundle--bundlereport-gaze-document)
7. [MCP chokepoint observability (`gaze-mcp-core`)](#7-mcp-chokepoint-observability-gaze-mcp-core)
8. [CLI exit codes (`gaze-cli`)](#8-cli-exit-codes-gaze-cli)

## 1. Audit-row fields (`gaze-audit`)

Source contract: [`RedactionEntry`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:1769`. Storage:
[`SqliteLogger`](../../crates/gaze-audit/src/sqlite.rs) at
`crates/gaze-audit/src/sqlite.rs:76`. Query surface:
[`AuditLogRow`](../../crates/gaze-audit/src/query.rs) +
[`AuditFilter`](../../crates/gaze-audit/src/query.rs) +
[`AUDIT_RESTRICTED_COLUMNS`](../../crates/gaze-audit/src/query.rs) at
`crates/gaze-audit/src/query.rs:13`/`40`/`90`. Companion deep-dives:
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

| Column | Type | What | Where | Stability | Landed |
|---|---|---|---|---|---|
| `source` | `TEXT NOT NULL` | Detector or recognizer source identifier emitting the row. | `RedactionEntry::source` (`gaze-types/src/lib.rs:1771`) | Free string | v0.4 |
| `recognizer_id` | `TEXT NULL` | Stable semantic recognizer identifier, e.g. `email.global`. Rows written before v0.7 are backfilled as `legacy_unversioned`. | `RedactionEntry::recognizer_id` (`gaze-types/src/lib.rs:1773`); migration at `gaze-audit/src/sqlite.rs:284-294` | Free string | v0.7 |
| `recognizer_version_id` | `TEXT NULL` | Versioned recognizer artifact/rule identifier for audit lineage. | `RedactionEntry::recognizer_version_id` (`gaze-types/src/lib.rs:1775`); migration at `gaze-audit/src/sqlite.rs:296-305` | Free string | v0.7 |
| `class` | `TEXT NOT NULL` | Canonical PII class string: `email`, `name`, `location`, `organization`, or `custom:<name>`. Built-in serialization: `PiiClass::to_canonical_str` (`gaze-types/src/lib.rs:177`). | `RedactionEntry::class` | Closed enum (built-ins) + free string (`custom:*`) | v0.4 |
| `action` | `TEXT NOT NULL` | Policy action applied. Closed serialization at `gaze-types/src/lib.rs:1849`: `tokenize` / `redact` / `format_preserve` / `generalize` / `preserve`. | `RedactionEntry::action`; [`Action`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:1687` | `#[non_exhaustive]` enum | v0.4 |
| `field_name` | `TEXT NULL` | Structured-field name when the row came from a `RawDocument::Structured` document. | `RedactionEntry::field_name` | Free string | v0.4 |
| `document_kind` | `TEXT NOT NULL` | Source document kind. Closed: `structured` / `text` (`gaze-types/src/lib.rs:1859`). | `RedactionEntry::document_kind`; [`DocumentKind`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:1751` | `#[non_exhaustive]` enum | v0.4 |
| `conflict_loser` | `INTEGER NOT NULL` | `1` when the row records a candidate that *lost* conflict resolution (paired with a separate winner row); `0` otherwise. | `RedactionEntry::conflict_loser` | Closed (boolean) | v0.4 |
| `decided_by` | `TEXT NOT NULL DEFAULT 'none'` | Conflict tier that decided the outcome. See §2 for the full enum. | `RedactionEntry::decided_by`; [`ConflictTier`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:1703` | `#[non_exhaustive]` enum | v0.4 |
| `created_at` | `INTEGER NULL` | Epoch-millisecond timestamp. | `RedactionEntry::created_at` | Numeric (i64 ms) | v0.4 |
| `session_id` | `TEXT NULL` | Audit session identifier. | `RedactionEntry::session_id` | Free string | v0.4 |
| `snapshot_scheme` | `TEXT NOT NULL DEFAULT 'gaze.snapshot.v1.sha256-salted'` | Snapshot-reference scheme name. Constant [`DEFAULT_SNAPSHOT_SCHEME`](../../crates/gaze-audit/src/query.rs) at `gaze-audit/src/query.rs:3`. | Per-row column (no `RedactionEntry` field — set by the snapshot layer) | Closed string | v0.5 |
| `snapshot_alg` | `TEXT NOT NULL DEFAULT 'SHA-256'` | Hash algorithm used by the snapshot scheme. Constant `DEFAULT_SNAPSHOT_ALG` at `gaze-audit/src/query.rs:4`. | Per-row column | Closed string | v0.5 |
| `snapshot_key_version` | `INTEGER NULL` | Key-rotation generation for the snapshot scheme. | Per-row column | Numeric (i64) | v0.5 |
| `validator_fail_reason` | `TEXT NULL` | Closed validator-failure reason when `ConflictTier::ValidatorVeto` rejected the candidate. JSON-serialized [`ValidatorFailReason`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:336`. | `RedactionEntry::validator_fail_reason` | `#[non_exhaustive]` enum (see §1.2) | v0.7 |
| `ambiguity_record` | `TEXT NULL` | JSON-encoded [`AmbiguityRecord`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:290` for family-level fallbacks. | `RedactionEntry::ambiguity_record` | `#[non_exhaustive]` struct | v0.7 |
| `collision_family` | `TEXT NULL` | Collision-family name that influenced this decision. | `RedactionEntry::collision_family` | Free string (bundled families reserved in [`RESERVED_BUNDLED_FAMILIES`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:66`) | v0.7 |
| `collision_variant` | `TEXT NULL` | Variant name within `collision_family`. | `RedactionEntry::collision_variant` | Free string | v0.7 |
| `fallback_triggered` | `TEXT NULL` | Safety-net fallback reason when fallback policy handled the row. JSON-serialized [`FallbackReason`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:1737`. | `RedactionEntry::fallback_triggered` | `#[non_exhaustive]` enum (see §1.2) | v0.8 |

The full list is also exported as the constant
[`AUDIT_RESTRICTED_COLUMNS`](../../crates/gaze-audit/src/query.rs) at
`crates/gaze-audit/src/query.rs:90`, which `audit export` selects from. New
columns added to the schema must be added to this constant; the Dylint
`gaze_module_isolation` gate ensures the clean path cannot route raw values
into the audit path.

### 1.2 Closed-enum value sets

These string columns serialize closed Rust enums. Adopters can pattern-match
on the *current* variants but must accept that the enum is `#[non_exhaustive]`
in Rust and additive variants can ship in any minor release.

**`decided_by` (`ConflictTier`)** — see §2 for the full catalog.

**`validator_fail_reason` (`ValidatorFailReason`)** — serialization at
`gaze-types/src/lib.rs:333-369` (snake_case via serde):

| Value | When | Landed |
|---|---|---|
| `luhn_failed` | Luhn checksum rejected the candidate. | v0.7 |
| `iban_mod97_failed` | IBAN MOD-97 validation failed. | v0.7 |
| `email_rfc_rejected` (alias `email_rfc_failed`) | Basic email-shape validation rejected the candidate. | v0.7 |
| `phone_e164_rejected` (alias `e164_phone_failed`) | E.164 phone validation failed (feature `phone-parser`). | v0.7 |
| `phone_national_region_mismatch` | National phone parser accepted the number but region validation failed. | v0.7 |
| `ipv4_parse_failed` | IPv4 parser rejected the candidate. | v0.7 |
| `ipv6_parse_failed` | IPv6 parser rejected the candidate. | v0.7 |
| `eth_eip55_checksum_failed` | EIP-55 Ethereum checksum validation failed. | v0.7 |
| `aadhaar_verhoeff_failed` | Aadhaar Verhoeff checksum failed. | v0.7 |
| `fr_nir_mod97_failed` | French NIR MOD-97 key failed. | v0.7 |
| `de_steuer_id_mod1110_failed` | German Steuer-ID MOD 11,10 checksum failed. | v0.7 |
| `bsn_mod11_failed` | Dutch BSN MOD-11 checksum failed. | v0.7 |
| `cpf_mod11_failed` | Brazilian CPF MOD-11 checksum failed. | v0.7 |
| `cnpj_mod11_failed` | Brazilian CNPJ MOD-11 checksum failed. | v0.7 |
| `uk_nhs_mod11_failed` | UK NHS number MOD-11 checksum failed. | v0.7 |

**`fallback_triggered` (`FallbackReason`)** — serialization at
`gaze-types/src/lib.rs:1735-1746`:

| Value | When | Landed |
|---|---|---|
| `overlap_conflict` | Suspect overlapped an emitted token in a way resolve could not promote. | v0.8 |
| `validator_veto` | A validator rejected the promoted candidate. | v0.8 |
| `anchor_missing` | A mandatory anchor was missing for the promoted candidate. | v0.8 |
| `residual_suspect` | A follow-up SafetyNet pass still observed a suspect. | v0.8 |

**`ambiguity_record.reason` (`AmbiguityReason`)** — serialization at
`gaze-types/src/lib.rs:317-330`:

| Value | When | Landed |
|---|---|---|
| `no_anchor` | Span matched a multi-recognizer family and no anchor cue resolved it. | v0.7 |
| `validator_indeterminate` | Multiple validator-stage recognizers remained viable. | v0.7 |
| `multi_family_match` | Recognizers across two or more distinct families matched. | v0.7 |
| `precedence_tie` | Multiple variants tied on precedence with no discriminator. | v0.7 |

### 1.3 `safety_net_log` columns (SafetyNet observer rows)

`SqliteLogger` writes a second table for SafetyNet suspect telemetry. Source:
[`LeakSuspectRow`](../../crates/gaze-audit/src/query.rs) at
`gaze-audit/src/query.rs:63`; CREATE at `gaze-audit/src/sqlite.rs:160-178`;
restricted-columns allowlist
[`SAFETY_NET_RESTRICTED_COLUMNS`](../../crates/gaze-audit/src/query.rs) at
`gaze-audit/src/query.rs:112`.

| Column | Type | What | Where | Stability | Landed |
|---|---|---|---|---|---|
| `id` | `INTEGER PRIMARY KEY` | Auto-increment row id. | `LeakSuspectRow::id` | Numeric | v0.6 |
| `safety_net_id` | `TEXT NOT NULL` | Backend identifier (`opf`, `kiji-distilbert`, ...). | `LeakSuspectRow::safety_net_id` | Free string (registered by adopter) | v0.6 |
| `raw_label` | `TEXT NOT NULL` | Raw backend label after validation/mapping. Never source text. | `LeakSuspectRow::raw_label` | Free string (backend-defined) | v0.6 |
| `mapped_class` | `TEXT NOT NULL` | Mapped Gaze `PiiClass` canonical string. | `LeakSuspectRow::mapped_class` | Closed enum + `custom:*` | v0.6 |
| `leak_kind` | `TEXT NOT NULL` | One of `uncovered` / `partial_bleed` / `class_mismatch`. See [`LeakKind`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:1170`. | `LeakSuspectRow::leak_kind` | `#[non_exhaustive]` enum | v0.6 |
| `span_len` | `INTEGER NOT NULL` | Length of the suspect span in bytes. | `LeakSuspectRow::span_len` | Numeric (i64) | v0.6 |
| `document_kind` | `TEXT NOT NULL` | `structured` / `text`. | `LeakSuspectRow::document_kind` | `#[non_exhaustive]` enum | v0.6 |
| `field_path` | `TEXT NULL` | Optional structured-document field path (e.g. `$.user.email`). | `LeakSuspectRow::field_path` | Free string | v0.6 |
| `score` | `REAL NULL` | Optional backend confidence in `0.0..=1.0`. | `LeakSuspectRow::score` | Numeric (f64) | v0.6 |
| `created_at` | `INTEGER NOT NULL` | Epoch-millisecond timestamp. | `LeakSuspectRow::created_at` | Numeric (i64 ms) | v0.6 |
| `session_id` | `TEXT NULL` | Audit session identifier. | `LeakSuspectRow::session_id` | Free string | v0.6 |
| `pipeline_class` | `TEXT NULL` | Class the deterministic pipeline emitted for the overlapping token (set when `leak_kind = class_mismatch`). | `LeakSuspectRow::pipeline_class` | Closed enum + `custom:*` | v0.6 |
| `safety_net_replay_hash` | `TEXT NULL` | Optional replay hash for deterministic backend replays. | `LeakSuspectRow::safety_net_replay_hash` | Free string | v0.6 |
| `backend_id` | `TEXT NULL` | Backend-supplied identifier; redundant with `safety_net_id` but distinct field. | `LeakSuspectRow::backend_id` | Free string | v0.6 |
| `backend_version` | `TEXT NULL` | Backend version string (e.g. ONNX model SHA prefix). | `LeakSuspectRow::backend_version` | Free string | v0.6 |
| `decoding_params_hash` | `TEXT NULL` | Hash of canonical decoding parameters for replay determinism. | `LeakSuspectRow::decoding_params_hash` | Hex string | v0.6 |
| `telemetry_kind` | `TEXT NULL` | Set for non-suspect telemetry rows (`locale_skipped`). See [`LeakReportTelemetry`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:1190`. | `LeakSuspectRow::telemetry_kind` | `#[non_exhaustive]` enum | v0.6 |

### 1.4 `AuditFilter` query dimensions

The query surface exposed to the CLI is [`AuditFilter`](../../crates/gaze-audit/src/query.rs)
at `crates/gaze-audit/src/query.rs:13`. Every field maps to a `WHERE`
predicate; unset fields select-all. SQL built in
[`build_audit_query_sql`](../../crates/gaze-audit/src/query.rs) at
`crates/gaze-audit/src/query.rs:136`.

| Filter field | Targets column | Type | Landed |
|---|---|---|---|
| `class` | `class` | String equality | v0.4 |
| `source` | `source` | String equality | v0.4 |
| `action` | `action` | String equality | v0.4 |
| `document_kind` | `document_kind` | String equality | v0.4 |
| `raw_label` | `raw_label` (safety-net only) | String equality | v0.6 |
| `field_path` | `field_path` (safety-net only) | String equality | v0.6 |
| `from_epoch_ms` | `created_at >= ?` | i64 ms | v0.4 |
| `to_epoch_ms` | `created_at <= ?` | i64 ms | v0.4 |
| `session_id` | `session_id` | String equality | v0.4 |
| `snapshot_scheme` | `snapshot_scheme` | String equality | v0.5 |
| `snapshot_alg` | `snapshot_alg` | String equality | v0.5 |
| `snapshot_key_version` | `snapshot_key_version` | i64 equality | v0.5 |
| `has_ambiguity` | `ambiguity_record IS [NOT] NULL` | bool | v0.7 |
| `ambiguity_reason` | JSON match on `ambiguity_record` | String | v0.7 |
| `collision_family` | `collision_family` | String equality | v0.7 |
| `collision_variant` | `collision_variant` | String equality | v0.7 |
| `recognizer_id` | `recognizer_id` | String equality | v0.7 |
| `recognizer_version_id` | `recognizer_version_id` | String equality | v0.7 |

### 1.5 Schema migration history

`SqliteLogger::new` runs an idempotent `PRAGMA table_info(redaction_log)` +
conditional `ALTER TABLE` block on every open. The full migration order is
in `crates/gaze-audit/src/sqlite.rs:195-305`. Versions when each column
shipped are tracked in the row tables above.

> **Adopter note.** No schema-version PRAGMA is read; the schema is detected
> by column presence. This is deliberate so older `redaction_log.sqlite`
> files always open with the newest binary. Adopters who run multiple binary
> versions against the same DB must accept that the newer binary will add
> columns lazily on first write.

## 2. Conflict-resolution tiers (`gaze`)

Source: [`ConflictTier`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:1703`. Serialization (audit-string form) at
`gaze-types/src/lib.rs:1866`. Resolver: `crates/gaze/src/resolver.rs`.
Companion deep-dives:
[`docs/explanation/detection/validator-veto.md`](../explanation/detection/validator-veto.md),
[`docs/explanation/detection/collision-family.md`](../explanation/detection/collision-family.md),
[`docs/explanation/detection/anchor-resolution.md`](../explanation/detection/anchor-resolution.md),
[`docs/explanation/safety-net/safety-net-modes.md`](../explanation/safety-net/safety-net-modes.md).

### 2.1 Tier order (canonical)

When candidates overlap, the resolver applies tiers in this fixed order;
the first tier to produce a decision wins. The order is part of
[KDD-6 (Conflict Resolution Is Deterministic)](../../ARCHITECTURE.md#kdd-6-conflict-resolution-is-deterministic).

1. **`ValidatorVeto`** (pre-resolver) — drops any candidate whose
   declared validator rejects the canonical form. Loser audit rows carry
   `validator_fail_reason`.
2. **`ClassPriority`** — PII class wins over class.
3. **`RulePriority`** — declared rule priority within class.
4. **`Score`** — recognizer confidence score.
5. **`SpanLength`** — longer span wins.
6. **`Validator`** — same-class containment validator tiebreak (distinct
   from pre-resolver `ValidatorVeto`).
7. **`CollisionPolicy`** — cross-class family precedence for declared
   collision families.
8. **`AnchoredContext`** — mandatory-anchor missing → family-level
   `Custom("family:<name>")` fallback emitted.
9. **`RecognizerId`** — final lexicographic tiebreak on recognizer id.
10. **`Merged`** — adjacent same-class candidates merged into one span.

SafetyNet modes layer on top of the resolver (after tokenization):

- **`Redact`** — `--safety-net-mode redact` overwrote a suspect span with
  the redaction sentinel. Axis 2 (reversibility) is sacrificed for that
  span; the original bytes are gone.
- **`Resolve`** — `--safety-net-mode resolve` promoted a suspect span into
  a synthetic custom-recognizer match that the resolver tokenized normally.
- **`Fallback`** — the configured `--safety-net-fallback` policy decided
  the outcome after the primary mode could not honor the suspect. Paired
  with `fallback_triggered` (see §1.2).

### 2.2 `decided_by` audit-string values

Canonical strings produced by `ConflictTier::as_str`. These are what land in
the `decided_by` column.

| String | Variant | Landed |
|---|---|---|
| `none` | `ConflictTier::None` (no conflict resolved this row) | v0.4 |
| `class_priority` | `ConflictTier::ClassPriority` | v0.4 |
| `rule_priority` | `ConflictTier::RulePriority` | v0.4 |
| `score` | `ConflictTier::Score` | v0.4 |
| `span_length` | `ConflictTier::SpanLength` | v0.4 |
| `validator` | `ConflictTier::Validator` | v0.4 |
| `validator_veto` | `ConflictTier::ValidatorVeto` | v0.7 |
| `collision_policy` | `ConflictTier::CollisionPolicy` | v0.7 |
| `anchored_context` | `ConflictTier::AnchoredContext` | v0.7 |
| `recognizer_id` | `ConflictTier::RecognizerId` | v0.4 |
| `merged` | `ConflictTier::Merged` | v0.4 |
| `redact` | `ConflictTier::Redact` | v0.8 |
| `resolve` | `ConflictTier::Resolve` | v0.8 |
| `fallback` | `ConflictTier::Fallback` | v0.8 |

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

Source: trait [`SafetyNet`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:966`; context
[`SafetyNetContext`](../../crates/gaze-types/src/lib.rs) at
`gaze-types/src/lib.rs:984`; error [`SafetyNetError`](../../crates/gaze-types/src/lib.rs)
at `gaze-types/src/lib.rs:1623`. Implementations under
`crates/gaze-recognizers/src/safety_net/`. Benchmark harness:
`crates/gaze-recognizers/benches/safety_net_matrix.rs`; pinned snapshot:
`crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json`. Deep-dives:
[`docs/explanation/safety-net/safety-nets.md`](../explanation/safety-net/safety-nets.md),
[`docs/explanation/safety-net/safety-net-modes.md`](../explanation/safety-net/safety-net-modes.md),
[`docs/reference/benchmarks/safety-net-benchmark.md`](benchmarks/safety-net-benchmark.md).

### 3.1 Per-suspect metrics (`LeakSuspect`)

Each suspect reported by a SafetyNet backend carries these fields. Source:
[`LeakSuspect`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:1125`. The exhaustive surface is also visible
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

Source: [`LeakReportStats`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:1205`. Emitted under
`leak_report.stats` in `gaze clean` JSON output
(`crates/gaze-cli/src/pipeline/run.rs:685-707`).

| Field | What | Surface | Landed |
|---|---|---|---|
| `suspect_count` | Total suspects in this report. | `leak_report.stats.suspect_count` (CLI JSON) | v0.6 |
| `uncovered_count` | Count of `LeakKind::Uncovered` suspects. | `leak_report.stats.uncovered_count` | v0.6 |
| `partial_bleed_count` | Count of `LeakKind::PartialBleed { .. }` suspects. | `leak_report.stats.partial_bleed_count` | v0.6 |
| `class_mismatch_count` | Count of `LeakKind::ClassMismatch { .. }` suspects. | `leak_report.stats.class_mismatch_count` | v0.6 |
| `locale_skipped_count` | Number of `LocaleSkipped` telemetry events. | `leak_report.stats.locale_skipped_count` | v0.6 |

> **Adopter contract.** Exit code `0` paired with `leak_report.stats.suspect_count = 0`
> is the "no leaks" contract documented in [`crates/gaze-cli/README.md`](../../crates/gaze-cli/README.md).

### 3.3 Benchmark-matrix snapshot fields

The pinned snapshot at
[`crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json`](../../crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json)
records per-cell metrics produced by
`cargo bench -p gaze-recognizers --features safety-net-kiji,safety-net-openai --bench safety_net_matrix`.
The schema is version 2; cells are keyed by `backend × locale × mode`
(`{kiji_distilbert, openai_privacy_filter} × {Global, EnUs, DeDe} × {direct_detector, observer_residual}`).
See [`docs/reference/benchmarks/safety-net-benchmark.md`](benchmarks/safety-net-benchmark.md).

**Top-level (mode-independent):**

| Field | What | Stability | Landed |
|---|---|---|---|
| `strict_span_leak_rate` | Per-(backend × locale) nullable fail-closed leak rate. End-to-end metric, not detector P/R. | `null` until pinned; numeric in `0.0..=1.0` when populated | v0.8 |

**Per cell (`direct_detector` mode):**

| Field | What | Stability | Landed |
|---|---|---|---|
| `precision` | Class-averaged precision. | Nullable f64 | v0.8 |
| `recall` | Class-averaged recall. | Nullable f64 | v0.8 |
| `f1` | Class-averaged F1. | Nullable f64 | v0.8 |
| per-class metrics | P / R / F1 per `PiiClass`. | Nullable f64 | v0.8 |

**Per cell (`observer_residual` mode):**

In addition to the `direct_detector` fields:

| Field | What | Stability | Landed |
|---|---|---|---|
| `observer_residual_recall` | Recall measured against the rule-floor residual (suspects the deterministic pipeline missed). | Nullable f64 | v0.8 |
| `agreement_with_rule_floor` | Fraction of safety-net spans that overlap a rule-floor emitted token of the same class. | Nullable f64 | v0.8 |
| `expansion_fraction` | Fraction of safety-net spans that extend an overlapping rule-floor span. | Nullable f64 | v0.8 |
| `contradiction_fraction` | Fraction of safety-net spans that contradict a rule-floor span (class mismatch). | Nullable f64 | v0.8 |
| `novel_tp_over_rule_floor` | True-positive safety-net spans with no rule-floor coverage, normalized by rule-floor TP. | Nullable f64 | v0.8 |

Methodology: [`docs/reference/benchmarks/v0.8-kiji-benchmark.md`](benchmarks/v0.8-kiji-benchmark.md).
Class-gap reference: [`docs/reference/benchmarks/v0.8-kiji-class-gap.md`](benchmarks/v0.8-kiji-class-gap.md).
Result cells are `null` until pinned local backend commands and model
directories are available — publishing numeric Kiji or OPF claims without
those pins violates the Axis 4 trust contract
([`safety-net-benchmark.md`](benchmarks/safety-net-benchmark.md)).

### 3.4 Mode + fallback observability

Mode and fallback selection drives the `decided_by` + `fallback_triggered`
columns (§1.2, §2). The CLI surface emits a single-line stderr warning per
suspect class when running `--safety-net-mode tolerant` or when the
fallback hop fires; see `crates/gaze-cli/src/pipeline/run.rs:792-820`.
Tolerant mode is gated behind `GAZE_ALLOW_TOLERANT=1` for production
deployments (see [`safety-net-modes.md`](../explanation/safety-net/safety-net-modes.md)
§3).

> **TODO (separate follow-up).** A `warn_once`-style counter for suspect-class
> warnings is referenced in the task scoping but not yet exposed on a public
> surface. Today the CLI emits one warning per non-zero `stats.*_count`
> bucket (see `emit_safety_net_warning` call sites at
> `gaze-cli/src/pipeline/run.rs:792-820`). File a follow-up todo if structured
> per-suspect-class counters are needed.

### 3.5 Backend integrity pins (`safety-net-benchmark.md`)

The benchmark doc declares backend pins (Kiji DistilBERT bundle SHA, model
SHA, tokenizer SHA, label-map SHA; OpenAI Privacy Filter source commit).
These pins are part of the Axis 4 evidence trail. Pin values are tracked in
[`docs/reference/benchmarks/safety-net-benchmark.md`](benchmarks/safety-net-benchmark.md)
and enforced via the `safety-net-sanity` xtask gate plus the `model-SHA`
integrity check in the Kiji backend.

## 4. Recognizer surface (`gaze-recognizers` + `gaze`)

Source: trait [`Recognizer`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:2835`; registry
[`RecognizerRegistry`](../../crates/gaze/src/registry.rs) at
`crates/gaze/src/registry.rs:27`; candidate
[`Candidate`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:2857`; detect context
[`DetectContext`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:2927`.

### 4.1 Per-recognizer fields

Every recognizer exposes the same metadata surface via the trait. The
registry indexes by `id` and uses these fields for conflict resolution and
locale gating.

| Method | What | Stability | Landed |
|---|---|---|---|
| `id() -> &str` | Stable recognizer identifier (e.g. `email.global`, `phone.national.de`). | Free string (per-recognizer-stable) | v0.4 |
| `supported_class() -> &PiiClass` | PII class emitted by this recognizer. | Closed enum + `custom:*` | v0.4 |
| `token_family() -> &str` | Token-family label used for output token shape. | Free string | v0.4 |
| `validator_kind() -> Option<ValidatorKind>` | Validator declared by this recognizer; pre-resolver validator-veto runs on it. Default `None`. | Closed enum ([`ValidatorKind`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:399`) | v0.4.2+ |
| `locales() -> &[LocaleTag]` | Locale tags where this recognizer activates. Empty / unset defaults to `[Global]`. | `#[non_exhaustive]` enum ([`LocaleTag`](../../crates/gaze-types/src/lib.rs) at `gaze-types/src/lib.rs:2067`) | v0.4 |

Versioned-rule lineage attaches to *candidates*, not recognizers:

| Candidate field | What | Surface | Landed |
|---|---|---|---|
| `Candidate::recognizer_id` | Stable id propagated to the audit row. | `redaction_log.recognizer_id` | v0.7 |
| `Candidate::recognizer_version_id` | Optional versioned artifact id. | `redaction_log.recognizer_version_id` | v0.7 |
| `Candidate::source` | Free-string source label (per-row pseudo-id). | `redaction_log.source` | v0.4 |
| `Candidate::priority` | Rule/recognizer priority used by `ConflictTier::RulePriority`. | Internal (resolver-only); not audited directly | v0.4 |
| `Candidate::score` | Confidence in `0.0..=1.0`. Used by `ConflictTier::Score`. | Internal | v0.4 |
| `Candidate::canonical_form` | Optional canonical form for validators / merge logic. | Internal | v0.4 |
| `Candidate::token_family` | Output token-family label. | Internal | v0.4 |
| `Candidate::merged_sources` | Sources merged into this candidate by `ConflictTier::Merged`. | Internal | v0.4 |
| `Candidate::decided_by` | Last `ConflictTier` to touch this candidate. | `redaction_log.decided_by` (via `RedactionEntry`) | v0.4 |

### 4.2 Collision-family membership

Recognizers participating in a collision family expose membership via
[`CollisionMembership`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:84`.

| Field | What | Surface | Landed |
|---|---|---|---|
| `family: String` | Cross-class family name. | `redaction_log.collision_family` | v0.7 |
| `variant: String` | Variant name within the family. | `redaction_log.collision_variant` | v0.7 |
| `precedence: u32` | Lower values win on overlap. | Internal (resolver-only) | v0.7 |
| `mandatory_anchor: Option<String>` | Anchor cue key required for this variant; missing anchor triggers `ConflictTier::AnchoredContext`. | `ambiguity_record.reason = no_anchor` | v0.7 |

Bundled family names reserved against adopter custom recognizers:
[`RESERVED_BUNDLED_FAMILIES`](../../crates/gaze-types/src/lib.rs) at
`gaze-types/src/lib.rs:66`.

### 4.3 Rulepack-load and registry-build observability

`gaze-cli` emits structured-log diagnostics when a rulepack loads;
unsupported validators / normalizers fail closed at load with typed errors:
[`ValidatorKindParseError`](../../crates/gaze-types/src/lib.rs) at
`gaze-types/src/lib.rs:387`, `RulepackError::UnsupportedValidator`,
`RulepackError::UnsupportedNormalizer`. These do not surface as audit-row
columns — they surface as CLI exit code `2` (config-level error; see §8).

Registry counters (recognizers registered, validators registered) are
**internal-only** as of v0.8; they live on
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

Source: [`Pipeline`](../../crates/gaze/src/pipeline.rs) at
`crates/gaze/src/pipeline.rs:137`; result struct
[`SafetyNetResult`](../../crates/gaze/src/pipeline.rs) at
`crates/gaze/src/pipeline.rs:147`. Companion deep-dives:
[`ARCHITECTURE.md`](../../ARCHITECTURE.md) (top-level pipeline ASCII),
[`docs/explanation/safety-net/safety-nets.md`](../explanation/safety-net/safety-nets.md) §"Trait shape".

### 5.1 Per-pass counters

The pipeline runs in up to three deterministic-ish passes. Source: structured
recognizer registry (Passes 1 + 2) and the safety-net loop (Pass 3) inside
`Pipeline::redact_*` and `Pipeline::clean_with_safety_net_*`.

| Pass | What runs | Counter surface | Stability | Landed |
|---|---|---|---|---|
| Pass 1 | Regex + dictionary recognizers from the registry. | Not exposed as a counter; visible by `Candidate::source` / `recognizer_id` on every emitted token. | Internal | v0.4 |
| Pass 2 | NER recognizers (optional feature `ner`); produces `Candidate { class=Name|Location|Organization, score, span }`. | Same as Pass 1; recognizers carry `id()` like `ner.davlan-mbert`. | Internal | v0.4 |
| Pass 3 | SafetyNet observer, post-tokenization. | `SafetyNetResult { nets_run, report }` returned by `Pipeline::clean_with_safety_net*`. `nets_run = N` registered nets; `report` is the aggregated [`LeakReport`](../../crates/gaze-types/src/lib.rs). | `#[non_exhaustive]` struct | v0.6 |

### 5.2 Per-call output

`Pipeline::clean_with_safety_net_detect_context` returns
`(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)`:

| Return field | What | Surface | Stability |
|---|---|---|---|
| `CleanDocument` | Tokenized text or structured doc. | `clean` JSON field in `gaze clean` output. | `#[non_exhaustive]` enum |
| `Vec<EmittedTokenSpan>` | Per-token (clean-span, raw-span, class) triples used by `Manifest` (§3) and restore. | Inside `Manifest`; manifest JSON in adopter restore paths. | `#[non_exhaustive]` struct |
| `LeakReport` | SafetyNet report; see §3.2. | `leak_report` JSON field. | `#[non_exhaustive]` struct |

### 5.3 Per-token / manifest counts

- **Token count per call** is `Manifest::spans.len()` (one entry per emitted
  token). Source: [`Manifest`](../../crates/gaze-types/src/lib.rs) at
  `crates/gaze-types/src/lib.rs:1044`.
- **Per-class token breakdown** is computed by `gaze-document` for its
  `BundleReport.pii_tokens_by_class` field (§6) but is **not** exposed on the
  generic `Pipeline` return — adopters who want it must group
  `EmittedTokenSpan.class` themselves.

### 5.4 Per-pass timing

**Not exposed** on a public surface as of v0.8. The benchmark harness times
backends in `crates/gaze-recognizers/benches/`, but per-call latency is not
returned from `Pipeline::redact_*` or `Pipeline::clean_with_safety_net_*`.

> **TODO (separate follow-up).** Per-pass timing is referenced in the task
> scoping but not currently exposed. If adopters need it, add a follow-up
> todo for a `PipelineTimings` return type rather than fixing it here.

### 5.5 Session-id propagation

`SafetyNetContext::session_id` (`gaze-types/src/lib.rs:994`) carries the
audit session identifier into Pass 3; the field appears on
`redaction_log.session_id` and `safety_net_log.session_id` (§1.1, §1.3) and
is filtered via `AuditFilter::session_id`. There is no `Pipeline`-level
session counter; sessions are created and bound externally by `gaze::Session`.

## 6. SafeBundle / `BundleReport` (`gaze-document`)

Source: [`BundleReport`](../../crates/gaze-document/src/bundle/mod.rs) at
`crates/gaze-document/src/bundle/mod.rs:190`; per-page record
[`PageReport`](../../crates/gaze-document/src/bundle/mod.rs) at
`crates/gaze-document/src/bundle/mod.rs:139`; class count
[`ClassCount`](../../crates/gaze-document/src/bundle/mod.rs); ocr-source
[`OcrSource`](../../crates/gaze-document/src/bundle/mod.rs) at
`crates/gaze-document/src/bundle/mod.rs:127`. Deep-dive:
[`docs/explanation/document/document-extension.md`](../explanation/document/document-extension.md).

### 6.1 Schema versioning

| Field | What | Versions |
|---|---|---|
| `bundle_version: u32` | Top-level schema version. | `1` in v0.7.1, `2` in v0.8. v1 bundles continue to parse on read; emission is always v2 in v0.8+. |

Field set is `#[non_exhaustive]` — adopters reading `report.json` must
forward-compat. Adopter-write contract is `bundle_version` first, all other
fields readable on a best-effort basis.

### 6.2 Top-level `BundleReport` fields

| Field | What | Surface | Stability | Landed |
|---|---|---|---|---|
| `bundle_version: u32` | Schema version. | `report.json` `bundle_version` | Numeric | v0.7.1 (v=1), v0.8 (v=2) |
| `input_kind: String` | Detected input kind. | `report.json` `input_kind` | Free string | v0.7.1 |
| `ocr_mean_confidence: Option<f32>` | Mean Tesseract word confidence (0..100). | `report.json` `ocr_mean_confidence` | Numeric | v0.7.1 |
| `ocr_word_count: usize` | Number of OCR words with non-negative confidence. | `report.json` `ocr_word_count` | Numeric | v0.7.1 |
| `ocr_lang: String` | Tesseract language code. | `report.json` `ocr_lang` | Free string | v0.7.1 |
| `clean_char_count: usize` | Character count of tokenized markdown. | `report.json` `clean_char_count` | Numeric | v0.7.1 |
| `pii_token_count: u32` | Total PII tokens across all classes. | `report.json` `pii_token_count` | Numeric | v0.7.1 |
| `pii_tokens_by_class: Vec<ClassCount>` | Per-class token counts. `ClassCount { class: String, count: u32 }`. | `report.json` `pii_tokens_by_class[]` | Free string + numeric | v0.7.1 |
| `pdf_page_count: Option<i32>` | PDF page count (`None` for image inputs). | `report.json` `pdf_page_count` | Numeric | v0.7.1 |
| `pdf_page_index: Option<i32>` | PDF page index rasterized (`None` for image inputs). | `report.json` `pdf_page_index` | Numeric | v0.7.1 |
| `pages: Vec<PageReport>` | Per-page extraction + confidence + layout provenance. | `report.json` `pages[]` | `#[non_exhaustive]` struct | v0.8 |
| `low_confidence_threshold: f32` | Threshold used to set `PageReport.low_confidence`. | `report.json` `low_confidence_threshold` | Numeric | v0.8 |

### 6.3 Per-page `PageReport` fields

| Field | What | Stability | Landed |
|---|---|---|---|
| `page_index: i32` | Zero-based page index. | Numeric | v0.8 |
| `ocr_source: OcrSource` | `vector_pdf` / `ocr` (closed enum at `crates/gaze-document/src/bundle/mod.rs:127`). | Closed enum (rename-stable) | v0.8 |
| `ocr_backend: Option<String>` | OCR backend name when `ocr_source = ocr`. | Free string | v0.8 |
| `confidence: Option<f32>` | Aggregated page confidence in `0.0..=1.0`. `None` for vector-PDF text. | Numeric | v0.8 |
| `low_confidence: bool` | True when `confidence < low_confidence_threshold`. | Boolean | v0.8 |
| `column_count: u32` | Detected text column count (`1` = single-column). | Numeric | v0.8 |
| `ocr_word_count: usize` | OCR words with confidence for this page. | Numeric | v0.8 |
| `ocr_mean_confidence: Option<f32>` | Legacy percent-scale mean confidence. | Numeric | v0.8 |

> **Adopter note.** The `pages[]` array shipped in v0.8 alongside the
> `bundle_version` bump. Downstream tooling reading v1 bundles will not see
> a `pages` field; downstream tooling reading v2 bundles must handle it.

### 6.4 `DocumentExtension` (signed snapshot envelope)

Owner-only signed bundle integrity envelope. Source:
[`DocumentExtension`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:1226`. Surfaces inside the session snapshot —
never inside agent-facing artifacts.

| Field | What | Stability | Landed |
|---|---|---|---|
| `schema_version: u16` | Bundle-level schema shared by clean/layout/preview/report/manifest. | Numeric | v0.7 |
| `clean_md_sha256: [u8; 32]` | SHA-256 of `clean.md` NFC bytes. | Bytes | v0.7 |
| `layout_json_sha256: [u8; 32]` | SHA-256 of canonical `layout.json` bytes. | Bytes | v0.7 |
| `report_json_sha256: [u8; 32]` | SHA-256 of canonical `report.json` bytes. | Bytes | v0.7 |
| `preview_png_sha256: Option<[u8; 32]>` | SHA-256 of `preview-redacted.png` when present. | Bytes | v0.7 |
| `page_count: u32` | Page count for the source document. | Numeric | v0.7 |
| `audit_session_id: String` | Audit session id mirrored from the writing session. | Free string | v0.7 |
| `clean_spans: Vec<EmittedTokenSpan>` | Signed clean.md byte spans for every emitted token. | `#[non_exhaustive]` struct | v0.7 |
| `codec_audit: Vec<CodecAuditRow>` | Per-decode codec audit rows (codec id, version, MIME, capabilities, text origin). | `#[non_exhaustive]` struct | v0.7 |

### 6.5 `CodecAuditRow`

Per-decode metadata-only row. Source:
[`CodecAuditRow`](../../crates/gaze-types/src/lib.rs) at
`crates/gaze-types/src/lib.rs:1435`. Adopters embedding alternate OCR /
codec backends populate one row per decode.

Notable fields: `codec_id` (e.g. `gaze.codec.tesseract`), `codec_version`,
`accepted_mime`, `advertised` / `delivered` capability bitsets, `text_origin`
(closed enum: `ocr` / `embedded_text` / `transcript` / `hybrid` at
`gaze-types/src/lib.rs:1360`), `codec_output_schema_version`, `options_hash_hex`,
`engine_provenance`, `extraction_density_policy`. Stable since v0.7.

## 7. MCP chokepoint observability (`gaze-mcp-core`)

Source: [`ToolCtx`](../../crates/gaze-mcp-core/src/ctx.rs) at
`crates/gaze-mcp-core/src/ctx.rs:131`; `ManifestStore` at
`crates/gaze-mcp-core/src/manifest.rs:175`; `AuthHook` at
`crates/gaze-mcp-core/src/auth.rs:102`. Deep-dive:
[`docs/explanation/mcp/mcp-runtime.md`](../explanation/mcp/mcp-runtime.md).

### 7.1 `ToolCtx` fields

Sealed tool-invocation context — every tool body receives exactly one
`ToolCtx<'a>` for the dispatch frame. Constructor is `pub(crate)`; the
fields enumerate the audit-correlation surface.

| Field / accessor | What | Surface | Stability | Landed |
|---|---|---|---|---|
| `call_id: Ulid` | Stable ULID per dispatch. Reused as `CallHandle`. | `redaction_log.session_id` per row when persisted via the audit logger bridge in `gaze-cli`; also flows into rmcp protocol fields. | Closed (ULID) | v0.7 |
| `tool_name: &'a str` | Name of the tool being dispatched. | Borrowed in `BeginCallContext.tool_name`; adopter-owned column in manifest store. | Free string | v0.7 |
| `principal_id: &'a str` | Principal stable id (post-`AuthHook`). | `BeginCallContext.principal_id`. | Free string | v0.7 |
| `session().audit_session_id() -> &'a str` | Audit-correlation session id. | `redaction_log.session_id`; `safety_net_log.session_id`. | Free string | v0.7 |
| `redacted_args: &serde_json::Value` | Post-redaction JSON args; safe to inspect / re-emit. | `BeginCallContext.redacted_args`. | Adopter-owned JSON | v0.7 |
| `resources().pipeline()` / `.session()` / `.manifest()` / `.locale_chain()` | Borrowed backend handles for tool bodies that need Gaze internals. | Internal (sealed) | Stable since v0.7 |

### 7.2 `ManifestStore` lifecycle counters

`ManifestStore` is an adopter-implemented trait
(`gaze-mcp-core/src/manifest.rs:175`). The dispatcher invokes the three
methods in fixed order:

| Method | What | When | Failure mode | Stability | Landed |
|---|---|---|---|---|---|
| `begin_call(BeginCallContext<'_>) -> Result<CallHandle, ManifestError>` | Opens a manifest entry. Must be idempotent on `call_id` collisions. | After auth + input redaction, before tool body runs. | `ManifestError::DuplicateCallId` on collision; `Backend` for adopter errors; `Validation` for malformed payloads. | `#[non_exhaustive]` trait | v0.7 |
| `finish_call(CallHandle, SnapshotRef) -> Result<(), ManifestError>` | Finalizes the entry on success. | After tool body returns and response is redacted; before chokepoint returns to transport. | `UnknownHandle` if `begin_call` was not run. | `#[non_exhaustive]` trait | v0.7 |
| `fail_call(CallHandle, FailureReason) -> Result<(), ManifestError>` | Finalizes the entry on failure. | When auth / tool body / response redaction returned an error. | Same as `finish_call`. | `#[non_exhaustive]` trait | v0.7 |

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

[`AuthHook`](../../crates/gaze-mcp-core/src/auth.rs) at
`crates/gaze-mcp-core/src/auth.rs:102` gates every tool dispatch.

| Method | What | Returns | Audit linkage | Landed |
|---|---|---|---|---|
| `authorize_agent(...)` | Agent-tier authorization. | `Result<Principal, AuthError>` | Failure → `ManifestStore::fail_call(_, FailureReason::AuthDenied { reason })` | v0.7 |
| `authorize_operator(...)` | Operator-tier authorization. | `Result<Principal, AuthError>` | Same as above. | v0.7 |

`Principal` (`gaze-mcp-core/src/auth.rs:24`) carries
`{ id: String, roles: Vec<String> }` — `id` becomes `ToolCtx.principal_id`.

### 7.4 Session-id format policy

[`SessionIdPolicy`](../../crates/gaze-mcp-core/src/session_id.rs) at
`crates/gaze-mcp-core/src/session_id.rs:68` validates transport-supplied
session ids. Closed enum
[`SessionIdFormat`](../../crates/gaze-mcp-core/src/session_id.rs) at
`crates/gaze-mcp-core/src/session_id.rs:26` declares accepted formats with
per-format `effective_entropy_bits()`. Validation failures fail closed at
the transport boundary before `ManifestStore::begin_call` runs.

## 8. CLI exit codes (`gaze-cli`)

Source: [`CliError`](../../crates/gaze-cli/src/error.rs) at
`crates/gaze-cli/src/error.rs:8`; `exit_code()` at
`crates/gaze-cli/src/error.rs:54`. Stderr emission is one JSON line per
error: `{"error":"<Variant>","exit":<N>, ...}`.

| Exit code | When | `error` field | Stability | Landed |
|---|---|---|---|---|
| `0` | Success. With SafetyNet active: `leak_report.stats.suspect_count = 0` is the "no leaks" contract. | n/a | Stable | v0.4 |
| `1` | Stdin parse / empty input / input-too-large / invalid-encoding. | `StdinParse` / `EmptyInput` / `InputTooLarge` / `InvalidEncoding` | Closed | v0.4 |
| `2` | Config-level error: policy malformed, unsupported policy schema, ISO-8601 parse failure on `audit purge`, pinned SafetyNet artifact missing. | `PolicyConfig` / `PolicySchemaUnsupported` / `AuditPurgeIso8601` / `SafetyNetArtifactMissing` | Closed | v0.4 / v0.8 (artifact-missing) |
| `3` | Runtime fail-closed: SafetyNet config rejected, SafetyNet runtime suspect (strict mode), pipeline error, unknown token at restore, unsupported session scope, invalid signature, invalid blob version, blob expired. **The dedicated "audit-logger fatal" path also exits 3 via `std::process::exit(3)`** (`crates/gaze-cli/src/logger.rs:12`). | `SafetyNetConfig` / `SafetyNet` / `Pipeline` / `UnknownToken` / `UnsupportedSessionScope` / `InvalidSignature` / `InvalidBlobVersion` / `BlobExpired` | Closed | v0.4 / v0.6 (SafetyNet*) |
| `4` | I/O or policy-file-open error. | `Io` / `PolicyOpen` | Closed | v0.4 |
| `5` | Document subcommand error (feature `document` only). | `Document` | Closed | v0.7.1 |
| `6` | MCP subcommand error (feature `mcp` only). | `Mcp` | Closed | v0.7 |
| `7` | Proxy subcommand error (feature `proxy` only). | `Proxy` | Closed | v0.8 |

> **Adopter contract.** Exit code is the primary CI / agent signal; the
> stderr JSON line is the structured detail. `CliError` is private to
> `gaze-cli` but the exit-code → behavior contract above is part of the
> adopter-facing surface and is documented in
> [`crates/gaze-cli/README.md`](../../crates/gaze-cli/README.md).

## Companion architecture docs

- [`docs/explanation/detection/ambiguity-side-channel.md`](../explanation/detection/ambiguity-side-channel.md) — `ambiguity_record`, `validator_fail_reason`, `collision_*` schema.
- [`docs/explanation/detection/validator-veto.md`](../explanation/detection/validator-veto.md) — `ConflictTier::ValidatorVeto` semantics.
- [`docs/explanation/detection/collision-family.md`](../explanation/detection/collision-family.md) — `CollisionMembership`, `ConflictTier::CollisionPolicy`.
- [`docs/explanation/detection/anchor-resolution.md`](../explanation/detection/anchor-resolution.md) — `mandatory_anchor`, `ConflictTier::AnchoredContext`.
- [`docs/explanation/safety-net/safety-nets.md`](../explanation/safety-net/safety-nets.md) — Pass-3 observer contract, trait shape, manifest invariants.
- [`docs/explanation/safety-net/safety-net-modes.md`](../explanation/safety-net/safety-net-modes.md) — `resolve` / `redact` / `fallback` modes and `decided_by` extensions.
- [`docs/reference/benchmarks/safety-net-benchmark.md`](benchmarks/safety-net-benchmark.md) — `safety_net_matrix` snapshot pins + matrix shape.
- [`docs/explanation/mcp/mcp-runtime.md`](../explanation/mcp/mcp-runtime.md) — `ToolCtx` seal, dispatch ordering, manifest persistence.
- [`docs/explanation/document/document-extension.md`](../explanation/document/document-extension.md) — signed snapshot envelope.
- [`docs/explanation/policy/locale-chain.md`](../explanation/policy/locale-chain.md) — 4-tier locale resolution.
- [`docs/explanation/detection/feedback-loop.md`](../explanation/detection/feedback-loop.md) — resolve-mode promotion plumbing.

## Versioning posture

`gaze-types` value contracts are `#[non_exhaustive]` across the board.
Adopters must:

- Match every closed enum with a wildcard arm (`_ => …`).
- Forward-compat all `#[non_exhaustive]` structs (do not destructure
  positionally; use named field patterns plus `..`).
- Treat free-string columns as opaque grouping keys, not as enum values
  for alert rules. Switch on canonical `Closed`-enum columns (`action`,
  `document_kind`, `decided_by`, `validator_fail_reason`,
  `fallback_triggered`, `ambiguity_record.reason`) — those have the
  Axis 4 stability guarantee.

When a column or field is added to a metric in this catalog, update the
"Landed" cell and bump the version note in [`CHANGELOG.md`](../../CHANGELOG.md).
This document is the single source of truth — divergence between metrics.md
and source is a docs bug.
