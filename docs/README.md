# Gaze documentation

Gaze is a reliable, reversible PII pseudonymization runtime for agentic workflows. It
replaces personal data with restorable tokens *before* text reaches an LLM, then restores
the original values from the model's response. The goal is a hard one: no PII leaks
between your agent and your data owner, and nothing is lost in the round trip.

## How good is it

The measured numbers for the current release, the corpora and scoring contract
behind them, and the commands to reproduce every figure are in
[Gaze Benchmarks](reference/benchmarks/README.md).

## How it works

[How Gaze works](explanation/how-gaze-works.md) follows one document from input to restore in plain English, with the modes, the limits, and a glossary.

## Quickstart

New here? Start with the [Getting Started tutorial](tutorials/getting-started.md) — a
working redact → send → restore loop in about ten minutes.

## Find your way

These docs follow the [Diátaxis](https://diataxis.fr/) model. Each page serves one job;
pick the column that matches *why* you're here:

- **[Tutorials](tutorials/README.md)** — learning-oriented. Begin here if Gaze is new to you.
- **[How-to guides](how-to/README.md)** — task-oriented recipes for a specific goal you already have.
- **[Reference](reference/README.md)** — dry, complete descriptions: the CLI, policy schema, crates, metrics, audit columns, benchmarks.
- **[Explanation](explanation/README.md)** — the design contracts: *why* the never-leak and restore guarantees hold.

## Docs by feature

Each row is a Gaze capability; each column is the kind of page you want. A dash (—) means
there is no dedicated page in that mode yet — the section index for that column lists
everything in full.

| Feature | Tutorial | How-to | Reference | Explanation |
|---|---|---|---|---|
| Core redaction & restore | [Getting Started](tutorials/getting-started.md) | [Use the setup policy from Rust](how-to/rust-library.md) | [CLI](reference/cli.md) · [Metrics](reference/metrics.md) | [Restore boundary](explanation/core/restore-boundary.md) · [Session contract](explanation/core/session-contract.md) |
| CLI (`gaze clean` / `restore`) | [Getting Started](tutorials/getting-started.md) | — | [CLI reference](reference/cli.md) | — |
| Policy & recognizers | — | [Custom recognizers](how-to/policy/custom-recognizers.md) · [Policy by hand](how-to/manual-policy.md) · [Policy examples](how-to/policy/policy-examples.md) | [Policy schema](reference/policy.md) | [Locale chain](explanation/policy/locale-chain.md) · [detection contracts](explanation/README.md#detection-and-conflict-resolution) |
| Safety nets / NER | — | [Safety net setup in the CLI README](../crates/gaze-cli/README.md#safety-net) | [Benchmarks](reference/benchmarks/README.md) | [Safety nets](explanation/safety-net/safety-nets.md) · [Modes](explanation/safety-net/safety-net-modes.md) |
| Proxy (OpenAI / Anthropic / Gemini) | — | [Set up the proxy](how-to/proxy/set-up-proxy.md) | — | [Proxy runtime](explanation/proxy/proxy-runtime.md) · [Strict Anthropic Messages contract](explanation/proxy/anthropic-messages-contract.md) |
| Inspection dashboard (opt-in) | — | [Run the local dashboard](how-to/dashboard/run-local-dashboard.md) | [Accessibility & visual verification](reference/dashboard/accessibility-and-visual-verification.md) · [Browser security](reference/dashboard/browser-security.md) | [Dashboard trust boundary](explanation/dashboard/trust-boundary.md) |
| MCP adapter & runtime | — | [Set up the MCP adapter](how-to/mcp/set-up-mcp-adapter.md) | — | [MCP runtime](explanation/mcp/mcp-runtime.md) |
| MCP bridge | — | [Set up the MCP bridge](how-to/mcp/set-up-mcp-bridge.md) | — | [MCP bridge](explanation/mcp/mcp-bridge.md) |
| Daemon mode | — | [Run the daemon](how-to/daemon/run-daemon.md) | — | [Daemon mode](explanation/daemon/daemon-mode.md) |
| Document ingestion / OCR | — | [Ingest documents](how-to/document/ingest-documents.md) | — | [Document extension](explanation/document/document-extension.md) |
| Audit | — | [Audit and restore](how-to/audit-and-restore.md) | [Metrics & audit columns](reference/metrics.md) · [Crates](reference/crates.md) | [Ambiguity side-channel](explanation/detection/ambiguity-side-channel.md) |
| Compliance / GDPR | — | [GDPR adopter guidance](how-to/compliance/gdpr-adopter-guidance.md) | [Security review](reference/security-review.md) · [Accessibility](reference/accessibility.md) | [Governance](explanation/governance.md) |
| Project & crates | — | [Release process](how-to/maintainers/release-process.md) | [Crate map](reference/crates.md) | [xtask gates](explanation/contributing/xtask-gates.md) |

## Adapters

Framework adapters (for example **gaze-laravel** and **gaze-lens**) live in separate
repositories so the core runtime stays dependency-light. See the
[AI support drafts in production](explanation/support-drafts-in-production.md) for the
Laravel adapter in use, and the [project README](../README.md) for the rest.

## Project root documents

[README](../README.md) · [ARCHITECTURE](../ARCHITECTURE.md) · [AGENTS — north star & five axes](../AGENTS.md) · [SECURITY](../SECURITY.md) · [UPGRADE](../UPGRADE.md) · [CHANGELOG](../CHANGELOG.md) · [CONTRIBUTING](../CONTRIBUTING.md)
