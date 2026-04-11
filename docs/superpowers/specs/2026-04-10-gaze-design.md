# Gaze — GDPR-Compliant Data Proxy for AI Agents

**Status:** Design / Pre-v0.1
**Author:** Krishan Koenig
**Date:** 2026-04-10

---

## Problem

AI coding agents (Claude Code, Cursor, Copilot CLI, Gemini CLI) increasingly need access to production data sources — databases, application logs, error trackers — to debug issues effectively. Under DSGVO/GDPR, sending personal data to external LLM APIs constitutes third-party data processing and triggers strict legal obligations.

Current options force a binary choice:

1. **Give the agent raw production access** → GDPR violation risk; PII enters LLM context.
2. **Don't use agents for debugging** → lose significant productivity.

No existing tool combines inline anonymization, policy-driven governance, multi-source access (DB + logs), and developer-first UX in an MCP proxy.

## Core Insight

An LLM agent cannot be trusted to follow prompts deterministically. Even with a system prompt that says "only query metadata," the agent could still issue `SELECT * FROM users`. Once PII enters the LLM context window, it has been "processed" under GDPR — anonymization after the fact is too late.

**Therefore: the agent must never have direct access to production data. All access must go through a deterministic, non-agentic gate that enforces policy and anonymizes before returning results.**

Gaze is that gate.

## Goal

A single Rust binary that:

1. Sits between AI agents and production data sources
2. Exposes structured MCP tools (not raw SQL)
3. Anonymizes all responses before they reach the agent
4. Audits every access deterministically
5. Provides a TOML policy file that a Datenschutzbeauftragter can review

The v0.1 target is **dogfoodable on Krishan's own Laravel/MySQL projects (Artistfy, Sandorian)** — the first user is the author.

## Vision (beyond v0.1)

Gaze is designed to grow through three phases on the **same codebase**:

1. **v0.1 — MCP mode:** Rust CLI + stdio MCP server. AI coding agents use it for debugging.
2. **v0.2 — Pipe mode:** Same Rust binary exposes `gaze clean` / `gaze restore` subcommands that read stdin and write stdout. Production apps (e.g., a Laravel app anonymizing inbound emails before passing them to an LLM feature) shell out to Gaze to sanitize content on the way in and restore PII on the way out. No HTTP server, no daemon, no port — plain Unix pipes. The host app handles confidentiality in flight (e.g., Laravel wraps the session blob in `Crypt::encryptString` before queueing).
3. **vFuture — Native desktop app:** Tauri-based macOS app (Raycast/Herd-style) combining manual debugging UI with agentic capabilities. Same Rust core, new frontend.

**Architectural rule:** the anonymizer is transport-agnostic from day one. `Anonymizer::clean()` and `Anonymizer::restore()` are both functional v0.1 requirements (see the Type-Safety Boundary section for why `restore()` is load-bearing even in MCP mode). Adding pipe mode in v0.2 = new CLI subcommands + a serialization format for the session blob, zero anonymizer rewrites. Never web UI — future GUI is native desktop.

**Why pipe mode, not HTTP:** An HTTP server would add auth, port management, TLS, a daemon lifecycle, and a network attack surface — all for a tool that runs on the same box as the host app. Stdin/stdout is simpler, smaller, easier to integrate, and has zero attack surface beyond the process boundary. Under high throughput, Gaze can gain a `gaze serve --stdio` daemon mode (Unix socket) without changing the CLI contract. Not a v0.1 or v0.2 concern.

## Non-Goals

- **Not a general PII library** — Gaze depends on Worka PII for detection primitives.
- **Not a query builder** — agents use structured filter objects, never raw SQL.
- **Not multi-tenant** — one Gaze process = one project = one developer.
- **Not a pentesting tool** — Gaze prevents PII from entering the LLM context; it does not try to break its own anonymization.
- **Not a replacement for read-only DB credentials** — defense-in-depth still requires limiting the DB user Gaze connects with.
- **Not a reviewer of agent reasoning** — what the agent does with the clean data is its own problem.

## Legal Basis

**Disclaimer:** This section is engineering analysis, not legal advice. Customers should validate with their own Datenschutzbeauftragter (DSB) or counsel.

### What Gaze is (and isn't) under GDPR

Gaze is a **dual-anchor technical measure** under GDPR:

- **Art. 32 — Security of processing.** Gaze implements technical and organizational measures (pseudonymization of output, audit log, deterministic policy enforcement, memory hygiene) that reduce the risk of personal data reaching third-party LLM subprocessors.
- **Art. 25 — Data protection by design and by default.** Gaze is the textbook shape of privacy-by-design: the architecture itself enforces minimization rather than relying on after-the-fact scrubbing. A developer using Gaze cannot accidentally send raw PII to an LLM because the only path to the LLM runs through the anonymizer. Art. 25 is a stronger regulatory hook than Art. 32 alone — it maps to a proactive design obligation, not a reactive security obligation, and DSBs explicitly look for it in DPIA review.

Claiming both is intentional: Art. 25 is the architectural story, Art. 32 is the operational story. Gaze does **not** exempt customers from GDPR — it reduces the scope of what obligations cover.

Earlier drafts of this spec claimed session-scoped anonymization placed Gaze output "outside GDPR scope" under Recital 26 and therefore eliminated DPA / Verarbeitungsverzeichnis requirements. That framing was wrong and has been removed:

- **Reading prod DB/logs into RAM is processing** under Art. 4(2), regardless of whether the output is later anonymized. The input step does not vanish.
- **Session-scoped HMAC tokens are pseudonymous, not anonymous, while the session is live.** Gaze holds the key and the mapping in memory. Recital 26's "means reasonably likely to be used" test fails for any operator with process access. Post-exit, the data becomes unlinkable — but the regulatory question is about the processing event, not its aftermath.
- Persistent consistent tokens across records (which debugging requires) are **pseudonymous** under WP29/EDPB guidance, not anonymous.

### What customers actually need

Click-through DPAs with major LLM vendors are solved infrastructure in 2026:

| Vendor | DPA |
|---|---|
| Anthropic | Auto-integrated into Commercial Terms; SCCs included |
| OpenAI | Click-through via Services Agreement; EEA customers contract via OpenAI Ireland Ltd (within EEA) |
| Google, Microsoft, Mistral | Same click-through pattern via their respective commercial terms |

Customers using Gaze with any of these vendors already have a valid Art. 28 processor contract. **Gaze does not replace the DPA** — it works alongside it.

What Gaze **does** remove from the customer's compliance burden (compared to sending raw PII to the same LLM vendors):

| GDPR obligation | Raw PII path | With Gaze |
|---|---|---|
| Art. 28 DPA with LLM vendor | Click-through (trivial either way) | Click-through |
| Art. 6 lawful basis + balancing test | Required, complex justification | Required, substantially simplified — the subprocessor receives only pseudonymized, minimized content, which dramatically strengthens the legitimate-interest balancing test |
| Art. 5(1)(c) data minimization defense | Hard once Gaze exists in the ecosystem (DSB: "a minimization tool was available — why didn't you use it?") | Gaze IS the evidence of minimization |
| Art. 30 Verarbeitungsverzeichnis entry | Required, complex categorization | Required, one-line entry |
| Art. 35 DPIA | Often triggered by "new tech + production data" — 1–2 days + possible DSB consultation | Strengthens the DPIA risk-mitigation section and may reduce the overall risk rating; whether a DPIA is required at all depends on the broader processing activity, not solely on the Gaze layer. Confirm with DSB. |
| Art. 13/14 privacy policy disclosure | "We send your data to Anthropic/OpenAI" (customer-hostile) | "Anonymized debugging snippets via Gaze" (soft) |
| B2B customer DPAs with "no AI subprocessor" clauses | Often blocks you entirely | Usually satisfiable — the subprocessor receives minimized, pseudonymized content, so "no PII transmitted" clauses typically apply differently. Clause-by-clause reading still required. |
| DSAR disclosure of recipients (Art. 15) | Must list LLM vendor as recipient of personal data | Vendor should still be disclosed; the disclosure can honestly characterize transmitted content as pseudonymized and minimized rather than as raw personal data |

**Harmonization note.** Earlier drafts of this table used phrases like "no PII transmitted to subprocessor" in several rows. That framing is inconsistent with the "pseudonymous while the session is live" position taken elsewhere in this section — the LLM receives *pseudonymized* content, which is still personal data from the controller's perspective even if it is not directly identifying for the recipient. The rows above have been harmonized to use "pseudonymized and minimized" consistently. A DSB should not be able to find "no PII" in one cell and "pseudonymous" three pages later.

**Typical one-time compliance work for a small EU business deploying Gaze:** one legitimate-interest balancing test, one Verarbeitungsverzeichnis entry, one privacy-policy disclosure line, plus the `policy.toml` itself. Often reduced from several days of work to a handful of hours — the exact reduction depends on the DSB's reading of the specific processing activity and should not be promised in advance.

### Why This Architecture Is Defensible

The LLM agent never has direct DB or log access. The proxy is deterministic Rust code with compile-time type-safety guarantees that PII cannot cross the sanitization boundary. The DSB does not need to trust an LLM — they review:

1. A TOML policy file
2. A structured audit log
3. The open source of a Rust type system that enforces `RawRow` → `CleanRow` via the anonymizer
4. Evidence that Gaze's session key and in-memory mapping are protected against swap/core-dump leaks (`mlock` + `MADV_DONTDUMP`, documented below)

This is the same trust model as any other deterministic data processor, which regulators understand.

### What Gaze Does NOT Do

To prevent the old "skip compliance" framing from creeping back, explicit non-claims:

- Gaze does **not** eliminate Art. 30 Verarbeitungsverzeichnis obligations.
- Gaze does **not** eliminate the need for a DPA with the LLM vendor (though click-through DPAs make this trivial).
- Gaze does **not** eliminate the need for an Art. 6 lawful basis for processing production data.
- Gaze does **not** guarantee zero residual PII in output — the safety net is defense in depth, not a silver bullet. The customer is responsible for reviewing safety-net warnings and tightening `policy.toml`.
- Gaze does **not** provide legal advice. The DSB and the customer's counsel remain the authorities on what the customer's specific deployment requires.

### EU AI Act

Gaze is a debugging/data-access tool, not a high-risk AI system under Annex III. No special AI Act obligations apply beyond ordinary GDPR.

## Core Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| **Name** | Gaze | "Looking at data without touching it." Clean in Go/Rust namespaces. |
| **Language** | Rust | Unified path to Tauri desktop app. Type system enforces anonymization safety. Official MCP SDK (`rmcp`) exists. |
| **Anonymization mode** | Session-scoped pseudonymous | HMAC key lives in RAM only, discarded on process exit. Pseudonymous while the session is live (Gaze holds the key + mapping); becomes unlinkable after exit. Consistent within one session so the agent can correlate records. Key material protected via `mlock` + `MADV_DONTDUMP`. |
| **Access model** | Everything accessible, everything anonymized | Not allowlist-based. Agent can query any table or log. All data flows through the anonymizer regardless. |
| **Policy model** | Auto-scan → human review → runtime safety net | `gaze init` scans the DB, flags likely PII columns, generates a draft `policy.toml`. Human/DSB reviews. Runtime safety net catches what column rules missed. |
| **Query model** | Structured MCP tools only | No raw SQL. Agent uses `db.schema`, `db.sample`, `db.count`, `db.distinct`. Proxy generates parameterized SQL internally. |
| **v0.1 DB adapter** | MySQL (not Postgres) | Dogfooding on Laravel/Ploi stack which uses MySQL. |
| **v0.1 log adapter** | Laravel log files | `search`, `tail`, `context` by request ID. |
| **Anonymization dependency** | Worka PII crate + `fake` crate | Worka PII = Rust-first deterministic PII detection + anonymization library with stable offsets, policy-driven operators, audit-friendly outputs. |
| **Transport** | stdio MCP via `rmcp` | v0.1 is MCP only. v0.2 adds **pipe mode** (`gaze clean` / `gaze restore` subcommands reading stdin / writing stdout) for host-app integration — **not** an HTTP server. See "Pipe Mode Preview (v0.2)". |
| **License** | Deferred (all rights reserved) | Krishan considering commercialization. Decide model after dogfooding. |

## Architecture

### Project Layout

```
gaze/
├── Cargo.toml
├── src/
│   ├── main.rs                → CLI entry (Clap)
│   ├── cli/
│   │   ├── init.rs            → gaze init (onboarding scanner)
│   │   ├── check.rs           → gaze check (policy validation)
│   │   ├── serve.rs           → gaze serve (starts MCP server)
│   │   └── audit.rs           → gaze audit (query audit log)
│   │
│   ├── mcp/
│   │   ├── server.rs          → MCP server (rmcp crate, stdio transport)
│   │   └── tools.rs           → Tool handler registry (db.*, logs.*)
│   │
│   ├── adapter/
│   │   ├── mod.rs             → DatabaseAdapter / LogAdapter traits
│   │   ├── mysql.rs           → MySQL implementation (sqlx)
│   │   └── laravel_log.rs     → Laravel log file parser
│   │
│   ├── policy/
│   │   ├── mod.rs             → Policy engine
│   │   ├── parser.rs          → TOML parsing + validation
│   │   └── classifier.rs      → Column PII classification
│   │
│   ├── anon/
│   │   ├── mod.rs             → Anonymizer orchestrator
│   │   ├── session.rs         → Session-scoped key + mapping management
│   │   ├── replacer.rs        → Per-type replacement strategies (uses `fake`)
│   │   └── detector.rs        → Safety net (uses Worka PII)
│   │
│   ├── scanner/
│   │   └── mod.rs             → DB introspection + auto-classification for `gaze init`
│   │
│   ├── audit/
│   │   └── mod.rs             → SQLite audit logger (rusqlite)
│   │
│   └── types.rs               → RawRow / CleanRow (type-safe PII boundary)
│
├── tests/
│   ├── fixtures/              → Known inputs + expected outputs
│   ├── ui/                    → Compile-fail tests for type safety
│   └── e2e/                   → MySQL testcontainer tests
│
└── policy.example.toml
```

### Data Flow

```
Agent → MCP tool call
      → Policy check (env exists, filter columns valid)
      → Adapter executes query (parameterized SQL)
      → Raw result wrapped in RawRow
      → Anonymizer::clean(RawRow, schema) → CleanRow
            ├── Layer 1: column rules from policy.toml
            ├── Layer 2: Worka PII safety net (entity detection)
            └── Layer 3: regex patterns (log-specific)
      → Audit log entry written (no PII — metadata only)
      → CleanRow serialized to MCP response
      → Agent receives clean data
```

No shortcut paths. Every response goes through the anonymizer, even if the policy says the column is not PII (the safety net catches what column rules missed).

### Type-Safety Boundary

```rust
/// Raw data from adapters — NEVER exposed to the agent.
/// Does not implement Serialize; cannot be written to the MCP response.
pub struct RawRow(Vec<RawValue>);

/// Anonymized data — safe for the MCP response.
/// Can ONLY be constructed via `Anonymizer::clean()`.
#[derive(Serialize)]
pub struct CleanRow(Vec<CleanValue>);

impl Anonymizer {
    pub fn clean(&self, raw: RawRow, table: &str, schema: &TableSchema) -> Result<CleanRow, AnonError> {
        // Layer 1: column rules
        // Layer 2: safety net (Worka PII)
        // Layer 3: regex patterns (if log data)
    }

    /// Reverse-map a clean value back to its raw form using the in-session mapping.
    /// Functional in v0.1: used internally to translate agent-supplied filter values
    /// (e.g., `db.sample(orders, {user_id: "user_7"})`) back to real IDs before hitting MySQL.
    /// Also the foundation for pipe mode (`gaze restore`) in v0.2.
    /// Returns `Err` if the clean value is not in the current session's mapping.
    pub fn restore(&self, clean: &CleanValue) -> Result<RawValue, AnonError> {
        // reverse session-scoped mapping
    }
}
```

`RawRow` has no `Serialize` impl. If any MCP tool handler tries to return it, the code does not compile. This is the architectural guarantee that makes the GDPR argument airtight. A compile-fail test in `tests/ui/` verifies this invariant.

### Why `restore()` is load-bearing in v0.1

An earlier draft framed `restore()` as unused scaffolding for v0.2. The counselors review correctly flagged this as wrong. `restore()` is a functional v0.1 requirement because **agent-supplied filter values need reverse mapping** to work correctly across queries:

1. Agent calls `db.sample(users, {}, 10)` → gets 10 anonymized rows, one of which is `user_7@example.com` corresponding to real `krishan@example.com`.
2. Agent reasons: "I want to see orders for user_7."
3. Agent calls `db.sample(orders, {user_email: {eq: "user_7@example.com"}}, 50)`.

The filter value `user_7@example.com` does not exist in the real DB. Without `restore()`, Gaze queries MySQL for a non-existent email and returns zero rows — the agent's cross-table workflow is broken.

With `restore()`, Gaze:

1. Looks up the filter value in the current session's reverse mapping.
2. If present, substitutes the real value before generating parameterized SQL.
3. If absent (agent supplied a value not produced by this session), returns `AnonError::UnknownFilterValue` and the tool call fails with a clear message.

This also handles the Art. 5(1)(c) concern about filter-value PII exposure: **agents may only filter on values Gaze itself produced**. The agent cannot supply a raw `krishan@example.com` as a filter — that filter value is not in the session mapping and gets rejected. The audit log records both the rejected filter and the successful restored filter as separate metadata (without logging the raw value itself).

### Filter-value PII rules

To prevent filters from becoming a side channel for raw PII:

- **Filters on non-PII columns** (IDs by integer, dates, statuses, amounts): passed through verbatim.
- **Filters on PII columns**: the filter value MUST be a token produced by the current session (discovered via `restore()`), not a raw value. Internally this path uses `AnonError::RawPIIInFilter` (raw value detected) and `AnonError::UnknownFilterValue` (value not in session map) for debug logs and `trybuild`-style tests, but both error variants are **collapsed to a single generic `InvalidFilterValue` error** on the MCP response surface and in audit log entries. Reason: a specific `RawPIIInFilter` response would confirm to an observer that "this string matches Gaze's PII detection rules" — turning error messages into a confirmation oracle. The internal variant is retained for developer debuggability; the external variant is intentionally opaque.
- **Filters with `like` operator**: disallowed on PII columns entirely (prevents character-by-character brute force via repeated queries).
- **Filter-value audit**: the audit log records the **column and operator** used, never the raw value and never the specific internal error variant. For PII columns that succeed, the log records the token and the fact that `restore()` succeeded.

## MCP Tools Exposed to the Agent

### Database tools

```
db.tables(env)                          → all table names with column types and indexes
db.schema(env, table)                   → detailed schema for one table
db.sample(env, table, filters, n)       → n anonymized rows matching filters (capped by policy)
db.count(env, table, where)             → count only (no data exposure; PII filters restricted — see below)
db.distinct(env, table, column)         → distinct values (non-PII columns by default; PII columns require explicit opt-in)
```

### Side-channel protection on `db.count` and `db.distinct`

Both tools can be turned into PII oracles if used against PII columns with arbitrary predicates. Examples of what must be blocked:

- **Binary-search brute force via `db.count`:**
  `count(users, email LIKE 'k%')` → `'kr%'` → `'kri%'` — each query narrows the search, reconstructing a real email by watching count changes.
- **Cardinality leak via `db.distinct`:**
  `distinct(users, email)` on a high-cardinality PII column returns 10,000 anonymized tokens but reveals the size of the user base and the frequency distribution (via repeated calls + set comparison).

Rules enforced by the policy engine:

1. **`db.count` with a `where` clause referencing a PII column** is rejected unless the column is listed in `[policy.count_allowed_columns]`. PII columns are allowed in `count` only for equality on a restored token (same rule as filters).
2. **`db.count` `like` operator on any column** is rejected. Agents who need pattern matching use `db.sample` with `limit = 1` to verify a single token.
3. **`db.distinct` on a PII column** is rejected unless the column is listed in `[policy.distinct_allowed_columns]`. Even then, the result is capped by `max_distinct` (default 50) and **results are randomly shuffled per call using a fresh CSPRNG draw** — specifically, `rand::rngs::OsRng` sampled at the start of every request, never seeded from the session key or any column/table identifier. Deterministic ordering (including HMAC-derived ordering) would let an agent making repeated `db.distinct` calls infer frequency by watching which tokens consistently appear first when the result is capped — a side channel flagged by round 2 review. A per-call OS-RNG shuffle breaks that inference chain while preserving the per-session token consistency (the tokens themselves stay stable across calls — only their ordering in each response varies).
4. **Rate limiting:** an optional `[policy.rate_limit]` section can cap calls per tool per session. Not enabled by default; opt-in for paranoid deployments.

These rules go into `policy.toml` and are validated by `gaze check`. The default policy produced by `gaze init` lists no PII columns in the `_allowed` lists, so agents have to make a conscious decision to widen exposure.

### Log tools

```
logs.search(env, service, timerange, level, pattern)  → anonymized log lines matching criteria
logs.tail(env, service, n)                            → last n anonymized lines
logs.context(env, request_id)                         → full anonymized trace for a request ID
```

### Deliberately absent in v0.1

- `db.explain` — requires raw SQL; agents can reason about performance from schema + cardinality via `db.schema` and `db.count`. Deferred.
- `errors.*` — no Sentry/Flare adapter yet. Deferred to v0.2.

### Structured filters

`db.sample` accepts filters as a structured object; the proxy translates them to parameterized SQL internally. Supported operators: `eq` (default), `gt`, `lt`, `gte`, `lte`, `like`, `in`. The proxy rejects any column name not present in the table schema, preventing injection via crafted column names. The agent never constructs SQL.

## Policy File

The policy file's role is an **anonymization ruleset plus connection config**, not an access allowlist. The agent can read any table; every column is either explicitly classified or caught by the safety net.

```toml
# policy.toml — the Datenschutzbeauftragter reviews THIS file
# Gaze is scoped to production access only. Exactly one [connection.*] block per file.

[connection.production]
type = "mysql"
credentials = "env:PROD_DB_URL"           # never plaintext
# Optional SSH tunnel. Gaze opens and closes it automatically — no separate `ssh -L` command.
# Values follow ~/.ssh/config aliases so agent forwarding and keys keep working.
ssh_tunnel = "deploy@prod.example.com"

[connection.production.logs]
type = "laravel"
path = "/var/log/laravel/laravel.log"

# --- Anonymization Rules ---

[anonymize]
mode = "session"                          # session-scoped anonymous (only mode in v0.1)
max_rows = 50                             # cap per db.sample call

# Columns explicitly tagged as PII (from `gaze init` scan + human review)
[anonymize.columns]
"*.email"           = "email"             # any table, column named 'email'
"*.owner_email"     = "email"
"*.phone"           = "phone"
"*.owner_name"      = "name"
"*.owner_address"   = "address"
"*.iban"            = "iban"
"*.ip_address"      = "ip"
"users.name"        = "name"              # table-specific override
"orders.notes"      = "freetext"          # free-text field — scan content inline

# Replacement types:
#   email    → user_{n}@example.com
#   phone    → +49 000 000{n}
#   name     → Person_{n}
#   address  → Musterstraße {n}, 00000 Berlin
#   iban     → DE00 0000 0000 0000 0000 {n}
#   ip       → 10.0.0.{n}
#   date     → shift by constant offset (per session)
#   freetext → run Worka PII detector, replace inline matches

# --- Safety Net ---

[safety_net]
enabled = true
patterns = [
  "email",
  "phone",
  "ip_address",
  "iban",
  "credit_card",
]
action = "anonymize_and_warn"             # anonymize + log warning to audit log

# --- Log Scrubbing ---

[anonymize.logs]
strip_patterns = [
  "Bearer [A-Za-z0-9_\\-\\.]+",           # auth tokens
  "password[=:]\\S+",                      # password values
]
```

### Onboarding Flow (`gaze init`)

```
$ gaze init
Connecting to production... ok
Scanning 47 tables, 312 columns...

Found 23 likely PII columns:
  ok users.email           → email
  ok users.name            → name
  ok users.phone           → phone
  ok orders.owner_email    → email
  ok orders.notes          → freetext (contains email-like patterns)
  ?  devices.label         → skipped (ambiguous)
  ...

Generated policy.toml with 23 anonymization rules.
Review and adjust, then run: gaze check
```

`gaze check` validates the policy against the live DB schema, flagging columns in the policy that do not exist and columns in the DB that look like PII but are not covered.

## Anonymizer Design

### Session-scoped pseudonymous mode

```
Session starts → generate random session key (32 bytes, in-memory only)
                → mlock(key) + madvise(MADV_DONTDUMP)       [strict — 32 bytes]
                → allocate mapping HashMap (best-effort mlock — see below)
                                    ↓
For each PII value encountered:
    HMAC(session_key, raw_value) → deterministic hash
    hash → mapped to fake value via consistent lookup
                                    ↓
Session ends → zeroize(key) → zeroize(mapping) → unlock what we locked
            → no re-identification possible post-exit
```

**Within a session:** `krishan@example.com` always maps to `user_7@example.com`. The agent can correlate "user_7 in `orders` = user_7 in logs."

**Across sessions:** A fresh key produces `user_23@example.com` for the same input. No continuity. After process exit the data becomes unlinkable.

### Memory hygiene

The session key and the in-memory raw→token `HashMap` are as sensitive as the raw data itself. While the process is live they are pseudonymization material — an attacker with process access can reverse tokens. Gaze hardens memory handling:

1. **`mlock` on the 32-byte session key — strict.** The key lives in a single fixed-size allocation. Locking it against swap is a one-syscall operation that Gaze treats as mandatory: if `mlock` on the key fails (e.g., `RLIMIT_MEMLOCK` too low, container capability dropped), Gaze emits a loud warning and the operator must either raise the limit or pass `--allow-unlocked-key` to continue. This is the hard guarantee.
2. **`mlock` on the `HashMap<Raw, Fake>` mapping — best-effort.** Rust's default allocator spreads `HashMap` buckets across non-contiguous heap pages, and those allocations grow as entries are inserted. There is no portable way to `mlock` "the whole HashMap" without either a custom allocator or a bespoke mmap-backed arena — both of which are heavy for v0.1. Gaze does the pragmatic thing: (a) uses the `zeroize` crate on `Drop` as the hard guarantee that mapping contents are scrubbed when the session ends, (b) attempts `mlock` on the current allocation pages with `mlockall(MCL_CURRENT)` at session start as a best-effort cover, and (c) documents honestly in the README that mapping memory may swap on systems under memory pressure unless the operator also configures system-wide swap encryption (macOS default) or disables swap entirely. A custom-allocator implementation is tracked as a v0.2 hardening task.
3. **`madvise(MADV_DONTDUMP)`** — excludes the key page (and best-effort the mapping pages) from core dumps. A crash handler cannot accidentally write raw PII to a dump file.
4. **`zeroize` crate** — explicit secure-erase of key bytes AND every entry in the mapping on `Drop`, via wrapper types (`SecretString`, `SecretBytes`) that guarantee scrubbing regardless of allocator behavior. This is the real load-bearing guarantee for the mapping — `mlock` reduces the attack surface, but `zeroize` is what prevents residue.
5. **No `Debug` / `Display` impls on `SessionKey` or the mapping** — prevents accidental logging. Compile-fail tests in `tests/ui/` verify this.
6. **No serialization** — neither the key nor the mapping implement `Serialize`. They cannot accidentally end up in the audit log, a config file, or a crash report.
7. **Operational guidance** in the README: do not run Gaze under an untrusted debugger, do not enable core dumps in production, prefer running under a user account that is not sudo-capable, ensure swap is encrypted (macOS default; Linux: `cryptsetup` on the swap partition) since mapping pages are only best-effort locked.

These are Art. 32 TOMs the DSB can review directly. The hard guarantees (strict `mlock` on key, `zeroize` on drop for both key and mapping, compile-fail tests for serialization) are what Gaze commits to. The best-effort guarantee (`mlock` on mapping pages) is called out explicitly rather than overclaimed — a DSB who reviews this section sees an engineering team that knows what its tools can and cannot deliver.

### Replacement strategies

| Type | Detection source | Replacement | Example |
|---|---|---|---|
| `id` | column rule (primary key / foreign key flagged PII) | integer; first candidate is `HMAC(raw) mod 2^31`, rehashed on collision (see below) | `42` → `1043782` |
| `email` | column rule + safety net | `user_{n}@example.com` | `user_7@example.com` |
| `name` | column rule + safety net | `Person_{n}` | `Person_7` |
| `phone` | column rule + safety net | `+49 000 000{n}` | `+49 000 0007` |
| `address` | column rule | `Musterstraße {n}, 00000 Berlin` | `Musterstraße 7, 00000 Berlin` |
| `iban` | column rule + safety net | `DE00 0000 0000 0000 0000 {n}` | `DE00 0000 0000 0000 0000 07` |
| `ip` | column rule + safety net | `10.0.0.{n}` | `10.0.0.7` |
| `date` | column rule | shift by constant offset per session | `2026-01-15 → 2026-02-14` |
| `freetext` | Worka PII inline scan | replace inline matches, preserve surrounding text | `"Sent to krishan@x.com"` → `"Sent to user_7@example.com"` |

`{n}` is derived from the HMAC hash, so the same input always yields the same `n` within a session.

**Type-preserving filter round-trip for the `id` type.** Integer primary keys are the most common filter column in debugging workflows (`db.sample(orders, {user_id: {eq: 1043782}})`). The `id` replacement emits an integer so the agent can pass it straight through to a filter without type coercion errors from the MySQL driver. `restore()` reverses via the same session map. Compound filters mixing PII integer columns, PII string columns, and non-PII columns are supported — each column is validated independently: non-PII columns pass through verbatim, PII columns must carry session tokens. The `in` operator applies the same per-element rule: every element on a PII column must be a session token, or the whole filter is rejected with the generic `InvalidFilterValue`.

**Collision handling for the `id` type.** `HMAC(raw) mod 2^31` is not bijective: two distinct raw integers can hash to the same candidate fake. With 2^31 ≈ 2.1 billion slots, birthday collisions cross 50% probability at ~54k distinct IDs in one session. A silent collision would make `restore()` ambiguous — the reverse map would point one fake at two different raw values — and the wrong row would be returned. Gaze resolves this inside the anonymizer's session map:

1. Compute `candidate = HMAC(raw) mod 2^31`.
2. If `candidate` is not in the reverse map, insert `(raw → candidate, candidate → raw)` and return `candidate`.
3. If `candidate` is already bound to the same `raw`, return the existing value (session-stable).
4. If `candidate` is bound to a *different* raw, rehash: `candidate = HMAC(raw || counter) mod 2^31` with an incrementing counter stored next to the original raw value. Retry until a free slot is found.

This guarantees the mapping is bijective within a session regardless of cardinality. The counter stays inside the session map (never sent to the agent), so the fake value the agent sees is still just an integer. A pathological fixture with >1 billion distinct IDs would eventually saturate the 2^31 space, but long before that the operator should be hitting `max_rows` / `max_distinct` caps — the realistic ceiling is millions of distinct IDs per session, which stays well inside the collision-resistance of a counter-rehashed HMAC.

### Three-layer defense

```
Layer 1 — Column rules (policy.toml)
    "this column IS PII, always anonymize"
    Fastest, most reliable, human-reviewed.

Layer 2 — Worka PII safety net
    Scans ALL output values regardless of column rules.
    Catches PII in untagged columns or freetext fields.
    Fires on entity types: email, phone, IP, IBAN, credit card.

Layer 3 — Regex scrubbing
    Additional patterns for tokens, passwords, API keys.
    Applied to log output specifically.
```

If Layer 2 or 3 fires on a value Layer 1 did not catch, the value is anonymized AND a warning is written to the audit log: `"Safety net fired: email pattern detected in column orders.internal_notes — consider adding to policy.toml"`. This makes the policy file self-improving.

## Audit Log

Every MCP tool call creates exactly one audit entry. The audit log contains **no PII** — only access metadata.

```sql
CREATE TABLE audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp       TEXT NOT NULL,          -- ISO 8601 UTC
    session_id      TEXT NOT NULL,          -- UUID per gaze serve invocation
    tool_name       TEXT NOT NULL,          -- e.g., "db.sample"
    env             TEXT NOT NULL,          -- e.g., "production"
    request_json    TEXT NOT NULL,          -- structured request (tokens, not raw PII; filter values on PII columns are session tokens)
    target          TEXT,                   -- "mysql.production.orders"
    rows_returned   INTEGER,
    columns_touched TEXT,                   -- JSON array of column names accessed
    anonymized      TEXT,                   -- JSON: {"orders.email": "email"}
    safety_net_hits TEXT,                   -- JSON array of warnings
    duration_ms     INTEGER,
    status          TEXT NOT NULL           -- "ok" | "error" | "policy_rejected"
);
CREATE INDEX idx_audit_timestamp ON audit_log(timestamp);
CREATE INDEX idx_audit_session ON audit_log(session_id);
CREATE INDEX idx_audit_tool ON audit_log(tool_name);
```

### Storage

```
./.gaze/                     # per-project (default) — added to .gitignore by `gaze init`
├── audit.db                 # SQLite audit log scoped to this project
└── sessions/                # optional: per-session debug traces
    └── 7f3a9c01-....jsonl

~/.gaze/                     # global per-developer
├── config.toml              # shared Gaze config
└── audit.db                 # only present if invoked with `--global`
```

`gaze init` writes the per-project `.gaze/` directory and ensures it is listed in the project's `.gitignore`. Global mode (`gaze serve --global`) is opt-in for developers who want cross-project forensics; default is per-project so a leaked audit DB blast-radius is bounded to one repo's query history.

Per-project `policy.toml` lives in the project directory and is committed to git. The audit log is global per-developer so the trail is continuous across projects.

### Inspection (v0.1)

```bash
gaze audit --last 20
gaze audit --session 7f3a9c01
gaze audit --tool db.sample --env production
gaze audit --export json > report.json     # for DSB review
```

No TUI in v0.1. Ratatui TUI is a v0.2 target.

### DSB workflow

When the Datenschutzbeauftragter asks "how are you using AI with our production data?":

1. Show `policy.toml` — declares what data gets anonymized and how.
2. Run `gaze audit --env production --export json` — full access report.
3. Reference Worka PII — independently auditable detection engine.
4. Show the `RawRow` → `CleanRow` type-safety proof — compile-time guarantee of no PII leak.

### Retention

v0.1: no automatic rotation. Audit log grows until manually exported and truncated. Retention policy (e.g., `keep_days = 90`) is a v0.2 configuration option — jurisdiction-dependent, punted on defaults.

## Error Handling

The proxy must fail closed, never fail open. If any stage cannot complete safely, the response is an error — never raw data.

| Failure | Behavior |
|---|---|
| Adapter query fails | Return MCP error with **actively sanitized** message (see below). Never pass the raw driver error string through. |
| Policy validation fails (unknown column in filter) | Return MCP error: `"column X not in schema"` |
| Agent supplies raw PII as filter value on a PII column | Internally `AnonError::RawPIIInFilter`. Surfaced as generic `InvalidFilterValue` on the MCP response and in the audit log entry — specific variant retained only in developer-mode debug logs. Audit log records the column and operator, not the value. |
| Agent supplies a filter value for a PII column that is not in the session mapping | Internally `AnonError::UnknownFilterValue`. Surfaced as generic `InvalidFilterValue` (same collapsing as above) so error messages cannot be used as a token-existence oracle. |
| Anonymizer returns `Err` on a value | Tool call fails; audit log records `status = "error"`. Never return partial data. The anonymizer must not panic — all failure modes return `Result<CleanRow, AnonError>`. |
| Safety net detects PII type not in policy | Anonymize, return clean data, log warning. Not a hard failure. |
| Audit log write fails | Hard failure: abort the response. An un-audited access is a compliance failure. |
| MCP transport error | Standard MCP error response. |

### Active error sanitization

Database drivers routinely embed user values in error messages:

```
Error 1062 (23000): Duplicate entry 'krishan@example.com' for key 'users.email'
```

If such an error is returned verbatim, PII leaks through the error channel and the GDPR argument collapses. Gaze must actively scrub all driver errors before they leave the proxy:

1. **Structural stripping:** extract only the SQLSTATE and error code; drop the trailing detail string that may contain values.
2. **Safety-net pass:** run the remaining message through the same Worka PII detector used on row values. Any detected PII is replaced with `<redacted>`.
3. **Whitelist of safe error types:** schema errors, timeout errors, and permission errors pass through (with values stripped); constraint violations and "row not found" errors are reduced to their code only.
4. **Audit trail:** the raw driver error is recorded in the audit log **as metadata with Worka PII detection applied to it as well**. The audit log does not contain scrubbed user values either.

### Canary leak test for the error channel

The existing canary test (see Testing Strategy) is extended to cover error paths:

1. Seed a fixture table with a row containing `CANARY_EMAIL_DO_NOT_LEAK@test.local`.
2. Issue a query that deliberately triggers a driver error embedding the canary (e.g., a `duplicate key` insert, a malformed predicate).
3. Assert the canary string appears in **neither** the tool response **nor** the audit log.

This runs on every PR. Any code path that bypasses error sanitization is caught immediately.

The rule: if the audit log cannot be written or the anonymizer cannot complete, the tool call fails. There is no path where data reaches the agent without passing through both.

## Testing Strategy

### Property-based tests (`proptest`)

Invariants to verify with thousands of generated inputs:

- No PII pattern (email/phone/IP/IBAN) ever appears in output
- Session consistency: same input → same output within one session
- Session isolation: different sessions → different mappings (statistically)
- Structural preservation: output types match input types
- Column rule precedence: `passthrough` columns are untouched
- Safety net catches what column rules missed
- Freetext replacement preserves non-PII surrounding context

### Fixture-based tests

```
tests/fixtures/
├── mysql/
│   ├── users_table.json           → mock DB response
│   ├── users_expected.json        → expected anonymized output
│   └── orders_with_freetext.json
├── laravel/
│   ├── sample.log                 → raw Laravel log with PII
│   └── sample_expected.log        → expected anonymized output
└── policies/
    └── basic.toml
```

### End-to-end tests (`testcontainers-rs`)

Spin up a MySQL container, seed with known PII, run Gaze against it, assert no raw PII in the response.

### Canary leak test

A special test that runs every tool handler against a table containing a known marker value and asserts the marker never appears in any response. Runs on every PR. If a new tool handler is added without going through the anonymizer, this test catches it immediately.

```rust
const LEAK_MARKER: &str = "CANARY_EMAIL_DO_NOT_LEAK@test.local";
```

### Compile-fail tests (`trybuild`)

Verify that `RawRow` does not implement `Serialize` and that no public code path can construct a `CleanRow` without going through the anonymizer.

### CI pipeline

Every PR runs:

1. `cargo fmt --check`
2. `cargo clippy -- -D warnings`
3. `cargo test` (unit + property + fixture)
4. `cargo test --test e2e` (MySQL testcontainer)
5. Canary leak test
6. `trybuild` compile-fail tests

### Out of scope for v0.1 tests

- Performance benchmarking
- Concurrent session isolation (v0.2 with HTTP mode)
- Adversarial attacks on the anonymizer
- Correctness of Worka PII internals (trusted dependency)

## v0.1 Scope

### Included

**Anonymizer core (transport-agnostic):**
- `SessionKey` + `Anonymizer` with `clean()` AND `restore()` (both functional — `restore()` powers filter-value translation)
- `RawRow` / `CleanRow` type-safe boundary
- Worka PII integration as safety net (wrapped behind a `PiiDetector` trait to keep the dependency swappable)
- Regex-based log scrubbing
- Session-scoped consistent mapping (in-memory `HashMap`)
- Strict `mlock` + `MADV_DONTDUMP` on the 32-byte session key (mandatory, `--allow-unlocked-key` escape hatch)
- Best-effort `mlockall(MCL_CURRENT)` on mapping pages; hard `zeroize` on `Drop` for both key and mapping as the real load-bearing guarantee (see Memory Hygiene section)
- Replacement strategies: structured, freetext, passthrough

**Adapters:**
- MySQL (`sqlx`): `tables`, `schema`, `sample`, `count`, `distinct`
- Laravel log file: `search`, `tail`, `context`
- SSH tunnel lifecycle: when `ssh_tunnel` is set, Gaze spawns `ssh -f -N -L ...` at startup and kills it on shutdown. Single-connection scope — production only.

**Policy engine:**
- TOML parsing
- Wildcard column matching (`*.email`)
- Schema validation (`gaze check`)

**MCP server:**
- `rmcp` stdio transport
- Tool handler registry for `db.*` and `logs.*`
- Every response passes through `Anonymizer::clean()`

**CLI (Clap):**
- `gaze init` — onboarding scanner
- `gaze check` — policy validation
- `gaze serve` — start MCP stdio server
- `gaze audit` — query audit log

**Audit:**
- SQLite audit log
- Entry per tool call
- Safety net warnings persisted

**Distribution:**
- `cargo install gaze`
- Homebrew tap for macOS
- Claude Code skill file (teaches agents how to use the tools)

### Deferred

- **Pipe mode (`gaze clean` / `gaze restore`)** → v0.2 (replaces the previously-planned HTTP proxy mode)
- Postgres adapter → v0.2
- Sentry/Flare adapters → v0.2
- Persistent-key mode with on-disk HMAC key → v0.3 (cross-session consistency; legally pseudonymous and must be documented as such)
- Pure-Rust SSH tunnel (via `russh`) → v0.2 — v0.1 shells out to the system `ssh` client to reuse `~/.ssh/config` and agent forwarding
- Ratatui audit TUI → v0.2
- `gaze demo` with bundled SQLite fixture for first-run UX → v0.2 (remove the "needs prod credentials to try the tool" barrier)
- Tauri desktop app → vFuture
- Server infrastructure commands → deferred

## Pipe Mode Preview (v0.2)

Pipe mode is out of scope for v0.1 but documented here because it drives several v0.1 architectural decisions (notably: `restore()` being a real function, the anonymizer being transport-agnostic, and the session-blob format).

### CLI contract

```bash
# Anonymize text on the way in. Input: raw text on stdin. Output: JSON on stdout.
$ echo "$RAW_EMAIL" | gaze clean --format=json --policy=policy.toml
{
  "clean_text": "Hi Person_7, your order #1234 ships to Musterstraße 7, 00000 Berlin",
  "session_blob": "<opaque plaintext JSON blob with integrity HMAC>"
}

# De-anonymize a reply from the LLM. Input: JSON on stdin. Output: JSON on stdout.
$ echo '{"session_blob":"...","text":"Hi Person_7, we will ship..."}' | gaze restore --format=json
{
  "text": "Hi Krishan Koenig, we will ship..."
}
```

### Session blob shape

```json
{
  "v": 1,
  "nonce": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  "map": {
    "Person_7": "Krishan Koenig",
    "user_7@example.com": "krishan@example.com",
    "Musterstrasse_7_00000_Berlin": "Musterstraße 5, 10115 Berlin"
  },
  "created_at": 1775895000,
  "ttl_sec": 3600
}
```

- `v` — format version.
- `nonce` — ULID generated fresh per `gaze clean` invocation. Makes two identical inputs produce distinct blobs (and therefore distinct ciphertexts under host encryption), so cache-based inference on ciphertext length alone cannot correlate blobs to known plaintexts.
- `map` — anonymized token → raw value.
- `created_at` + `ttl_sec` — `gaze restore` rejects stale blobs (evaluated against the Gaze process's wall clock at restore time — operators should ensure NTP sync on queue workers, since clock drift across a DST boundary or missed NTP correction could reject a legitimate blob).

### Integrity model — host AEAD owns it

The blob has **no Gaze-level HMAC tag**. An earlier draft bound integrity to a per-invocation HMAC key derived from the Gaze process's session state — but `gaze clean` and `gaze restore` run in *different* processes in pipe mode, so the signing key cannot survive the process boundary without either (a) embedding itself in the blob (self-defeating) or (b) pushing key management onto the host. Neither is worth the ceremony, because the host platform already provides authenticated encryption.

**The rule:** any blob that leaves the `gaze clean` process's stdout must be wrapped in an AEAD construction by the host before it touches any durable store (queue, cache, log, database column). Laravel's `Crypt::encryptString` is AES-256-CBC + HMAC-SHA256 — authenticated encryption in practice; any tamper attempt fails `decryptString` before the bytes reach `gaze restore`. Node's `libsodium`, Python's `cryptography.fernet`, and Go's `crypto/aes` + `crypto/hmac` all offer equivalent primitives. Integration docs must call this out as **mandatory**, not best-practice.

This means: a plaintext blob sitting unencrypted in Redis is both a confidentiality bug (the raw PII map is visible) *and* an integrity bug (a tampered blob produces wrong restores with no detection). The two failures collapse into a single operational rule — "encrypt the blob before it leaves the current function's local scope" — which is easier to teach and audit than two separate obligations.

**Replay resistance:** the TTL bounds the replay window. Within TTL, the same blob can be passed to `gaze restore` multiple times — this is deliberate, because a queue job retrying after a failure must be able to re-use the same blob. Gaze does not maintain a seen-nonces store (that would defeat the stateless pipe design). Applications that need one-shot semantics must enforce it at the application layer (e.g., Laravel's `ShouldBeUnique` job + idempotency key on the email-send action).

**Gaze does not encrypt the blob and does not sign it.** Both guarantees come from the host AEAD envelope. This keeps Gaze's key-management surface at zero and lets host platforms reuse their existing primitives (e.g., Laravel's `APP_KEY`, or a dedicated `GAZE_ENCRYPTION_KEY` for rotation independence — see the Laravel integration doc).

### Host-side encryption pattern

The canonical recommendation for async/queued workloads: wrap the blob in the host platform's symmetric encryption before passing it through queues, caches, or logs.

```php
// Laravel — clean path
$session = Gaze::clean($email->body);
$encryptedBlob = Crypt::encryptString($session->blob);

dispatch(new ReplyToEmailJob(
    cleanText: $session->cleanText,
    encryptedSession: $encryptedBlob,
));
```

```php
// Laravel — restore path (inside the job)
$blob = Crypt::decryptString($this->encryptedSession);
$final = Gaze::restore(new GazeSession($blob), $llmReply);
```

This protects the blob while it sits in Redis, MySQL `jobs`/`failed_jobs`, debug logs, and Horizon dashboards. Plaintext only exists inside the Laravel worker process during the brief window between decrypt and restore. A standalone integration doc (`docs/roadmap/v0.3/laravel.md`) walks through the full Ghostwriter-style email reply pipeline.

### Why pipe mode, not HTTP (reprise)

- **No server lifecycle** — no `systemd` unit, no port, no TLS, no auth tokens.
- **No network attack surface** — process boundary is the only boundary.
- **Trivial for queues** — the session blob serializes cleanly into Laravel queue payloads; no session ID that has to survive worker restarts.
- **Language-agnostic** — any language that can spawn a subprocess can use Gaze. Wrappers for Laravel, Node, Python.
- **Deterministic operational footprint** — cold-start cost per invocation (~5–15 ms for a statically-linked Rust binary) is acceptable for the SMB scale Gaze targets. A later `gaze serve --stdio` daemon mode can fix this without changing the CLI contract.

## Build Sequence (Milestones)

### M1a — Anonymizer core (functional)

- `SessionKey`, `Anonymizer::clean`, `Anonymizer::restore` (both functional from day one)
- Per-type replacement strategies (including the `id` integer type)
- Filter-value round-tripping: `restore()` on session tokens returns original raw values
- Unit + property tests (determinism, cross-session isolation, type-preservation)

**Exit:** `cargo test` proves session consistency, session isolation, filter-value round-tripping, and that `clean` → `restore` is an identity on all supported types. At this point the anonymizer is usable from a test harness even without the hardening layer — enough to unblock M2 (MySQL adapter).

### M1b — Anonymizer hardening

- `PiiDetector` trait + Worka PII implementation (swappable behind trait so Worka can be replaced if it stalls)
- Strict `mlock` + `MADV_DONTDUMP` on the 32-byte session key
- Best-effort `mlock` on mapping pages + mandatory `zeroize` on `Drop` for both key and mapping (via `SecretString` / `SecretBytes` wrapper types)
- Graceful fallback warnings when `mlock` unavailable (`--allow-unlocked-key` escape hatch)
- Compile-fail tests (`tests/ui/` via `trybuild`) proving no `Debug`, `Display`, `Serialize` on sensitive types
- Canary leak test harness (used from M5 onwards by the error-sanitization layer)

**Exit:** memory-hygiene compile-fail tests pass, canary harness is a reusable test fixture, and the anonymizer behaves identically to M1a from an API perspective but with the hardening layer active underneath.

**Why split M1.** Round 2 review (all three counselors) flagged that the combined M1 was roughly doubled in scope from the original round-1 plan and risked delaying the first end-to-end MySQL test. Splitting keeps the critical path — core anonymizer → MySQL adapter → MCP wiring — unblocked by the hardening work. M1b runs in parallel with M2 where possible and must be complete before M5 (audit log wiring), not before M2.

### M2 — Policy engine + MySQL adapter

- TOML policy parser and validator
- `sqlx` MySQL connection and schema introspection
- `db.tables`, `db.schema`, `db.sample`, `db.count`, `db.distinct`
- Testcontainer integration tests

**Exit:** end-to-end test against a seeded MySQL container returns anonymized sample data with no raw PII.

### M3 — CLI commands

- `gaze init` — scanner and draft policy generator
- `gaze check` — policy validator against live schema

**Exit:** running `gaze init` on a real Artistfy-like schema produces a usable draft `policy.toml`.

### M4 — Laravel log adapter

- File parsing, pattern extraction
- `logs.search`, `logs.tail`, `logs.context`

**Exit:** `logs.tail` on a real Laravel log with PII returns anonymized lines.

### M5 — MCP server wiring

- `rmcp` stdio transport
- Tool handler registry
- Audit log writes
- Canary leak test passing across all tools

**Exit:** Claude Code can connect to `gaze serve`, call all tools, and the canary test confirms no PII leak paths.

### M6 — Dogfood at Artistfy

- Install on Artistfy project
- Write Claude Code skill file
- Debug a real production issue end-to-end

**Exit:** successfully debugged an Artistfy issue without PII reaching Claude's context.

### M7 — Dogfood at Sandorian + release

- Second production install
- Fix whatever broke at Artistfy
- Tag v0.1, publish to crates.io, Homebrew tap

**Exit:** another developer at Sandorian can install and use Gaze.

## Dependencies

### Rust crates

| Crate | Purpose |
|---|---|
| `rmcp` | Official MCP SDK (stdio transport) |
| `clap` | CLI framework |
| `sqlx` | Async MySQL driver |
| `pii` (Worka PII) | Deterministic PII detection + redaction |
| `fake` | Generating replacement values |
| `toml` | Policy file parsing |
| `serde`, `serde_json` | Serialization |
| `rusqlite` | Audit log storage |
| `tokio` | Async runtime |
| `hmac`, `sha2` | Session-scoped hashing |
| `proptest` | Property-based tests |
| `testcontainers` | E2E MySQL tests |
| `trybuild` | Compile-fail tests |

### External

- Worka.ai (crate maintainer). Monitor for deprecation or direction changes, but Gaze's safety net is the only consumer of the dep and can be swapped for raw regex if needed.

## Open Questions

1. **SSH tunnel management — resolved.** v0.1 owns the tunnel lifecycle. When `[connection.production].ssh_tunnel` is set, Gaze opens the forwarded port itself at `gaze serve` startup (and at every `gaze clean` / `gaze restore` invocation in v0.2 pipe mode) and tears it down on exit. No separate `ssh -L ...` command. Implementation: either shell out to the system `ssh` client with `-f -N -L` flags (simple, delegates auth to user's `~/.ssh/config`) or use `russh` / `openssh-portable` FFI for a pure-Rust path. Leaning toward shelling out to `ssh` in v0.1 so the user's existing SSH agent, config aliases, and key passphrase prompts keep working unchanged. Library choice and pure-Rust migration can still be revisited in v0.2.
2. **Single production connection — resolved.** Gaze is a production-data access tool, not a general-purpose dev database client. v0.1 (and for the foreseeable future) supports exactly **one** connection per policy file, scoped to production. Local / staging / dev environments are out of scope — if you want to query local MySQL you already have a normal client for that. The policy schema keeps the `[connection.production]` namespace for clarity (future-proofs the TOML shape if multi-env ever becomes relevant) but `gaze check` rejects any policy file that declares more than one `[connection.*]` block. MCP tools still take an `env` parameter, but v0.1 hard-codes it to `"production"` — the parameter exists so pipe mode and future expansion don't need a schema break.
3. **Session expiry in MCP mode** — MCP stdio session ends when the server process exits. Is this the right boundary, or should long-running sessions expire independently (e.g., timeout)? v0.1 decision: session = process lifetime, no timeout.
4. **`gaze init` ambiguity handling** — when the scanner is uncertain about a column, should it default to anonymize-as-freetext (safer) or skip with a warning (current plan)? Skipping is safer for false positives but riskier for false negatives. The safety net should catch false negatives, so skipping is acceptable.
5. **Crate name availability** — `gaze` on crates.io may be taken. Verify before M7 release. Fallback: `gaze-proxy` or namespace under `gaze-rs`.
6. **Audit log retention defaults** — v0.1 has no rotation, unbounded growth. Need at least a "warn at N MB" default and a documented `gaze audit --export json && rm ~/.gaze/audit.db` workflow. Retention rules proper are v0.2.
7. **Audit log file location — resolved.** v0.1 default is **per-project** at `./.gaze/audit.db`. `gaze init` adds `.gaze/` to the project's `.gitignore` so the audit DB is never committed. Per-project keeps the blast radius if leaked to a single project's query history (vs. every project the developer has ever touched), and makes it natural to hand an auditor "the audit log for this repo." A `--global` flag writing to `~/.gaze/audit.db` is available as an opt-in for developers who want cross-project forensics. The `~/.gaze/` directory is still used for the global `config.toml`, just not for audit storage by default.
8. **Worka PII German-locale coverage** — confirm Worka PII reliably detects German formats (IBAN, phone, addresses with Umlauts) before M1 exit. If coverage is weak, extend the `PiiDetector` trait impl with German-specific rules on top of Worka PII.
9. **JSON / view / computed columns** — MySQL `JSON` columns and views can smuggle PII past column-level classification. v0.1 should at least emit a safety-net warning; full policy-level JSON-path support is v0.2.
10. **Should v0.1 narrow to DB-only and ship logs as v0.1.1?** — one counselor suggested deferring the Laravel log adapter to reduce v0.1 surface. Leaning against: debugging without logs is half-debugging, and dogfooding on Artistfy issues will routinely need log context. Staying with DB + logs in v0.1.
11. **Differential privacy on `db.count` / `db.distinct`** — one counselor (Gemini) suggested replacing the `count_allowed_columns` allowlist mechanism with Laplace noise or rounding-to-nearest-5 on PII-column counts, giving agents the "gist" without enabling brute force. Rejected for v0.1 because: (a) non-reproducible results break debugging workflows (same query twice returns different numbers), (b) picking a correct noise scale requires DP expertise (ε parameter, sensitivity analysis), (c) explicit allowlists are clearer than "counts return fuzzy numbers sometimes" for the dogfooding phase. Keep allowlists in v0.1. Revisit as opt-in `[policy.count_mode] = "differential"` in v0.2 if real users request it.

## Appendix: Relationship to Worka

Worka.ai maintains the PII crate Gaze depends on. Worka operates at a different layer:

- **Worka Anvil/runtime** = execution-layer trust ("can this code run?"). WASM-sandboxed AI automation packs.
- **Gaze** = data-layer trust ("can this data be seen?"). Deterministic proxy + anonymizer.

They are complementary. Gaze does not depend on the Worka runtime — only on the PII library. A future integration (Gaze as a Worka pack) is possible but not a goal.
