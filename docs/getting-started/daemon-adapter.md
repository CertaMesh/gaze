# Daemon Adapter Quickstart

This page is an adopter setup guide for `gaze daemon`, a long-lived **stdio
server** in the LSP / MCP / language-server-protocol tradition: a foreground
child process that inherits stdin/stdout from its parent and exchanges one JSON
object per line. Despite the subcommand name, this is not a Unix daemon in the
strict sense (detached, backgrounded, no controlling terminal). The historical
name is preserved for binary stability; a `gaze serve` alias is planned for
v0.10. For the full runtime contract, see
[`docs/architecture/daemon-mode.md`](../architecture/daemon-mode.md).

## When To Use

Use this stdio server when an adapter needs repeated low-latency redaction for
a multi-turn agent, chat session, or worker loop. The long-lived stdio runtime
keeps one process, one policy-loaded pipeline, and any configured model load hot
across requests, so callers avoid paying binary startup and model cold-start
cost on every turn.

## Terminology

`gaze daemon` is not a Unix daemon in the strict sense. A classic daemon (sshd,
cupsd, cron) is a backgrounded process detached from any controlling terminal,
with stdin/stdout closed or redirected to log files. `gaze daemon` is a
long-lived foreground child that owns stdin/stdout for line-delimited JSON
request/response - the same pattern as LSP language servers, MCP servers,
tsserver, and rust-analyzer.

The subcommand verb is kept as `gaze daemon` for binary stability through
v0.9.x. A `gaze serve` canonical alias lands in v0.10 with a deprecation
warning on the legacy verb; the alias drops in v0.11.

If you need an actual Unix daemon (backgrounded, supervised, persistent), use
`gaze proxy start` - the proxy is the daemon-style surface in this binary.

Use one-shot `gaze clean` when a shell pipeline or batch job only needs one
document and does not benefit from a resident process.

## Prerequisites

- A `gaze` binary on PATH.
- A policy TOML file on disk. See [`docs/policy.md`](../policy.md) for policy
  authoring.
- Optional: an audit database path if you want stdio-server metadata rows
  stamped with `provenance_stage = "daemon"`.

## Spawn The Stdio Server

Start one `gaze daemon` process per adapter worker or trust boundary:

```sh
gaze daemon --policy ./policy.toml --session-cap 1000 --session-idle-timeout 3600 --idle-timeout 1800
```

The stdio server reads one JSON request per stdin line and writes one JSON
response per stdout line. Keep stderr for logs and diagnostics; do not parse
stderr as protocol output.

## Send A Request

Write a single JSON object plus a newline:

```json
{"session_id":"conversation-1","text":"Contact alice@example.invalid before the meeting."}
```

`session_id` is supplied by the adapter. Reusing the same ID reuses that
session's manifest state inside the stdio runtime. A different ID gets a
different session and cannot see the first session's restore material.

## Read The Response

Successful responses include the same `session_id`, tokenized text, emitted
spans, and the current token list:

```json
{"session_id":"conversation-1","clean_text":"Contact <...:Email_1> before the meeting.","manifest":[],"tokens":[]}
```

Protocol and pipeline failures are typed JSON objects:

```json
{"session_id":null,"error":"JsonMalformed","detail":"malformed JSON line"}
```

```json
{"session_id":"conversation-1","error":"Pipeline","detail":"gaze daemon request failed closed"}
```

Errors never echo the input line. That is part of the fail-closed protocol:
caller logs can record the variant and detail without storing raw PII.

## Restore Round-Trip

The stdio runtime is a one-way clean protocol. It does not expose a `restore`
request or emit the `session_blob` consumed by `gaze restore`.

For the inverse direction, use the existing restore flow outside this runtime:
produce a signed restore manifest with `gaze clean`, send only `clean_text` to
the LLM, then pass `{session_blob, text}` to `gaze restore`. The CLI restore
contract is documented in
[`crates/gaze-cli/README.md#restore`](../../crates/gaze-cli/README.md#restore).

## Graceful Shutdown

SIGINT and SIGTERM set a shutdown flag. The foreground loop finishes the
current line, flushes stdout and audit writes, then exits. If no request line
arrives for `--idle-timeout` seconds, the stdio server also exits cleanly.

Session eviction is independent of process shutdown. When `--session-cap` is
exceeded, the least recently used session is evicted. Sessions idle longer than
`--session-idle-timeout` seconds are also evicted. Eviction writes audit
metadata with source `daemon.session_eviction` when audit logging is enabled.

## Multi-Session Example

The adapter owns process supervision and line framing. This Python sketch keeps
the example language-agnostic enough to translate to any runtime:

```python
import json
import subprocess

daemon = subprocess.Popen(
    [
        "gaze",
        "daemon",
        "--policy",
        "./policy.toml",
        "--session-cap",
        "1000",
        "--session-idle-timeout",
        "3600",
        "--idle-timeout",
        "1800",
    ],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
)

requests = [
    {
        "session_id": "agent-thread-a",
        "text": "Email alice@example.invalid about order TEST-1001.",
    },
    {
        "session_id": "agent-thread-b",
        "text": "Email bob@example.invalid about order TEST-2002.",
    },
]

for payload in requests:
    daemon.stdin.write(json.dumps(payload) + "\n")
    daemon.stdin.flush()

responses = [
    json.loads(daemon.stdout.readline()),
    json.loads(daemon.stdout.readline()),
]

assert responses[0]["session_id"] == "agent-thread-a"
assert responses[1]["session_id"] == "agent-thread-b"

tokens_by_session = {
    response["session_id"]: response["tokens"] for response in responses
}

daemon.terminate()
daemon.wait(timeout=10)
```

The two `session_id` values produce isolated stdio-runtime sessions. Token
counters, manifest state, and eviction lifecycle are scoped per session ID, so
a later request for `agent-thread-a` must only use the `agent-thread-a` entry in
`tokens_by_session`.

## Five-Axis Pitch

- Reliability: malformed JSON and pipeline failures return typed errors without
  echoing input.
- Reversibility: each live stdio-runtime session owns its manifest state; this
  mode does not merge restore material across sessions.
- Agentic-first: JSONL stdio lets an adapter keep the redaction boundary hot
  across multi-turn agent loops.
- Trust: stdio-runtime audit rows identify `provenance_stage = "daemon"` and
  session eviction uses `daemon.session_eviction` metadata.
- Adopter ergonomics: one process, one line in, one line out, with no per-turn
  binary startup or model load.

## Next Steps

- [`docs/architecture/daemon-mode.md`](../architecture/daemon-mode.md) — full
  stdio-runtime contract.
- [`docs/cli.md#gaze-daemon`](../cli.md#gaze-daemon) — CLI reference and flag
  summary.
- [`docs/policy.md`](../policy.md) — policy authoring.
