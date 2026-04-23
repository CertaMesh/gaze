# Gaze — Standalone CLI (v0.3 Pipe Mode)

**Status:** Spec / in-flight for v0.3. The `gaze` binary is being built in
`crates/gaze` alongside the library. See [`laravel.md`](laravel.md) for the
host-side wrapper that shells out to this CLI.

---

## Why this exists

`docs/roadmap/v0.3/laravel.md` commits to a two-call dataflow per LLM request:
Laravel calls `gaze clean` to strip PII before the LLM sees the prompt, then
`gaze restore` on the LLM's reply before it reaches the user. Nothing about
that contract is Laravel-specific — any host language that can spawn a
subprocess and pipe stdin/stdout JSON can drive it. This document locks in the
subcommand surface, the wire format, and the failure semantics so the wrapper
and the binary can be built independently without rework.

Scope: stdin/stdout JSON contract, exit codes, session handling. Policy loading
has its own spec — see solo todo `#3 policy.toml loader (Phase 0 gate)`.

## Binary layout

- Inline `[[bin]] name = "gaze"` in `crates/gaze/Cargo.toml`, source at
  `crates/gaze/src/main.rs`. One crate, one bin, same version as the library.
- Precedent: the deleted `ghostwriter` crate used the same pattern and it
  worked — a separate `gaze-cli` crate would add a compile-graph edge with no
  benefit. The `ort` / `onnxruntime` download-binaries cost is already baked
  into the library; moving the bin out would not avoid it.

## Subcommands

```text
gaze clean   --policy=<path> [--format=json] [--session-ttl=<secs>]
gaze restore [--format=json]
gaze --version
```

`--format` exists purely as a forward-compat hook. Only `json` is accepted
today; any other value exits `2` with `PolicyConfig`.

A follow-up `gaze policy check --policy=<path>` subcommand is planned for the
policy loader work (solo #3). It is not part of this spec.

### `gaze clean`

- **Stdin:** raw text, UTF-8, unbounded (callers cap at whatever their queue
  payload limit allows; see `laravel.md` "Session blob size limits" open
  question).
- **Stdout:** one JSON object, no trailing whitespace beyond a single `\n`:
  ```json
  {
    "clean_text": "...",
    "session_blob": "<base64>",
    "stats": { "detections": 2 }
  }
  ```
- **`clean_text`** — redacted input. Tokens follow the library's existing
  conventions (`Name_1`, `Email_1`, `Location_1`, `Organization_1`,
  `email1@example.test` for format-preserved emails, or `<CustomName>_N`
  for policy-declared custom classes).
- **`session_blob`** — base64 of the library's `SensitiveSnapshot`: an
  ed25519-signed payload that carries the token→PII map plus the class
  counters. Signing is provided by the library; the CLI does not add its own
  authentication. Hosts that cache or enqueue the blob are expected to wrap it
  in AEAD (`laravel.md` §"Encryption-in-Flight" makes this mandatory on the
  Laravel side).
- **`stats.detections`** — count of non-conflict-loser text redactions the
  pipeline performed in this invocation. Used by callers for quick sanity
  checks ("did we actually strip anything?"). Not authoritative; the
  redaction log (optional, via `RedactionLogger` trait) is.

### `gaze restore`

- **Stdin:** one JSON object:
  ```json
  { "session_blob": "<base64>", "text": "..." }
  ```
- **Stdout:** one JSON object:
  ```json
  { "text": "..." }
  ```
- **Restore strategy — regex scanner (decision 2026-04-23).**
  The restore handler compiles a single pattern covering every token shape
  the library emits, scans the LLM response for matches, and calls
  `session.restore_strict(token)` on each. Token shapes:

  ```text
  (?:Name|Email|Location|Organization)_\d+
  <PascalCasedCustomName>_\d+           # for PiiClass::Custom(...)
  email\d+@example\.test                # FormatPreserve on Email
  ```

  A hallucinated token the LLM invents that matches the shape but was never
  in the session map triggers `Error::UnknownToken(_)` → exit `3`. Per
  `laravel.md:424` this is the canonical "draft corruption, flag for human
  review" signal. The alternative we rejected — walking the session map and
  substring-replacing — would leave hallucinated shapes in the output and
  violate the fail-closed rule at `laravel.md:429`.

## Exit codes and stderr variants

Exit codes stay coarse (five buckets); the `error` field on stderr carries the
finer-grained diagnostic the Laravel failure matrix (`laravel.md:420`) needs
to decide between retry, re-clean, and flag-for-human-review.

| Code | Variant             | Trigger                                                                 | Caller action                                     |
|------|---------------------|-------------------------------------------------------------------------|---------------------------------------------------|
| 0    | —                   | Success; stdout holds the JSON response.                                | proceed                                           |
| 1    | `StdinParse`        | Restore stdin is not valid JSON.                                        | caller bug, fix upstream                          |
| 1    | `EmptyInput`        | `clean` stdin was zero bytes.                                           | caller bug, do not retry                          |
| 2    | `PolicyConfig`      | `--policy` missing / unparseable, or `--format` is not `json`.          | ops / config fix                                  |
| 3    | `UnknownToken`      | Restore saw a token-shaped string not in the session map.               | draft corruption — flag for human review, do **not** retry |
| 3    | `InvalidSignature`  | Snapshot signature or version rejected (maps `InvalidSnapshotSignature` + `InvalidSnapshotVersion`). | tamper or cross-version — hard fail               |
| 3    | `BlobExpired`       | Session TTL elapsed before restore. *(Reserved — library does not emit this in v0.3.0; see `laravel.md:425`.)* | re-run `clean` from scratch on original input     |
| 3    | `Pipeline`          | Any other library error during redaction or restore (`ExportForbidden`, `NerLoad`, `SnapshotDecode`, `InvalidRegex`, `Sqlite`). | retry with backoff, then alert                    |
| 4    | `Io`                | Stream IO or filesystem error (unreadable / non-UTF-8 stdin on `clean`, stdout write failure, policy open). | infra — alert                                     |

Exit codes are stable for v0.3.x. Stderr variants may be added (e.g.
`BlobExpired` when the library starts enforcing TTL) but never renamed
within a minor version.

### Stderr discipline (active sanitization)

Per `laravel.md:165` the binary is expected to actively sanitize its own
stderr. Gaze CLI commits to this by emitting **only** a single-line JSON
object on failure:

```json
{"error":"UnknownToken","exit":3}
```

No raw input, no decoded blob entries, no panic backtraces, no error `Display`
strings. The variant name is safe to forward — the set of variants above is
closed, all values are Gaze-generated ASCII identifiers, never user PII. An
operator who needs more than the variant correlates against the optional
`RedactionLogger`'s audit log.

This is deliberately stricter than a typical CLI would be — the payoff is
that a Laravel wrapper forwarding stderr into `failed_jobs.exception` or a
Sentry breadcrumb cannot accidentally leak PII.

The Laravel wrapper adds a second layer: it sha256s stderr and logs the hash
only. These two defenses are independent.

### Exit 0 silence

On success the CLI writes exactly one JSON object to stdout followed by a
single `\n`, and nothing to stderr. No "processing…" logs, no timing traces,
no warning lines. A Laravel wrapper that captures stderr and logs it when
the exit code is non-zero is therefore safe: a successful call produces a
blank stderr string, not a stderr string it has to filter.

## Session handling

### Scope default — `Persistent { ttl: 24h }` (decision 2026-04-23)

`gaze clean` opens a `Scope::Persistent { ttl }` session because the blob must
survive the CLI invocation (it lives in the caller's queue payload). The TTL
defaults to 86 400 seconds (24 hours) to match the retention window of a
typical Laravel queue with `--hours=24` failed-jobs pruning. Callers with
different retention can override via `--session-ttl=<secs>`.

`Scope::Ephemeral` is deliberately unavailable from the CLI — the library
already rejects it in `Session::export` (`Error::ExportForbidden`) and
pipe mode without an exportable blob is meaningless.

### Blob format

Base64-encoded output of `SensitiveSnapshot::into_bytes()`. The library's
snapshot layout is:

```text
[1 byte  version=1]
[32 bytes ed25519 verifying key]
[64 bytes ed25519 signature]
[N bytes  JSON SnapshotPayload]
```

Forward-compat: the version byte lets us evolve the payload schema without
breaking old blobs. A blob with a version the current binary does not
understand exits `3` with `InvalidSignature` (mapped from
`InvalidSnapshotVersion`).

## Edge cases

- **Empty stdin on `clean`** → exit 1 (`EmptyInput`). Zero-byte input is
  treated as a caller bug: a Laravel wrapper should never dispatch a job
  without content. Accepting empty input and emitting an empty-session blob
  would just paper over that bug on the Gaze side.
- **Non-UTF-8 stdin on `clean`** → exit 4 (`Io`). `read_to_string` collapses
  the decode failure into an IO error and the bin does not split the two
  paths today; the extra byte-level reader is not worth the surface for a
  failure that only happens when the caller bypasses their own encoding
  layer.
- **Empty `text` in `restore` stdin** → success, returns `{ "text": "" }`.
- **Empty `session_blob` in `restore` stdin** → exit 3 (`InvalidSignature`);
  the library rejects any payload shorter than the 97-byte version +
  key + signature header.
- **No detections on `clean`** → success. `clean_text` equals the input,
  `session_blob` encodes a valid but empty session map (still signed), and
  `stats.detections` is `0`.

## `stats` field stability

The `stats` object in `gaze clean`'s response is a forward-compatible
namespace. The v0.3.x contract:

- Keys present in v0.3.0 (`detections`) stay in every subsequent v0.3.x
  release with the same type and the same semantics.
- New keys may be added. Parsers MUST ignore unknown keys.
- No key is renamed or removed within a minor version.

Consumers that want to lock specific field shapes should pin the keys they
depend on explicitly and tolerate extras.

## Timeouts

Gaze CLI has no built-in timeout. The handler reads all of stdin eagerly,
runs the pipeline synchronously, and writes the response before exit.

Callers that need bounded wall-clock time wrap the subprocess in their own
timeout (Laravel's `Process::timeout(30)` at `laravel.md:96` is the canonical
example). The CLI installs no signal handlers and does not self-kill —
`SIGTERM` / `SIGKILL` from the supervisor is the correct termination path.

Rationale: the CLI does one thing; deadline enforcement belongs to whoever
owns the job queue and knows the SLA. Stacking an internal deadline on top
of `Process::timeout` would mean two competing clocks with no guarantee
about which fires first.

## Pipeline wiring today vs. after #3

**Today (this spec).** The `clean` handler builds a stub pipeline inline:
one regex email detector, `Action::Tokenize` on `PiiClass::Email`,
`Action::Preserve` default. This is enough to land the CLI surface, the wire
format, and the test suite without blocking on policy-file design.

**After solo #3.** The stub is replaced by:

```rust
let policy = Policy::load(&policy_path)?;
let pipeline = Pipeline::from_policy(&policy)?;
```

The stdin/stdout contract, exit codes, and session handling do not change.
A Laravel integration written against this spec today continues to work
unmodified after #3 lands — it just starts seeing detections from the full
detector set rather than just email regex.

## Test strategy

Integration tests live at `crates/gaze/tests/cli_pipe.rs` and drive the bin
via `assert_cmd`. The suite covers:

1. **Roundtrip.** `clean` a block of text, feed the blob + a mocked LLM reply
   (which reuses the emitted tokens) into `restore`, assert the original PII
   comes back.
2. **Canary.** Inject `CANARY_DO_NOT_LEAK@test.local` into the input, assert
   it is absent from `clean_text`, then assert it reappears in the `restore`
   output. Mirrors the test strategy at `laravel.md:433`.
3. **UnknownToken.** Hand `restore` an LLM reply containing `Email_999` that
   was never in the session. Assert exit `3` and stderr JSON
   `{"error":"UnknownToken","exit":3}`.
4. **Tamper.** Flip a byte inside the base64-decoded blob before re-encoding
   and calling `restore`. Assert exit `3` and stderr JSON
   `{"error":"InvalidSignature","exit":3}`.
5. **Format rejection.** `--format=xml` exits `2` with
   `{"error":"PolicyConfig","exit":2}`.
6. **Empty-stdin on clean.** Zero-byte stdin exits `1` with
   `{"error":"EmptyInput","exit":1}`.
7. **Silence on success.** Assert stderr is empty on every successful
   invocation across the suite.

No unit tests on the CLI module itself — the bin is thin glue over library
calls, and the library has its own unit tests. Integration tests are the
load-bearing layer for the pipe contract.

## What this spec deliberately does not cover

- **Policy file format.** Owned by solo #3.
- **NER model installation.** Already covered by the library's model-dir
  resolution; the CLI inherits it through `Policy`/`Pipeline::from_policy`.
- **Persistent-key / cross-call token stability.** That is the v0.3 "persistent
  mode" discussed in `laravel.md`'s Open Questions section, not pipe mode.
- **Structured documents.** `gaze clean` only accepts text on stdin. If a
  caller needs structured redaction, they use the library directly; adding a
  structured-input JSON shape to the CLI would balloon the surface for a case
  the Laravel wrapper does not need.

## Decision log

| Date       | Decision                                                    | Rationale                                                                 |
|------------|-------------------------------------------------------------|---------------------------------------------------------------------------|
| 2026-04-23 | Bin inline in `crates/gaze`, not a separate `gaze-cli`      | Matches deleted ghostwriter precedent; no compile-graph benefit to split. |
| 2026-04-23 | `Scope::Persistent { ttl: 24h }` default                    | Matches common queue retention; `Ephemeral` is forbidden by the lib.      |
| 2026-04-23 | Regex scanner for restore (not map-walk)                    | Map-walk cannot surface `UnknownToken` — fail-closed contract needs it.   |
| 2026-04-23 | Stderr = `{"error":"Variant","exit":N}` only, one line      | Active sanitization per `laravel.md:165`; second layer on wrapper side.   |
| 2026-04-23 | Policy loader split to solo #3, runs in parallel            | Lets CLI surface + tests land without blocking on file-format bikeshed.   |
| 2026-04-23 | Expanded stderr variants, exit codes stay coarse (option 1a) | Laravel's failure matrix needs `UnknownToken` ≠ `InvalidSignature`; variant names are Gaze-generated, safe to emit. |
| 2026-04-23 | Empty stdin on `clean` → exit 1 `EmptyInput`                | Zero-byte input is a caller bug; accepting it would paper over upstream failure. |
| 2026-04-23 | No built-in timeout                                         | Caller owns the deadline (`Process::timeout`); two clocks is worse than one. |
| 2026-04-23 | `stats` is forward-compatible: keys stable, new keys may be added | Standard JSON-evolution rule; parsers must tolerate unknown keys. |
| 2026-04-23 | Exit 0 guarantees empty stderr                              | Laravel can trust "non-zero ⇒ log stderr" without filtering a success banner. |
| 2026-04-23 | Windows not a target for v0.3                               | Library uses `libc::mlock`/`madvise` for key protection; a Windows path is out of scope. |
