# Restore-Boundary Integrity

Gaze enforces **manifest-authorized re-materialization** of sensitive data at the restore boundary.

This is **restore-boundary integrity**: only values explicitly authorized by the session manifest may cross from pseudonymous form back into raw form. Restore is a privileged egress boundary, so Gaze treats it as deterministic **outbound DLP** and manifest-integrity enforcement.

This is manifest-integrity enforcement, not prompt-injection detection.

Gaze is NOT trying to determine: "Is this prompt malicious?"

Gaze IS trying to determine: "Was this sensitive value authorized to exist in this restore context?"

That distinction is the core contract for v0.10 restore-boundary work. The restore path must answer an authorization question against the manifest, not infer intent or motive from surrounding text.

## Core Principle

The restore boundary is where pseudonymous content becomes owner-side sensitive data again. That makes restore an egress point, not a normal string substitution helper.

The invariant is:

1. A sensitive value may be re-materialized only when the active restore context has a manifest entry that authorizes that exact token-to-value mapping.
2. Unknown, stale, cross-session, or malformed tokens fail closed instead of being guessed or passed through as raw values.
3. Restore-side checks must be deterministic and auditable. A restore decision must be traceable to the active manifest, the structural recognizer that observed unauthorized raw sensitive data, or restore telemetry metadata.
4. Restore must not silently expand scope. If a later phase wants identity-sensitive policy, it must be explicit, opt-in, and separately approved.

## Phase Summary

| Phase | Scope | v0.10 status |
|---|---|---|
| A | Strict manifest-bound restore: unknown token means typed failure | Core, default-on |
| B | Unauthorized raw-PII detection at restore for structural identifiers | Audit-only, opt-in |
| C | Optional restore-risk rulepack | Deferred v0.11+, identity-sensitive |
| D | Restore audit telemetry, metadata-only | Core, observability foundation |

Phase C is explicitly deferred to v0.11+ and is not part of the v0.10 dispatch set. It is identity-sensitive and must not be dispatched without the user's explicit `lock C` signal.

## Phase A: Strict Manifest-Bound Restore

Phase A makes the manifest the only authority for re-materialization.

Expected behavior:

- Known token in the active manifest: restore to the manifest-authorized value.
- Unknown token: return a typed restore failure.
- Token known to another session or tenant: return a typed restore failure.
- Malformed token: return a typed restore failure.

The important property is that restore never guesses. A token-shaped value without an active manifest grant is not proof of authorization.

## Phase B: Unauthorized Raw-PII Detection

Phase B checks restored output for raw sensitive values that were not authorized by the manifest. In v0.10, this is audit-only and opt-in.

Phase B is scoped to structural identifiers such as email addresses, phone numbers, IBANs, payment card numbers, and API-key-shaped secrets. It should reuse deterministic recognizer behavior rather than adding open-ended judgment layers.

Phase B distinguishes:

- Manifest bypass: a sensitive value appears raw even though it should have been mediated by a manifest entry.
- Fresh raw sensitive data: a model, tool, or integration inserts a new structural sensitive value during restore.
- Wrong-context restore: an integration uses the wrong manifest, session, or tenant boundary.

Blocking behavior is deferred until telemetry shows an acceptable false-positive profile. v0.10 Phase B records evidence without turning restore into a broad judgment layer.

## Phase D: Restore Audit Telemetry

Phase D records metadata-only restore events so adopters can inspect restore-boundary behavior without storing raw sensitive values in the audit sink.

Telemetry should support questions like:

- Which restore context was active?
- Which manifest entry authorized a re-materialization?
- Which typed failure occurred?
- Which structural recognizer observed unauthorized raw sensitive data?
- Was Phase B running in audit-only mode?

The audit surface must preserve Gaze's existing trust posture: no raw sensitive values in audit rows, explicit provenance, and closed typed outcomes where practical.

## Risks Addressed By Phase A And Phase B

Phase A and Phase B address these restore-boundary risk classes:

- Hallucinated tokens.
- Manifest bypasses.
- Raw PII emitted directly by the model.
- Accidental provider or context leakage.
- Wrong-session restore integration bugs.
- Unauthorized re-materialization of sensitive values.

## Non-Goals

This initiative does NOT attempt to solve:

- generic prompt injection
- jailbreak prevention
- semantic adversarial reasoning
- LLM-as-judge gating
- malicious intent classification
- agent policy alignment
- tool permission enforcement

These are outside the v0.10 restore-boundary contract. Pulling them into the core restore path would blur Gaze's role as a reversible PII pseudonymization runtime and weaken the deterministic manifest contract.

## Design Constraints

- Fail closed on missing or mismatched manifest authority.
- Keep core restore deterministic.
- Keep restore decisions auditable without writing raw sensitive values to telemetry.
- Keep Phase B audit-only in v0.10.
- Keep Phase C deferred until v0.11+ and require explicit user `lock C` authorization before dispatch.
- Preserve Gaze's identity as a PII pseudonymization runtime for agentic workflows.
