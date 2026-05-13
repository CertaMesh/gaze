# Gaze Security Review

This page summarizes the security invariants reviewers should verify before approving
Gaze for production use. Each shipped claim cites a named test. Claims without named
coverage stay in the unverified bucket until the suite proves them.

---

## Invariants

### Invariant 1 — Audit log is metadata-only; never stores raw PII

**Claim:** audit query/export surfaces only metadata. Raw document text, restored PII,
and emitted token payloads must not appear in audit output.

**Enforcement:** `SqliteLogger` records redaction metadata, and audit query/export uses
the restricted column set exposed as `AUDIT_RESTRICTED_COLUMNS`.

**Verified by:** `crates/gaze-cli/tests/cli_pipe.rs:s4_audit_export_does_not_return_raw_pii`

### Invariant 2 — Unknown validator fails closed

**Claim:** a rulepack that names an unsupported validator must fail before detection
runs. It must not silently drop validation or proceed with a weaker recognizer.

**Enforcement:** validator parsing returns `RulepackError::UnsupportedValidator` /
`RecognizerError::UnsupportedValidator` for unsupported names.

**Verified by:** `crates/gaze-recognizers/tests/no_phone_parser_fail_closed.rs:phone_validators_fail_closed_at_rulepack_load_without_phone_parser`

### Invariant 3 — Unknown TOML policy key fails closed

**Claim:** policy files reject unknown keys. A typo in policy TOML returns an error
instead of silently loading a partial policy.

**Enforcement:** policy structures use `serde(deny_unknown_fields)`.

**Verified by:** `crates/gaze/src/policy.rs:rejects_unknown_keys`

### Invariant 4 — Session blob TTL is enforced

**Claim:** importing a `SensitiveSnapshot` after its persistent-session TTL expires
returns `Error::BlobExpired { .. }`.

**Enforcement:** `Session::import` checks the snapshot issue time against the embedded
TTL before restoring the session.

**Verified by:** `crates/gaze/src/session.rs:import_rejects_expired_persistent_snapshot`

### Invariant 5 — Clean path has no network client dependency

**Claim:** Gaze's normal clean path does not add network client dependencies. Networked
safety-net behavior is opt-in and feature-gated; reviewers should treat any new
`reqwest`, `hyper`, `tokio`, or `ureq` edge in protected feature graphs as a security
review event.

**Enforcement:** `gaze` core has no direct network client dependency, and the xtask
metadata gate rejects new prohibited network clients in the safety-net base graph.

**Verified by:** `crates/xtask/src/cargo_metadata_audit_isolation.rs:safety_net_base_rejects_new_network_clients_from_gaze_types`

---

## Unverified — confirm before shipping

These claims are security-relevant but are not yet proven by a named test.

- **Token opacity:** token shape constrains the surface form, but no property test
  proves tokens leak no key material or source value bytes beyond class and ordinal.
- **Format-preserving restorability:** restore round-trip behavior exists, but no
  dedicated test covers every `Action::FormatPreserve` path.
- **NER-load fail-closed:** missing or corrupt local model load should fail closed, but
  the suite needs an explicit named test for that failure mode.
- **Strict no-network clean execution:** current evidence covers dependency shape, not
  a runtime test proving `Pipeline::redact` cannot perform network I/O.

---

## What Gaze Does Not Guarantee

1. Gaze does not protect against a compromised LLM echoing tokens verbatim.
2. Gaze does not protect against prompt injection that exfiltrates `SensitiveSnapshot`.
3. Gaze covers only content passed to `Pipeline::redact` or `gaze clean` stdin.
   System prompts, tool schemas, and agent instructions are out of scope.
4. `Redact` and `Generalize` are not reversible. Use `Tokenize` or `FormatPreserve`
   when restore is required.
5. The restore path trusts the snapshot. A stolen `SensitiveSnapshot` can recover
   original PII, so callers must encrypt snapshots at rest and keep them away from LLMs.
6. Gaze is not a universal PII detector. Coverage depends on active rulepacks, locale
   chain, custom recognizers, and optional safety nets.

---

## Threat Model

See [`gaze-threat-model.md`](https://github.com/PIInuts/business/blob/main/research/gaze-threat-model.md)
(hosted in `PIInuts/business:research/`) for trust boundaries, adversary model,
and broader attack-surface notes.
