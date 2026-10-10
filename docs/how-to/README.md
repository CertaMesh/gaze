# How-to guides

Start with [Getting Started](../tutorials/getting-started.md) if Gaze is new to you.
Use the [reference](../reference/README.md) for exact behavior and schemas.

## Library and audit

- [Evaluate annotated data](evaluate-annotated-data.md): measure leaks and false positives on owner-side JSONL.
- [Use the setup policy from Rust](rust-library.md): build a Nym-enabled pipeline.
- [Audit and restore](audit-and-restore.md): query, export, purge, and restore.

## Policy

- [Custom recognizers](policy/custom-recognizers.md): detect tenant-specific PII.
- [Manual setup](manual-policy.md): add rules, NER, and safety nets.
- [Policy examples](policy/policy-examples.md): four complete TOML files.

## Proxy

[Set up the proxy](proxy/set-up-proxy.md) for OpenAI, Anthropic, or Gemini API-key traffic.

## MCP

- [MCP adapter](mcp/set-up-mcp-adapter.md): expose Gaze document tools over stdio.
- [MCP bridge](mcp/set-up-mcp-bridge.md): protect calls to downstream servers.

## Daemon

[Run the daemon](daemon/run-daemon.md) for multi-session JSONL/stdio cleaning.

## Document ingestion

[Ingest documents](document/ingest-documents.md) from PNG/JPG/PDF into a split SafeBundle.

## Dashboard

[Run the local dashboard](dashboard/run-local-dashboard.md) in a Rust proxy host.
CLI: `gaze proxy serve --dashboard`.

## Compliance

[GDPR guidance](compliance/gdpr-adopter-guidance.md): Gaze controls and adopter duties.

## Maintainers

[Release process](maintainers/release-process.md): gates, publishing, and tags.
