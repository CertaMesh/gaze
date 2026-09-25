# Safety nets

Safety nets are observer-only privacy backends that audit Gaze's clean output
for PII the deterministic pipeline missed. They never replace bytes, never
mutate the [`Manifest`](../../../crates/gaze-types/src/lib.rs), and never reach
the restore path. They exist to surface leak suspects so the deterministic
detectors and rulepacks can be improved.

A policy without `[safety_net]` runs no net. `gaze setup` enables the in-process
Nym-small adapter by default; `gaze setup --safety-net none` opts out. The OpenAI
Privacy Filter subprocess adapter remains opt-in (`--safety-net openai-filter`). CLI flags and setup:
[`crates/gaze-cli/README.md`](../../../crates/gaze-cli/README.md#safety-net).

Validator-backed self-validation is handled earlier by the deterministic
[`validator-veto`](../detection/validator-veto.md) stage. Safety nets do not veto candidates
and do not participate in conflict resolution.

## Benchmark

The committed safety-net matrix populates direct-detector and observer-residual
cells for the OpenAI Privacy Filter against the 150-fixture coverage-loop
corpus. Full numbers, pins, and caveats are in
[`docs/reference/benchmarks/README.md`](../../reference/benchmarks/README.md#safety-net-matrix);
the original v0.9 report is archived at the `v0.13.0` tag as
[v0.9 safety-net benchmark](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9-safety-net-benchmark.md).
The Nym-small measurements are in [Measured](#measured).

This document describes the safety-net contract introduced in v0.6 through
PR #91. The first shipped backend is the OpenAI Privacy Filter
(`opf`) subprocess adapter; the contract is generic so additional backends
can land without changing the trait shape or audit schema.

```text
                    GAZE CLEAN INVOCATION
                            │
                            ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │ PASS 1 — REGEX + DICTIONARY (deterministic)                     │
   │   "Contact alice@example.invalid"                               │
   │     → recognizers (email.global, name.de, iban, …)              │
   │     → Candidate { class=Email, score=1.0, span=(8,28), … }      │
   └────────────────────────────┬────────────────────────────────────┘
                                ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │ PASS 2 — NER (optional, opt-in feature)                         │
   │   mBERT (Davlan) emits B-PER / I-PER / B-LOC … per token        │
   │   → Candidate { class=Name, score=0.91, span=(0,7) }            │
   └────────────────────────────┬────────────────────────────────────┘
                                ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │ CONFLICT RESOLUTION + TOKENIZATION                              │
   │   class-priority > rule-priority > score > span-len > id        │
   │   emit tokens → "Contact <{sess}:Email_1>" + Manifest           │
   └────────────────────────────┬────────────────────────────────────┘
                                │ clean_text + manifest committed
                                │ (this is what restore will reverse)
                                ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │ PASS 3 — SAFETYNET (setup enables Nym by default)               │
   │   Selector: --safety-net-backend or registry dispatch           │
   │                  ↓                  ↓                           │
   │   ┌──────────────────────┐  ┌────────────────────────────┐      │
   │   │  openai-filter       │  │  nym (setup default)       │      │
   │   │  (OPF subprocess)    │  │  (in process, ORT)         │      │
   │   │                      │  │                            │      │
   │   │  ─ heavier weights   │  │  ─ Nym-small v3 int8       │      │
   │   │  ─ OpenAI's PII set  │  │  ─ op-B label allowlist    │      │
   │   │  ─ requires `opf`    │  │  ─ `gaze setup`            │      │
   │   │    binary install    │  │    enables Nym             │      │
   │   └──────────┬───────────┘  └────────────┬───────────────┘      │
   │              │                            │                     │
   │              └──────────────┬─────────────┘                     │
   │                             ▼                                   │
   │   Net output: span array [{start, end, label, score}, …]        │
   │   over clean_text (post-tokenization!)                          │
   │                                                                 │
   │   Gaze compares the SafetyNet spans against the manifest:       │
   │     ─ Span overlaps an emitted token → covered (no leak)        │
   │     ─ Span outside every token       → "Uncovered" suspect      │
   │     ─ Span overlaps partial token    → "PartialBleed" suspect   │
   │     ─ Span overlaps wrong class      → "ClassMismatch" suspect  │
   │                                                                 │
   │   Result: leak_report attached to JSON output. Manifest         │
   │   UNCHANGED. Restore UNAFFECTED. (Axis 2 reversibility intact.) │
   └─────────────────────────────────────────────────────────────────┘
```

## `gaze index`

`gaze index ingest` detects prose names and organizations with the pinned Davlan
NER bundle (`--ner-model-dir` or `GAZE_NER_MODEL_DIR`), not with a safety net.
A net there is optional and checks ingest output under `--on-residual`.
`gaze index search` always needs an output net: TokenBridge scans every snippet
before it is shown and denies a search when no net ran, so the CLI refuses
up front with a typed `SafetyNetConfig` error. Both remaining nets satisfy it:
`--safety-net openai-filter` (with `--opf-command` and `--opf-checkpoint`) or
`--safety-net nym` (with `--nym-model-dir`).

## Locale-Aware Registry Dispatch

`Pipeline::with_safety_net(single_backend)` remains the compatibility path. For deployments with language-specific safety nets, `Pipeline::with_safety_net_registry(LocaleAwareModelRegistry)` activates locale-aware Pass-3 dispatch instead. The registry resolves one backend per clean segment using the existing four-tier order: exact locale, parent language, `Global`, then fail-closed.

The v1 dispatch contract is first-match wins. If a tier resolves multiple backends, Gaze invokes only the first registered backend and records that resolved backend id on the safety-net audit row. Multi-backend aggregation is intentionally left as follow-up work so the audit trail stays simple and deterministic.

CLI registry activation is explicit:

```sh
gaze clean \
  --policy quickstart-policy.toml \
  --locale de-DE \
  --safety-net-registry \
  --safety-net-add openai-filter \
  --opf-command /opt/opf/bin/opf \
  --opf-checkpoint ~/.local/share/gaze/models/opf \
  --opf-locales de-DE,de-AT
```

`--safety-net-registry` cannot be combined with `--safety-net-backend`; the registry is the backend selector in that mode. The only registry-capable backend is `openai-filter`; `nym` is deliberately not registry-capable (see [Audit](#audit)).

## North-star fit

Safety nets exist because of axis 1 (reliability — never leak) but must
not weaken axes 2–4. The contract therefore mandates:

- **A1 — never leak.** Safety nets read clean text after pseudonymization
  and report metadata-only suspects. Raw input never leaves the deterministic
  core. Backend-side raw bytes never cross the adapter boundary.
- **A2 — reversibility preserved.** Safety nets do not mutate the manifest
  or emit tokens, so restore round-trips are unaffected by their presence,
  failure, or absence.
- **A3 — agentic-first.** Per-field structured-document traversal lets agent
  tool-call JSON be checked field-by-field, producing field-pathed suspects
  that downstream FP-adjudication tooling can route to the right team.
- **A4 — auditable + deterministic.** Suspects carry the backend id, version,
  decoding-params hash, and an optional replay hash. The closed
  [`SafetyNetError`](../../../crates/gaze-types/src/lib.rs) variant set keeps
  failures typed; the optional `safety_net_log` SQLite table records
  metadata-only rows that the `gaze audit safety-net` subcommand can replay.

If a safety net cannot be initialized, the strict-mode CLI fails closed with
exit `3` and an error variant; tolerant mode logs the suspects and continues.
Both modes preserve the manifest contract.

## Trait shape

`gaze-types` defines the public contract.

```rust
pub trait SafetyNet: Send + Sync {
    fn id(&self) -> &str;
    fn supported_locales(&self) -> &[LocaleTag];
    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError>;
}
```

`SafetyNetContext` is `#[derive(Copy)]`, byte-free, and exposes:

- `manifest: &Manifest` — emitted token spans for the clean text segment,
  used by `Manifest::diff_against` to classify each suspect as
  `Uncovered`, `PartialBleed`, or `ClassMismatch`.
- `locale_chain: &[LocaleTag]` — session-level locale fallback chain.
  `RawDocument::Structured` shares one chain across all fields by design;
  per-field locale annotations are out of scope for v0.6.
- `document_kind: DocumentKind` — `Text` or `Structured`.
- `session_id: Option<&str>` — opaque audit session id.
- `field_path: Option<&str>` — JSONPath-style field selector for structured
  fields, e.g. `$.user.email`.

`SafetyNet::check` returns `Vec<LeakSuspect>`. A suspect carries a clean-text
byte span, a mapped Gaze `PiiClass`, the backend id, an optional confidence
score, the `LeakKind` produced by manifest correlation, the validated
`raw_label`, and an optional `field_path`. Raw payload bytes never appear on
this struct.

### Observer-only contract

The *backend* is observer-only; the *pipeline* may still act on what it reports.
A `SafetyNet` can never rewrite bytes itself — the trait has no return channel
for replacement text and no mutable handle to the manifest, by construction —
but the `SafetyNetPolicy` the caller passes decides what the deterministic core
does with the resulting `LeakReport`: nothing (`Strict`, `Tolerant`), replace the
suspect spans with a one-way marker (`Redact`), or tokenize them reversibly and re-run
(`Resolve`). The policy-less entry points below use
`SafetyNetPolicy::default()`, which is `Resolve` + `Redact` — the shipped
production default since v0.8.1. Pass an explicit `Strict` policy to
`Pipeline::clean_with_safety_net_policy_detect_context`, or use
`Pipeline::scan_safety_nets`, when you want report-only behaviour. Mode catalog
and the full lowering table:
[`safety-net-modes.md`](safety-net-modes.md#6-fallback-flag).

The pipeline calls
`Pipeline::clean_with_safety_net_detect_context`, which:

1. Runs the deterministic detection-and-redaction pipeline.
2. Records the emitted token spans into a `Manifest`.
3. Iterates the registered safety nets after a successful clean. Each
   backend receives the clean text and the immutable manifest snapshot.
4. Returns `(CleanDocument, LeakReport)` to the caller.

The bytes on `CleanDocument` are produced exclusively by the deterministic
core. A safety net cannot rewrite, append to, or veto the clean text: under an
enforcing policy it is still the core's tokenizer and redactor that mutate the
document, driven by the report, never the backend.

### The redaction marker

Where the redact path once wrote the empty string, it now writes
`[REDACTED:<class>]` — for example `[REDACTED:name]` or
`[REDACTED:custom:phone]`.

Deleting was a silent one-way loss. Nobody downstream could tell a redaction
from a typo: not the person reading the clean document, not the model consuming
it, and not a later pass of gaze itself, which saw two fragments that deletion
had glued together and read the join as a new finding. The marker keeps the same
decision — those bytes do not cross the boundary — and makes the decision
legible.

**It is not a token.** No session prefix, no ordinal, nothing to look up. Restore
never substitutes it, the hallucination guard never judges it, and the strict
restore scan treats it as ordinary prose. `gaze::is_redaction_marker` is the one
predicate every consumer asks; a second spelling elsewhere would be a second
thing to keep in step with the emitter.

The class path renders lowercased, with `:` kept as the namespace separator and
every other non-alphanumeric byte mapped to `-`. Mapping `_` is load-bearing
rather than cosmetic: every bare arm of the token-shape grammar needs a trailing
`_<digits>` inside word boundaries, so a custom class legitimately named
`address_2` would otherwise make `[REDACTED:custom:address_2]` contain the token
shape `custom:address_2` — the marker would parse as a token. Dropping the
underscore makes that unrepresentable rather than merely untested;
`a_redaction_marker_never_parses_as_a_token` pins it against every builtin class
and the adversarial custom ones.

Mapping everything else is what keeps the emitter and the predicate from drifting
apart. `PiiClass::custom` normalises, but `PiiClass::Custom` is a public variant
an adopter's own `SafetyNet` can build directly or deserialize, and
`PiiClass::family` does not normalise its name; a class carrying an uppercase
letter, a space or a `]` used to render a marker `is_redaction_marker` rejected.
The index is the one production consumer of that predicate — it skips markers so
a one-way redaction never becomes a searchable, translatable entity — so the
divergence meant those redactions were indexed. Sanitising in the emitter makes
`is_redaction_marker(redaction_marker(c))` true for every `PiiClass` by
construction. The exact class is still carried by the audit row.

**In the manifest.** A marker is recorded like any other one-way replacement:
`Action::Redact`, not owned, standing for the original bytes it covered, with
the ids of every suspect that drove it. A merged region is one marker carrying
the class of its lowest-offset suspect, while the audit log still writes one row
per suspect — merging must not merge away who asked for the redaction.

**A marker is never redacted again.** `REDACTED` is a capitalised word in
ordinary prose, which is exactly what a NER model reads as an organization. A
suspect lying wholly inside a marker is dropped as already protected, on the
manifest's authority rather than the text's — a document that merely *types*
`[REDACTED:name]` gains nothing.

**Containment, not overlap.** The first design dropped any suspect that merely
*overlapped* a marker. That was rejected because it leaks: a net that reports
`[REDACTED:name] Schmidt` has flagged a surname, and dropping the whole finding
because half of it is a marker ships `Schmidt` raw. It also excused suspects that
were simply malformed -- out of bounds, reversed, splitting a character --
whenever they happened to touch a marker, where those must stay unjudgeable and
deny. So a suspect is protected only when it lies *wholly inside* a marker gaze
recorded. A suspect that straddles a marker and real text is judged by the
ordinary rules, and since it overlaps a manifest entry it denies the document:
fail-closed, never a leak. On the benchmark corpus no straddling suspect occurs
-- see the evidence below -- so the denial costs nothing measured; if one ever
appears in practice, the refinement is to act on the bytes outside the marker,
not to relax containment.

**Evidence.** The benchmark's `full-stack-nym-redact` arm runs Nym-small under
`SafetyNetMode::Redact` so that every suspect goes through the redaction path;
the shipped `full-stack-nym-resolve` arm cannot show this, because on the corpus
it resolves every Nym suspect reversibly and its fallback never fires. Compared
against the deleting implementation over the full 2,910-document corpus,
`scripts/bench/marker_ab.py` found the same 1,296 spans redacted in the same
1,014 documents, identical leaked and false-positive byte counts, no new
rejections, and -- in every document -- clean text identical to the deleting
output once the markers are removed.

**What it costs.** The output is longer than the input for those spans, where
deleting made it shorter. Adopters who diffed clean text against raw byte counts
will see that change; nothing about which spans get redacted moved.

### Terminal admission after a `Redact` fallback

Under `Resolve` + `Redact` the fallback *replaces* the residual spans it could
not resolve with a one-way marker. That changes the input string, so the scan
that follows is the first pass to see that text, and it routinely reports a
short sub-word span that the earlier passes read and accepted. Denying on every
such span held a fallback document to a standard no completing document has to
meet.

The terminal report instead gets one reversible round and one bounded
replacement, and each remaining suspect is classified into a closed set:

| Case | Condition | Outcome |
|------|-----------|---------|
| `FallbackIncomplete` | The suspect covers bytes the fallback's own audit rows say it removed. | Deny. Nothing further is replaced first. |
| `SeamManufactured` | The suspect's span strictly **contains** a deletion seam, so part of its shape exists only because the fallback removed what sat between two fragments. Abutting a seam is not this. | One bounded replacement through the ordinary fallback path. A second one denies. **Unreachable since the fallback started writing a marker** — a marker separates the fragments a deletion used to glue together, so no seam exists to contain. Kept, unmeasured-for-removal, under solo todo 3739. |
| `Unjudgeable` | The suspect names no real range of the document, its own coverage claim contradicts the manifest, or the bytes it covers carry a token shape this pipeline never minted. | Deny. |
| `Admit` | Anything else: a finding about the document that no stage is permitted to act on. | Merged into the returned `LeakReport`; the document completes carrying it. |

Both bounds — one reversible round, one replacement — are straight-line code, not a
loop with a counter. After they are spent, one more scan runs and the same
classification applies: `Admit` ships with an honest report, anything else
denies. Admission is strictly wider than the rule it replaced, so no document
that completed before can begin denying.

The round tokenizes; it never redacts. Its audit rows are
`decided_by: Resolve` with `action: Tokenize`, carrying the `FallbackReason`
that made the round run — the combination that tells them apart from the
second batch's rows. The protection trace projects them as an ordinary
`("safety_net", "resolve", "tokenize")`, so the benchmark scorer's closed
provenance set is unchanged.

**Coordinates.** `map_clean_boundary_to_raw` infers original-request offsets
from the manifest alone, assuming every untokenized clean run stands for an
equal-length raw run. A *deletion* removes clean bytes and no raw bytes, so that
assumption failed for everything after the first removed region, and every later
mapping had to be rebuilt from a deletion ledger.

Writing a marker is what removed that whole branch from the product path. A
marker is an ordinary one-way manifest entry — `Action::Redact`, not owned,
exactly the shape the primary pass has always emitted for a redacting policy —
so the clean/raw alignment stays affine and the plain mapper describes the
document end to end. The layout code that reconciled a deletion ledger against
the manifest remains in tree but is no longer reachable; solo todo 3739 measures
its removal.

A
resolution gap must map to exactly as many raw bytes as it has clean bytes,
which is what prevents a token standing for bytes on both sides of a seam.

**Cost.** A fallback document whose terminal scan reports anything runs one
extra model pass. Only documents that reach the `Redact` fallback can.

### Sub-word suspects are never acted on

A name, location or organization suspect whose action span starts or ends
between two letters or digits is a model firing on part of a word (`Pass` in
`Passwort`). Tokenizing or deleting it protects nothing whole and hands the
agent a mangled word, so under `Resolve` and `Redact` no stage acts on it: not
the first pass, the second batch, the terminal round, `Redact` mode or the
`Redact` fallback. Its bytes stay, it gets the same `Preserve` audit row as a
suspect inside a live token, it stays in the returned report, and a
`LeakReportTelemetry::UnactionableSubword` row (CLI JSON kind
`UnactionableSubword`) carries its net, class and offsets. `Observe` modes are
unchanged.

| Rule | Why |
|------|-----|
| Judged in the text the net reported on | The terminal round judges at scan time, so a whole word that its own seam deletion later glues to a neighbour is still resolved. |
| A token's `<`/`>` is a word boundary | A gap starting right after a token is a whole word. |
| A span touching a token shape is never a sub-word | Foreign-token handling (fallback, `Unjudgeable`) must still see it. |
| No minimum length | A standalone letter is a whole word and often an initial (`J.`). |
| Identifier classes are exempt | Their values legitimately sit inside longer strings (`ID12345`). |
| `FallbackIncomplete` and a second seam finding still deny | A sub-word shape does not excuse a fallback that failed or a deletion outpacing itself. |

A net that decodes whole words does not need this guard; for every other net
and for registry models it is defense in depth.

**Cost (axis 1).** A net that does not decode whole words (OPF, adopter nets)
can flag a real name inside a longer word, for example `Meier` in `Meiers`. Under `Resolve` with the `Redact` fallback and
in `Redact` mode that suspect ships raw, with its `Preserve` audit row and
`UnactionableSubword` row. Under the `Strict` fallback it counts as a residual
and the document is refused. Earlier releases tokenized or deleted the flagged
part of the word instead.

### Locale gating

Each `SafetyNet` declares `supported_locales`. When the session-level locale
chain does not intersect the backend's supported locales, the orchestrator
emits a `LeakReportTelemetry::LocaleSkipped` event instead of running the
backend. Skip telemetry is bytes-free and is recorded against the same
`safety_net_log` table as suspects.

`LocaleTag::Other(_)` matches strictly against the wire form, not the BCP-47
prefix; this matches the locale-chain semantics described in
[`docs/explanation/policy/locale-chain.md`](../policy/locale-chain.md).

### Closed error variant set

[`SafetyNetError`](../../../crates/gaze-types/src/lib.rs) is an exhaustive,
serde-stable enum:

| Variant | Meaning |
|---------|---------|
| `Unavailable { reason }` | Safety net was requested but is not configured. |
| `WeightsMissing { path }` | Required checkpoint or model file is missing. Path is sanitized to `<missing:filename>`. |
| `ModelUnavailable { reason }` | Backend could not be loaded, perms verification failed, or runtime is missing. |
| `InputTooLarge { limit, actual }` | Clean text exceeded the configured input cap. |
| `Runtime { message }` | Backend execution failed, including subprocess timeouts. |
| `InvalidOutput { message }` | Backend returned malformed output (non-UTF-8 stdout, non-finite score, unknown label). |

The CLI maps each variant to a stable `SafetyNetFailure` exit-3 sub-variant
so adopters can branch on `Unavailable` versus `Timeout` versus
`InvalidOutput` without parsing free-form text.

## OpenAI Privacy Filter adapter

The first shipped backend is the
[`OpenAiFilterSafetyNet`](../../../crates/gaze-recognizers/src/safety_net/openai_filter/mod.rs).
It calls the official `openai/privacy-filter` CLI as a subprocess.

### Command choice

Gaze binds to the **official** `openai/privacy-filter` repository. Adopters
must install `opf` from a pinned upstream Git revision or an official release
tarball. The official CLI was chosen over the `chiefautism/privacy-parser`
fork because it documents pipe input, exposes a stable JSON schema, and
publishes a reproducible Git history. The fork could be re-evaluated if a
later review confirms native byte spans and the absence of PII-bearing JSON
fields, but it is not the v0.6 default.

The adapter always invokes `opf --format json --output-mode typed`. Output
mode `typed` is the only accepted shape; other modes are not parsed.

### Whole-text input

Piped stdin is not one input for the stock CLI: `opf` reads it line by line,
skips blank lines, and prints one JSON result per line with offsets relative
to that line. After the configured arguments (and `--checkpoint`), the adapter
therefore always appends:

```text
--no-print-color-coded-text --text-file /dev/stdin
```

The text still travels over the stdin pipe and is never written to disk. The
colour flag stops the ANSI section `opf` otherwise prints after the JSON.
`opf` reads the file in Python text mode, so `\r\n` and a lone `\r` each become
one `\n` character in the offsets it returns; the adapter maps those offsets
back to UTF-8 byte offsets in the exact clean text it sent.

The adapter accepts a result only if it is exactly one JSON object whose echoed
`text` equals the text `opf` should have read. A second document, a missing
`text`, or any other `text` (a line, a trimmed or rewritten input) is
`InvalidOutput`, so offsets relative to some other text can never be applied.
Empty clean text returns no spans without starting `opf`.

A wrapper command configured instead of `opf` must accept these arguments and
echo the analysed text. Windows has no `/dev/stdin`: there only the colour flag
is appended, `opf` still splits lines, and the echo check refuses multi-line
text rather than mis-mapping it.

### Subprocess configuration

[`SubprocessOpenAiFilterConfig`](../../../crates/gaze-recognizers/src/safety_net/openai_filter/backend/subprocess.rs)
is a builder with the following defaults:

- `timeout`: 5 seconds. Configurable via `--safety-net-timeout-ms`.
- `max_input_bytes`: 1 MiB. Configurable via `--safety-net-input-limit-bytes`.
- `max_stdout_bytes`: 4 MiB.
- `capture_stderr`: `false`. Stderr is routed to `Stdio::null()` by default.
- Decoding params: `format=json`, `output_mode=typed`. Operating-point flags
  add `min_score` and `operating_point` entries.

`SubprocessOpenAiFilterConfig::from_env()` reads `GAZE_OPENAI_FILTER_OPF` so
adopters can pin the install path centrally.

### PII-bearing upstream JSON fields are stripped at the boundary

Upstream OPF emits per-span `text` and `placeholder` fields that carry the
literal source bytes. These are private deserialization details inside the
adapter:

- `PrivateOpfSpan` and `PrivatePiiString` are private structs in the
  adapter module. The crate root re-exports the public trait shape and the
  config builder, but never the private structs themselves.
- `PrivatePiiString::Debug` writes `<private-opf-field>` instead of the raw
  contents.
- `PrivatePiiString::Drop` clears the underlying string buffer when the
  span is dropped.
- After `serde_json::from_str` returns, the adapter calls
  `PrivateOpfSpan::into_raw_span`, which produces a `RawSpan` containing
  only `start`, `end`, `label`, and `score`. The per-span `text` and
  `placeholder` fields are never deserialized, so their contents are skipped
  by the parser and never held in memory.
- The top-level `text` echo is held in a `PrivatePiiString`, compared with
  the text the adapter sent, and scrubbed on drop. `redacted_text` is never
  deserialized.

After this projection, no part of Gaze that consumes safety-net output sees
upstream raw bytes. The adversarial regression
`safety_net_correlates_raw_spans_with_manifest_without_source_text` covers
this invariant.

### Stderr discipline

By default `child.stderr` is `Stdio::null()`, so verbose backend logs cannot
race with the JSON adapter or appear in operator logs. Adopters who need
diagnostics can enable `SubprocessOpenAiFilterConfig::with_stderr_diagnostics(true)`,
which:

1. Captures a stderr prefix in a bounded buffer of at most 256 bytes and
   drains/discards the rest to EOF so a verbose child cannot block its pipe.
   Diagnostic overflow alone does not fail inference. An incomplete trailing
   token is discarded back to the last captured ASCII whitespace before
   redaction. Arbitrary Unicode bytes and control bytes are not evidence of
   a complete raw token; a partial email or phone token could evade the sanitizer.
2. Maps non-printable bytes to spaces.
3. Sanitizes whitespace-separated tokens with the same redactor used for
   error messages: any token containing `@` or seven or more ASCII digits
   is replaced with `<redacted>`. This catches the most common email and
   phone shapes that backends might log.
4. Truncates sanitized output to the 256-byte cap, including a
   `[truncated]` marker when capture or display was shortened.

Stdout still has
a hard byte cap: overflow, I/O errors, invalid model output, and timeouts remain
errors. Diagnostics stay disabled by default. The heuristic redactor does not
provide general PII-detection completeness.

The `verbose_stderr_is_stripped_and_capped` test locks both the cap and the
sanitization rule.

### Subprocess timeout and resource isolation

Both subprocess adapters provide cancellable pipe I/O on Unix and Windows.
Windows uses exclusively owned parent pipes with nonblocking writes and
availability-bounded reads; see [Windows pipe ownership](windows-subprocess-io.md).
Targets that are neither Unix nor Windows currently have no adapter and return
`ModelUnavailable` before spawn. This describes this implementation, not a claim
that those targets cannot support subprocesses. In-process backends are unaffected.

The subprocess runner enforces a single deadline that covers stdin write,
stdout read, stderr read, and child wait. Parent pipe ends are nonblocking,
with cancellation checked between I/O operations and at most 5 ms of idle
polling delay (plus OS scheduling). On timeout or worker failure the adapter:

1. Signals cancellation to all pipe workers.
2. Sends `SIGKILL` (`Child::kill`) and reaps the direct child via `wait` to
   prevent zombies.
3. Joins every worker, closing all parent pipe ends without waiting for EOF
   from descendants. Descendant processes themselves are not killed.
4. Returns the original failure; for a deadline, `SafetyNetError::Runtime`
   with the message
   `"opf subprocess timed out and was killed"`. The CLI maps this branch to
   exit-code `3` with `variant = "Timeout"`.

Success still requires completed stdin, stdout/stderr EOF, and successful
direct-child exit. A descendant keeping a pipe open produces a timeout, never
partial successful output. Cleanup is cooperative, not a hard real-time
guarantee against OS scheduling delays or an uninterruptible child wait.

Initialization failures are cached in a `OnceLock<Result<Arc<_>, Arc<_>>>`
so deterministic problems (missing checkpoint, malformed config) are not
retried on every safety-net check. This is the explicit fix for "retry storm
on every clean" and is locked by `empty_command_failure_is_cached`.

### Checkpoint and cache permission verification

`SubprocessOpenAiFilterBackend::new` runs path-safety checks before any
spawn:

- The `opf` command path must be a regular file (not a symlink) when an
  absolute path is supplied. Bare command names are accepted so adopters
  can rely on `PATH` resolution when the host is hardened.
- `--openai-filter-checkpoint` must exist; missing files produce
  `WeightsMissing { path: "<missing:<filename>>" }`. The path is sanitized
  to the file basename so logs cannot leak operator directory layout.
- Checkpoint files and directories must be owned by the current uid, must
  not be symlinks, and must not be group/world writable on Unix. Directories
  must be mode `0700`. Windows enforces non-symlink + readonly ACL.
- The optional cache directory is created mode `0700` if it does not exist
  and is then verified by the same recursive walk.

The `group_writable_checkpoint_file_fails_closed` test pins the perm rule.

### Class mapping

`map_openai_label` accepts the closed set `private_person`,
`private_address`, `private_email`, `private_phone`, `private_url`,
`private_date`, `account_number`, `secret`. Unknown labels return
`SafetyNetError::InvalidOutput`. The mapping into Gaze's `PiiClass` lives
in [`class_map.rs`](../../../crates/gaze-recognizers/src/safety_net/openai_filter/class_map.rs);
the `class-map-override-safety` xtask gate runs the
`all_official_labels_map_exactly_to_gaze_classes` test on every PR.

## Nym-small adapter

`--safety-net nym` runs [`Wismut/nym-pii-multilingual-small`](https://huggingface.co/Wismut/nym-pii-multilingual-small)
v3 (int8 ONNX, ModernBERT, 22 languages including German and English) in
process through ONNX Runtime. The `gaze setup` policy enables it by default. With no policy, nothing loads
unless `nym` is selected explicitly. Source:
[`crates/gaze-recognizers/src/safety_net/nym/`](../../../crates/gaze-recognizers/src/safety_net/nym/mod.rs),
behind the `safety-net-nym` feature (on in the default `gaze-cli` build through
`setup`).

The net flags; the pipeline decides. Suspects go through the same resolve,
fallback and audit path as every other net.

### Pinned bundle

`gaze setup` downloads `int8/config.json`,
`int8/model_int8.onnx` and `int8/tokenizer.json` at revision
`4348999cd3c2e20c49615e9af7c6bbb45b64cd85` into
`${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/nym-small-int8`, writes the
canonical `SHA256SUMS`, and verifies it. At load the backend checks the SHA-256
of `SHA256SUMS` against `NYM_SMALL_INT8_BUNDLE_SHA256`, every listed file
against its digest, owner and modes (directory `0700`, no group/world write, no
symlinks), and that `config.json` lists exactly the 81 BIO labels the decoder
assumes. Any failure is a typed `SafetyNetError` before the model loads; there
is no download at inference time. `gaze mcp doctor` reports the bundle: absent
passes (the net is opt-in), present-but-invalid fails.

### Which labels can fire

The model labels 40 entity types. Six carry a Gaze class; the other 34 can
never be enabled, and none is folded into a generic class such as `Name`.

| Nym label | Gaze class | op-B default |
|---|---|---|
| `BUILDING_NUMBER` | `custom:building_number` | on, `>= 0.5` |
| `LICENSE_PLATE` | `custom:license_plate` | on, `>= 0.5` |
| `USERNAME` | `custom:username` | on, `>= 0.5` |
| `DATE_OF_BIRTH` | `custom:date` | on, `>= 0.9` |
| `TAX_ID` | `custom:tax_id` | off |
| `ZIP_CODE` | `custom:postal_code` | off |

The allowlist and thresholds are policy data (`[safety_net.nym]`, see
[policy reference](../../reference/policy.md#safety_netnym)). Without the table
the backend uses op-B, the operating point measured in the probe below. An
unknown label, a label without a Gaze class, a label without a threshold, a
threshold for a label that is not enabled, or a threshold outside `(0, 1]` fails
at policy load. TAX_ID stays off because its precision was 0.23 to 0.26 at every
threshold; ZIP_CODE stays off because it flagged the invalid-identifier decoys
in the negative corpus (op-A precision 0.572 on the gate set).

### Decoding

Per piece, the entity mass of a label is `P(B-label) + P(I-label)`. The piece's
label is the argmax over **all 40** labels, so a piece that looks most like
`GIVEN_NAME` is never relabelled into an enabled label. It counts only when that
label is enabled and its mass reaches the label's threshold. Spans are assembled
from whole words with the same word rule as the pipeline's sub-word guard
(`gaze_types::is_inside_word`, one definition): any counted piece labels its
word, the strongest piece picks the label, the score is the minimum over the
pieces carrying it, and a word whose first counted piece is `I-` extends an open
span of the same label.

The tokenizer reports character offsets; the decoder trims metaspace whitespace
and converts them to UTF-8 byte offsets, with fixtures on umlauts, NFD combining
marks, emoji, NBSP, NARROW NBSP, CRLF line breaks and a span that ends the
text.

A decoded span is then trimmed of structural punctuation at both edges: JSON
double quotes, colons, commas, square brackets, braces and whitespace. A piece
can carry the quote or brace next to a value (`"Anna`), so without the trim a
suspect over tool-call JSON reaches into the syntax around the value. Trimming
only narrows a span and never widens it; the new edges border a structural
character, so they never cut a word, and a span that is syntax alone is
dropped. Inner punctuation stays: `M-AB 1234` keeps its hyphen and space.

### Every byte is scanned

Input is tokenized without truncation and scored in windows of 512 pieces that
overlap by 64; a piece seen twice keeps the row from the window where it sits
furthest from an edge. A piece no window scored, or a non-whitespace character
no piece covers, is `SafetyNetError::InvalidOutput`, never a silent gap. Input
above `--safety-net-input-limit-bytes` is `InputTooLarge`.

### Audit

Every suspect carries `safety_net_id = "nym-small-int8"`, the score, and
`raw_label = "LABEL>=THRESHOLD"` (for example `LICENSE_PLATE>=0.5`), so the row
records which rule fired without a schema change. For that reason Nym is not
available through `--safety-net-registry`: registry dispatch reports a model
span's class, not its label and threshold, and the CLI refuses the combination.

### Runtime

ONNX Runtime on CPU with deterministic compute, one inter-op thread and one
intra-op thread by default (`--nym-intra-threads`, `GAZE_NYM_INTRA_THREADS`).
The model weights are embedding-int8 with fp16 body weights and fp32 compute,
so there is no int8-kernel speedup.

### Measured

The 2,910-document probe (todo 3675) ran op-B through the full pipeline and
measured: **6,017 leaked gold bytes bought** under scored-label contract v2,
**+517 false-positive bytes**, action precision 0.890, 1 false flag across
1,024 PII-free documents, 1 one-way deletion. See
[the in-process reproduction](#reproduction-in-process) for the numbers of this
backend on the canonical harness (`clean_for_bench --config
full-stack-nym-resolve`).

### Reproduction in process

The same population through `clean_for_bench --config full-stack-nym-resolve`
(this backend, one intra-op thread) against `pass2-ner` on the same commit:

| Row | Leaked bytes v2 | Bought v2 | False-positive bytes added | Action precision | One-way deletions | Exact restore |
|---|---:|---:|---:|---:|---:|---:|
| rules + NER, no net | 20,727 | | | | | 2,910 / 2,910 |
| `full-stack-nym-resolve` | 14,573 | 6,154 | +526 | 0.891 | 1 | 2,909 / 2,910 |

The 137 bytes more than the probe come from class routing: the probe mapped
building numbers to `location` and plates to `account_number`, so a span next
to a rule token of that class resolved against it; with their own classes 42
more spans tokenize. Timings from that run are provisional (shared host under
load) and are not a latency claim.

Every residual suspect a post-policy re-scan reports sits inside a Gaze token:
the model reads token text such as `Custom:building_number` as a building
number. A suspect inside a live token is never acted on, so bytes and restore
are unaffected, under every `Resolve` fallback including `strict`
(`nym_suspect_inside_its_own_token_text_is_protected_under_every_resolve_fallback`).
Masking token text before the net reads it is a follow-up (todo 3681). A first
attempt replaced every manifest token with same-length spaces before inference
and was measured and not shipped: it removed the token-text flags, but the
model lost the tokens as context and bought 18 % fewer leaked bytes (v2 6,154
to 5,039 on the 2,910 documents), so a different mask shape needs its own
measured proposal.

### Known gaps and open review items

- **Room, platform and seat numbers.** `BUILDING_NUMBER` fires on "Raum 204"
  and "Gleis 9, Wagen 23, Platz 45": the 1,024-document negative corpus
  contains none of these shapes, so its false-flag rate says nothing about them.
  The fixture `room-number-known-gap` pins the current behaviour; an
  address-context guard is a follow-up.
- **Latency measured (2026-09-24).** On a quiet MacBook Pro M5 Max with 64 GB RAM, macOS 26.5, release build, 30 documents: rules + NER p50 40.0 ms / p95 65.3 ms; with Nym p50 89.8 ms / p95 171.9 ms, peak memory 1,027 MB. This closes the default-decision latency item; it is a small local sample, not a fleet guarantee.

### Licence review (open)

The model card declares MIT (inherited from `jhu-clsp/mmBERT-small`); the
repository has no LICENSE file. v3 training data includes 77.5k Wikipedia
passages auto-labelled by gemma-4-26b. Whether CC-BY-SA obligations reach
weights trained on that text, and whether the teacher model's terms add
conditions, still needs legal review.

On 2026-09-24 the user accepted this open question and decided to enable Nym
in generated setup policies with an explicit setup notice. That notice names
the model and MIT model-card licence, links this item, names the upstream
source and pinned revision, and gives `gaze setup --safety-net none` as the
opt-out. Gaze does not vendor the weights; setup fetches the pinned revision
from the upstream repository.

### Replacement candidates

Which lighter models are worth testing in place of OPF, and the bars they
must clear, is recorded in [Safety-net candidates](safety-net-candidates.md).

## Structured-document per-field behavior

`Pipeline::clean_with_safety_net_detect_context` traverses
`RawDocument::Structured` field by field. For each scalar string field it:

1. Cleans the field through the deterministic pipeline.
2. Builds a per-field `Manifest` from the emitted token spans.
3. Runs each registered safety net with `field_path = Some(<JSONPath>)`.
4. Aggregates the per-field reports into the run-level `LeakReport`.

This means a class mismatch detected on `$.user.email` is reported with
that field path, and the FP-adjudication query
`gaze audit safety-net query --field-path '$.user.email'` can isolate
it. Locale-skip telemetry is also recorded per field when the session-level
locale chain does not match.

### The structured path is observer-only, and says so

**A structured document accepts only an observer policy.** Passing
`SafetyNetMode::Redact` or `SafetyNetMode::Resolve` with a
`RawDocument::Structured` returns
`Error::UnsupportedSafetyNetModeForStructured` before any field is
tokenized. The traversal above has no enforcement stage: it cleans each
leaf, runs the nets over the result, and reports. Accepting an enforcing
policy and quietly performing observation would be the worst of both — the
caller is told `Ok`, and the suspect bytes are still in the document.
Failing closed is the axis-1 answer.

Use `SafetyNetMode::Strict` (reject at your boundary when the report is
non-empty) or `SafetyNetMode::Tolerant`, via
`Pipeline::clean_with_safety_net_policy_detect_context`, or
`Pipeline::scan_safety_nets_structured` for a read-only pass over an
already-clean document. Note that the policy-less
`Pipeline::clean_with_safety_net*` entry points default to `Resolve`
(`SafetyNetPolicy::default()`), so they are text-only in practice.

Enforcement for structured documents is not implemented rather than
forbidden: per-field enforcement is a coherent future feature (each leaf
carries its own manifest, so a leaf could be resolved or redacted in
isolation). Until it exists, the contract says so out loud.

### One walker

All three structured traversals — pseudonymize, clean-and-scan, and
scan-only — are the single `walk_structured` in
`crates/gaze/src/pipeline.rs`, parameterized by a `LeafOp`. They were three
near-identical copies and had already drifted. What the op varies is
documented on `LeafOp` itself: empty-string skipping, whether scalar leaves
are scanned, whether the document is rebuilt, and the root field-path
prefix.

The integration coverage lives in
`crates/gaze/tests/safety_net.rs`:
`structured_safety_net_traverses_nested_fields_and_preserves_shape`,
`structured_walk_has_nested_parity_across_every_leaf_op`, and
`structured_documents_do_not_silently_observe_when_enforcement_is_requested`.

## Replay hash

`LeakReport.replay_hash` is an `Option<String>`. When set, it is a stable
hash over the backend id, backend version, decoding-params, and operating
point used for the run. The hash supports replaying the same input through
the same configuration to see whether the FP set has stabilized.

Replay determinism is only guaranteed when the operator fixes the command
path, checkpoint, operating point, minimum score, and decode parameters
**externally**. The adapter emits and stores the hash; it does not pin
upstream weights or downloads. Adopters using a different `opf` checkpoint
will see a different hash and a different suspect set, by design.

## `safety_net_log` audit table

When `gaze clean --audit-db <path>` is combined with `--safety-net <kind>`,
each suspect plus each `LocaleSkipped` telemetry event is appended to the
`safety_net_log` table in the same SQLite database that holds the
deterministic redaction log.

```sql
CREATE TABLE IF NOT EXISTS safety_net_log (
    id INTEGER PRIMARY KEY,
    safety_net_id TEXT NOT NULL,
    raw_label TEXT NOT NULL,
    mapped_class TEXT NOT NULL,
    leak_kind TEXT NOT NULL,
    span_len INTEGER NOT NULL,
    document_kind TEXT NOT NULL,
    field_path TEXT NULL,
    score REAL NULL,
    created_at INTEGER NOT NULL,
    session_id TEXT NULL,
    pipeline_class TEXT NULL,
    safety_net_replay_hash TEXT NULL,
    backend_id TEXT NULL,
    backend_version TEXT NULL,
    decoding_params_hash TEXT NULL,
    telemetry_kind TEXT NULL
);
```

The schema stores **metadata only**:

- `raw_label` is the validated upstream label, such as `private_email` —
  not the upstream raw text.
- `mapped_class` is the Gaze `PiiClass` produced by the class map.
- `span_len` is the byte length of the suspect span; the offsets are not
  persisted.
- `field_path` is the structured field selector when applicable.
- `pipeline_class` is the manifest class for `ClassMismatch` rows.
- `telemetry_kind` is set for `LocaleSkipped` rows so downstream queries
  can filter telemetry from suspects.

The `safety_net_log_does_not_persist_suspect_or_placeholder_bytes` and
`restricted_columns_have_no_raw_payload_fields` tests are run on every PR
by the `safety-net-sanity` gate; both lock that no upstream raw text or
placeholder bytes are stored.

The schema lives in `gaze-audit`; the protected-path Dylint gate keeps
`gaze` core free of `gaze-audit` imports outside the explicit
audit-responsible allowlist
(see [`docs/explanation/contributing/xtask-gates.md`](../contributing/xtask-gates.md#cargo_metadata_audit_isolation-self-test)).

### Querying suspects

`gaze audit safety-net query --audit-db <path>` filters the
`safety_net_log` table by leak kind, raw label, mapped class, structured
field path, and creation time. The query is opened
`SQLITE_OPEN_READ_ONLY` so the CLI cannot mutate the log even if compromised.

## CI gate

`safety-net-sanity` is the canonical local pre-push gate for the safety-net
surface.

The xtask command lives at
[`crates/xtask/src/safety_net_sanity.rs`](../../../crates/xtask/src/safety_net_sanity.rs)
and batches required behavioral tests across four target suites:

- `gaze` — manifest diff and structured traversal invariants.
- `gaze-cli` — strict/tolerant exit-code behavior.
- `gaze-recognizers` — OPF subprocess boundary, stderr sanitization, label
  mapping.
- `gaze-audit` — `safety_net_log` schema and bytes-free invariants.

Run the xtask gate manually before shared-branch pushes; the gate is **not**
scheduled nightly in v0.6 and the live-model nightly workflow is deferred —
see the "Future work" section below.

## Activation surface

`[safety_net].backend = "nym"` activates Nym for policy-driven assembly in
Rust and for CLI verbs that load the policy. `[safety_net.nym]` configures its
bundle location, allowlist and thresholds. An absent table or `backend =
"none"` runs no net. OpenAI Privacy Filter can only be selected on the
command line in this release. A missing feature or invalid bundle fails
closed before input is processed.

The minimum CLI form is:

```sh
gaze clean \
  --policy=policy.toml \
  --safety-net=openai-filter \
  --openai-filter-command=/opt/opf/bin/opf \
  --openai-filter-checkpoint=/opt/opf/checkpoint \
  --safety-net-mode=strict
```

Programmatic adopters call `Pipeline::with_safety_net(OpenAiFilterSafetyNet::new(config))`
behind the `safety-net` feature on `gaze` and `safety-net-openai` on
`gaze-recognizers`. Both features are off by default; the safety-net code
path is excluded from the default `cargo build` graph.

For policy-driven Rust assembly, enable `gaze-assembly/safety-net-nym` and
call `build_pipeline`. The CLI calls the same Nym attachment code. Its
repeatable `--safety-net` values replace policy selection; `none` disables
all nets for one run, with a notice if policy Nym was active. Multiple selected
nets run and the pipeline unions their suspects. Bundle paths resolve from
CLI flag, then environment, then policy; library assembly reads only the
policy path unless given an explicit override.

## Future work (deferred to a post-v0.6.0 release)

The following items are filed for a release after v0.6.0 and intentionally
not in the v0.6 SafetyNet rollup scope:

- **Live-model nightly workflow.** A scheduled cron that runs the safety
  net against a non-empty synthetic corpus to detect FP-rate drift between
  checkpoint upgrades.
- **Native `ort` backend.** A first-party in-process backend that loads OPF
  weights through `ort` plus a `weights.rs` SHA-pinned scaffolding module,
  removing the subprocess hop. The trait shape on `OpenAiFilterBackend`
  was designed so the same adapter API serves both subprocess and in-process
  implementations.
- **Fetch / download command.** A `gaze safety-net fetch` UX that pulls a
  pinned `opf` build into a private cache directory and verifies the
  checksum offline. Closes the "first-run requires manual install" gap.
- **Long-lived subprocess / daemon mode.** The current adapter spawns one
  `opf` invocation per clean. A persistent helper would amortize startup
  cost when latency budgets tighten.
- **False-positive adjudication dashboard.** A UI on top of
  `gaze audit safety-net query` and `audit export` that lets reviewers
  triage suspects across runs.

Cross-references:

- PR #91 — Pass-3 SafetyNet rollup.
- v0.6.0 audit feature shim drop. Adopters must import concrete audit sinks
  from `gaze-audit` directly.

## See also

- [`docs/reference/policy.md`](../../reference/policy.md) — explicit note that the v0.6 safety
  nets are CLI-only.
- [`crates/gaze-cli/README.md`](../../../crates/gaze-cli/README.md) — full flag
  reference, exit-code map, and synthetic examples.
- [`docs/reference/crates.md`](../../reference/crates.md) — workspace map, including the
  safety-net feature gates on `gaze`, `gaze-recognizers`, and `gaze-cli`.
- [`docs/explanation/contributing/xtask-gates.md`](../contributing/xtask-gates.md) — `safety-net-sanity` and
  `class-map-override-safety` gates.
- [`AGENTS.md`](../../../AGENTS.md#project-north-star) —
  the five-axis north star that the safety-net contract is checked against.
