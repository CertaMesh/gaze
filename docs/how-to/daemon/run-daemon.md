# Run the daemon

`gaze daemon` exchanges one JSON object per line over stdin/stdout.
See the [runtime contract](../../explanation/daemon/daemon-mode.md).

## When to use the daemon

Use `gaze daemon` for repeated cleaning in agent, chat, or worker loops. It keeps
the policy pipeline and configured models loaded.

It is a foreground JSONL/stdio child, like an LSP server. The command name stays
`daemon`; there is no `gaze serve` alias. For a background HTTP process, use
`gaze proxy start`. For a single document, use `gaze clean`.

## Prerequisites

- A `gaze` binary on PATH.
- A policy TOML file on disk. See [`docs/reference/policy.md`](../../reference/policy.md) for policy
  authoring.
- Optional: an audit database path if you want stdio-server metadata rows
  stamped with `provenance_stage = "daemon"`.

## Start the stdio server

Start one `gaze daemon` process per adapter worker or trust boundary:

```sh
gaze daemon --policy ./policy.toml --session-cap 1000 --session-idle-timeout 3600 --idle-timeout 1800
```

The stdio server reads one JSON request per stdin line and writes one JSON
response per stdout line. Keep stderr for logs and diagnostics; do not parse
stderr as protocol output.

## Send a request

Write a single JSON object plus a newline:

```json
{"session_id":"conversation-1","text":"Contact alice\u0040example.invalid before the meeting."}
```

The JSON escape becomes `@` before Gaze detects the synthetic email.

`session_id` is supplied by the adapter. Reusing the same ID reuses that
session's manifest state inside the stdio runtime. A different ID gets a
different session and cannot see the first session's restore material.

## Read the response

Success returns `session_id`, `clean_text`, `manifest`, and `tokens`. The token's
session prefix changes each run.

Protocol and pipeline failures are typed JSON objects:

```json
{"session_id":null,"error":"JsonMalformed","detail":"malformed JSON line"}
```

```json
{"session_id":null,"error":"Pipeline","detail":"gaze daemon request failed closed"}
```

Errors fail closed and return `session_id: null`; they never echo the caller's
session ID or input. Log only the variant and detail.

## Run multiple sessions

The adapter supervises the process and frames lines:

```python
import json
import subprocess

daemon = subprocess.Popen(
    ["gaze", "daemon", "--policy", "./policy.toml", "--session-cap", "1000",
     "--session-idle-timeout", "3600", "--idle-timeout", "1800"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
)  # stderr remains separate from protocol output

tokens_by_session = {}
for session_id in ["agent-thread-a", "agent-thread-b"]:
    payload = {"session_id": session_id,
               "text": "Email {}@{} about TEST-1001.".format("alice", "example.invalid")}
    daemon.stdin.write(json.dumps(payload) + "\n")
    daemon.stdin.flush()
    response = json.loads(daemon.stdout.readline())
    assert response["session_id"] == session_id
    if "error" in response:
        raise RuntimeError(response["error"])
    tokens_by_session[session_id] = response["tokens"]

daemon.terminate()
daemon.wait(timeout=10)
```

Counters, manifests, and eviction belong to each `session_id`. Reuse only that
session's entry in `tokens_by_session`.

## Reversible resident sessions (protocol version 1)

Use the versioned protocol when an agent needs stable tokens across model calls
and owner-side restore. Keep the daemon on a trusted local stdio connection.
Each request and response is one JSON line. The adapter chooses a unique
`session_id` for one active run and sends these operations in order:

```json
{"version":1,"op":"open","session_id":"run-1"}
{"version":1,"op":"clean","session_id":"run-1","text":"Email alice\u0040example.invalid"}
{"version":1,"op":"restore","session_id":"run-1","text":"Done for <token from clean_text>"}
{"version":1,"op":"close","session_id":"run-1"}
```

`open` creates a new session and refuses an existing ID. `clean` returns
`clean_text`, `manifest`, and `tokens` with `version: 1` and `op: "clean"`.
`restore` uses strict unknown-token handling and returns restored `text`.
`close` destroys the live session. Both `clean` and `restore` refuse a missing
or evicted session; they never silently create one. The daemon does not emit a
`session_blob` for this protocol. It keeps restore material in memory until
`close`, eviction, or process exit. If the process exits during a model stream,
discard that unfinished stream and start a new run from owner-side source data.

The unversioned `{session_id,text}` cleaning request remains supported for
existing adapters. It does not support restore. For a one-shot shell flow,
[`gaze clean` and `gaze restore`](../../../crates/gaze-cli/README.md#restore)
still exchange a signed `session_blob`.

## Protect caller-known values for one clean request

Attach `context` to the clean frame when your trusted application already knows
record values the recognizers might miss:

```json
{"version":1,"op":"clean","session_id":"run-1","text":"Handle Zelmora Quentrix.","context":{"record":{"full_name":"Zelmora Quentrix"}}}
```

The session must already be open. Legacy clean frames accept the same field.
Use the [context schema and limits](../../reference/policy.md#caller-known-record-context)
and a policy that tokenizes or format-preserves every record class. Forward
only `clean_text` to the model; keep context on the owner side.

Invalid context returns `PolicyConfig` and no output, leaving the session
unchanged. The envelope is request-local. Values actually matched become rule
evidence for eligible repeats on later turns; unused values are forgotten.
See the [runtime contract](../../explanation/daemon/daemon-mode.md#caller-known-record-context).

## Shut down the daemon

SIGINT and SIGTERM set a shutdown flag. The foreground loop finishes the
current line, flushes stdout and audit writes, then exits. If no request line
arrives for `--idle-timeout` seconds, the stdio server also exits cleanly.

Session eviction is independent of process shutdown. When the registry exceeds
`--session-cap`, it evicts the least recently used session. It also evicts
sessions idle longer than `--session-idle-timeout` seconds. Eviction writes audit
metadata with source `daemon.session_eviction` when audit logging is enabled.

## Next steps

- [`docs/explanation/daemon/daemon-mode.md`](../../explanation/daemon/daemon-mode.md) — full
  stdio-runtime contract.
- [`docs/reference/cli.md#gaze-daemon`](../../reference/cli.md#gaze-daemon) — CLI reference and flag
  summary.
- [`docs/reference/policy.md`](../../reference/policy.md) — policy authoring.
