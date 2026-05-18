# Gaze Daemon Mode

> **Terminology note.** `gaze daemon` is a long-lived stdio server in the LSP /
> MCP tradition, not a Unix daemon in the strict sense. The subcommand verb is
> preserved through v0.9.x for binary stability; a `gaze serve` canonical alias
> lands in v0.10 with a deprecation warning on `gaze daemon`. For the actual
> backgrounded supervised daemon, see
> [`docs/architecture/proxy-runtime.md`](proxy-runtime.md).

`gaze daemon` is the long-lived stdio runtime for adapters that need repeated
low-latency pseudonymization without paying a binary startup and model-load cost
for every request.

## Protocol

The wire format is JSON per line over stdin/stdout.

Request:

```json
{"session_id":"conversation-1","text":"input text"}
```

Success:

```json
{"session_id":"conversation-1","clean_text":"output text","manifest":[],"tokens":[]}
```

Error:

```json
{"session_id":"conversation-1","error":"Pipeline","detail":"gaze daemon request failed closed"}
```

Malformed JSON is fail-closed per line: the daemon emits `JsonMalformed` with a
null `session_id` and continues reading. Errors never echo the input line.

## Runtime Shape

The daemon constructs one `Pipeline` at launch from `--policy`. Optional
Pass-3 safety nets, including the in-process Kiji ORT backend, are initialized
through the same CLI build path as `gaze clean`, so pinned bundle SHA checks
still run during daemon startup/backend initialization.

Each request looks up a `Session` by client-provided `session_id`. First use
creates a new session from the policy. Reuse of the same `session_id` reuses the
same manifest and token map. Distinct `session_id` values never share a
manifest.

## Common Pitfalls

The cross-conversation pseudonym linkability pitfall (sharing one `Session::new(Scope::Ephemeral)` across logical conversations) applies to all `Session` consumers, not just daemon mode. See [`docs/architecture/session-contract.md#single-shared-session-across-conversations`](session-contract.md#single-shared-session-across-conversations).

## Session Lifecycle

The registry defaults to 1000 live sessions. When it exceeds `--session-cap`,
the least recently used session is evicted. Sessions idle longer than
`--session-idle-timeout` seconds are also evicted. Eviction logs a
`tracing::warn!` row and an audit metadata row with source
`daemon.session_eviction`.

`--idle-timeout` is process-level stdin inactivity. When no request line arrives
for that duration, the daemon exits cleanly.

## Signals

SIGINT and SIGTERM set a shutdown flag. The foreground loop finishes the current
line, flushes stdout and audit writes, then exits. SIGHUP policy reload is not
part of the v1 daemon contract.

## Audit

Daemon-mode redaction audit rows are passed through a logger wrapper that sets
`provenance_stage = "daemon"`. This lets adopters query daemon-emitted metadata
separately from one-shot `gaze clean` invocations without storing raw PII.

## Five-Axis Check

Reliability: malformed protocol input produces typed JSON errors and the stdio
server continues. Safety-net artifact verification remains fail-closed.

Reversibility: session manifests are owned by one `session_id` and live only in
that session entry. Eviction drops the restore map for that session.

Agentic-first: JSONL keeps a single stdio connection hot for keystroke and
multi-turn agent workflows.

Trust: audit rows identify stdio-runtime provenance and session IDs remain
opaque audit IDs, not token session hexes.

Adopter ergonomics: adapters can start one process, stream line-delimited JSON,
and avoid per-call binary startup or model cold starts.

## See Also

- [Daemon adapter quickstart](../getting-started/daemon-adapter.md)
- [`gaze daemon` CLI guide](../cli.md#gaze-daemon)
