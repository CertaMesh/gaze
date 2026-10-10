# Gaze daemon mode

`gaze daemon` keeps one pipeline and model load alive for repeated adapter
requests. It is a long-lived stdio server; there is no `gaze serve` alias.
For a supervised background service, see the [proxy runtime](../proxy/proxy-runtime.md).

## Wire protocol

Send one JSON object per line over stdin; read one response per line on stdout.

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

Malformed JSON emits `JsonMalformed` with a null `session_id`, then reading
continues. Errors never echo the input line.

## Runtime shape

Startup builds one `Pipeline` from `--policy` through the `gaze clean` build
path. Optional Pass-3 safety nets, including Nym-small, retain pinned bundle
SHA checks and fail-closed initialization.

First use of a `session_id` creates a session from policy. Reuse keeps its
manifest and token map; distinct IDs never share a manifest. See the
[session isolation contract](../core/session-contract.md#single-shared-session-across-conversations).

## Session lifecycle and eviction

| Control | Behavior |
| --- | --- |
| `--session-cap` | Default 1000 live sessions; evict least recently used above the cap |
| `--session-idle-timeout` | Evict sessions idle beyond this many seconds |
| `--idle-timeout` | Exit cleanly after this much stdin inactivity |

Session eviction drops its restore map and writes a `tracing::warn!` row and
audit metadata with source `daemon.session_eviction`.

## Signals and shutdown

SIGINT/SIGTERM set a shutdown flag. The loop finishes the current line, flushes
stdout and audit writes, then exits. SIGHUP policy reload is unsupported.

## Audit provenance

Redaction rows carry `provenance_stage = "daemon"`, without raw PII. Session IDs
are opaque audit IDs, separate from token session hexes.

## See also

- [Daemon adapter quickstart](../../how-to/daemon/run-daemon.md)
- [`gaze daemon` flags](../../reference/cli.md#gaze-daemon)
