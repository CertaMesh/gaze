# Security Policy

## Reporting a Vulnerability

Report PII leaks, recognizer bypasses, restore divergences and chokepoint escapes
privately to security@certamesh.com. Do not open a public GitHub issue.
Request a PGP key at that address if needed.

We acknowledge reports within 72 hours and aim to triage within 7 days.

## Scope

In scope:

- PII reaching an LLM outside the manifest contract through `gaze-mcp-core`,
  `gaze-mcp-rmcp`, `gaze` / `gaze-pii`, `gaze-recognizers`, `gaze-cli` or `gaze-assembly`.
- Restore differing from the original bytes on lossless classes.
- Audit-sink isolation bypasses (`gaze_module_isolation` Dylint).
- Recognizer fail-open behavior in default, `--no-default-features` or safety-net graphs.
- MCP caller-tier/tool-tier isolation bypasses.

Out of scope: adopter code bypassing `Pipeline` / MCP `ToolCtx`, performance
regressions without reliability impact, and private downstream projects
(which will be tracked separately when public).

## Supported versions

Security fixes cover the latest `0.15.x` minor (`v0.15.1`). The last `0.14.x`
minor receives one-cycle high-severity backports during the `0.15.x` upgrade.
Earlier versions receive no backports.

## Coordinated disclosure

High-severity findings have a 90-day disclosure window from acknowledgment,
extendable by mutual agreement. Advisories and CHANGELOG credit reporters
unless they request anonymity.

## Bug bounty

There is no formal bug bounty program.
