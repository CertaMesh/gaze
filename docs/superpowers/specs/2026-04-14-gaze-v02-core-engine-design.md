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

Restructure Gaze into a workspace with **two crates** for v0.2:

1. **`crates/gaze`** — channel-agnostic redaction engine. Pure library. No I/O, no protocol knowledge.
2. **`crates/debug-proxy`** — MCP server for AI agents debugging production MySQL + Laravel logs. Consumer of gaze core.

**`crates/ghostwriter`** stays where it is and migrates its internals to consume `gaze` core, but the binary/CLI surface is preserved. A third crate split happens in v0.3 when pipe-mode arrives — at that point we have three proven consumer patterns to factor. Splitting earlier is architecture astronautics.

Current `src/` is deleted in a clean-room rewrite. A `v0.1-final` git tag preserves the reference point; `git show v0.1-final:src/foo.rs` provides on-demand lookup. No in-tree `old/` — it invites copy-paste porting and re-inheriting v0.1's coupled architecture.

## Threat Model

See `docs/research/gaze-threat-model.md`. Core design decisions below reference adversary IDs (A1 curious LLM provider, A2 malicious agent, A3 on-path/at-rest, A4 supply chain, A5 prompt injection into detection).

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
Cargo.toml              (workspace root — src/ deleted, v0.1-final tag preserves history)
crates/
  gaze/                  core engine
    src/
      lib.rs
      pipeline.rs        Pipeline builder + execution, Unicode normalization pre-pass
      session.rs         SessionKey, SessionMap, Scope, export/import (signed opaque bytes)
      detector/
        mod.rs           Detector trait
        regex.rs         RegexDetector
        ner.rs           NER-backed detector (lib TBD — see open question)
        normalize.rs     Unicode NFC + ZWJ/ZWNJ strip + full-width → ASCII
      rule/
        mod.rs           Rule trait, Action enum
        column.rs        ColumnRule
        class.rs         ClassRule
        default.rs       DefaultRule
      redaction_log/     (renamed from "audit" — see Auditor contract below)
        mod.rs           RedactionLogger trait, RedactionEntry
        sqlite.rs        SqliteLogger
      types.rs           RawDocument, CleanDocument, PiiClass, Detection, Context, Value
      sandbox/           (v0.5; trait-shape landed in v0.2 for forward compat)
        mod.rs           Sandbox trait
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

Three extension points, all trait-based and stackable. All are `Send + Sync + 'static` so `Pipeline: Send + Sync + Clone` via `Arc`.

```rust
/// Finds PII in text. Multiple detectors run in sequence, spans merged.
trait Detector: Send + Sync {
    fn detect(&self, input: &str) -> Vec<Detection>;
}

/// Decides what to do with detected PII. First matching rule wins.
trait Rule: Send + Sync {
    fn action(&self, class: &PiiClass, context: &Context) -> Action;
}

/// Receives redaction-log entries. Every redaction decision is logged —
/// including losers of span conflicts, for detection-QA.
/// MUST NOT store raw values or token↔value pairs (Art.30 / EDPB 01/2025).
trait RedactionLogger: Send + Sync {
    fn log(&self, entry: &RedactionEntry) -> Result<()>;
}
```

Note the rename: `Auditor` → `RedactionLogger`. The previous name implied GDPR Art.30 processing-record compliance, which this log does *not* provide (no purpose, data-subject category, retention). Consumers who need Art.30 records must build that on top. This log is operational — what was redacted, when, by which detector.

Consumers compose these via the builder to construct their pipeline. Core provides common implementations; consumers bring domain-specific ones.

#### Pipeline

Entry point. Built via builder, immutable after construction.

```rust
let pipeline = Pipeline::builder()
    .detector(RegexDetector::new(patterns))
    .detector(IndexDetector::new(customer_data))
    .detector(NerDetector::new())
    .rule(ColumnRule::new("email", Action::Tokenize))  // Tokenize default; FormatPreserve opt-in per class (leaks structure)
    .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
    .rule(DefaultRule::new(Action::Redact))
    .redaction_logger(SqliteLogger::new(path))
    .build()?;
```

**Detector ordering is load-bearing.** Span-conflict resolution is deterministic (longest span wins, first-in-builder on exact-length tie). When two detectors overlap with *different* `PiiClass`, the loser is still emitted to the `RedactionLogger` as a `conflict` entry — free detection-QA signal.

**Unicode normalization** runs before the detector stack: NFC, zero-width-joiner/non-joiner strip, full-width → ASCII. Mitigates A5 (prompt injection into detection path) — attacker can't hide `K​r​i​s​h​a​n` with ZWJ chars. Configurable via `Pipeline::builder().normalize(NormalizeConfig::default())`; default is *on*.

Two operations:

```rust
let clean: CleanDocument = pipeline.redact(&session, raw_document)?;
let raw: Option<String>  = session.restore(token);
```

**Concurrency contract:** `Pipeline` is `Send + Sync + Clone` (internally `Arc`-wrapped). `Session` is `Send + Sync` and shared via `Arc<Session>`. Internal map uses `DashMap` (sharded) — multiple `pipeline.redact()` calls against the same session from concurrent tokio tasks are supported without external locking. A concurrent-redact test gates this contract in CI.

#### Session

Owns the HMAC key and bidirectional token map. Created by core, opaque to consumers.

```rust
let session = Session::new(Scope::Conversation("msg-42".into()))?;
let blob: SensitiveSnapshot = session.export()?;        // signed opaque bytes
let session = Session::import(blob)?;                    // verifies signature, new key, restored map
```

**Scope** is explicit, not implicit:

```rust
enum Scope {
    Ephemeral,                       // single call; snapshot never exported
    Conversation(ConversationId),    // bounded lifetime, consumer-provided id
    Persistent { ttl: Duration },    // long-lived; TTL mandatory, enforced on import
}
```

Why explicit scope: cross-session unlinkability is a contract, not an accident. Markus / counselors flagged that "session per agent interaction" is under-specified — different scopes imply different compositional-attack surfaces and different blob lifetimes. Consumers declare intent.

`Session::new()` generates a random 32-byte key in a `SecretBox`, mlocks the memory page (best-effort; on macOS dev without `ulimit -l` raised, degrades to unlocked with warning — a `--allow-unlocked-key` flag exists for containers and dev), zeroize-on-drop, `MADV_DONTDUMP`.

**Export semantics — corrected.** `Session::export()` returns `SensitiveSnapshot(Vec<u8>)` — an opaque signed byte string, NOT a structured type. The bytes contain:

1. **Version byte** — wire-format lock-in; lets us rotate format without breaking stored blobs.
2. The bidirectional token map (without the key).
3. An HMAC signature over the map, produced with the about-to-be-dropped key.

Consumers cannot inspect, log, or partially use the snapshot — they serialize the opaque bytes and hand them to storage. `Session::import()` verifies the signature (detects tampering: A3) before reconstituting the map under a fresh key.

**The snapshot is as sensitive as the raw PII it references.** Possession = full recovery. Consumers MUST encrypt the blob at rest and in transit (AEAD envelope; Laravel integration uses `APP_KEY` via Laravel's Crypt facade). Core does not implement the AEAD envelope — encryption is a deployment concern with consumer-specific KMS choices — but core signs the payload so tampering is detectable regardless of the storage layer.

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
    Custom(CustomPiiClass),  // domain-specific — see below
}

/// Normalized wrapper. Constructed only via `PiiClass::custom("order_id")`
/// which lowercases + trims + snake_case-normalizes. Prevents
/// Custom("order_id") vs Custom("ORDER_ID") vs Custom("orderId") divergence
/// silently breaking Rule matches.
struct CustomPiiClass(String);

impl PiiClass {
    pub fn custom(name: &str) -> Self { /* normalize → CustomPiiClass */ }
}

/// What to do with detected PII.
enum Action {
    Tokenize,        // session-scoped HMAC pseudonym (Person_7) — DEFAULT for sensitive classes
    Redact,          // [REDACTED] — destroys information
    FormatPreserve,  // deterministic fake (v0.3 impl). WARNING: leaks structure
                     //   (local-part length, domain distribution — NoPII-style leakage).
                     //   Opt-in per ClassRule; never a DefaultRule target.
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
- `NerDetector` — NER-based. Backed directly by **`ort` + `tokenizers`** with **`Davlan/bert-base-multilingual-cased-ner-hrl`** exported to ONNX and mounted as a pinned local artifact. Upgrade paths (stacked DE+EN detectors, or language-routed dispatch via `whatlang`) are additive and require no pipeline architecture change.

**Rules:**
- `ColumnRule` — matches on field name. "If field is X, action is Y."
- `ClassRule` — matches on PiiClass. "If class is Email, action is Tokenize."
- `DefaultRule` — catch-all. "Everything else gets Redacted." Fail-closed.

**Redaction loggers:**
- `SqliteLogger` — append-only SQLite log. Stores: timestamp, detector source, PiiClass, action taken, field name, document type, conflict-loser (if span conflict). Never stores raw values or token mappings. Enforced by doc-test + compile-time `RedactionEntry` shape.

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
    .detector(NerDetector::new())
    .rule(ClassRule::new(PiiClass::custom("customer_name"), Action::Tokenize))
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
    ├─ RedactionLogger: detector source, PiiClass, action, field name, conflict-losers
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
session.export() → SensitiveSnapshot(Vec<u8>)
    │  (version byte + serialized map + HMAC signature under session key)
    │  session key dropped immediately after signing
    ▼
Ghostwriter treats bytes as opaque, wraps in base64
    │
    ▼
Laravel AEAD-encrypts (APP_KEY via Crypt) and stores blob
    ... later ...
Laravel decrypts blob
    │
    ▼
Ghostwriter hands opaque bytes to Session::import
    │
    ▼
Session::import(bytes) → verifies signature, reconstitutes map under fresh key
    │  on signature mismatch → error (tampering detected; A3)
    ▼
session.restore(token) works against imported map
```

---

## Error Handling

### Core Errors

- **`DetectionError`** — a detector failed (regex compile error, NER model load failure). Pipeline continues with remaining detectors. Logged via RedactionLogger. Non-fatal by default. Configurable via builder: `.on_detector_error(FailStrategy::Continue | FailStrategy::Abort)`.
- **`SessionError`** — mlock failure is non-fatal on macOS dev (ulimit -l default too small); degrades to unlocked memory with warning. Key generation failure is fatal. Signature verification failure on `Session::import` is fatal (A3 tampering).
- **`RedactionLogError`** — logger write failure is non-fatal. Redaction still proceeds. Error logged to stderr.

### Fail-Closed Principle

Core's contract: "I anonymize what detectors find." If detectors find nothing, core returns the document unchanged. The responsibility for comprehensive detection lies with the consumer's detector configuration, not with core.

Consumers that want fail-closed behavior use `DefaultRule::new(Action::Redact)` — any unclassified text gets redacted rather than passed through.

### Span Conflicts

When multiple detectors find overlapping spans:
- Longest span wins.
- On exact-length tie, first detector in the stack wins.
- Deterministic and predictable — detector ordering in the builder matters.

### Restore on Unknown Token

`session.restore(token)` returns `Option<String>`. Behavior is **phase-dependent** and explicitly contractual:

- **Read-phase** (ghostwriter restoring LLM output text): may be lax. Emit warning, leave token as-is. Rationale: LLM paraphrased `Person_7` → `User_7`; the user still gets readable text.
- **Action-phase** (operations proxy; v0.5): **fail-closed**. Unknown token ⇒ abort action with error. Rationale: executing `send_email(User_7)` where `User_7` is an LLM hallucination could exfiltrate to an unintended recipient, escalate privilege, or corrupt state. Must never silently pass.

Core provides both `restore` (lax) and `restore_strict` (fail-closed); consumers pick per call site.

---

## Memory Hygiene

Carried from v0.1, enforced in core:

- **`SessionKey`** — 32 random bytes in a `SecretBox`. `mlock` on allocation (strict failure on key, best-effort on map). On macOS dev the default `ulimit -l` is tiny (64KB); sessions larger than that degrade to unlocked memory with a warning. Dev doc must call this out and point to `ulimit -l unlimited` or the `--allow-unlocked-key` flag. `zeroize` on drop. `MADV_DONTDUMP` on the memory page.
- **`RawDocument`** — intentionally not `Serialize`. Enforced by trybuild compile-fail test. Only `CleanDocument` can cross the consumer boundary.
- **`SensitiveSnapshot`** — opaque signed byte string. Contains the token map but not the key; the signature binds the payload to the session's creation event. **As sensitive as raw PII** — consumers MUST encrypt at rest and in transit. Version byte inside enables future crypto rotation without breaking stored blobs.
- **Redaction log** — never stores raw values or token-to-value mappings. The log must not become a re-identification vector (EDPB Guidelines 01/2025). Structural enforcement: the `RedactionEntry` type has no field that can hold raw PII; a doc-test verifies this.

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

### Phase 0: Decision Gate

Before cutting code, resolve open items that affect architecture:

1. **NER library choice** — resolved in `docs/research/ner-library-evaluation.md`. Adopt direct **`ort` + `tokenizers`** integration for `NerDetector`. Default model: **`Davlan/bert-base-multilingual-cased-ner-hrl`** (mBERT, 10 high-resource languages incl. German + English, CoNLL schema) exported to ONNX and mounted as a pinned local artifact. Upgrade paths (stacked DE+EN detectors, or language-routed dispatch via `whatlang`) are additive and require no architecture change. Drop `worka-ai/pii`. Encoderfile-sidecar packaging deferred to v0.3+.
2. **Threat model review** — `docs/research/gaze-threat-model.md` reviewed and adopted.
3. **Verify** — unit-test `redact-*` returns byte spans (not char offsets) on German umlaut / emoji text *before* building the wrapper.

### Phase 1: Freeze v0.1

Tag current `main` as `v0.1-final`. Push tag. Delete `src/` and `tests/` (the top-level monolith) in the same commit that lands the v0.2 workspace skeleton — no `old/` directory. Historical reference via `git show v0.1-final:src/foo.rs`.

### Phase 2: Build `crates/gaze` + Port Canary First

1. Workspace skeleton, `crates/gaze` empty.
2. **Port the canary e2e test (76e55b7) before any detector code.** The canary is the strongest leak guard we have; everything else builds against it.
3. Core types, traits, Pipeline builder. Unicode normalization. Span-conflict resolution with loser-logging. `RegexDetector`, `NerDetector` (backing lib per Phase 0 decision), `SqliteLogger`. `Session` with `Scope`, signed `SensitiveSnapshot`, `restore` / `restore_strict`. Concurrent-redact test. Full test suite. No consumer code.

### Phase 3: Build `crates/debug-proxy`

Adapters (MySQL, Laravel log, SSH tunnel) written clean-room against the v0.1 tag for reference. TOML policy → Rule + Detector construction. MCP handlers use `pipeline.redact()` with shared `Session` across channels. Error-path sanitization (stderr / DB errors quoting values go through `pipeline.redact`). Canary e2e passes with consistent cross-channel pseudonyms.

### Phase 4: Migrate `crates/ghostwriter` onto Core

Rip out ghostwriter's internal detection code. Add `ContextDetector` (known customer data) and `IndexDetector` (order IDs, songs, artists) as consumer-side detectors — both implement the core `Detector` trait. Update blob format to use `SensitiveSnapshot`. CLI surface preserved; existing roundtrip tests pass. Ghostwriter stays a separate crate but shares core.

### Phase 5: Sandbox Trait Landing

Land the `Sandbox` trait shape in core (no impls yet — v0.5 delivers birdcage-default and nono-upgrade impls). Shape must be pluggable; no direct `nono::*` dependency in core. Documents the v0.5 argv/env trust boundary (agent-controlled inputs to `gaze exec` are untrusted; validation is core's job, not the sandbox's).

---

## Open-Issue Acceptance Matrix

Every open GitHub issue is explicitly mapped to a v0.2 deliverable or deferred with reason. No silent drops.

| Issue | Topic | Status in v0.2 |
|-------|-------|---------------|
| #1 | k-anonymity | **Deferred to v0.3.** Consumer-level concern (debug-proxy policy). Threat-model A2 documents the gap. |
| #2 | Per-session query budget | **Deferred to v0.3.** Same as #1. |
| #3 | Audit-log reversibility | **Closed by v0.2 design.** `RedactionEntry` is structurally incapable of holding raw values; doc-test enforces. Rename from "audit" to "redaction log" removes the false Art.30 implication. |
| #4 | `typed_terms` for ghostwriter | **In scope for v0.2.** Covered by `IndexDetector`. |
| #5 | Date-shift trade-off doc | **In scope for v0.2.** Brief doc in `Action::Generalize` section + future-considerations. |
| #6 | Ghostwriter language config | **In scope for v0.2.** Pipeline builder accepts per-detector language config; NerDetector takes a locale. |

## References to Other Design Changes

- **Sandbox backend choice** — nono is no longer the default. `Sandbox` trait with pluggable backends; birdcage as conservative default (Phylum, production-used, cross-platform, deny-all network); nono as upgrade target once it hits 1.0. Windows-compatible backend (Tauri future) designed as a third impl. No `nono::*` in core.
- **NER library** — resolved in Phase 0 research. `NerDetector` is backed directly by `ort` + `tokenizers` with pinned local ONNX artifacts; no `worka-ai/pii` dependency remains in scope for v0.2.
- **Audit → Redaction Log** — rename throughout. Don't claim GDPR Art.30 compliance from this artifact; it's an operational trail.

---

## Future Considerations (Out of Scope)

Documented for context. Not designed or implemented in v0.2.

### Operations Proxy

Agents act on tokenized handles without seeing PII. `Session::restore_strict()` resolves tokens (fail-closed on unknown); consumer executes the action; result — **including stderr and structured errors** — goes back through `pipeline.redact()` before returning to the agent. DB constraint violations quoting values, SMTP bounces containing raw addresses, shell error output: all must be sanitized. The "agent never sees raw output" invariant only holds if the error path is included.

Kernel-level ACL enforcement via the `Sandbox` trait (Landlock/Seatbelt via birdcage default; nono once 1.0). Agent has `exec` on the gaze binary but not `read` on raw PII paths. Because gaze runs outside the sandbox boundary ("trusted"), every input it receives from the agent (argv, env, stdin) is untrusted and validated: script-path allowlist, shell-metachar rejection, env-var allowlist. Separate v0.5 brainstorming session will expand this.

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
- Threat model: `docs/research/gaze-threat-model.md`
- First-principles vision: `docs/research/gaze-first-principles-vision.md`
- Privacy research: `docs/research/privacy-conformant-agent-patterns.md`
- v0.2 reframe notes: `docs/research/gaze-v0.2-reframe.md`
- Counselors review (2026-04-15): `agents/counselors/1776242804-review-request-gaze-v02-design-first-pr/claude-opus.md`
- Markus's detection gap feedback: project memory `project_markus_feedback.md`
- Operations proxy concept: project memory `project_operations_proxy_idea.md`
- Sandbox candidates: birdcage `https://github.com/phylum-dev/birdcage`, nono `https://github.com/always-further/nono`
