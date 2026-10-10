# Ambiguity side-channel

Audit metadata explains which recognizer candidates were kept, dropped, or
generalized. It contains classes, recognizer IDs, and closed reason enums;
never original PII, token bytes, or restore material. Restore uses the manifest.

## Public value contract

`gaze-types` owns `AmbiguityRecord`, `LosingCandidate`, `AmbiguityReason`, and
`ValidatorFailReason`. Attach records with `RedactionEntry::with_ambiguity_record`
or `with_validator_fail_reason`. `RedactionEntry::new` retains its positional
signature; fields default to `None`. Future fields need builders or new constructors.

## Record shape

| Field | Meaning |
| --- | --- |
| `ambiguity_class` | Family class used when no precise variant won |
| `losing_candidates` | Plausible class/recognizer-ID pairs; sort by ascending `recognizer_id` |
| `reason` | Closed fallback reason |

Reasons are `NoAnchor`, `ValidatorIndeterminate`, `MultiFamilyMatch`, and
`PrecedenceTie`: missing cue, multiple validator survivors, multiple families,
and tied precedence, respectively.

See [validator reasons](validator-veto.md#type-ownership). Legacy
`email_rfc_failed` and `e164_phone_failed` still deserialize to `EmailRfcRejected`
and `PhoneE164Rejected`.

## Serialization

`PiiClass` uses audit strings: `email`, `name`, `location`, `organization`, and
`custom:<name>`. Deserialization also accepts legacy builtin names such as `Name`.

## SQLite storage

`gaze-audit::SqliteLogger` stores four nullable `redaction_log` columns:

| Column | Storage |
| --- | --- |
| `validator_fail_reason` | JSON text |
| `ambiguity_record` | JSON text |
| `collision_family` | Plain string or `NULL` when unset |
| `collision_variant` | Plain string or `NULL` when unset |

Fresh databases create them inline. `SqliteLogger::new(path)` lazily runs
`CREATE TABLE IF NOT EXISTS`, checks `PRAGMA table_info(redaction_log)`, and adds
missing columns with `ALTER TABLE`. There is no schema-version table; reopening
is idempotent. JSON encoding uses `serde_json` at this boundary; `gaze-types`
remains serde-only, without `serde_json` or `rusqlite`.

## Query semantics

`AuditLogRow` exposes all four as optional strings. CLI JSONL parses the JSON
fields into typed values. `build_audit_query_sql` uses `PresentColumns` and
projects missing columns as `NULL AS <column>`. Missing-column filters match
no rows except `has_ambiguity = false`, which includes legacy rows.

Filters are `has_ambiguity`, `ambiguity_reason`, `collision_family`, and
`collision_variant`. Reason filtering uses SQLite JSON1:

```sql
json_extract(ambiguity_record, '$.reason') = ?
```

## CLI surface

`gaze audit query` and `gaze audit export` accept `--has-ambiguity`,
`--ambiguity-reason <variant>` (kebab-case, such as `no-anchor`),
`--collision-family <family-id>`, and `--collision-variant <variant-id>`.

Text output adds an `ambiguity` column only if a returned row has a record:

```text
class=custom:postal_or_phone_de reason=no_anchor losing=[custom:postal_de:postal-de]
```

JSONL emits typed `validator_fail_reason` (for example `"luhn_failed"`),
`ambiguity_record`, and plain collision strings.

## Safety properties

Audit explains decisions without reconstructing input. Never use it as a
restore source or change existing positional constructor signatures.
