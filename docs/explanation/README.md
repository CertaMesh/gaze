# Explanation

Understanding-oriented. These pages explain *why* Gaze works the way it does — the contracts
behind the never-leak and reversibility guarantees, the conflict-resolution model, and the
runtime designs. They are for when you want the reasoning, not a recipe. For step-by-step
tasks see the [how-to guides](../how-to/README.md); for exact surfaces see the
[reference](../reference/README.md). The repo-root [ARCHITECTURE](../../ARCHITECTURE.md) gives
the crate-level overview these deep-dives sit under.

## Core

The redact ↔ restore contract that everything else protects.

- **[Restore boundary](core/restore-boundary.md)** — what the manifest-first restore path
  guarantees and where reversibility ends.
- **[Session contract](core/session-contract.md)** — the isolation boundary a `Session`
  owns, snapshot/import rules, and the common pitfalls.

## Detection & conflict resolution

How Gaze decides what is PII and what wins when recognizers disagree — the trust-by-evidence
axis in practice.

- **[Feedback loop](detection/feedback-loop.md)** — the coverage feedback loop behind detection completeness.
- **[Validator veto](detection/validator-veto.md)** — how validator-backed recognizer failures are rejected before conflict resolution.
- **[Collision family](detection/collision-family.md)** — cross-class recognizer rivalries and how family policy resolves them.
- **[Anchor resolution](detection/anchor-resolution.md)** — mandatory-anchor resolution and the fail-closed family token.
- **[Ambiguity side-channel](detection/ambiguity-side-channel.md)** — the optional validator/ambiguity metadata carried into the audit log.
- **[NER fail-closed](detection/ner-failclosed.md)** — why a missing or failed NER recognizer fails closed rather than silently degrading.
- **[Recognizer normalizer spans](detection/recognizer-normalizer-spans.md)** — why a normalizer must preserve the original byte span so restore stays exact (an axis-2 reversibility invariant).

## Policy

- **[Locale chain](policy/locale-chain.md)** — the four-tier locale resolution that gates recognizers.

## Safety nets

- **[Safety nets](safety-net/safety-nets.md)** — the Pass-3 observer-only check that runs against already-tokenized output without touching the manifest.
- **[Safety-net modes](safety-net/safety-net-modes.md)** — resolve, redact, and fallback modes and their typed fallback reasons.

## Pipeline

- **[Tier-4 pipeline gating](pipeline/tier4-pipeline-gating.md)** — the opt-in skip-gating, capitals heuristic, prefix cache, and length-bucketing optimizations.

## Proxy

- **[Proxy runtime](proxy/proxy-runtime.md)** — the API-key chokepoint design for the OpenAI, Anthropic, and Gemini base-URL paths.
- **[Strict Anthropic Messages contract](proxy/anthropic-messages-contract.md)** — the exact direct route, headers, admitted JSON/SSE surfaces, proof boundary, inspection trust model, migration, and retained official-SDK gate.

## MCP

- **[MCP runtime](mcp/mcp-runtime.md)** — the transport-free MCP-shaped chokepoint runtime.
- **[MCP bridge](mcp/mcp-bridge.md)** — the optional, policy-gated MCP bridge and its trust-inversion model.

## Daemon

- **[Daemon mode](daemon/daemon-mode.md)** — the JSONL/stdio protocol, per-session manifest registry, and eviction model.

## Document

- **[Document extension](document/document-extension.md)** — the document-ingestion design: OCR → redact → `SafeBundle`.

## Project

- **[Governance](governance.md)** — who decides what, the DCO-without-CLA model, and the open-detection commitment.
- **[xtask gates](contributing/xtask-gates.md)** — the gate runner and the audit-sink protected-path enforcement design.
