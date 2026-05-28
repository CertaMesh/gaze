# Gaze Threat Model & Proposed Audit Scope

**Status:** public, living document. **Last reviewed:** 2026-05-28 against `v0.9.0`.

This document consolidates Gaze's security posture for external reviewers and
prospective third-party auditors. It states who attacks Gaze, what they want,
where the attack surface is, which abuse cases the design must withstand, what
the protected-path enforcement gate covers today (and what it does not), when
the restore path must fail, and what an independent audit should examine.

> **No audit has been performed yet.** Section 6 *proposes* a scope for a future
> third-party review. Nothing here should be read as a claim that an external
> audit has occurred or that any control has been independently verified.

Every coverage claim below cites a real file or named test in this repository.
Where a property is security-relevant but not yet proven by a named test, it is
listed explicitly as **unverified** rather than implied to be covered. The
authoritative per-invariant list with named tests lives in
[`docs/security-review.md`](../security-review.md); this document extends it with
the adversary model, abuse cases, bypass-class detail, and audit scope.

---

## 0. North star & trust posture

Gaze is a reversible PII pseudonymization runtime for agentic workflows. The
governing security goal is: **zero PII leaks between the agent and the data
owner — any byte of PII that reaches an LLM outside the manifest contract is a
critical defect.** The runtime is evaluated against five axes — reliability
(fail-closed), reversibility (manifest-first restore), agentic-first design,
trust (auditable + deterministic), and adopter ergonomics. Correctness axes
always beat performance. See [`AGENTS.md`](../../AGENTS.md) and
[`CLAUDE.md`](../../CLAUDE.md) for the full north-star statement.

Two design choices shape the entire threat model:

1. **Fail-closed by construction.** Unknown validators, unknown policy keys,
   expired session blobs, and unknown restore tokens return typed errors rather
   than degrading silently. A weaker-but-working path is treated as a defect.
2. **Deterministic and auditable.** Detection is rule-based first; every token
   emission is traceable to a recognizer, and every conflict loser is logged
   with the tier that decided it. Neural components are an opt-in safety net,
   not the floor.

---

## 1. Threat model

### 1.1 Trust boundaries

| Boundary | Trusted side | Untrusted side |
|---|---|---|
| **Pseudonymization chokepoint** | Owner-side raw text entering `Pipeline::redact` / `gaze clean` / MCP `ToolCtx` dispatch | Tokenized output forwarded to the LLM / provider |
| **Restore boundary** | Session manifest (the only authority for re-materialization) | Token-shaped values arriving from the model, tools, or integrations |
| **Audit sink** | Metadata-only redaction log (`gaze-audit`) | Any consumer querying or exporting audit rows |
| **Session namespace** | A single `Session` pseudonym namespace | Other sessions / tenants sharing storage |
| **MCP tier isolation** | Tool-tier execution context (sealed `ToolCtx`) | Caller-tier principal requesting a tool |

The chokepoint is the load-bearing assumption: Gaze protects only content that
actually passes through `Pipeline::redact`, `gaze clean` stdin, or the MCP
`ToolCtx` dispatch path. Content routed around the chokepoint (system prompts,
tool schemas, agent instructions, adopter code that bypasses the documented API)
is out of scope by construction — see [`SECURITY.md`](../../SECURITY.md).

### 1.2 Adversaries and their goals

| Adversary | Capability | Goal |
|---|---|---|
| **Curious / hostile model provider** | Sees everything forwarded past the chokepoint | Recover raw PII from tokens; correlate pseudonyms across requests |
| **Log / audit-sink reader** | Read access to the audit database or its exports | Recover raw PII or restorable token→value material from audit rows |
| **Snapshot thief** | Obtains a persisted `SensitiveSnapshot` | Restore original PII offline |
| **Malicious / buggy integration** | Calls restore with attacker-influenced or wrong-context tokens | Coax `restore()` into re-materializing values it should not |
| **Cross-tenant correlator** | Observes two logs/sessions that should be independent | Link a subject across contexts via reused pseudonyms |
| **Contributor / supply-chain edit** | Can submit code to protected crates | Quietly route raw PII or audit material into a protected path |
| **Rulepack / policy author** | Supplies TOML policy or custom recognizers | Weaken detection via a malformed or fail-open configuration |

### 1.3 Attack surface (entry points)

- **Detection inputs:** free text and embedded JSON reaching the recognizer
  registry; locale chain selection; bundled and path rulepacks; custom
  recognizers and collision-family metadata.
- **Restore inputs:** token-shaped values presented for re-materialization;
  manifest/session/tenant selection at the call site.
- **Session lifecycle:** `Scope::{Ephemeral, Conversation, Persistent}`,
  snapshot export/import, TTL handling.
- **Audit egress:** `SqliteLogger` writes; `build_audit_query_sql` query/export.
- **MCP dispatch:** caller-tier vs tool-tier, `AuthHook`, `SessionIdPolicy`,
  `ManifestStore` (see [`docs/architecture/mcp-runtime.md`](../architecture/mcp-runtime.md)).
- **Optional networked safety net:** opt-in, feature-gated post-clean check.
- **Source tree itself:** edits to protected paths (`crates/gaze/src`,
  `crates/gaze-cli/src/restore`) that could import the audit sink.

---

## 2. Abuse cases

Each abuse case states the attacker action, the failure we must prevent, and the
control that resists it. Controls are cited to code or a named test where one
exists; otherwise the gap is marked **unverified**.

### 2.1 PII leak via boundary bypass

- **Action:** raw PII reaches the LLM without being tokenized — a recognizer
  fails open, a feature graph silently disables a detector, or detection is
  skipped under an optimization mode.
- **Must prevent:** any un-tokenized PII byte crossing the chokepoint.
- **Controls:**
  - Recognizer-native detection with a typed `DetectContext`; new detectors land
    as `Recognizer` impls, not bespoke pipeline hooks.
  - Unsupported validator/normalizer names **fail closed at rulepack load**
    (`RulepackError::UnsupportedValidator` / `UnsupportedNormalizer`), verified
    by `crates/gaze-recognizers/tests/no_phone_parser_fail_closed.rs:phone_validators_fail_closed_at_rulepack_load_without_phone_parser`.
  - Unknown policy keys fail closed via `serde(deny_unknown_fields)`, verified by
    `crates/gaze/src/policy.rs:rejects_unknown_keys`.
  - Fail-open regressions on the protected default, `--no-default-features`, and
    safety-net feature graphs are in scope for the vuln process
    ([`SECURITY.md`](../../SECURITY.md)) and exercised by the `ci-feature-matrix`
    xtask gate. The pipeline-optimization config (skip-gating, capitals
    heuristic, prefix cache, length bucketing) is opt-in and default-off, and
    skip-gating applies only to observer-only modes.
  - The opt-in Pass-3 SafetyNet is an observer-only net over already-tokenized
    output; it never mutates the manifest or restore path
    ([`docs/architecture/safety-nets.md`](../architecture/safety-nets.md)).
- **Residual risk (unverified):** there is no runtime test proving
  `Pipeline::redact` cannot perform network I/O on the clean path — current
  evidence covers dependency *shape*, not runtime behavior. Detection coverage is
  inherently bounded by active rulepacks, locale chain, and recognizers; Gaze is
  not a universal PII detector.

### 2.2 Restore-token leak (audit / log exfiltration)

- **Action:** an audit-sink reader queries or exports the redaction log hoping to
  recover raw PII or a restorable token→value mapping.
- **Must prevent:** raw document text, restored PII, or token payloads appearing
  in audit output.
- **Controls:**
  - The audit log is **metadata-only**; query/export is constrained to
    `AUDIT_RESTRICTED_COLUMNS`. Verified by
    `crates/gaze-cli/tests/cli_pipe.rs:s4_audit_export_does_not_return_raw_pii`.
  - `rusqlite` lives only in `gaze-audit`; the core `gaze` runtime carries no
    audit-sink dependency in any feature graph.
- **Residual risk (unverified):** **token opacity** is not yet proven by a
  property test — no test currently proves a token leaks no key material or
  source-value bytes beyond class and ordinal.

### 2.3 Restore-token leak (snapshot theft)

- **Action:** an attacker obtains a persisted `SensitiveSnapshot` and attempts an
  offline restore.
- **Must prevent:** restore of PII outside an authorized live context, and use of
  stale snapshots.
- **Controls:**
  - Persistent-session TTL is enforced on import:
    `Session::import` rejects snapshots past their embedded TTL with
    `Error::BlobExpired`, verified by
    `crates/gaze/src/session.rs:import_rejects_expired_persistent_snapshot`.
  - The restore path **trusts the snapshot by design.** This is an explicit
    non-guarantee: a stolen, in-TTL `SensitiveSnapshot` *can* recover original
    PII. Adopters MUST encrypt snapshots at rest and keep them away from LLMs
    ([`docs/security-review.md`](../security-review.md), "What Gaze Does Not
    Guarantee").

### 2.4 Manifest tampering / unauthorized re-materialization

- **Action:** a malicious or buggy integration presents hallucinated, stale,
  cross-session, or malformed tokens, or uses the wrong manifest/session/tenant,
  to coax restore into emitting raw values.
- **Must prevent:** re-materialization of any value not authorized by the active
  manifest.
- **Controls:** restore-boundary integrity — restore is treated as a privileged
  egress boundary (deterministic outbound DLP + manifest-integrity enforcement,
  **not** prompt-injection detection). The invariants are in §4 below and in
  [`docs/architecture/restore-boundary.md`](../architecture/restore-boundary.md).

### 2.5 Cross-context correlation via pseudonym reuse

- **Action:** an adversary correlating two logs exploits a stable pseudonym that
  should not be shared across independent contexts.
- **Must prevent:** the same subject mapping to the same token across contexts
  that should be isolated.
- **Controls:** a `Session` is the pseudonym-namespace boundary. Each `Session`
  has fresh per-class counters and a fresh `session_hex`; two sessions never
  share counters or value-keyed lookups. `Scope` chooses *persistence*, not
  *isolation* — sharing one `Session` across logical conversations is the
  documented pitfall (issue #275). See
  [`docs/architecture/session-contract.md`](../architecture/session-contract.md).

### 2.6 Audit-sink isolation bypass via source edits

- **Action:** a contributor edit imports `gaze-audit` symbols into a protected
  path so audit material (and thus restorable metadata) flows into core/restore
  code.
- **Must prevent:** any reference to forbidden audit-sink items inside
  `crates/gaze/src` or `crates/gaze-cli/src/restore`.
- **Controls:** the `gaze_module_isolation` Dylint gate — covered in §3.

---

## 3. Bypass classes — what the Dylint gate covers, and what it does not

The audit-sink isolation gate is the `gaze_module_isolation` Dylint lint
(`xtask/dylint/`), pinned to toolchain `nightly-2025-09-18`. It is **resolver-
based**: it resolves each reference to a `DefId` via `LateContext::qpath_res`
(and `type_dependent_def_id` for method calls, `extern_mod_stmt_cnum` for extern
crates) and flags it when the resolved crate or item is on the forbidden list
inside a protected path. Because it works on resolution, not on source text, it
is not defeated by renaming, aliasing, glob, or macro obfuscation.

**Default protected paths:** `crates/gaze/src`, `crates/gaze-cli/src/restore`.
**Default forbidden crate:** `gaze_audit`.
**Default forbidden items:** `gaze_audit::SqliteLogger`, `::AuditFilter`,
`::AuditLogRow`, `::build_audit_query_sql`, `::AUDIT_RESTRICTED_COLUMNS`.

### 3.1 Covered bypass classes (18 UI fixtures)

The lint ships 18 UI fixtures (`xtask/dylint/ui/`) — 16 positive fixtures, each
proving one bypass class is flagged, and 2 negative fixtures proving the lint
does **not** over-trigger:

**Import-position bypasses**
1. Top-level `use` (`use_top_level.rs`)
2. `use` inside a function body (`use_in_fn_body.rs`)
3. `use` inside an `impl` body (`use_in_impl_body.rs`)
4. `use` inside a trait default method (`use_in_trait_default.rs`)
5. Aliased import (`use_via_alias.rs`)
6. Glob import (`use_via_glob.rs`)
7. `extern crate` (`use_via_extern_crate.rs`)
8. `use` reached via a `const` initializer (`use_via_const_initializer.rs`)

**Path / file-inclusion bypasses**
9. `#[path]` attribute redirection (`use_via_path_attr.rs`)
10. `include!` expanding a forbidden path (`include_expands_forbidden_path.rs`)

**Macro-hygiene bypasses**
11. Macro emitting the reference at the call-site (`use_via_macro_emit_callsite.rs`)

**Type-position bypasses** (forbidden item used as a type, not imported)
12. Struct field type (`type_struct_field.rs`)
13. Function parameter / return type (`type_fn_param_return.rs`)
14. Generic argument (`type_generic_arg.rs`)
15. `PhantomData` type marker (`type_phantom_data.rs`)

**Trait-bound bypasses**
16. Trait bound in a `where` clause (`trait_bound_where_clause.rs`)

**Negative fixtures (must NOT trigger)**
- Clean restore code with no forbidden reference (`clean_no_violation.rs`).
- A macro defined elsewhere whose forbidden reference is def-site-hygienic and
  expands outside a protected path
  (`main/use_via_macro_emit_defsite_negative.rs`) — confirms the lint follows
  resolution and call-site/def-site span hygiene rather than blanket-flagging
  any macro that mentions an audit symbol.

The lint walks `use` items, expression paths, struct-expression paths, method
calls, type paths, trait bounds, and HIR paths, and climbs `source_callsite` /
`parent_callsite` plus the HIR parent chain so a reference is attributed to its
protected-path context even through macro expansion.

### 3.2 What the Dylint gate does NOT cover

The gate is a **source-level audit-sink isolation** control. It is explicitly
not a general PII-leak detector. It does not:

- Detect runtime PII leaks, fail-open recognizers, or detection-coverage gaps —
  those are governed by the recognizer suite and the `ci-feature-matrix` /
  `safety-net-sanity` xtask gates, not this lint.
- Constrain crates outside the configured protected paths, or forbidden items
  beyond the configured list (config is `protected_paths` / `forbidden_crates` /
  `forbidden_items`; widening the contract requires editing that config).
- Prevent audit material from reaching protected code via **dynamic / indirect**
  routes that carry no resolvable `DefId` to a forbidden item — e.g. data passed
  in as `&str`/bytes, reflection-style dispatch, FFI, or values produced by an
  allowed wrapper crate that itself depends on `gaze-audit`.
- Reason across process or network boundaries (IPC, a separate audit service).
- Replace human security review of new `reqwest`/`hyper`/`tokio`/`ureq` edges in
  protected feature graphs, which remain a manual review event.

Complementary gates: `cargo-metadata-audit-isolation` (dependency-graph
isolation of the audit sink) and `cargo deny` (ban rule on the audit dep in the
protected default / `--no-default-features` / safety-net graphs).

---

## 4. Restore-path invariants — when `restore()` MUST fail

Restore is a privileged egress boundary. The active manifest is the **only**
authority for re-materialization. Restore answers an authorization question
against the manifest; it does not infer intent. The following are hard
fail-closed conditions (Phase A, default-on — see
[`docs/architecture/restore-boundary.md`](../architecture/restore-boundary.md)):

1. **Unknown token** — a token-shaped value with no entry in the active manifest
   MUST return a typed restore failure. A token shape is not proof of
   authorization; restore never guesses.
2. **Cross-session / cross-tenant token** — a token known to a different session
   or tenant MUST fail; it MUST NOT resolve against the wrong namespace.
3. **Malformed token** — a structurally invalid token MUST fail closed, not pass
   through as a raw value.
4. **Stale snapshot** — importing a `SensitiveSnapshot` past its persistent-
   session TTL MUST fail with `Error::BlobExpired` (verified, §2.3).
5. **No silent scope expansion** — restore MUST NOT broaden what it
   re-materializes. Identity-sensitive restore-risk policy (Phase C) is deferred
   to v0.11+ and must be explicit, opt-in, and separately authorized.
6. **Lossless round-trip** — for reversible classes the manifest contract
   requires byte-for-byte round-trip; a restore that produces different bytes
   than the original source is a divergence and in scope for the vuln process
   ([`SECURITY.md`](../../SECURITY.md)). `Redact` and `Generalize` are **not**
   reversible by design — use `Tokenize` or `FormatPreserve` when restore is
   required.

Phase B (audit-only, opt-in in v0.10) records — without blocking — evidence of
manifest bypasses, fresh raw structural PII inserted during restore, and
wrong-context restores. Phase D records metadata-only restore telemetry. Neither
writes raw sensitive values to the audit sink.

**Restore-boundary residual risk (unverified):** format-preserving restorability
exists but is not yet covered by a dedicated test across every
`Action::FormatPreserve` path; NER-load fail-closed behavior (missing/corrupt
local model) needs an explicit named test. Both are listed as unverified in
[`docs/security-review.md`](../security-review.md).

---

## 5. Explicit non-guarantees

Carried verbatim in intent from [`docs/security-review.md`](../security-review.md)
so the threat model and the invariant list cannot drift:

1. Gaze does not protect against a compromised LLM echoing tokens verbatim.
2. Gaze does not protect against prompt injection that exfiltrates a
   `SensitiveSnapshot`.
3. Gaze covers only content passed to `Pipeline::redact` or `gaze clean` stdin.
   System prompts, tool schemas, and agent instructions are out of scope.
4. `Redact` and `Generalize` are not reversible.
5. The restore path trusts the snapshot; a stolen snapshot can recover PII.
6. Gaze is not a universal PII detector; coverage depends on configuration.

The restore boundary is **not** an AI-guardrail or prompt-injection-defense
layer. It is deterministic outbound DLP and manifest-integrity enforcement.
Generic prompt injection, jailbreak prevention, semantic adversarial reasoning,
LLM-as-judge gating, and intent classification are explicit non-goals.

---

## 6. Proposed third-party audit scope

> Proposed only. No external audit has been conducted. This section describes
> what an independent reviewer *should* examine if an audit is commissioned, and
> is intended to support an NLnet-style security-audit engagement.

An external auditor should focus on the correctness axes (reliability,
reversibility, trust) at the trust boundaries in §1.1, prioritizing the
**unverified** items above (those are where coverage is asserted by design but
not yet proven by a named test).

**A. Boundary-bypass / fail-closed (axis 1 — reliability)**
- Verify recognizers fail closed, not open, across the protected default,
  `--no-default-features`, and `--all-features` (safety-net) graphs; attempt to
  construct a feature combination that silently disables a detector.
- Probe the opt-in pipeline-optimization paths (skip-gating, capitals heuristic,
  prefix cache, length bucketing) for any mode that drops detection on a
  non-observer path.
- Close the **unverified** gap: a runtime test that `Pipeline::redact` performs
  no network I/O on the clean path.
- Adversarial detection-coverage testing across locales and embedded-JSON
  payloads to characterize real-world recall, not just rule presence.

**B. Token opacity & reversibility (axes 2 & 4)**
- Property-test **token opacity**: prove emitted tokens leak no key material or
  source-value bytes beyond class and ordinal.
- Property-test **format-preserving round-trip** across every
  `Action::FormatPreserve` path for byte-for-byte restorability.
- Review pseudonym derivation for cross-session/cross-tenant unlinkability
  (§2.5) and for resistance to dictionary/correlation attacks on tokens.

**C. Restore-boundary integrity (axes 1 & 2)**
- Exercise every MUST-fail condition in §4 (unknown, cross-session, malformed,
  stale, scope-expansion, lossless divergence) and attempt to find a token that
  re-materializes without a manifest grant.
- Review snapshot handling and the snapshot-trust non-guarantee: confirm TTL
  enforcement and that no code path restores past expiry.

**D. Audit-sink isolation (axis 4)**
- Independently review the `gaze_module_isolation` Dylint lint and its 18
  fixtures; attempt a bypass class not in the fixture set (especially the
  dynamic/indirect routes called out in §3.2).
- Verify `AUDIT_RESTRICTED_COLUMNS` truly bounds query/export and that no raw
  PII or restorable mapping is reachable through the audit surface.
- Confirm the dependency-graph gates (`cargo-metadata-audit-isolation`,
  `cargo deny`) cannot be satisfied while `rusqlite`/`gaze-audit` leaks into a
  protected graph.

**E. MCP tier isolation & chokepoint completeness (axes 1 & 3)**
- Review caller-tier vs tool-tier separation, the sealed `ToolCtx`, `AuthHook`,
  and `SessionIdPolicy` for a path that dispatches a tool above the caller's
  tier or routes PII around the chokepoint
  ([`docs/architecture/mcp-runtime.md`](../architecture/mcp-runtime.md)).

**F. Process & supply chain**
- Review the pinned toolchain / third-party action pins, the vulnerability-
  reporting and coordinated-disclosure process ([`SECURITY.md`](../../SECURITY.md)),
  and reproducibility of the published benchmark and bundle-SHA pins.

### Out of scope for any audit (by design)
Prompt-injection / jailbreak defense, LLM-as-judge gating, intent
classification, adopter code that bypasses the documented chokepoints,
performance-only regressions, and currently-private downstream projects.

---

## References

- [`docs/security-review.md`](../security-review.md) — invariants with named tests (authoritative)
- [`SECURITY.md`](../../SECURITY.md) — reporting, scope, supported versions, disclosure
- [`docs/architecture/restore-boundary.md`](../architecture/restore-boundary.md) — restore-boundary integrity
- [`docs/architecture/session-contract.md`](../architecture/session-contract.md) — session isolation contract
- [`docs/architecture/mcp-runtime.md`](../architecture/mcp-runtime.md) — MCP chokepoint & tier isolation
- [`docs/architecture/safety-nets.md`](../architecture/safety-nets.md) — Pass-3 SafetyNet
- [`docs/policy.md`](../policy.md) — policy surface & fail-closed parsing
- `xtask/dylint/` — `gaze_module_isolation` lint + 18 UI fixtures
- [`AGENTS.md`](../../AGENTS.md) / [`CLAUDE.md`](../../CLAUDE.md) — north star & five axes
