# Daemon v2: complete text inventories

The opt-in `text-blocks-v1` profile protects one complete outgoing text inventory
atomically. Existing legacy and version 1 operations keep their wire format and
sequential behavior. A session accepts one protocol version for its lifetime.

Every v2 request fully cleans every active block. No detector or safety-net work
is reused; every returned `cache_hit` is `false`. Hash-only uploads save transport
bytes, not inference. Selective reuse requires a separate implementation.

## Open and identity

Start a daemon with its normal policy and model configuration. Send one JSON
object per line:

```json
{"version":2,"op":"open","request_id":"opening-a","session_id":"conversation-a","cache":"blocks","framing_profile":"text-blocks-v1","projection_id":"projection-v1"}
```

The reply contains `version`, `op`, `request_id`, `session_id`, `daemon_epoch`,
`session_epoch`, `pipeline_id`, `framing_profile`, `projection_id`,
`conversation_revision`, `manifest_generation`, `next_request_seq`,
`capabilities: ["block-cache-v1"]`, and `limits`. Initial counters are 0, 0, and 1.
Epochs are unpredictable instance identifiers. The pipeline identity binds the
immutable loaded assembly; restart or reconfiguration requires a fresh namespace.

The caller owns a versioned projection that extracts **every** provider-visible
text leaf in its protection contract and reinserts every returned leaf. Every
block is an original complete UTF-8 Text input with no field name or role-specific
interpretation. JSON-looking text still uses the Text pipeline. IDs never select
policy. A projection or profile change requires a new session and full upload.

Session, request, block, and projection IDs contain 1–128 printable ASCII bytes.
Use opaque IDs without owner data or filenames. The profile is exactly
`text-blocks-v1`. Unknown fields and duplicate protocol keys are rejected. Context
retains its existing strict parser. Counters are JSON integers no larger than
9,007,199,254,740,991.

## Clean and resync

Every post-open operation supplies `version: 2`, `op`, `session_id`, `request_id`,
`request_seq`, and all five immutable identity fields from open: `daemon_epoch`,
`session_epoch`, `pipeline_id`, `framing_profile`, and `projection_id`.

`clean_blocks` additionally requires `expected_conversation_revision`,
`expected_manifest_generation`, and `blocks`. Each block has a unique `id`, a
lowercase 64-character `sha256` of its exact decoded UTF-8 bytes, and optionally
`text`. Empty text is valid. Omitted text must match retained bytes for that ID
and digest in this session. The array is the complete ordered inventory;
omitting a former block deletes its active payload while preserving restore
mappings. No cross-session lookup or deduplication is performed.

Optional `context` applies to the whole batch. Send it on every request that needs
it; omission removes the request context. A present `null` is invalid. Previously
matched manifest knowledge remains available for restore and repeat protection.

The daemon stages one session transaction, cleans every original block in order,
and repeats the full ordered pass whenever mappings, evidence, counters, or
source spelling change. It returns only after a complete stable pass. A late
rule declaration can therefore protect earlier blocks in the same request.
Eight unsuccessful stabilization passes refuse the whole transaction.

Before publication, the daemon validates and locks the staged session commit.
With `--audit-db`, it writes the final pass's redaction metadata and safety-net
reports in one SQLite transaction. Only after that transaction commits does it
publish the prepared session and block inventory. An audit error rolls back all
rows and leaves counters, manifest, inventory, and the request sequence unchanged;
the exact request can be retried. Session publication cannot fail after the audit
commit. A process crash between the durable audit commit and memory publication
can leave rows for a lost in-memory session; a restart invalidates that session's
epoch and requires a new namespace.

A clean reply carries the common identity envelope and counters, plus exactly
one result per requested block in order. Each result contains `id`, `sha256`,
`clean_text`, `manifest`, `tokens`, `cache_hit`, and `validated_generation`.
Manifest spans retain v1 byte coordinates. Token metadata contains class, token,
and family, never raw values. A successful clean increments the conversation
revision; semantic manifest generation increments once if the transaction changed
manifest state. The operation consumes one sequence.

`resync` uses the same common request fields, `blocks`, and optional `context`.
It **forbids** expected revision/generation fields and requires every raw body.
It cleans against the actual current manifest, without rewinding uncertain
operations. Its success has the clean schema with `op: "resync"`.

Buffer and validate the entire reply, including identities, counters, order,
hashes, and spans, then replace the complete outgoing projection atomically.
Never combine old and new responses. Never split an oversized outgoing inventory
into independently committed requests. A compacted history is a new complete
inventory that must receive its own full clean before dispatch.

## Restore and close

`restore` requires common fields, `minimum_manifest_generation`, and `text`.
It forbids blocks, context, and expected revision fields. The minimum generation
may be older than current but never newer. Restore uses the originating retained
namespace and strict source spelling. Unknown tokens refuse. Its reply has the
common envelope and restored `text`, with unchanged conversation and manifest
counters and the next sequence. Restored owner text must stay outside the cloud
projection. Deletion and compaction do not discard old restore mappings.

`close` requires only common fields. Its reply has the common envelope, unchanged
conversation and manifest counters, and the next sequence. Close drops the live
manifest and raw blocks. A value-free close tombstone supports exact retries for
60 seconds, with an oldest-first process cap of 1,000 tombstones. It cannot close
a newly opened namespace with the same caller ID.

## Recovery and refusals

Serialize exchanges; allow one outstanding operation per conversation. Retain
exact pending frame bytes, including whitespace, until the outcome is known.

An exact live open retry returns its original opening reply without resetting
state. Other opens for that caller ID return `SessionExists`. The last successful
operation retains one bounded receipt. Exact retries return its original bytes;
changed bytes or request ID at that retained sequence return `RequestConflict`.
Receipt loss does not reset the sequence high-water mark. Stale/future sequences
and stale clean counters return `CacheConflict`. A receipt cannot bypass fresh
model admission when the daemon lacks a model-health identity; resync at the
reported next sequence instead.

Errors have `version: 2`, `session_id: null`, `error`, and fixed value-free
`detail`. Syntactically valid bounded `request_id` and `request_seq` are echoed
when available. A `current` object is included only for matching live epochs and
contains the immutable identities, current counters, and next sequence. Wrong
epochs reveal no newer session metadata. No error contains text, context, block
IDs, hashes, token mappings, or partial output.

New error variants are `BlockMissing`, `HashMismatch`, `CacheConflict`,
`RequestConflict`, `CacheCapacity`, and `CacheConvergence`. Validation and
capacity failures consume no sequence and publish no staged manifest or inventory.
After refusal, correct the request using a new request ID at the unconsumed
sequence. After an uncertain clean, retry the exact frame first. If its receipt
is unavailable, resync all original bodies at `current.next_request_seq` without
revision preconditions. Lost restore replies follow the same receipt rule; run a
new strict restore at the current sequence if necessary.

Restart, eviction, missing sessions, and wrong epochs mean the old namespace is
unavailable. Open a fresh session and upload original owner-side raw leaves.
Never use old cleaned tokens as raw history, import old results, downgrade to v1,
or dispatch after an uncertain outcome.

## Limits and memory

| Resource | Default ceiling |
| --- | ---: |
| Shared transport frame and packed queue | 160 MiB each |
| V2 encoded request or response, excluding LF | 64 MiB |
| Decoded raw text per block | 10 MiB |
| Complete active raw inventory | 32 MiB |
| Active blocks | 4,096 |
| Per-session cache allocation | 32 MiB |
| Per-session manifest allocation | 32 MiB |
| Global manifest allocation | 128 MiB |
| Aggregate v2 state and staging allocation | 256 MiB |
| Stabilization passes | 8 |

The shared reader preserves delimiter, CRLF, EOF, backpressure, and overflow
behavior. A transport overflow is terminal. A v2 application-cap refusal is
nonterminal. The 10 MiB decoded block ceiling is new v2 enforcement; v1 retains
its prior acceptance, including escaped frames between 64 and 160 MiB.

Hash-only retained bodies count toward the complete inventory limits. Old and
staged state coexist and count toward allocation budgets, as do response and
receipt buffers. Tighter allocation limits can refuse below the advertised raw
ceilings. Responses are completely serialized and checked before commit; no
partial output is dispatchable. Session-cap and idle eviction discard the whole
namespace. Defaults remain 1,000 sessions, 3,600 seconds session idle, and 1,800
seconds process idle.

Raw state stays in process memory and is never persisted by this protocol.
Allocation release does not guarantee physical zeroization or prevent swap/core
dumps. Resident models, pipeline work buffers, transport/wire buffers, and the
runtime are separate resources; the v2 state budget is not a total RSS limit.
Stdio remains a trusted-owner boundary. Session IDs are not authentication.
