# Security Policy

## Reporting a Vulnerability

If you believe you have found a security vulnerability in Gaze — whether a
PII leak, a recognizer bypass, a manifest-restore divergence, or a
chokepoint escape — please report it privately. **Do not open a public
GitHub issue.**

Email: **security@empiretwo.dev**
PGP: optional; request a key via the same address.

We will acknowledge receipt within 72 hours and aim to provide a triage
verdict within 7 days.

## Scope

In scope:

- Any path through `gaze-mcp-core`, `gaze-mcp-rmcp`, the `gaze` (umbrella)
  / `gaze-pii` runtime, `gaze-recognizers`, `gaze-cli`, or `gaze-assembly`
  that allows PII to reach an LLM outside the manifest contract.
- Restore-path divergences that produce different bytes than the original
  source (manifest contract requires byte-for-byte round-trip on lossless
  classes).
- Audit-sink isolation bypasses (the `gaze_module_isolation` Dylint gate).
- Recognizer fail-open regressions on the protected default,
  `--no-default-features`, and safety-net feature graphs.
- Tier-isolation bypasses in MCP tool dispatch (caller-tier vs tool-tier).

Out of scope:

- Issues only reproducible in adopter code that bypasses the documented
  `Pipeline` / MCP `ToolCtx` chokepoints.
- Performance-only regressions with no reliability impact.
- Issues in `gaze-lens` (separate repo: `EmpireTwo/gaze-lens`) — please
  report there.

## Supported versions

We currently support security fixes only on the latest minor of the `0.6.x`
series and (when released) the latest minor of `0.7.x`. Earlier versions
do not receive backports.

## Coordinated disclosure

For high-severity findings we follow a 90-day coordinated-disclosure
window from the date of acknowledgment, extendable by mutual agreement.
We will credit reporters in the security advisory and CHANGELOG unless
they request anonymity.

## Bug bounty

There is no formal bug bounty program at this time.
