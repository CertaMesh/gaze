# Gaze — Standalone CLI (v0.3 Pipe Mode)

**Status:** Shipped in v0.3.0 (2026-04-24). The `gaze` binary lives in
`crates/gaze` alongside the library. See [`laravel.md`](laravel.md) for the
host-side wrapper that shells out to this CLI.

**Roadmap context.** This is the "pipe mode" row in
`docs/research/gaze-first-principles-vision.md:47` (v0.3 — *"Pipe mode,
format-preserving output"*). Threat-model references below cite
`docs/research/gaze-threat-model.md`.

---

## Known pre-release gaps (v0.3.0)

> **Historical — all gaps closed in v0.3.0 (2026-04-24).** This table tracked the
> pre-release blockers between the v0.3 spec and the shipping binary. Every row
> below was resolved before v0.3.0 tagged. Kept here so readers cross-referencing
> earlier commits can see what landed when.

| Gap | Tracked by | Status |
|---|---|---|
| `--policy` argument accepted but ignored; `clean` uses a hardcoded stub pipeline (email-only) | solo #3 (policy.toml loader) | ✅ shipped in v0.3.0-rc.1 (`Policy::load` + `Pipeline::from_policy`) |
| `issued_at` field on `SnapshotPayload` — missing today → `BlobExpired` cannot fire | solo #4 (library TTL enforcement) | ✅ shipped in v0.3.0-rc.1 (`issued_at` on payload, `BlobExpired` exit bucket 3) |
| `PiiClass::Custom("email")` collides with built-in `PiiClass::Email` — both produce `Email_N` | solo #5 (reserve built-in names) | ✅ shipped in v0.3.0-rc.1 (custom namespace fix — emits `Custom:{name}_N`) |
| Library does not expose a placeholder-matching API — CLI's token grammar duplicates library internals | solo #6 (library placeholder API) | ✅ shipped in v0.3.0 final (`gaze::token_shape::pattern()` + `contains_token()`) |
| `panic::set_hook`, `clap::try_parse` error routing, SIGPIPE documentation — stderr sanitisation is aspirational without these | this spec's §"Stderr discipline" | ✅ shipped in v0.3.0-rc.1 (panic hook + `Cli::try_parse` route through structured stderr) |
| CLI `CliError` enum has 4 variants (`StdinParse`, `PolicyConfig`, `Pipeline`, `Io`); spec lists 9 | follow-up commits on `gaze-v03-cli` branch | ✅ shipped in v0.3.0-rc.1 (full typed `CliError` with `UnknownToken`, `Tamper`, `VersionByte`, `EmptyInput`, `InvalidEncoding`, `BlobExpired`, `MaxBytes`) |

Laravel wrapper `laravel.md` targets this now-shipped contract. Integrators
enabling pipe mode in production should pin against v0.3.0 and consult
[`CHANGELOG.md`](../../../CHANGELOG.md) for the full shipped scope.

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
has its own spec — see solo todo `#3 policy.toml loader (Phase 0 gate)`. The
user-facing authoring guide for `policy.toml` lives at
[`docs/policy.md`](../../policy.md).

## Binary layout

- Inline `[[bin]] name = "gaze"` in `crates/gaze/Cargo.toml`, source at
  `crates/gaze/src/main.rs`. One crate, one bin, same version as the library.
- Precedent: the deleted `ghostwriter` crate used the same pattern and it
  worked — a separate `gaze-cli` crate would add a compile-graph edge with no
  benefit. The `ort` / `onnxruntime` download-binaries cost is already baked
  into the library; moving the bin out would not avoid it.

## Subcommands

```text
gaze clean   --policy=<path> [--format=json] [--session-ttl=<secs>] [--max-bytes=<n>]
gaze restore [--format=json] [--max-bytes=<n>]
gaze --version
```

`--format` exists purely as a forward-compat hook. Only `json` is accepted
today; any other value exits `2` with `PolicyConfig`.

`--max-bytes` caps stdin length; the default is **10 MB**. Input larger than
the cap exits `1` with `InputTooLarge`. Rationale: `read_to_string` buffers
the whole stream; an uncapped read of a 10 GB LLM response or a runaway
attacker-controlled email OOMs the worker, which takes down every job on
that worker, not just this one.

A follow-up `gaze policy check --policy=<path>` subcommand is planned for the
policy loader work (solo #3). It is not part of this spec.

### `gaze clean`

- **Stdin:** raw text, UTF-8, capped by `--max-bytes` (default 10 MB).
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
- **`session_blob`** — base64 of the library's `SensitiveSnapshot`. See
  §"Blob format" below for the authentication story — the tl;dr is that the
  blob is *bit-flip-resistant plaintext*, NOT tamper-evident, and hosts that
  cache or enqueue it MUST wrap it in AEAD (`laravel.md:180`).
- **`stats.detections`** — count of **redactions** performed in this
  invocation. Counts only winning (non-conflict-loser) detections whose
  action is one of `Tokenize`, `Redact`, `FormatPreserve`, `Generalize`.
  `Action::Preserve` is explicitly excluded: a "preserved" detection is
  something the detector *could* have redacted but the policy explicitly
  chose to let through, and callers sanity-checking "did we actually strip
  anything?" are asking about redactions, not decisions. This semantic is
  fixed for the stability rule in §"`stats` field stability".

### `gaze restore`

- **Stdin:** one JSON object:
  ```json
  { "session_blob": "<base64>", "text": "..." }
  ```
  Either field missing → exit `1` with `StdinParse`. Empty `text` is valid
  and returns `{ "text": "" }`; empty `session_blob` fails at the library
  (payload shorter than 97 bytes → `InvalidSignature`).
- **Stdout:** one JSON object:
  ```json
  { "text": "..." }
  ```

#### Restore strategy — two-pass: tokens-first exact match + shape-validator

The restore handler runs two sequential passes over the LLM response:

**Pass 1 — Exact-literal alternation from `Session::tokens()`.**
After `Session::import`, enumerate the live token map via `Session::tokens()`
and build a regex whose alternatives are the actual emitted token strings,
escaped, sorted longest-first. Wrapped counter tokens like `<Email_2>` stay
literal, while bare format-preserving tokens like `email1@example.test` keep
word-boundary guards. Scan the LLM text with this pattern; every match is an
exact literal from the session map and gets substring-replaced with the real
PII via `session.restore_strict`.

```
session.tokens()  =  ["<Email_2>", "<Name_1>", "email1@example.test"]
pattern           =  "email1@example\.test|<Email_2>|<Name_1>"   (longest-first)
scan("Hello <Name_1>, your email1@example.test order is ready")
                   ^ matches <Name_1> and email1@example.test exactly
```

Because matches are literal, Pass 1 **cannot straddle word boundaries**
the way a class-shape regex can — it physically cannot eat `Name_1` inside
`hostName_1s-record` without first matching the whole longer string, which
is not in the session map.

**Pass 2 — Shape-validator on Pass 1's output.**
Run `gaze::token_shape::contains_token(text)` against the text *already
restored by Pass 1*. That library-owned matcher is the canonical grammar for
every token shape Gaze can plausibly emit.

The matched shape families are:

- Wrapped counter tokens: `<Email_1>`, `<Name_1>`, `<Location_1>`,
  `<Organization_1>`, `<Custom:order_id_1>`.
- Bare format-preserving tokens: `name_1`, `location_1`, `organization_1`,
  `custom:order_id_1`, `email1@example.test`.
- Wide defense-in-depth branches for wrapped or bare `Class_N`-style shapes
  beyond the current built-ins, so future classes still fail closed if Pass 2
  sees them before the CLI learns about them explicitly.

Any remaining match is a token-shaped string that Pass 1 did not resolve —
by definition, an LLM hallucination the session never emitted. Emit
`Error::UnknownToken(_)` → exit `3` with `UnknownToken`. This preserves
`laravel.md:424`'s "flag for human review" signal without requiring the
scanner to know class names up front.

The exact regex is an implementation detail. The stable contract for v0.3.x is
that `gaze::token_shape::contains_token` owns the grammar, Pass 2 calls it,
and a `true` result maps to the CLI's `UnknownToken` failure path.

**Why not map-walk alone?** Map-walk (Pass 1 only) leaves hallucinated
token shapes untouched; the caller never sees `UnknownToken` and ships
silently corrupt output. Fails closed only on tamper/signature, not on LLM
invention.

**Why not shape-regex alone?** A shape-only scan silently corrupts
legitimate adjacent text (`"hostName_1s-record"` → scanner eats `Name_1`
if there's a session entry for it) and misses lowercase `FormatPreserve`
tokens (library emits `location_1`, spec-advertised regex wants
`Location_\d+`). Original spec version had both bugs; counselors review
2026-04-23 exposed them.

**Residual collision risk.** If the *clean* input genuinely contained the
same string Gaze then chose as a token (e.g. the text "the fake address is
`email1@example.test`" is in the raw input AND Gaze's FormatPreserve
assigned the same shape to a real address), Pass 1 replaces both
occurrences. Extremely unlikely for built-in `Class_N` tokens; possible for
FormatPreserve tokens that share a predictable domain. Mitigation deferred
to a library change (move fake domain to `.gaze-fake.invalid` or similar)
— tracked under solo #6 scope.

## Host stderr consumption protocol

The CLI emits a closed-set `error` variant on stderr as a single-line JSON
object (see §"Stderr discipline"). For the variant to reach the host's
routing logic, the host wrapper **must parse stderr JSON before hashing or
logging it**.

The Laravel wrapper at `laravel.md:150-162` is being updated to:

```php
private function buildException(string $stage, ProcessResult $result): GazeException
{
    $stderrRaw  = $result->errorOutput() ?: '';
    $variant    = $this->extractVariant($stderrRaw);   // parses {"error":"<Name>","exit":N}
    $stderrHash = hash('sha256', $stderrRaw);

    Log::warning("{$stage} failed", [
        'exit_code'     => $result->exitCode(),
        'error_variant' => $variant,                   // safelisted enum value only
        'stderr_sha256' => $stderrHash,
    ]);

    return new GazeException(
        "{$stage} failed (variant={$variant}, exit={$result->exitCode()}, stderr_sha256={$stderrHash})",
        $result->exitCode(),
        variant: $variant,
    );
}
```

Safelisting rule on the host side: `extractVariant` must validate the
extracted string against the closed variant set from the table below. An
unknown variant (newer binary, older wrapper) falls back to the
most-conservative action in that exit bucket:

| Exit | Unknown-variant fallback action       |
|------|---------------------------------------|
| 1    | Treat as `StdinParse` — caller bug, do not retry. |
| 2    | Treat as `PolicyConfig` — ops fix.    |
| 3    | Treat as `UnknownToken` — flag for human review, do **not** retry. |
| 4    | Treat as `Io` — infra, alert.         |

Unparseable stderr (empty, malformed JSON, or any stderr the wrapper
couldn't extract a variant from) gets the same unknown-variant treatment
as the exit bucket suggests. The host **never** retries based solely on
exit code without applying the fallback.

---

## Exit codes and stderr variants

Exit codes stay coarse; the `error` field on stderr carries the
finer-grained diagnostic the Laravel failure matrix (`laravel.md:420`) needs
to decide between retry, re-clean, and flag-for-human-review.

| Code | Variant               | Trigger                                                                 | Host action                                     |
|------|-----------------------|-------------------------------------------------------------------------|-------------------------------------------------|
| 0    | —                     | Success; stdout holds the JSON response.                                | proceed                                         |
| 1    | `StdinParse`          | Restore stdin is not valid JSON, or a required field is missing.        | caller bug, do not retry                        |
| 1    | `EmptyInput`          | `clean` stdin was zero bytes.                                           | caller bug, do not retry                        |
| 1    | `InputTooLarge`       | stdin exceeded `--max-bytes`.                                           | caller bug, do not retry                        |
| 1    | `InvalidEncoding`     | stdin was not valid UTF-8 on `clean`.                                   | caller bug, do not retry                        |
| 2    | `PolicyConfig`        | `--policy` missing / unparseable, `--format` not `json`, or argv parse failure. | ops / config fix                                |
| 3    | `UnknownToken`        | Restore saw a token-shaped string not in the session map.               | draft corruption — flag for human review, do **not** retry |
| 3    | `InvalidSignature`    | Snapshot signature verification failed.                                 | tamper suspected — hard fail                    |
| 3    | `InvalidBlobVersion`  | Snapshot version byte unknown to this binary.                           | binary/blob mismatch — re-run `clean` after rollout |
| 3    | `BlobExpired`         | Session TTL elapsed before restore. *(Reserved — library cannot emit this until solo #4 lands `issued_at`; see `laravel.md:425`.)* | re-run `clean` from scratch on original input   |
| 3    | `Pipeline`            | Any other library error (`ExportForbidden`, `NerLoad`, `SnapshotDecode`, `InvalidRegex`, `Sqlite`). | retry with backoff, then alert                  |
| 4    | `Io`                  | Stream IO failure (stdout write failed, stdin read failed with OS error).         | infra — alert                                   |
| 4    | `PolicyOpen`          | `--policy` path exists but could not be opened (permissions, EIO).      | ops — alert                                     |

Exit codes are stable for v0.3.x. Stderr variants may be added (e.g.
`BlobExpired` when the library starts enforcing TTL via solo #4) but never
renamed within a minor version.

### Stderr discipline (active sanitization)

Per `laravel.md:165` the binary is expected to actively sanitize its own
stderr. Gaze CLI commits to this by emitting **only** a single-line JSON
object on failure:

```json
{"error":"UnknownToken","exit":3}
```

No raw input, no decoded blob entries, no panic backtraces, no error `Display`
strings. The variant name is safe to forward — the set of variants above is
closed, all values are Gaze-generated ASCII identifiers, never user PII.

Three concrete leaks must be closed in the bin to make this discipline real,
not aspirational:

1. **Panic hook.** `std::panic::set_hook` is installed before any pipeline
   work. Any panic (from `ort`, `regex`, `serde_json`, future NER backends)
   writes `{"error":"Pipeline","exit":3}` to stderr and exits with code 3.
   Backtraces are never printed, even with `RUST_BACKTRACE=1` set in the
   caller's environment.
2. **Argv parse errors.** The bin uses `Cli::try_parse()` + manual error
   handling, **not** `Cli::parse()` (which dumps clap's usage text to
   stderr before the sanitizer runs). Any argv error — unknown subcommand,
   bad flag, missing value — routes through `CliError::PolicyConfig` and
   emits the standard JSON stderr line.
3. **SIGPIPE.** If the caller closes stdout mid-write, Rust's default
   SIGPIPE handler terminates the process silently (exit `141` on POSIX).
   The CLI does **not** install a custom handler — stderr stays empty, and
   a caller that closed the pipe has by definition stopped listening, so
   there is no one to receive a sanitized error. Hosts should interpret a
   terminated-by-SIGPIPE subprocess as "I cancelled this call" and not
   alert.

The host wrapper adds a second layer: it sha256s raw stderr and logs the
hash alongside the extracted variant (§"Host stderr consumption protocol").
These two defenses are independent — the hash is a belt-and-braces check
on anything that escaped the CLI sanitizer.

### Exit 0 silence

On success the CLI writes exactly one JSON object to stdout followed by a
single `\n`, and nothing to stderr. No "processing…" logs, no timing traces,
no warning lines. A host wrapper that captures stderr and logs it when the
exit code is non-zero is therefore safe: a successful call produces a blank
stderr string, not a stderr string it has to filter.

---

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

**TTL enforcement status.** In v0.3.0, `--session-ttl` is **advisory metadata
only**. The library currently does not record an `issued_at` timestamp in
the snapshot payload and therefore cannot check TTL on import — an expired
blob restores successfully. The `BlobExpired` stderr variant is reserved so
Laravel's failure matrix compiles, but the variant will not be emitted
until solo #4 lands (adds `issued_at` to `SnapshotPayload` and the
import-side expiry check). **Callers must not rely on TTL as a real expiry
control until that work ships.**

### Session key lifecycle

Each `gaze clean` invocation creates a fresh `SessionKey` (32 random bytes,
`mlock`-locked and `MADV_DONTDUMP`-tagged per `crates/gaze/src/session.rs`).
The signing key signs the exported blob and then dies with the process —
the CLI holds no persistent key store. `gaze restore` reconstructs a session
from the blob's embedded verifying key, runs the signature check (see §"Blob
format" for what the check actually proves), and spawns a **new**
`SessionKey` for the restore process that never touches the original token
map.

Destroyed-key per-call signing prevents in-process stale-blob replay within
a long-running worker: once `clean` exits, no future `clean` or `restore`
call in the same binary can forge a signature matching an old blob. This is
a narrower claim than "destroyed-key pseudonymization" from the EDPB
guidance — the EDPB framing applies to confidentiality of the *token map*,
which in pipe mode is plaintext inside the blob and depends entirely on the
host's AEAD envelope, not on the session signing key.

### Caller contract — blob↔text pairing (scope isolation)

**Tokens are session-local, not globally unique.** Counter-family shapes
like `Email_1`, `Name_1`, `<Email_1>` (post-v0.3.0-rc.3 angle-bracket wrap)
are counters reset on every `clean` invocation. Two independent `clean`
calls legitimately emit identical token shapes for unrelated PII — Session
A's `Email_1` maps to `alice@example.com` inside *A's* blob, and Session
B's `Email_1` maps to `bob@example.com` inside *B's* blob. This is by
design: per-request sessions are the rotation model (see §"Snapshot
rotation"), and giving each session a private counter space is cheaper
than a global counter that would require coordination across workers.

**The caller contract, stated sharply:** every `restore` call MUST pair
the blob returned by `clean` with text that originated from the *same*
`clean` invocation. The tuple `(blob_i, cleanText_i)` is load-bearing;
splitting it is undefined behaviour.

Concretely, `restore(blob_A, cleanText_B)` — where `cleanText_B` was
produced by a different `clean` call than `blob_A` — may, without warning:

- Substitute A's PII into B's text wherever B happens to contain a shape
  A's manifest also knows (e.g. both manifests have `Email_1`, and the
  substitution silently cross-pollinates A's `alice@example.com` into B's
  prose).
- Leave B's tokens un-substituted when B's shapes don't appear in A's
  manifest.
- Exit `3` with `UnknownToken` when Pass 2 finds a B-shape that A's manifest
  doesn't cover and the fail-closed validator trips.

**The CLI does not detect blob/text mismatch.** The ed25519 signature
proves the blob is internally consistent (see §"Blob format" for what the
signature actually proves); it does not bind the blob to any particular
cleanText. No text-side fingerprint is computed in v0.3.

**Host responsibility.** Hosts are the authoritative binding layer. The
Laravel wrapper (see `laravel.md` §"Session handling") keeps the blob on
the `GazeSession` value object that travels alongside the `cleanText`
through the queue/job/LLM round-trip. Any host writing a different
wrapper MUST replicate the same 1:1 association — never pool cleanTexts
across sessions, never cache blobs and match them against arbitrary LLM
output, never let one request's blob restore another request's text.

**v0.4 direction (non-binding note).** v0.4 Phase 1 adds a text-provenance
fingerprint to the blob so `restore` can reject mismatched pairs at the
library layer instead of relying purely on caller discipline. The v0.3
contract stays callers-own-pairing; v0.4 hardens it in the library. No
token-grammar break is planned for the harden step.

### Snapshot rotation

For v0.3.0 pipe mode, every `clean` call produces a fresh session so
rotation happens per request automatically. Long-lived sessions that
outlive a single LLM round-trip (Laravel-side "persistent tokens across
jobs", `laravel.md:484`) are out of scope for this spec. The threat model's
rotation recommendation (`gaze-threat-model.md:75`) is therefore satisfied
by the request-scoped design until persistent-session mode lands — hosts
that need persistent sessions will own rotation policy themselves.

### Blob format

Base64-encoded output of `SensitiveSnapshot::into_bytes()`. The library's
snapshot layout is:

```text
[1 byte  version=1]
[32 bytes ed25519 verifying key]
[64 bytes ed25519 signature]
[N bytes  JSON SnapshotPayload]
```

**What the signature actually proves (and what it doesn't).** The ed25519
signature is computed over the JSON payload using a keypair generated at
`export` time, and the *public* half of that keypair is embedded in the
blob next to the signature. The consequence: an attacker who can write
arbitrary bytes into the blob can also generate a fresh keypair, sign a
different payload, and the blob still verifies. The signature therefore
resists:

- Random memory corruption and serialization glitches.
- Truncation (payload length mismatches the signed range).
- Accidental field-level edits by tooling that forgets to re-sign.

It does **not** resist forgery by an actor who controls the bytes. The
only tamper-evidence in pipe mode comes from the host's AEAD envelope
(Laravel's `Crypt::encryptString` = AES-256-CBC + HMAC-SHA256, see
`laravel.md:186`). The spec deliberately does not say "the signature
protects integrity" without this qualifier. A host that skips AEAD has a
blob with neither confidentiality nor forgery-resistance — the ed25519
layer is a bit-flip checksum, not security.

**Why keep signing at all?** Per-process signing destroys the signing key
on process exit, which prevents stale-blob replay *within* the lifetime of
a long-running worker (see §"Session key lifecycle"). That is a narrow but
real defense. The signature also catches tooling bugs (queue drivers that
double-encode, byte-sloppy serializers) that pure plaintext would hide.

**Confidentiality is entirely on the host.** The `SnapshotPayload` JSON
carries the plaintext token↔PII map. Possession of the blob is equivalent
to possession of the underlying PII
(`docs/research/gaze-threat-model.md:13`). Every caller that writes the
blob to a queue payload, cache entry, log line, or disk **must** wrap it
in an AEAD envelope. The CLI deliberately does not add its own encryption
so the key-management surface stays zero — but this places the full burden
on the host wrapper.

**Forward-compat.** The version byte lets us evolve the payload schema
without breaking old blobs. A blob with an unknown version byte exits `3`
with `InvalidBlobVersion` (not `InvalidSignature`), distinguishing staggered
binary rollout from attack.

Solo #4 adds `issued_at` (unix seconds) to `SnapshotPayload` as an additive
JSON field under the existing `version=1`. Blobs exported by older v0.3.0
binaries are forward-compatible via `serde(default)` → `0`; v0.3.1 treats
absent-or-zero `issued_at` as "ancient, bypass TTL" to avoid rejecting
in-flight v0.3.0 blobs during rollout.

---

## Edge cases

- **Empty stdin on `clean`** → exit 1 (`EmptyInput`). Zero-byte input is
  treated as a caller bug: a Laravel wrapper should never dispatch a job
  without content.
- **Whitespace-only stdin on `clean`** (`"   \n"`) → success. Pipeline runs,
  zero detections, returns an empty-session blob. The CLI does not try to
  distinguish "meaningfully empty" from "actually empty"; if a host wants
  that distinction it should validate before calling.
- **UTF-8 BOM on `clean` stdin** (`0xEF 0xBB 0xBF`) → passed through as
  input bytes. Appears in `clean_text` if no detector matches on it. Hosts
  that care should strip it before piping. The CLI does not silently
  strip, because silent strips are worse than honest passthrough.
- **Non-UTF-8 stdin on `clean`** → exit 1 (`InvalidEncoding`). Reclassified
  from `Io` (exit 4) per 2026-04-23 counselors review: non-UTF-8 is a
  caller bug — retrying is pointless and would create an infinite loop
  under `laravel.md:418`'s "retry with backoff" default for exit 4.
- **stdin exceeds `--max-bytes`** → exit 1 (`InputTooLarge`). Default cap
  is 10 MB on both subcommands.
- **`clean_text` containing embedded NUL bytes.** JSON strings permit
  ` `, but PHP's `json_decode`, MySQL TEXT columns under strict mode,
  and some observability systems do not round-trip NULs cleanly. The CLI
  does not strip them — again, silent strips are worse than honest
  passthrough. Hosts that route `clean_text` through NUL-hostile storage
  should scrub before persisting.
- **Empty `text` in `restore` stdin** → success, returns `{ "text": "" }`.
- **Missing required field in `restore` stdin** (`{"text": "..."}` with no
  `session_blob`, or vice versa) → exit 1 (`StdinParse`).
- **Empty `session_blob` in `restore` stdin** → exit 3 (`InvalidSignature`);
  the library rejects any payload shorter than the 97-byte version +
  key + signature header.
- **No detections on `clean`** → success. `clean_text` equals the input,
  `session_blob` encodes a valid but empty session map (still signed), and
  `stats.detections` is `0`.
- **Token-shaped literal in `clean` input.** If user content happens to
  include the string `Name_1` or `email1@example.test` as literal text and
  the current `clean` produces the same token for some real PII, Pass 1 of
  restore will swap both occurrences. This residual collision is
  documented as a known limitation; mitigation (distinct fake domain) is
  deferred to solo #6's library work.

## `stats` field stability

The `stats` object in `gaze clean`'s response is a forward-compatible
namespace. The v0.3.x contract:

- Keys present in v0.3.0 (`detections`) stay in every subsequent v0.3.x
  release with the same type and the same semantics. `detections` counts
  non-conflict-loser winning detections whose action is one of `Tokenize`,
  `Redact`, `FormatPreserve`, `Generalize` — i.e., things that actually
  altered `clean_text`. `Action::Preserve` is excluded.
- New keys may be added. Parsers MUST ignore unknown keys.
- No key is renamed or removed within a minor version.

A CI snapshot test on `stats.detections` across a fixture set guards
against accidental semantic drift (e.g. a future refactor that decides to
include `Preserve` by mistake).

## Timeouts

Gaze CLI has no built-in timeout. The handler reads all of stdin up to
`--max-bytes`, runs the pipeline synchronously, and writes the response
before exit.

Callers that need bounded wall-clock time wrap the subprocess in their own
timeout (Laravel's `Process::timeout(30)` at `laravel.md:96` is the
canonical example). The CLI installs no signal handlers and does not
self-kill — `SIGTERM` / `SIGKILL` from the supervisor is the correct
termination path.

Rationale: the CLI does one thing; deadline enforcement belongs to whoever
owns the job queue and knows the SLA. Stacking an internal deadline on top
of `Process::timeout` would mean two competing clocks with no guarantee
about which fires first.

## Pipeline wiring today vs. after solo #3

**Today (this spec).** The `clean` handler builds a stub pipeline inline:
one regex email detector, `Action::Tokenize` on `PiiClass::Email`,
`Action::Preserve` default. This is enough to land the CLI surface, the wire
format, and the test suite without blocking on policy-file design. **The
`--policy` argument is accepted but ignored in this mode.** Integration
tests against the stub exercise the wire contract, not the detector set;
see §"Test strategy" for the regression gate that runs once solo #3 lands.

**After solo #3.** The stub is replaced by:

```rust
let policy = Policy::load(&policy_path)?;
let pipeline = Pipeline::from_policy(&policy)?;
```

The stdin/stdout contract, exit codes, and session handling do not change.
A host integration written against this spec today continues to work
unmodified after #3 lands — it just starts seeing detections from the full
detector set rather than just email regex.

See [`docs/policy.md`](../../policy.md) for the schema users author against
once `--policy` is wired through.

## Test strategy

Integration tests live at `crates/gaze/tests/cli_pipe.rs` and drive the bin
via `assert_cmd`. The suite covers:

1. **Roundtrip.** `clean` a block of text, feed the blob + a mocked LLM reply
   (which reuses the emitted tokens) into `restore`, assert the original PII
   comes back.
2. **Canary.** Inject `CANARY_DO_NOT_LEAK@test.local` into the input, assert
   it is absent from `clean_text`, then assert it reappears in the `restore`
   output. Mirrors the test strategy at `laravel.md:433`.
3. **UnknownToken — hallucinated class shape.** Hand `restore` an LLM reply
   containing `Email_999` that was never in the session. Assert exit `3`
   and stderr JSON `{"error":"UnknownToken","exit":3}`.
4. **UnknownToken — lowercase FormatPreserve shape.** Hand `restore` an
   LLM reply containing `location_7` when the session emitted no Location
   tokens. Assert the shape-validator (Pass 2) catches it → exit `3`
   `UnknownToken`.
5. **Adjacency corruption regression.** `clean` a doc where `Name_1` is
   tokenized; feed `restore` an LLM reply containing `hostName_1s-record`.
   Assert the output is unchanged (`\b` boundaries hold, Pass 1 does not
   eat the substring).
6. **Tamper.** Flip a byte inside the base64-decoded blob before re-encoding
   and calling `restore`. Assert exit `3` and stderr JSON
   `{"error":"InvalidSignature","exit":3}`.
7. **Version-byte rejection.** Set the blob version byte to `99` and call
   `restore`. Assert exit `3` and stderr JSON
   `{"error":"InvalidBlobVersion","exit":3}`.
8. **Format rejection.** `--format=xml` exits `2` with
   `{"error":"PolicyConfig","exit":2}`.
9. **Argv error sanitization.** `gaze --bad-flag` exits `2` with a JSON
   stderr line, not clap's usage dump.
10. **Panic sanitization.** Inject a panic in a test-only hook; assert
    stderr is exactly `{"error":"Pipeline","exit":3}` with no backtrace,
    even with `RUST_BACKTRACE=1` in the test environment.
11. **Empty-stdin on `clean`.** Zero-byte stdin exits `1` with
    `{"error":"EmptyInput","exit":1}`.
12. **Non-UTF-8 stdin on `clean`.** Invalid UTF-8 bytes exit `1` with
    `{"error":"InvalidEncoding","exit":1}`.
13. **Oversized stdin.** Stdin larger than `--max-bytes` exits `1` with
    `{"error":"InputTooLarge","exit":1}` (test uses `--max-bytes=1024` +
    2 KB input).
14. **Silence on success.** Assert stderr is empty on every successful
    invocation across the suite.
15. **Stats semantics.** `clean` a doc with both a tokenized email and a
    detected-but-preserved organization. Assert `stats.detections == 1`.

No unit tests on the CLI module itself — the bin is thin glue over library
calls, and the library has its own unit tests. Integration tests are the
load-bearing layer for the pipe contract.

**Post-solo-#3 regression gate.** When the real policy loader replaces the
stub pipeline, the above suite must be re-run against a German address +
name canary to catch any detector silently dropping out. The suite as
written uses the stub's email-only detector, so it cannot guarantee
coverage of Name / Location / custom classes end-to-end.

## What this spec deliberately does not cover

- **Policy file format.** Owned by solo #3.
- **NER model installation.** Already covered by the library's model-dir
  resolution; the CLI inherits it through `Policy`/`Pipeline::from_policy`.
- **Persistent-key / cross-call token stability.** That is the v0.3
  persistent-session mode discussed in `laravel.md`'s Open Questions
  section, not pipe mode. Hosts that enable persistent sessions own their
  own rotation policy.
- **Structured documents.** `gaze clean` only accepts text on stdin. If a
  caller needs structured redaction, they use the library directly.
- **Windows.** Library uses `libc::mlock` / `madvise` for key protection;
  a Windows path is a v0.4+ item, not a v0.3.x patch.

## Decision log

| Date       | Decision                                                    | Rationale                                                                 |
|------------|-------------------------------------------------------------|---------------------------------------------------------------------------|
| 2026-04-23 | Bin inline in `crates/gaze`, not a separate `gaze-cli`      | Matches deleted ghostwriter precedent; no compile-graph benefit to split. |
| 2026-04-23 | `Scope::Persistent { ttl: 24h }` default                    | Matches common queue retention; `Ephemeral` is forbidden by the lib.      |
| 2026-04-23 | Restore = two-pass: tokens-first exact match + shape-validator | Shape-regex-only silently corrupts adjacent text and misses lowercase FormatPreserve tokens; tokens-first alone misses hallucinations. Two passes get both. |
| 2026-04-23 | Stderr = `{"error":"Variant","exit":N}` only, one line      | Active sanitization per `laravel.md:165`; closed variant set is PII-safe. |
| 2026-04-23 | Host MUST parse stderr JSON before hashing                  | Variant-based routing is the contract; hash-only logging is hash plus variant-typed field. |
| 2026-04-23 | Unknown variant / unparseable stderr → most-conservative action in bucket | Forward-compat across staggered rollouts without silent misrouting.       |
| 2026-04-23 | Expanded stderr variants, exit codes stay coarse            | Laravel's failure matrix needs `UnknownToken` ≠ `InvalidSignature`; variant names are Gaze-generated, safe to emit. |
| 2026-04-23 | Split `InvalidBlobVersion` from `InvalidSignature`          | Rolling binary upgrade must be distinguishable from tamper.               |
| 2026-04-23 | Policy loader split to solo #3, runs in parallel            | Lets CLI surface + tests land without blocking on file-format bikeshed.   |
| 2026-04-23 | Empty stdin on `clean` → exit 1 `EmptyInput`                | Zero-byte input is a caller bug; accepting it would paper over upstream failure. |
| 2026-04-23 | Non-UTF-8 stdin reclassified to exit 1 `InvalidEncoding`    | Exit 4 invites "retry with backoff" per host matrix; non-UTF-8 is caller bug, retry loop is wrong. |
| 2026-04-23 | `--max-bytes=<n>` default 10 MB, exit 1 `InputTooLarge`     | Uncapped `read_to_string` OOMs the worker process, not just the job.      |
| 2026-04-23 | `stats.detections` excludes `Action::Preserve`              | Caller sanity-check is "did we strip anything?", not "did we look at anything?". Locked for v0.3.x stability rule. |
| 2026-04-23 | No built-in timeout                                         | Caller owns the deadline (`Process::timeout`); two clocks is worse than one. |
| 2026-04-23 | SIGPIPE: keep Rust default, document exit 141               | Caller that closed the pipe has stopped listening; a custom handler would only add surface. |
| 2026-04-23 | `panic::set_hook` + `Cli::try_parse` mandatory in bin       | Without these, stderr discipline is aspirational.                         |
| 2026-04-23 | `stats` is forward-compatible: keys stable, new keys may be added | Standard JSON-evolution rule; parsers must tolerate unknown keys.         |
| 2026-04-23 | Exit 0 guarantees empty stderr                              | Laravel can trust "non-zero ⇒ log stderr" without filtering a success banner. |
| 2026-04-23 | Ed25519 signing reframed: bit-flip-resistant, not tamper-evident | Verifying key is embedded in the blob; forgery is possible by any actor controlling the bytes. Real integrity is AEAD on the host. |
| 2026-04-23 | `issued_at` in `SnapshotPayload` ship-blocks v0.3.0 (solo #4) | Adding it in v0.3.1 without a version-byte bump is impossible on v0.3.0 blobs; `BlobExpired` contract depends on it. |
| 2026-04-23 | Windows not a target for v0.3                               | Library uses `libc::mlock`/`madvise` for key protection; v0.4+ scope.     |
| 2026-04-24 | Blob↔text pairing = caller responsibility (v0.3 contract)   | Counter-family tokens are session-local; `restore(blob_A, cleanText_B)` is undefined behaviour. Library does not fingerprint text. v0.4 Phase 1 hardens via manifest+text provenance (no grammar break). Hybrid direction (v0.3 spec revision now, v0.4 library impl later) picked for solo #44. |
