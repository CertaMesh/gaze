# Glossary

Gaze × basemind composed-system vocabulary, captured as terms resolve. Implementation-free by design;
specs live in ADRs and the architecture spec.

## Engine-era fingerprint

A record of which detection engine, token-grammar version, and redaction configuration produced a given
index slice or store generation. Two entries with different fingerprints must not be mixed: a fingerprint
mismatch blocks reuse and signals a rebuild. It is the single consistency gate across stores.

## Class-stable projection

A deterministic spelling of pseudonymization tokens in which each PII class renders identically across
documents and sessions (as opposed to session-scoped tokens whose distinguishing parts vary per session).
The projected form is what gets embedded and what the lexical index scores. Session-scoped tokens and
format-preserving fake values must not reach the index.

## Human-ward rehydration

The restoration of real values from pseudonymized text exclusively toward the document owner, through
reviewed, owner-operated channels (CLI rehydration, owner-only file export). The defining invariant:
no agent-facing surface — including MCP tool results — ever carries rehydrated content. One-way markers
are the opposite extreme: visible redactions that can never be rehydrated by anyone.

## Dual store

The restore-data topology in which one detection pass writes two complementary stores once and only once:
a provenance-canonical manifest (audit-grade: evidence tiers, conflict records, validator reasons) and an
operational rehydration map consumed by the context-layer's existing flows. Drift is impossible by
construction; the engine-era fingerprint gates mixing across generations.
