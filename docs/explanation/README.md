# Explanation

These pages explain Gaze's contracts and runtime designs.
For tasks, use the [how-to guides](../how-to/README.md); for exact APIs and flags,
use the [reference](../reference/README.md). [Architecture](../../ARCHITECTURE.md)
maps the crates.

## Core

- [How Gaze works](how-gaze-works.md)
- [Feature comparison](feature-comparison.md)
- [AI support drafts in production](support-drafts-in-production.md)
- [Restore boundary](core/restore-boundary.md)
- [Session contract](core/session-contract.md)

## Detection and conflict resolution

- [Feedback loop](detection/feedback-loop.md)
- [Validator veto](detection/validator-veto.md)
- [Repeat-value sweep](detection/manifest-sweep.md)
- [Caller-known record context](detection/known-record-context.md)
- [Collision family](detection/collision-family.md)
- [Anchor resolution](detection/anchor-resolution.md)
- [Ambiguity side-channel](detection/ambiguity-side-channel.md)
- [NER fail-closed](detection/ner-failclosed.md)
- [Recognizer normalizer spans](detection/recognizer-normalizer-spans.md)

## Policy

- [Locale chain](policy/locale-chain.md)

## Safety nets

- [Safety nets](safety-net/safety-nets.md)
- [Safety-net modes](safety-net/safety-net-modes.md)
- [OpenAI Privacy Filter adapter](safety-net/opf-adapter.md)
- [Windows subprocess pipe ownership](safety-net/windows-subprocess-io.md)
- [Safety-net modes: v0.8 design record](safety-net/safety-net-modes-design.md)

## Pipeline

- [Tier-4 pipeline gating](pipeline/tier4-pipeline-gating.md)

## Proxy

- [Proxy runtime](proxy/proxy-runtime.md)
- [Strict Anthropic Messages contract](proxy/anthropic-messages-contract.md)

## Dashboard

- [Dashboard trust boundary](dashboard/trust-boundary.md)

## MCP

- [MCP runtime](mcp/mcp-runtime.md)
- [MCP bridge](mcp/mcp-bridge.md)

## Daemon

- [Daemon mode](daemon/daemon-mode.md)

## Document

- [Document extension](document/document-extension.md)

## Project

- [Governance](governance.md)
- [xtask gates](contributing/xtask-gates.md)
