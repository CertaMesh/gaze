# Gaze v0.2 — Channel-Agnostic Redaction Engine

**Status:** Design
**Author:** Krishan Koenig
**Date:** 2026-04-14

---

## Problem

Gaze v0.1 is a MySQL+Laravel-logs debug proxy. It works, but its redaction logic is hardcoded to two channels with two separate pipelines: `Anonymizer` for DB rows (session-scoped HMAC tokens) and `Scanner` for log lines (stateless regex → `[REDACTED]`). These pipelines share no session state — an agent can't correlate `Person_7` in a DB result with the same person in a log line.

Ghostwriter (deterministic PII sanitization for LLM conversations) duplicates its own detection code with zero shared logic.

Adding a new channel (email, files, API responses) requires building a third redaction path from scratch.

## Core Insight

Gaze is the **black marker** on information an agent needs to work with but isn't allowed to see raw. From first principles, the pipeline is:

```
detect → anonymize → restore
```

Everything else — databases, logs, MCP protocol, CLI, TOML config — is a consumer concern. The core engine should be channel-agnostic: take any document in, find PII, apply the appropriate action, return a clean version with session-scoped tokens.

## Goal

Restructure Gaze into a workspace with three crates:

1. **`crates/gaze`** — channel-agnostic redaction engine. Pure library. No I/O, no protocol knowledge.
2. **`crates/debug-proxy`** — MCP server for AI agents debugging production MySQL + Laravel logs. Consumer of gaze core.
3. **`crates/ghostwriter`** — deterministic text sanitization + restoration for LLM conversations. Consumer of gaze core.

Current `src/` is moved to `old/src/` as read-only reference. All three crates are built fresh with clean APIs. `old/` is deleted once migration is complete and tests pass.

## Non-Goals

- **Operations proxy** — agents acting on tokenized handles (send email to `Email_3`, update record for `Person_7`) is a future product. Documented in memory, not designed here. Core enables it via `Session::restore()`.
- **Pipe mode** — `gaze clean | gaze restore` stdin/stdout processing. Core is ready for this — just a thin CLI consumer. v0.3 scope.
- **Compositional attack defense** — query budgets, cardinality guards. Consumer-level concern (debug-proxy). Research doc has details.
- **Format-preserving output implementation** — `Action::FormatPreserve` is defined as a variant but generating valid-looking fake emails/phones from HMAC is v0.3 work.
- **Fuzzy matching detector** — handling typos ("Frnk Einstein" → `<CUSTOMER_NAME>`). Future `Detector` impl.
- **Desktop app** — Tauri-based native UI. Same core, new consumer. vFuture.
- **nono integration** — kernel-level ACL enforcement for agent execution sandboxing. Separate brainstorming session.

---

## Architecture

### Workspace Layout

```
Cargo.toml              (workspace root)
old/
  src/                   read-only reference (current v0.1 code)
  tests/                 read-only reference (current v0.1 tests)
crates/
  gaze/                  core engine
    src/
      lib.rs
      pipeline.rs        Pipeline builder + execution
      session.rs         SessionKey, SessionMap, export/import
      detector/
        mod.rs           Detector trait
        regex.rs         RegexDetector
        worka.rs         WorkaDetector (NER)
      rule/
        mod.rs           Rule trait, Action enum
        column.rs        ColumnRule
        class.rs         ClassRule
        default.rs       DefaultRule
      audit/
        mod.rs           Auditor trait, AuditEntry
        sqlite.rs        SqliteAuditor
      types.rs           RawDocument, CleanDocument, PiiClass, Detection, Context, Value
  debug-proxy/           MCP debug server
    src/
      main.rs
      cli.rs             clap: init/check/serve/audit
      policy.rs          TOML parser → Detector + Rule construction
      adapter/
        mod.rs           DatabaseAdapter, LogAdapter traits
        mysql.rs         sqlx MySQL impl
        laravel_log.rs   file-based log impl
        ssh_tunnel.rs    SSH tunnel lifecycle
      mcp/
        mod.rs           rmcp server + tool handlers
        errors.rs        error sanitization
  ghostwriter/           text sanitization for LLM conversations
    src/
      lib.rs             sanitize() + restore() public API
      main.rs            CLI (JSON stdin/stdout)
      context.rs         ContextDetector (known customer data)
      index.rs           IndexDetector (domain-specific values)
      blob.rs            session export → base64 JSON serialization
```

### Core Crate (`crates/gaze`)

Pure library. Zero I/O, zero protocol knowledge, zero config file formats.

#### Composable Traits

Three extension points, all trait-based and stackable:

```rust
/// Finds PII in text. Multiple detectors run in sequence, spans merged.
trait Detector: Send + Sync {
    fn detect(&self, input: &str) -> Vec<Detection>;
}

/// Decides what to do with detected PII. First matching rule wins.
trait Rule: Send + Sync {
    fn action(&self, class: PiiClass, context: &Context) -> Action;
}

/// Receives audit entries. Every redaction is logged.
trait Auditor: Send + Sync {
    fn log(&self, entry: &AuditEntry) -> Result<()>;
}
```

Consumers compose these via the builder to construct their pipeline. Core provides common implementations; consumers bring domain-specific ones.

#### Pipeline

Entry point. Built via builder, immutable after construction.

```rust
let pipeline = Pipeline::builder()
    .detector(RegexDetector::new(patterns))
    .detector(IndexDetector::new(customer_data))
    .detector(WorkaDetector::new())
    .rule(ColumnRule::new("email", Action::FormatPreserve))
    .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
    .rule(DefaultRule::new(Action::Redact))
    .auditor(SqliteAuditor::new(path))
    .build()?;
```

Two operations:

```rust
let clean: CleanDocument = pipeline.redact(&session, raw_document)?;
let raw: Option<String>  = session.restore(token);
```

#### Session

Owns the HMAC key and bidirectional token map. One per agent interaction. Created by core, opaque to consumers.

```rust
let session = Session::new()?;           // generates key, mlocks memory
let snapshot = session.export()?;        // for serialization (Ghostwriter blob)
let session = Session::import(snapshot)?; // restore from snapshot (new key, restored map)
```

`Session::new()` generates a random 32-byte key in a `SecretBox`, mlocks the memory page, and sets up zeroize-on-drop. The key never leaves the `Session` struct, is never serializable, and is never written to disk.

`Session::export()` returns the token map (bidirectional mapping of `(class, raw) ↔ fake`) without the key. This is what Ghostwriter serializes into its blob. `Session::import()` creates a new session with a fresh key and the restored map — the old key is gone, but restore still works because the map is the lookup structure.

#### Types

```rust
/// Input document. Not serializable — enforced by compile-fail test.
enum RawDocument {
    Structured(BTreeMap<String, Value>),
    Text(String),
}

/// Output document. Only constructable inside the pipeline.
enum CleanDocument {
    Structured(BTreeMap<String, serde_json::Value>),
    Text(String),
}

/// A detected PII span.
struct Detection {
    span: Range<usize>,
    class: PiiClass,
    source: String,  // which detector found it
}

/// PII categories.
enum PiiClass {
    Name, Email, Phone, Address, Id, Iban, Ip, Date, GenericText,
    Custom(String),  // domain-specific (e.g., "order_id", "song_title", "artist_name")
}

/// What to do with detected PII.
enum Action {
    Tokenize,        // session-scoped HMAC pseudonym (Person_7)
    Redact,          // [REDACTED] — destroys information
    FormatPreserve,  // deterministic fake that validates as original type (v0.3 impl)
    Generalize,      // category token (Berlin → [REGION])
    Preserve,        // pass through untouched
}

/// Context available to Rules for decision-making.
struct Context {
    field_name: Option<String>,
    source: Option<String>,
    data_type: Option<ColumnType>,
}
```

#### Core-Provided Implementations

**Detectors:**
- `RegexDetector` — compiled `RegexSet` or Aho-Corasick automaton. Constructed from a list of patterns. Returns spans with PiiClass inferred from which pattern matched.
- `WorkaDetector` — wraps worka-ai/pii crate. NER-based detection. Returns spans with PiiClass translated from Worka's `EntityType`.

**Rules:**
- `ColumnRule` — matches on field name. "If field is X, action is Y."
- `ClassRule` — matches on PiiClass. "If class is Email, action is FormatPreserve."
- `DefaultRule` — catch-all. "Everything else gets Redacted."

**Auditors:**
- `SqliteAuditor` — append-only SQLite log. Stores: timestamp, detector source, PiiClass, action taken, field name, document type. Never stores raw values or token mappings.

#### What Core Does NOT Do

- No I/O (no DB connections, no file reads, no network calls)
- No config file format (no TOML, no YAML, no env vars)
- No CLI
- No protocol knowledge (no MCP, no JSON-RPC, no HTTP)
- No operations or execution (no sending emails, no running queries)

---

### Debug Proxy (`crates/debug-proxy`)

MCP server for AI agents debugging production MySQL + Laravel logs. Binary crate.

#### Responsibilities

- **CLI** — `gaze init|check|serve|audit` via clap.
- **TOML policy** — parses `policy.toml`. Translates column rules into `ColumnRule` + `ClassRule` instances. Translates strip patterns into `RegexDetector`. Translates allowed tables / blocked columns into access control logic (consumer-level, not core).
- **MySQL adapter** — `DatabaseAdapter` trait with sqlx impl. Methods: `schema()`, `sample()`, `count()`, `distinct()`, `explain()`. Returns `RawDocument::Structured`.
- **Laravel log adapter** — `LogAdapter` trait with file impl. Methods: `search()`, `tail()`, `context()`. Returns `RawDocument::Text`.
- **SSH tunnel** — lifecycle management for tunneled DB connections via shelled-out `ssh -f -N -L`.
- **MCP server** — rmcp stdio transport. 8 tool handlers: `db.tables`, `db.schema`, `db.sample`, `db.count`, `db.distinct`, `logs.search`, `logs.tail`, `logs.context`.
- **Error sanitizer** — runs MySQL error messages through `pipeline.redact()` before returning to agent.

#### Key Change from v0.1

DB rows and log lines go through the same `pipeline.redact()` call with the same `Session`. Agent sees consistent pseudonyms across channels — `Person_7` in a DB result is the same person as `Person_7` in a log line.

#### Tool Handler Pattern

Every MCP tool handler follows this flow:

```
validate args against policy
    → adapter call → RawDocument
    → pipeline.redact(&session, raw) → CleanDocument
    → auditor logs the access
    → serialize CleanDocument → MCP response
```

Filter translation (agent passes `Person_7` in a WHERE clause):

```
session.restore("Person_7") → "John Doe"
    → build real SQL filter
    → adapter query
    → pipeline.redact() on results
    → return to agent
```

---

### Ghostwriter (`crates/ghostwriter`)

Deterministic text sanitization + exact-token restoration for LLM conversations. Library + binary crate.

#### Public API

```rust
pub fn sanitize(request: SanitizeRequest) -> Result<SanitizeResponse>;
pub fn restore(request: RestoreRequest) -> Result<RestoreResponse>;
```

#### How It Uses Core

**Detection** — builds a pipeline with domain-specific detectors:

```rust
let pipeline = Pipeline::builder()
    .detector(ContextDetector::new(customer.name, customer.email, customer.phone))
    .detector(IndexDetector::new(customer.order_ids, customer.songs, customer.artists))
    .detector(RegexDetector::new(standard_patterns))
    .detector(WorkaDetector::new())
    .rule(ClassRule::new(PiiClass::Custom("customer_name".into()), Action::Tokenize))
    .rule(DefaultRule::new(Action::Tokenize))
    .build()?;
```

**ContextDetector** — takes known customer identity (name, email, phone). Exact-match detection. Returns `Detection` with semantic PiiClass like `Custom("customer_name")` which the Rule chain can map to `<CUSTOMER_NAME>` style tokens.

**IndexDetector** — takes customer-specific domain values (order IDs, song titles, artist names). HashMap-backed lookup. This addresses Markus's identified gap: arbitrary strings like song titles and artist names that no regex or NER can catch.

**Session + blob** — `session.export()` produces a `SessionSnapshot`. Ghostwriter serializes this to base64 JSON — the "session blob" that Laravel stores encrypted. On restore, `Session::import(snapshot)` reconstructs the map for token reversal.

#### What Ghostwriter Owns

- **CLI** — JSON stdin/stdout for Laravel queue integration.
- **Blob format** — base64 JSON serialization of `SessionSnapshot`. Ghostwriter owns the wire format; core owns the data.
- **ContextDetector** — known customer identity matching.
- **IndexDetector** — domain-specific value lookup (order IDs, songs, artists).
- **Restore warnings** — unused placeholders, unknown tokens in LLM output.

---

## Data Flow

### Redact

```
Consumer receives raw input (DB row, log line, customer message, etc.)
    │
    ▼
Wraps as RawDocument (Structured or Text)
    │
    ▼
pipeline.redact(&session, raw_document)
    │
    ├─ Detector stack runs in order
    │   Each returns Vec<Detection> (byte spans + PiiClass)
    │   Core merges spans, deduplicates overlaps (longest span wins)
    │   On exact-length tie, first detector in stack wins
    │
    ├─ Rule chain evaluates each detection
    │   First matching rule determines Action
    │
    ├─ Session applies the action per detection:
    │   Tokenize     → HMAC pseudonym, stored in bidirectional map
    │   Redact       → [REDACTED], no map entry
    │   FormatPreserve → deterministic fake value, stored in map
    │   Generalize   → category token, no map entry
    │   Preserve     → pass through untouched
    │
    ├─ Auditor logs: detector source, PiiClass, action, field name
    │   Never logs raw values or token mappings
    │
    ▼
CleanDocument returned to consumer
```

### Restore

```
Agent sends token (e.g. "Person_7") back in a request
    │
    ▼
session.restore("Person_7") → Option<"John Doe">
    │
    ▼
Consumer uses real value internally (DB query, email send, etc.)
    │
    ▼
Result goes through pipeline.redact() again before returning to agent
```

### Session Export/Import (Ghostwriter)

```
session.export() → SessionSnapshot (token map without key)
    │
    ▼
Ghostwriter serializes → base64 JSON blob
    │
    ▼
Laravel encrypts and stores blob
    ... later ...
Laravel decrypts blob
    │
    ▼
Ghostwriter deserializes → SessionSnapshot
    │
    ▼
Session::import(snapshot) → Session (fresh key, restored map)
    │
    ▼
session.restore(token) works against imported map
```

---

## Error Handling

### Core Errors

- **`DetectionError`** — a detector failed (regex compile error, NER model load failure). Pipeline continues with remaining detectors. Logged via auditor. Non-fatal by default. Configurable via builder: `.on_detector_error(FailStrategy::Continue | FailStrategy::Abort)`.
- **`SessionError`** — mlock failure is non-fatal (degrades to unlocked memory with warning). Key generation failure is fatal.
- **`AuditError`** — auditor write failure is non-fatal. Redaction still proceeds. Error logged to stderr.

### Fail-Closed Principle

Core's contract: "I anonymize what detectors find." If detectors find nothing, core returns the document unchanged. The responsibility for comprehensive detection lies with the consumer's detector configuration, not with core.

Consumers that want fail-closed behavior use `DefaultRule::new(Action::Redact)` — any unclassified text gets redacted rather than passed through.

### Span Conflicts

When multiple detectors find overlapping spans:
- Longest span wins.
- On exact-length tie, first detector in the stack wins.
- Deterministic and predictable — detector ordering in the builder matters.

### Restore on Unknown Token

`session.restore(token)` returns `Option<String>`. Consumer decides behavior:
- Debug-proxy: rejects the query with an error.
- Ghostwriter: emits a warning, leaves the token as-is in restored text.

---

## Memory Hygiene

Carried from v0.1, enforced in core:

- **`SessionKey`** — 32 random bytes in a `SecretBox`. `mlock` on allocation (strict failure on key, best-effort on map). `zeroize` on drop. `MADV_DONTDUMP` on the memory page.
- **`RawDocument`** — intentionally not `Serialize`. Enforced by trybuild compile-fail test. Only `CleanDocument` can cross the consumer boundary.
- **`SessionSnapshot`** — contains the token map but not the key. Safe to serialize. Key material never leaves `Session`.
- **Audit log** — never stores raw values or token-to-value mappings. The audit trail itself must not become a re-identification vector (EDPB Guidelines 01/2025).

---

## Testing Strategy

### Core Unit Tests

- Each `Detector` impl: known inputs → expected `Detection` spans (byte offsets + PiiClass).
- Each `Rule` impl: given PiiClass + Context → expected Action.
- Pipeline integration: detector stack + rule chain → RawDocument in → CleanDocument out.
- Session: token determinism (same input → same token within session), bidirectional restore, export/import roundtrip.
- Span merging: overlapping detections resolved correctly (longest wins, first on tie).
- Type safety: `RawDocument` not serializable (trybuild compile-fail test).
- Idempotency: `pipeline.redact(clean_output)` produces identical output (no double-tokenization).

### Consumer Integration Tests

- **Debug-proxy:** real MySQL (test container or local) → adapter → pipeline → verify no PII in CleanDocument. Canary test (seeded PII, assert never appears in output).
- **Ghostwriter:** sanitize → export blob → import blob → restore → exact match with original. Customer-specific index detector catches order IDs and song titles.

### Property Tests

- Any string through `pipeline.redact()` is idempotent.
- For `Action::Tokenize`: `session.restore(token)` always recovers the original value.
- No raw PII substring appears in `CleanDocument` serialization.

---

## Migration Strategy

### Phase 1: Create Reference Copy

Move current `src/` and `tests/` to `old/`. Read-only reference, not compiled. Delete workspace-level `[[bin]]` and `[lib]` entries that pointed to `src/`.

### Phase 2: Build `crates/gaze`

Core types, traits, Pipeline builder. RegexDetector, WorkaDetector, SqliteAuditor. Session with export/import. Full test suite. No consumer code.

### Phase 3: Build `crates/debug-proxy`

Port adapters (MySQL, Laravel log, SSH tunnel) referencing `old/` for logic. Port TOML policy → Rule + Detector construction. Port MCP handlers to use `pipeline.redact()`. Port CLI. Canary e2e test passes with consistent cross-channel pseudonyms.

### Phase 4: Rebuild `crates/ghostwriter`

ContextDetector for known customer data. IndexDetector for domain-specific values (order IDs, songs, artists). Session export/import for blob format. CLI preserved. Roundtrip tests pass.

### Phase 5: Delete `old/`

All tests pass. Old code no longer referenced. Remove `old/` directory.

---

## Future Considerations (Out of Scope)

Documented for context. Not designed or implemented in v0.2.

### Operations Proxy

Agents act on tokenized handles without seeing PII. `Session::restore()` resolves tokens; consumer executes the action; result goes back through `pipeline.redact()`. Combined with [nono](https://github.com/always-further/nono) for kernel-level ACL enforcement (Landlock/Seatbelt): agent has `exec` permission on gaze binary but not `read` on raw PII path. Separate brainstorming session planned.

### Pipe Mode (v0.3)

`gaze clean` / `gaze restore` subcommands reading stdin, writing stdout. Enables `mysql ... | gaze clean | agent` workflows. Core is ready — just a thin CLI consumer of Pipeline. Research doc covers buffer boundary handling (64KB blocks, carry remainder) and performance targets (100-500 MB/s regex-only).

### Multi-Stage Detection Enhancements

Per Markus's feedback and research doc:
- **Bloom filter pre-filter** — per-category bloom filters as `Detector` impl for when index grows large. False positives desirable. Only on hit, query DB for exact match.
- **Fuzzy matching** — typo-tolerant detector ("Frnk Einstein" → `<CUSTOMER_NAME>`). Levenshtein or phonetic matching.
- **SQL literal scanning** — extract string literals from WHERE clauses via `sqlparser-rs`, run through pipeline. Catches PII in query text, not just result sets.

### Compositional Attack Defense

Per-session query budgets, minimum result-set cardinality (k≥5), cross-query narrowing-predicate detection. Consumer-level concern (debug-proxy). Research doc section 6 has threat model.

### Format-Preserving Output

`Action::FormatPreserve` generates structurally valid fake values from HMAC hash (email → fake email, IP → fake IP). Improves agent reasoning quality (NoPII research: 91-96% preserved vs 54-68% for `[REDACTED]`). Does not require FPE/FF1 — deterministic fake generation suffices.

### Quasi-Identifier Suppression

Configurable generalization of attributes (city → region, exact age → age range) to prevent inference-driven re-identification. Addressed by `Action::Generalize` variant — implementation of generalization strategies is v0.3+.

---

## References

- Gaze v0.1 design: `docs/superpowers/specs/2026-04-10-gaze-design.md`
- Ghostwriter v0.1 design: `docs/superpowers/specs/2026-04-11-ghostwriter-sanitization-design.md`
- Privacy research: `docs/research/privacy-conformant-agent-patterns.md`
- v0.2 reframe notes: `docs/research/gaze-v0.2-reframe.md`
- Markus's detection gap feedback: project memory `project_markus_feedback.md`
- Operations proxy concept: project memory `project_operations_proxy_idea.md`
- nono agent sandbox: `https://github.com/always-further/nono`
