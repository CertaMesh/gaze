# Restore-Boundary Integrity

Gaze enforces manifest-authorized re-materialization of sensitive data at the restore boundary.

This is manifest-integrity enforcement, not prompt-injection detection.

Gaze is NOT trying to determine: "Is this prompt malicious?"

Gaze IS trying to determine: "Was this sensitive value authorized to exist in this restore context?"

The restore boundary is a privileged egress point where pseudonymous content becomes owner-side sensitive data again. Restore decisions must be deterministic, metadata-only in audit sinks, and traceable to the active manifest or a structural recognizer.

## Phase Summary

| Phase | Scope | v0.10 status |
|---|---|---|
| A | Strict manifest-bound restore: unknown token means typed failure | Core, default-on |
| B | Unauthorized raw-PII detection at restore for structural identifiers | Audit-only |
| C | Optional restore-risk rulepack | Deferred v0.11+, identity-sensitive |
| D | Restore audit telemetry, metadata-only | Core, observability foundation |

## Phase A: Strict Manifest-Bound Restore

Phase A makes the manifest the only authority for re-materialization.

- Known token in the active manifest restores to the manifest-authorized value.
- Unknown, stale, cross-session, or malformed token-shaped strings fail closed with a typed restore error.
- Restore never guesses or falls back to string-map behavior outside the manifest.

## Phase B: Unauthorized Raw-PII Detection

Phase B runs an audit-only structural scan before token replacement. That ordering matters: manifest-authorized token restoration must not self-trigger raw-PII findings after restore inserts owner-side values.

Phase B is limited to deterministic structural identifiers:

- email
- phone
- IBAN
- credit-card
- API-key-like strings

Names, entities, NER, semantic intent detection, LLM-as-judge, classifiers, and prompt-injection defenses are out of scope.

Phase B emits typed events:

- `ManifestBypass`: manifest-known raw PII appears outside token form.
- `FreshPiiDetected`: raw structural PII appears that was never present in the manifest.

In v0.10 these events are audit-only and default-off unless the adopter enables restore-boundary DLP audit. Restore continues after raw-PII findings. Blocking modes are deferred to v0.11+ after telemetry evidence.

Audit rows must remain metadata-only. They may store class, event kind, byte location, and a hash of the detected value, but must not persist raw detected values.

## Non-Goals

This initiative does not attempt to solve generic prompt injection, jailbreak prevention, semantic adversarial reasoning, malicious intent classification, agent policy alignment, tool permission enforcement, LLM-as-judge moderation, or classifier-heavy guardrail architecture.
