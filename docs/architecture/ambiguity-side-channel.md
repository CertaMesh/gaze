# Ambiguity Side-Channel

Gaze keeps pseudonymization decisions reversible and audit-safe by separating
token emission from metadata explaining why a recognizer outcome was kept,
dropped, or generalized. The ambiguity side-channel extends that audit metadata
for v0.7.x collision handling.

## Contract

`gaze-types` owns the public value contract:

- `AmbiguityRecord`
- `LosingCandidate`
- `AmbiguityReason`
- `ValidatorFailReason`

`AmbiguityRecord` is attached to `RedactionEntry` with
`.with_ambiguity_record(...)`. `ValidatorFailReason` is attached with
`.with_validator_fail_reason(...)`. `RedactionEntry::new(...)` keeps its
existing positional signature; new fields default to `None`.

The side-channel is metadata-only. It records PII classes, recognizer IDs, and
closed reason enums. It does not store original PII bytes, emitted token bytes,
or restore material.

## Shape

An ambiguity record carries:

- `ambiguity_class`: family-level class assigned when a precise variant could
  not be selected.
- `losing_candidates`: candidate class plus recognizer ID pairs that were
  plausible but not emitted. Producers sort this list by `recognizer_id`
  ascending for stable audit serialization.
- `reason`: closed enum explaining why the fallback happened.

Current ambiguity reasons:

- `NoAnchor`: no mandatory anchor cue resolved the family.
- `ValidatorIndeterminate`: validators left multiple candidates viable.
- `MultiFamilyMatch`: recognizers matched across more than one collision family.
- `PrecedenceTie`: family policy precedence tied without a discriminator.

Validator failure reasons are also closed:

- `LuhnFailed`
- `IbanMod97Failed`
- `EmailRfcFailed`
- `E164PhoneFailed`

## Serialization

`PiiClass` serializes as the same canonical strings used by audit storage:

- `email`
- `name`
- `location`
- `organization`
- `custom:<name>`

This keeps JSON side-channel blobs, SQLite class columns, and CLI exports on one
taxonomy. Deserialization accepts legacy builtin names such as `Name` so checked
in and adopter-owned older snapshots remain importable.

## SQLite Storage

`gaze-audit::SqliteLogger` owns persistence. The v0.7.x migration adds four
nullable columns to `redaction_log`:

- `validator_fail_reason TEXT NULL`
- `ambiguity_record TEXT NULL`
- `collision_family TEXT NULL`
- `collision_variant TEXT NULL`

Fresh databases create these columns inline. Existing databases migrate lazily
on the next `SqliteLogger::new(path)` call through the existing idempotent
pattern:

1. `CREATE TABLE IF NOT EXISTS redaction_log (...)`
2. `PRAGMA table_info(redaction_log)`
3. `ALTER TABLE redaction_log ADD COLUMN ...` for each missing column

There is no schema version table. Reopening the same database is a no-op after
the columns exist.

`validator_fail_reason` and `ambiguity_record` store JSON produced at the SQLite
boundary with `serde_json`. `gaze-types` remains serde-only and does not depend
on `serde_json` or `rusqlite`.

`collision_family` and `collision_variant` are reserved nullable string columns
for the collision-family producer work. Until producers populate them, they are
`NULL`.

## Query Semantics

`AuditLogRow` exposes the four new columns as optional strings:

- JSON string for `validator_fail_reason`
- JSON string for `ambiguity_record`
- plain string for `collision_family`
- plain string for `collision_variant`

The lower-level audit query keeps raw JSON strings so `gaze-audit` does not
interpret CLI presentation concerns. CLI JSONL parses the JSON strings back into
typed `ValidatorFailReason` and `AmbiguityRecord` values before writing output.

`build_audit_query_sql` accepts column-presence booleans for cross-version
compatibility. If a database lacks a Spike 4 column, query projection uses
`NULL AS <column>`. Filters against missing columns naturally return no matching
rows, except `has_ambiguity = false`, which matches legacy rows because their
projected ambiguity value is `NULL`.

Supported filters:

- `has_ambiguity`
- `ambiguity_reason`
- `collision_family`
- `collision_variant`

`ambiguity_reason` uses SQLite JSON1:

```sql
json_extract(ambiguity_record, '$.reason') = ?
```

This is intentionally simple for v0.7.x. If audit logs become large enough that
reason filtering needs indexes, add generated/indexed columns in a later
migration.

## CLI Surface

`gaze audit query` and `gaze audit export` accept:

- `--has-ambiguity`
- `--ambiguity-reason <variant>` using kebab-case, such as `no-anchor`
- `--collision-family <family-id>`
- `--collision-variant <variant-id>`

Text query output includes the approved audit columns. When any returned row has
an ambiguity record, it appends an `ambiguity` display column:

```text
class=custom:postal_or_phone_de reason=no_anchor losing=[custom:postal_de:postal-de]
```

When no returned row has ambiguity metadata, the display column is omitted to
preserve compact output for the common path.

JSONL export emits parsed typed values:

- `validator_fail_reason: "luhn_failed"`
- `ambiguity_record: { ... }`
- `collision_family: "de-postal-phone"`
- `collision_variant: "postal-de"`

## Safety Properties

The side-channel strengthens axis 4 by making fallback decisions traceable to
closed reason enums and recognizer IDs. It strengthens axis 5 by bundling
validator, ambiguity, and collision metadata into one additive migration window.

The side-channel must not become a restore source. Restore remains manifest
first. Audit metadata explains decisions; it never reconstructs original input.

Any future field added to public structs must preserve the constructor contract:
add `with_*` builders or new constructors rather than changing existing
positional `new(...)` signatures.
