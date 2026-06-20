# How-to guides

Task-oriented recipes. Each guide assumes you already know what you want to achieve and
walks you to that goal. If Gaze is new to you, start with the
[Getting Started tutorial](../tutorials/getting-started.md) first. For exact behavior and
schemas, see the [reference](../reference/index.md).

## Proxy

- **[Set up the proxy](proxy/set-up-proxy.md)** — route OpenAI, Anthropic, or Gemini SDK
  traffic through Gaze's API-key HTTP chokepoint so PII is tokenized in flight.

## MCP

- **[Set up the MCP adapter](mcp/set-up-mcp-adapter.md)** — expose Gaze's document tools to
  an MCP client and serve the stdio chokepoint.
- **[Set up the MCP bridge](mcp/set-up-mcp-bridge.md)** — route an agent through Gaze before
  forwarding approved tool calls to downstream MCP servers.

## Daemon

- **[Run the daemon](daemon/run-daemon.md)** — start the long-lived JSONL/stdio daemon for
  multi-session redaction with a per-session manifest registry.

## Document ingestion

- **[Ingest documents](document/ingest-documents.md)** — turn PNG/JPG/PDF input into a
  redacted `SafeBundle` (clean Markdown + manifest + report) via OCR.

## Safety net

- **[Set up the Kiji safety net](safety-net/set-up-kiji-safetynet.md)** — install and pin
  the Kiji DistilBERT bundle and turn on the observer-only safety net.

## Policy

- **[Write custom recognizers](policy/custom-recognizers.md)** — add tenant-specific PII
  classes (order IDs, song names, artist names) with policy rules and recognizers.

## Compliance

- **[GDPR adopter guidance](compliance/gdpr-adopter-guidance.md)** — how Gaze maps onto
  GDPR pseudonymization obligations, and what stays the adopter's responsibility.

## Maintainers

- **[Release process](maintainers/release-process.md)** — the gate sequence and tagging
  workflow for cutting a Gaze release.
