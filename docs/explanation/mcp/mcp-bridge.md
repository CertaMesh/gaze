# MCP bridge architecture

`gaze-mcp-bridge` exposes Gaze as the agent's only MCP server and calls real
downstream servers as an MCP client.

## When to use the bridge

Use it for side effects such as email, calendar, filesystem, and computer use.
It restores tokens only in sensitive fields policy allows, forwards the call,
then protects text results. Resources/prompts are discoverable but denied by
default; only tool calls are proxied.

## Trust model

The agent is untrusted. Restoring a wrong value can inject PII into a real
side effect, so bridge egress is stricter than core redaction. Missing auth or
session identity, unknown tokens, unsupported blocks, oversized responses,
and audit failure all deny.

## Dispatch order

`BridgeHost` implements `gaze_mcp_core::DispatchHost`, so it does not inherit
`PiiEnvelope` internals. It re-implements the required guards:

1. Validate the external session ID before using it as a session key.
2. Authorize with a default-deny auth hook.
3. Load the per-session Gaze session.
4. Apply egress policy and scan all arguments for raw PII.
5. Require approval when policy says a sensitive field needs approval.
6. Persist path-only audit metadata before forwarding.
7. Forward to the downstream MCP server with a timeout.
8. Deny unsupported content and redact all text-bearing result fields.
9. Persist encrypted session state when file mode is enabled.

## Policy resolution

Argument policy is fail-closed by default and resolves at top-level argument
boundaries. A policy entry such as `[policy.tools."email.send".arguments.to]`
matches `$.to`; nested selectors such as `contact.email` are not interpreted as
JSONPath and do not match `$.contact.email`. Nested values inherit the nearest
matching top-level argument policy, or the base tool/server/default policy when
none exists.

Policy merges are least-privilege only when broad scopes stay restrictive.
Boolean guard fields are monotonic: if an outer scope sets
`allow_sensitive_fields` or `requires_approval` to `true`, a narrower scope
cannot reset that flag to `false`. Keep `[policy.default]` and server-wide
policy deny-by-default, then allow only the smallest top-level argument needed.

## Session storage

Ephemeral mode uses `Scope::Ephemeral` and never exports a session snapshot.
File mode stores one encrypted file per validated external session ID. Gaze's
`Session::export()` is signed plaintext, so the bridge encrypts it with AEAD
before writing. The AEAD key comes from `session.key_env`; file mode refuses to
start if the variable is absent or empty.

The AEAD key can decrypt session files into token-to-PII mappings. Inject it
from a secret manager; rotate deliberately. Old files become unreadable unless
migrated.

## Stderr containment

Child MCP stderr is piped and drained without publishing raw bytes to stdout,
stderr, tracing, or audit. Downstream tools may still write their own logs;
operators must review those.

## Result handling

Only fully handled text content blocks pass in v1. Image, audio, embedded
resource, resource link, blob, and unknown future content kinds are denied.
`isError=true` results and downstream JSON-RPC errors are redacted through the
same path as successful text results. `result.mode = "allow"` is an explicit
unsafe opt-in and emits a startup warning; it should not be used in normal
agent-facing deployments.

## CLI

```bash
gaze mcp bridge --config gaze.mcp.toml
gaze mcp bridge --config gaze.mcp.toml --dry-run
gaze mcp bridge --config gaze.mcp.toml --print-tools
```

`--session-dir` overrides `[session].dir` for file mode. The bridge writes audit
JSONL under the same MCP manifest directory used by `gaze mcp serve`.
