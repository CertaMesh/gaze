# gaze-token-bridge

> **Status:** work in progress (`publish = false`). API is pre-1.0 and may change.

The token bridge is the **owner-side authorization + translation layer** that lets an
agent search long-lived, policy-scoped document corpora **without ever seeing raw PII**.

An agent works in a short-lived [`RedactionSession`](src/session.rs) whose only
vocabulary is *session tokens* (e.g. `<…:Name_1>`). When it wants to search a corpus,
the bridge — running entirely owner-side — resolves the token, checks policy
(default-deny), mints a single-use, entity-bound capability, runs the search against a
**redact-before-index** corpus, and translates the owner-side hits back into the
agent's current session namespace. Raw PII, index aliases, and the restore manifest stay
owner-side by construction; the agent only ever receives current-session tokens. This
serves the project's north star directly: **zero PII leaks between the agent and the
data owner — ever** (fail-closed on every error path).

For the architecture and the frozen data-model contract, see the crate docs
([`src/lib.rs`](src/lib.rs)) and [`src/model.rs`](src/model.rs)'s three visibility tiers.

## Local demo (try it)

A runnable, **synthetic-data-only** walkthrough lives in
[`examples/local_demo.rs`](examples/local_demo.rs). It uses only the crate's public API
(plus `serde_json`, already a dependency) and exits `0` on success / panics loudly on any
leak. No setup, no network, no real PII.

### Run it

```bash
cargo run -p gaze-token-bridge --example local_demo
```

### Expected output

```text
gaze-token-bridge — local demo (SYNTHETIC DATA ONLY)
Owner-side authorization + translation bridge. Zero raw PII reaches the agent.

=== Step 1 — ingest synthetic corpus (redact-before-index) ===
Ingested 5 synthetic docs into 2 IndexDomains via the Track B redact-before-index pipeline:
  - tenant_demo/customer_docs/v1
  - tenant_demo/legal_docs/v1
Raw values were tokenized at ingest; the index holds only projected aliases.

=== Step 2 — support session mints a token for the lookup name ===
owner-side input (never sent to the agent): name = "Markus Gottschaue"
session token the agent sees instead:        <0cd5a5d9:Name_1>

=== Step 3 — ALLOWED: support -> customer_docs ===
ALLOWED. target_domain = tenant_demo/customer_docs/v1
session-translated snippets (note the <...:Class_n> session tokens):
  [cust-001] Customer profile <0cd5a5d9:Name_1> / <0cd5a5d9:Email_1> / <0cd5a5d9:Custom:customer_id_1> is linked to <0cd5a5d9:Organization_1>.

=== Step 4 — DENIED: support -> legal_docs ===
DENIED (authorization failed)

=== Step 4 (contrast) — ALLOWED: admin -> legal_docs ===
ALLOWED. target_domain = tenant_demo/legal_docs/v1
  [cust-001] Legal matter references <48904f81:Name_1> at <48904f81:Organization_1>; contact route <48904f81:Email_1>.

=== Step 5 — assert NO raw PII in agent-visible output ===
Scanned 480 bytes of agent-visible JSON against 20 raw fixture values.
✅ NO RAW PII IN AGENT-VISIBLE OUTPUT
audit events recorded (one per bridge decision): 3
```

> **Note on the token prefix.** The 8-hex prefix (`0cd5a5d9`, `48904f81`, …) is a
> **per-session salt** and will differ on every run — that is the point: a token minted
> in one session is meaningless (`UnknownToken`) in another, so tokens cannot be
> correlated across sessions. Only the structure (`:Name_1>`, `:Email_1>`, …) is stable.
> The support and admin lines use different prefixes because they are different sessions.

### What each step proves

Mapped to the four spike blockers this runtime had to close, plus the never-leak assertion:

1. **Session ↔ principal binding** — each principal mints tokens in its *own*
   principal-bound session. Step 4 shows `support → legal_docs` fail closed (a uniform
   deny), while `admin → legal_docs` is allowed from the admin's own session; a session
   may not be reused across principals.
2. **Owner-bound purpose** — the request carries a `purpose`, but the policy gate ignores
   it and uses the owner-bound purpose resolved from config. The agent (or a prompt
   injection) cannot widen its authority by restating intent.
3. **entity_ref binding** — authorization mints a single-use capability bound to one
   specific entity, so the ALLOWED search returns only that entity's document
   (`cust-001`), never the whole corpus.
4. **Filter / value projection** — the corpus was redacted *before* indexing, so the
   index holds only projected aliases; returned snippets are translated into the active
   session's tokens. Raw values never reach the index, the adapter, or the agent.
5. **Never-leak assertion** — Step 5 serializes *all* agent-visible responses and scans
   them against every raw value in the synthetic fixture. Any match panics (non-zero
   exit). The deny path emits a single uniform `DENIED (authorization failed)` line — the
   typed `DenyReason` is owner-side audit only, so the agent gets no probing oracle.

### What's NOT shown

- **Vector / semantic search** is deferred; the demo uses exact entity-bound lookup over
  the in-memory adapter.
- **Raw-filter projection over the wire** (passing a raw PII filter value that the bridge
  projects owner-side before the adapter sees it) is exercised by the test suite
  (`raw_filter_values_are_projected_before_adapter_receives_request` in
  [`tests/track_c_bridge.rs`](tests/track_c_bridge.rs)) rather than this script.
- **The MCP `search_documents` chokepoint tool** (the sealed-handle integration that
  exposes this bridge as an agent tool) lives behind the `chokepoint` feature — see PR2.
  This example drives the bridge through its library API directly.

All behavior here is covered by the acceptance suite in
[`tests/track_c_bridge.rs`](tests/track_c_bridge.rs); run it with
`cargo test -p gaze-token-bridge`.
