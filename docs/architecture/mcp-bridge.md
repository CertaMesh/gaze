# MCP Bridge Architecture

`gaze-mcp-bridge` is an optional MCP bridge for deployments where an agent
should call real downstream MCP servers without ever seeing raw PII. Gaze is
the only MCP server exposed to the agent. The bridge is also an MCP client to
the real servers.

## Fit

The bridge is for side-effecting agent workflows that already use Gaze tokens:
email and calendar actions, filesystem tools, and computer-use agents. The
agent sends pseudonymous tokens such as `<Email_1>`. The bridge restores those
tokens only for fields that policy explicitly marks as sensitive and allowed,
forwards the call to the downstream server, then redacts every text result
before returning it to the agent.

Resources and prompts are discovered in v1 so operators can see the downstream
surface, but they are denied by default. Tool calls are the only proxied path.

## Trust Model

The agent is untrusted. It must never receive raw PII and must not be able to
smuggle raw PII into downstream tools. Missing auth, missing session IDs,
unknown tokens, unsupported content blocks, oversized responses, and audit
write failures all fail closed.

The key inversion is that normal Gaze redacts on egress to an untrusted model.
Redaction is a safe, lossy operation. The bridge restores on egress from an
untrusted agent into a real tool. Restore is dangerous and lossless: a wrong
restore injects raw PII into a real side effect. For that reason the bridge is
stricter than the core redaction path.

## Dispatch Order

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

## Policy Resolution

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

## Session Storage

Ephemeral mode uses `Scope::Ephemeral` and never exports a session snapshot.
File mode stores one encrypted file per validated external session ID. Gaze's
`Session::export()` is signed plaintext, so the bridge encrypts it with AEAD
before writing. The AEAD key comes from `session.key_env`; file mode refuses to
start if the variable is absent or empty.

The session key is a restore key. If it is exposed, an attacker with session
files can decrypt the token-to-PII map. Operators should inject it through a
secret manager, rotate it deliberately, and treat old encrypted session files
as unreadable after rotation unless migrated.

## Stderr Containment

Downstream child MCP servers can log restored arguments. rmcp's child process
transport inherits stderr by default, which would leak raw PII to parent logs.
The bridge forces child stderr to a pipe and drains it without writing raw
bytes to stdout, stderr, tracing, or audit. Operators should still review
downstream server logging because those processes may write to their own files.

## Result Handling

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

`--session-dir` overrides `[session].dir` for file mode. Audit JSONL is written
under the same MCP manifest directory used by `gaze mcp serve`.
