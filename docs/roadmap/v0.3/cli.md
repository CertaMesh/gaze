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

## Exit codes

| Code | Variant          | Meaning                                                                 |
|------|------------------|-------------------------------------------------------------------------|
| 0    | —                | Success; stdout holds the JSON response.                                |
| 1    | `StdinParse`     | Stdin was not valid JSON (restore) or not readable UTF-8 (clean).       |
| 2    | `PolicyConfig`   | `--policy` missing, unparseable, or `--format` is not `json`.           |
| 3    | `Pipeline`       | Redaction or restore failure: `UnknownToken`, `InvalidSnapshotSignature`, `InvalidSnapshotVersion`, `ExportForbidden`, `NerLoad`, etc. |
| 4    | `Io`             | Filesystem or stream IO error (stdin read, stdout write, policy open).  |

Codes are stable for v0.3.x. New error classes either slot into an existing
code or, if they need a new code, wait for v0.4.

### Stderr discipline (active sanitization)

Per `laravel.md:165` the binary is expected to actively sanitize its own
stderr. Gaze CLI commits to this by emitting **only** a single-line JSON
object on failure:

```json
{"error":"Pipeline","exit":3}
```

No raw input, no decoded blob entries, no panic backtraces, no error `Display`
strings. The variant name is enough for an operator to triage; if they need
more, they correlate against the optional `RedactionLogger`'s audit log. This
is deliberately stricter than a typical CLI would be — the payoff is that a
Laravel wrapper forwarding stderr into `failed_jobs.exception` or a Sentry
breadcrumb cannot accidentally leak PII.

The Laravel wrapper adds a second layer: it sha256s stderr and logs the hash
only. These two defenses are independent.

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
understand exits `3` with `Pipeline` (wrapping `InvalidSnapshotVersion`).

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
   was never in the session. Assert exit code `3` and stderr JSON
   `{"error":"Pipeline","exit":3}`.
4. **Tamper.** Flip a byte inside the base64-decoded blob before re-encoding
   and calling `restore`. Assert exit code `3` (maps from
   `InvalidSnapshotSignature`).
5. **Format rejection.** `--format=xml` exits `2`.

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
