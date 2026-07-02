# gaze-token-bridge

> **Status:** experimental and published as `gaze-token-bridge = "0.11.3"`.
> API is pre-1.0 and may change.

```toml
[dependencies]
gaze-token-bridge = "0.11.3"
```

The token bridge is the **owner-side authorization + translation layer** that lets an
agent search long-lived, policy-scoped document corpora while keeping raw values on
the owner side of the bridge.

An agent works in a short-lived [`RedactionSession`](src/session.rs) whose only
vocabulary is *session tokens* (e.g. `<…:Name_1>`). When it wants to search a corpus,
the bridge — running entirely owner-side — resolves the token, checks policy
(default-deny), mints a single-use, entity-bound capability, runs the search against a
**redact-before-index** corpus, and translates the owner-side hits back into the
agent's current session namespace. Raw PII, index aliases, and the restore manifest stay
owner-side; the agent-visible response is current-session tokens plus non-sensitive text.

For the architecture and the frozen data-model contract, see the crate docs
([`src/lib.rs`](src/lib.rs)) and [`src/model.rs`](src/model.rs)'s three visibility tiers.

## Local demo (try it)

A runnable, **synthetic-data-only** walkthrough lives in
[`examples/local_demo.rs`](examples/local_demo.rs). It uses the crate's public API and
prints the allow/deny outcomes and translated snippets for the bundled fixture corpus.

### Run it

```bash
cargo run -p gaze-token-bridge --example local_demo
```

### Expected output

```text
gaze-token-bridge - local synthetic demo
Owner-side authorization and translation over bundled fixtures.

=== Step 1 - ingest synthetic corpus ===
Ingested 5 synthetic docs into two policy-scoped domains:
  - tenant_demo/customer_docs/v1
  - tenant_demo/legal_docs/v1

=== Step 2 - mint a lookup token ===
owner-side synthetic input: name = "Markus Gottschaue"
session token passed to the bridge: <0cd5a5d9:Name_1>

=== Step 3 - support searches customer docs ===
ALLOWED. target_domain = tenant_demo/customer_docs/v1
  [cust-001] Customer profile <0cd5a5d9:Name_1> / <0cd5a5d9:Email_1> / <0cd5a5d9:Custom:customer_id_1> is linked to <0cd5a5d9:Organization_1>.

=== Step 4 - support searches legal docs ===
DENIED (authorization failed)

=== Step 5 - admin searches legal docs ===
ALLOWED. target_domain = tenant_demo/legal_docs/v1
  [cust-001] Legal matter references <48904f81:Name_1> at <48904f81:Organization_1>; contact route <48904f81:Email_1>.

audit events recorded: 3
```

> **Note on the token prefix.** The 8-hex prefix (`0cd5a5d9`, `48904f81`, …) is a
> **per-session salt** and will differ on every run — that is the point: a token minted
> in one session is meaningless (`UnknownToken`) in another, so tokens cannot be
> correlated across sessions. Only the structure (`:Name_1>`, `:Email_1>`, …) is stable.
> The support and admin lines use different prefixes because they are different sessions.

### What each step demonstrates

1. **Session and principal binding** - each principal mints tokens in its own
   principal-bound session. The support principal is denied for `legal_docs`, while
   the admin principal is allowed from the admin's own session.
2. **Owner-bound purpose** - the request carries a `purpose`, but the policy gate uses
   the owner-bound purpose resolved from config.
3. **entity_ref binding** - authorization mints a single-use capability bound to one
   specific entity, so the allowed search returns that entity's document.
4. **Filter and value projection** - the corpus is redacted before indexing, and
   returned snippets are translated into the active session's tokens.

Integration coverage for expected outcomes and fixture leak checks lives in
[`tests/local_demo_assertions.rs`](tests/local_demo_assertions.rs).

### What's NOT shown

- **Vector / semantic search** is deferred; the demo uses exact entity-bound lookup over
  the in-memory adapter.
- **Raw-filter projection over the wire** (passing a raw PII filter value that the bridge
  projects owner-side before the adapter sees it) is exercised by the test suite
  (`raw_filter_values_are_projected_before_adapter_receives_request` in
  [`tests/track_c_bridge.rs`](tests/track_c_bridge.rs)) rather than this script.
- **The MCP `search_documents` chokepoint tool** (the sealed-handle integration that
  exposes this bridge as an agent tool) lives behind the `chokepoint` feature.
  This example drives the bridge through its library API directly.

For bring-your-own-data redaction, use the core folder scan example:

```bash
cargo run -p gaze-pii --example scan_folder -- --path ./my-data
```
