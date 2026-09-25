# Restore-boundary integrity

Gaze enforces **manifest-authorized re-materialization** of sensitive data at the restore boundary.

This is **restore-boundary integrity**: only values explicitly authorized by the session manifest may cross from pseudonymous form back into raw form. Restore is a privileged egress boundary, so Gaze treats it as deterministic **outbound DLP** and manifest-integrity enforcement.

This is manifest-integrity enforcement, not prompt-injection detection.

Gaze is NOT trying to determine: "Is this prompt malicious?"

Gaze IS trying to determine: "Was this sensitive value authorized to exist in this restore context?"

That distinction is the core contract for v0.10 restore-boundary work. The restore path must answer an authorization question against the manifest, not infer intent or motive from surrounding text.

## What restore authorizes

The restore boundary is where pseudonymous content becomes owner-side sensitive data again. That makes restore an egress point, not a normal string substitution helper.

The invariant is:

1. A sensitive value may be re-materialized only when the active restore context has a manifest entry that authorizes that exact token-to-value mapping.
2. Unmapped canonical placeholders and incomplete prefixed wrappers fail closed. Session strict APIs also reject malformed and nested token syntax. Bare identifier-like text is an audit signal, never authority to re-materialize a value.
3. Restore-side checks must be deterministic and auditable. A restore decision must be traceable to the active manifest, the structural recognizer that observed unauthorized raw sensitive data, or restore telemetry metadata.
4. Restore must not silently expand scope. If a later phase wants identity-sensitive policy, it must be explicit, opt-in, and separately approved.

## Phase status

| Phase | Scope | v0.10 status |
|---|---|---|
| A | Strict manifest-bound restore: unknown token means typed failure | Core, default-on |
| B | Unauthorized raw-PII detection at restore for structural identifiers | Audit-only, opt-in |
| C | Optional restore-risk rulepack | Deferred v0.11+, identity-sensitive |
| D | Restore audit telemetry, metadata-only | Core, observability foundation |

Phase C is explicitly deferred to v0.11+. It is identity-sensitive and is not implemented.

## Phase A: strict manifest-bound restore

Phase A makes the manifest the only authority for re-materialization.

Expected behavior:

- Known token in the active manifest: restore to the manifest-authorized value.
- Unmapped canonical placeholder, including own-prefix, foreign-prefix, legacy wrapped, and legacy emitted formats: return a typed restore failure.
- Incomplete prefixed wrapper: return a typed restore failure.
- Token known to another session or tenant: return a typed restore failure.
- Malformed token: return a typed restore failure.

Restore never guesses a mapping. Strict no longer blocks on bare identifier-shaped
literals outside authorized ranges (moved to audit-only `manifest_bypass`); it still
blocks on any unmapped canonical placeholder (own-prefix, foreign-prefix, legacy
wrapped) and on incomplete prefixed wrappers. Legacy emitted formats such as
`location_7`, `custom:class_alpha_1`, and `email1@gaze-fake.invalid` also remain
blocking. Only broad bare identifiers such as `Kunde_7`, `ORDER_12345`, and `run_1`
move to audit-only.

This change deliberately removes the former bare-identifier rejection boundary in
pipeline, Session/MCP, and CLI restore. The trade improves exact round-trips and
ordinary prose handling without granting any new token-to-value mapping. It is
a narrower heuristic rejection net, not evidence of improved PII detection.

The classifier is owned by `Session::assess_restore_text`. It restores exact
manifest keys and scans the result using one immutable session view. Matches
fully contained in `authorized_output_ranges`, the UTF-8 output ranges written
by authorized substitutions, do not count as unknowns or manifest bypasses.
Adjacent unknowns and matches crossing a substitution boundary remain subject
to classification. No original-text allowlist or new snapshot state is stored.

For known bare format-preserving tokens beginning with the session's eight-hex
prefix, leading ASCII or Unicode word adjacency is allowed: `rec_a7f3b8e2:name_1`
restores the exact known token after `rec_`. The mapping must exist in the active
manifest. Matching uses the original input once, longest known keys first; raw
values containing token-like text are never recursively substituted.

A trailing Unicode word boundary is still required. `name_1` cannot consume the
start of `name_10`, `name_1x`, or `name_1é`. A known longer key restores in full;
an unknown longer canonical token remains subject to strict rejection. Arbitrary
word-suffixed text that is not a canonical token remains unchanged, without a
new promise that the lexical classifier will reject it. Family labels also allow
hyphens, so a known `custom:family:tenant_1` does not consume the start of
`custom:family:tenant_1-other_999`. A hyphen immediately after a family token is
ambiguous and therefore prevents substitution; separate prose punctuation with
whitespace when needed. Other punctuation boundaries and legacy/email-shaped
leading-boundary rules remain unchanged.

CLI, pipeline restore, and Session strict restore share this assessment. Staged
transaction protection uses the same known-token ranges so its provenance proof
agrees with owner-side restore. Strict syntax validation and unknown-token
classification remain separate from exact-known matching.

The API failure contracts remain distinct:

- `Pipeline::restore_with_policy_telemetry` returns restored text and telemetry.
  A positive `unknown_token_count` produces `failed` under Strict or `partial`
  under Lenient; zero produces the existing exact spelling `success`.
- `Session::restore_strict_text`, its provenance/events variants, and the MCP
  operator `restore_strict` tool return a typed error on unmapped canonical
  placeholders and incomplete prefixed wrappers. Session strict parsing also retains malformed/nested input rejection.
- CLI strict restore exits 3 for unmapped canonical placeholders and incomplete
  prefixed wrappers. Tolerant restore
  preserves them and returns warnings plus `partial` telemetry when requested.
  CLI uses the same assessment for output, warnings, telemetry, and audit.
- Transaction token validation and committed-snapshot restore used by the proxy
  retain their separate strict contract. Proxy residual DLP is unchanged.

A `success` decision is a restore-classification result, not a byte-equality
claim or a detection-quality certificate. Byte-exact inverse equality and PII
detection metrics must be evaluated independently.

## Phase B: unauthorized raw-PII detection

Phase B checks restored output for raw sensitive values that were not authorized by the manifest. In v0.10, this is audit-only and opt-in.

Phase B is scoped to structural identifiers such as email addresses, phone numbers, IBANs, payment card numbers, and API-key-shaped secrets. It should reuse deterministic recognizer behavior rather than adding open-ended judgment layers.

Phase B distinguishes:

- Manifest bypass: a sensitive value appears raw even though it should have been mediated by a manifest entry.
- Fresh raw sensitive data: a model, tool, or integration inserts a new structural sensitive value during restore.
- Wrong-context restore: an integration uses the wrong manifest, session, or tenant boundary.

Blocking behavior is deferred until telemetry shows an acceptable false-positive
profile. Structural raw-PII checks remain audit-only and opt-in. The lightweight
token-shape audit scan in restore telemetry always runs: `manifest_bypass_count`
counts broad bare identifier shapes outside authorized substitutions. It is a lexical
suspicion count, not proof that raw PII bypassed the manifest. It never drives
the Strict decision. `trap_shape_count` counts all unprefixed trap matches,
including canonical legacy shapes and those inside authorized values. A bare literal can therefore produce
`unknown_token_count = 0`, `manifest_bypass_count = 1`, and `success`.

Neither counter claims that a fresh-PII detector executed. The fresh-PII phase bit
remains unset when that detector did not run.

## Phase D: restore audit telemetry

Phase D records metadata-only restore events so adopters can inspect restore-boundary behavior without storing raw sensitive values in the audit sink.

Telemetry should support questions like:

- Which restore context was active?
- Which manifest entry authorized a re-materialization?
- Which typed failure occurred?
- Which structural recognizer observed unauthorized raw sensitive data?
- Was Phase B running in audit-only mode?

The audit surface contains counts and existing decision/policy spellings, never
matched text, offsets, original values, or hashes of those values. The additive
`restore_trap_shape_count` audit column is nullable for historical rows; existing
rows are not rewritten. Telemetry JSON defaults a missing `trap_shape_count` to
zero for compatibility. Session snapshot payload versions and token grammar
are unchanged. See the [metrics reference](../../reference/metrics.md#restore-telemetry)
for field semantics.

## Risks addressed by phases A and B

Phase A and Phase B address these restore-boundary risk classes:

- Hallucinated tokens.
- Manifest bypasses.
- Raw PII emitted directly by the model.
- Accidental provider or context leakage.
- Wrong-session restore integration bugs.
- Unauthorized re-materialization of sensitive values.

## Out of scope

This initiative does NOT attempt to solve:

- generic prompt injection
- jailbreak prevention
- semantic adversarial reasoning
- LLM-as-judge gating
- malicious intent classification
- agent policy alignment
- tool permission enforcement

These are outside the v0.10 restore-boundary contract. Pulling them into the core restore path would blur Gaze's role as a reversible PII pseudonymization runtime and weaken the deterministic manifest contract.

## Design constraints

- Fail closed on missing or mismatched manifest authority.
- Keep core restore deterministic.
- Keep restore decisions auditable without writing raw sensitive values to telemetry.
- Keep Phase B audit-only in v0.10.
- Keep Phase C deferred until v0.11+; it is identity-sensitive and needs its own explicit approval.
- Preserve Gaze's identity as a PII pseudonymization runtime for agentic workflows.
