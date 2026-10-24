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
{"session_id":null,"error":"Pipeline","detail":"gaze daemon request failed closed"}
```

Malformed JSON syntax or malformed unversioned field types emit `JsonMalformed`
with a null `session_id`, then reading continues. All errors carry a null
`session_id` and never echo caller input.

The legacy frame above is clean-only. Protocol version 1 adds four operations:

| Request | Success | Failure condition |
| --- | --- | --- |
| `{"version":1,"op":"open","session_id":"run"}` | `op: "open"` | `SessionExists` |
| `{"version":1,"op":"clean","session_id":"run","text":"..."}` | `op: "clean"`, `clean_text`, `manifest`, `tokens` | `SessionMissing` or pipeline error |
| `{"version":1,"op":"restore","session_id":"run","text":"..."}` | `op: "restore"`, restored `text` | `SessionMissing`, `UnknownToken`, or `RestoreFailed` |
| `{"version":1,"op":"close","session_id":"run"}` | `op: "close"` | `SessionMissing` |

Version 1 successes and errors carry `version: 1`. Unsupported versions return
`ProtocolVersion`; invalid operation shapes return `ProtocolInvalid`. Restore
uses the same strict unknown-token validation as `gaze restore` and never
creates a session. The caller must buffer model output until it can restore it;
after daemon exit or session eviction, unfinished output cannot be restored.

Invalid JSON syntax returns unversioned `JsonMalformed`. A syntactically valid
version 1 object with missing or wrongly typed fields returns versioned
`ProtocolInvalid`, including when `session_id` is empty.

## Input resource limits

The daemon bounds raw wire frames before converting bytes to UTF-8 or parsing
JSON. The cap is 160 MiB (167772160 bytes), derived with checked arithmetic as
16 times `gaze clean`'s existing 10 MiB input cap. It includes JSON syntax and
context, excludes LF, and counts CR in a CRLF ending. This resource limit adds
a refusal for oversized frames that earlier versions accepted.

Overflow emits exactly one unversioned `FrameTooLarge` error with a null
`session_id`. Its detail contains only fixed text and the numeric byte limit.
The daemon then closes input without processing further frames or draining an
unbounded line to find LF. Already queued valid frames keep their order.

The queue has a separate 160 MiB byte budget, derived from the same checked
16-times input cap. A packed buffer stores UTF-8 frames with one LF delimiter
each, including empty frames, without per-frame allocation overhead. The
reader acquires byte credits before copying a frame into the queue. Processing
returns those credits as soon as it takes the frame. A frame that does not fit
the remaining credits waits; an empty queue always admits a frame at the cap
plus its one-byte delimiter.

Reader and queue buffers together retain at most the 160 MiB frame cap plus
the 160 MiB queue budget, a fixed stdin read buffer, and constant bookkeeping
(including the extra delimiter byte for a frame at the cap). Each allocation's
requested capacity is bounded. This does not bound allocator overhead, model
memory, session storage, the frame under processing, decoded JSON, or response
size.

A batch within the queue budget can finish writing before reading stdout, as
in earlier versions. The queue introduces backpressure only beyond that budget:
the reader waits until processing makes room, without dropping, reordering, or
refusing frames. Larger batch clients should read stdout concurrently with
stdin writes to avoid a wait cycle when both stdio pipes fill.

An EOF tail without LF is still processed. Invalid UTF-8 still closes input
silently after earlier valid frames, matching the previous reader.

## Caller-known record context

Both legacy clean frames and version 1 `clean` frames accept an optional typed
`context` object, with the [same schema and limits as `--context-json`](../../reference/policy.md#caller-known-record-context):

```json
{"version":1,"op":"clean","session_id":"run","text":"Handle Zelmora Quentrix.","context":{"record":{"full_name":"Zelmora Quentrix"}}}
```

Supply context from the trusted owner application, before forwarding only
`clean_text` to a model. Context on `open`, `restore`, or `close` returns
`ProtocolInvalid`. A present `null` is invalid context, not an omitted field.

Context that violates the schema, repeats keys, exceeds a limit, or conflicts
with policy returns `PolicyConfig` without cleaned output or a session change.
Errors never include context field names or values. The size limit counts raw
encoded context bytes, including whitespace and escape sequences.

The envelope lasts for one request. It is never echoed, logged, audited, or
stored as session state. Normal manifest entries and metadata-only audit rows
still describe values actually protected. Those matches are caller-asserted,
deterministic rule evidence: the [repeat-value sweep](../detection/manifest-sweep.md)
protects eligible copies on later turns without context. Unmatched context
values are forgotten. Other sessions receive no context or sweep evidence.

## Runtime shape

Startup builds one `Pipeline` from `--policy` through the `gaze clean` build
path. Optional Pass-3 safety nets, including Nym-small, retain pinned bundle
SHA checks and fail-closed initialization.

The legacy clean request creates a session on first use. Version 1 requires
an explicit `open` and refuses a missing session on `clean` or `restore`.
Reuse keeps its manifest and token map; distinct IDs never share a manifest. See the
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
