# Safety-net modes

A safety net rereads Gaze's clean output and reports suspects: bytes that look
like PII the deterministic pipeline did not tokenize. The net itself never
edits anything ([observer-only contract](safety-nets.md#observer-only-contract)).
The safety-net **mode** decides what the pipeline does with those suspects, and
the **fallback** decides what happens when the chosen mode cannot finish the
job.

The default is `resolve` with a `redact` fallback. Set the mode and fallback
with `--safety-net-mode` and `--safety-net-fallback`; `gaze clean` and
`gaze daemon` share the same two flags. In Rust, pass a `SafetyNetPolicy` to
`Pipeline::clean_with_safety_net_policy_detect_context`;
`SafetyNetPolicy::default()` is the same `resolve` + `redact` pair.

## The four modes

| Mode | What happens to a suspect | Reversible? | Production use |
|---|---|---|---|
| `resolve` **(default)** | Promoted into a synthetic custom-recognizer match; the resolver runs again and the suspect becomes a normal, restorable token. What resolve cannot handle goes to the fallback. | Yes, for every resolved suspect | Default |
| `redact` | The suspect span is replaced with a one-way `[REDACTED:<class>]` marker, and an audit row is written. There is no fallback. | No, for that span | Opt-in, when you want to skip the resolve pass |
| `strict` | Nothing is changed; the report is returned. The CLI refuses the document: exit code `3`, empty stdout. | Nothing was sent | Opt-in, when any suspect must stop the run |
| `tolerant` | Nothing is changed; the CLI prints a warning and ships the document. **The suspect reaches the model.** | Yes | Never. Development only |

Two kinds of finding are never acted on in any mode:

- A finding inside a placeholder Gaze issued is dropped before any action.
- A name, location, or organization suspect that starts or ends inside a word
  is left in place with a `Preserve` audit row; see
  [sub-word suspects](safety-nets.md#sub-word-suspects-are-never-acted-on).

## The fallback applies only under resolve

`SafetyNetPolicy::decision()` in `crates/gaze/src/pipeline.rs` turns the
`(mode, fallback)` pair into one runtime decision:

| `--safety-net-mode` | `--safety-net-fallback` | `SafetyNetDecision` | Runtime behaviour |
|---|---|---|---|
| `strict` | any | `Observe { strict: true }` | report only; the CLI boundary exits `3` |
| `tolerant` | any | `Observe { strict: false }` | report only; the CLI boundary warns and ships |
| `redact` | any | `Redact` | replace every suspect span with a marker; **no fallback** |
| `resolve` | `f` | `Resolve { on_residual: f }` | tokenize, re-run the nets, apply `f` to the residual |

The test `safety_net_policy_lowering_covers_all_twelve_representable_pairs` in
`crates/gaze/tests/safety_net.rs` pins all twelve pairs.

Under `resolve`, a suspect goes to the fallback when:

- **`ValidatorVeto`**: the promoted span fails its validator, for example a
  malformed phone number.
- **`AnchorMissing`**: the promoted recognizer needs a
  [mandatory anchor](../detection/anchor-resolution.md) that the context does
  not contain.
- **`ResidualSuspect`**: after the one resolve pass, the re-run still reports a
  suspect.
- **`OverlapConflict`**: the suspect is a `ClassMismatch` on a token that is
  not a placeholder Gaze issued, so promoting it would re-tokenize a token.

These four are the closed `FallbackReason` enum. The CLI also prints a
`ClassMismatch` warning on stderr in every mode. The fallback acts on the report from the re-run, at
post-resolve positions, not on the first report, whose spans may already be
tokenized.

The three fallbacks:

| `--safety-net-fallback` | What happens to a residual suspect |
|---|---|
| `redact` **(default)** | Replaced with a one-way `[REDACTED:<class>]` marker. |
| `strict` | The document is rejected with `Error::SafetyNetFallback(reason)`; the CLI exits `3`. |
| `tolerant` | The residual bytes ship. Development only. |

The cascade is one hop: there is no `resolve` → `redact` → `strict` chain.

`redact` has no fallback because it cannot partly fail. A suspect that overlaps
a token is widened to swallow the whole token
(`expand_span_to_overlapping_manifest_entries`), overlapping spans are merged,
and a span that splits a character is rounded outward. A span outside the text,
or a manifest that contradicts its own alignment, is a typed error that rejects
the document.

## Choosing a mode

- **Agent loops and batch pseudonymization:** keep the default. The reversible
  path runs first, only what it cannot handle is replaced with a marker, and the
  agent never meets a hard failure.
- **Latency-sensitive loops:** `redact` skips the extra resolve pass. The cost
  is that every suspect span is one-way.
- **Hard stop for a human or CI to investigate:** `strict`, or `resolve` with
  `--safety-net-fallback strict` to resolve first and stop only on what is
  left.
- **Measuring a net's false positives on a known-clean corpus:** `tolerant`,
  never in production.

## Why resolve is the default

Under `resolve` with the `redact` fallback, no suspect reaches the model: each
one either becomes a manifest token or is replaced with a marker before the
clean text leaves Gaze. Compared with `strict`, agent loops no longer stall on
exit `3`. Compared with `redact` alone, every suspect that resolve can handle
stays restorable. The price is one more pipeline pass when a suspect is found.

## Why tolerant is not a production mode

`tolerant` ships bytes that the safety net flagged. Any byte of PII that
reaches a model outside the manifest contract is a critical defect, so the mode
exists only for development work where you own both the input and the output
and send neither to a model.

The CLI guards it. `--safety-net-mode tolerant` and `--safety-net-fallback
tolerant` both require the environment variable `GAZE_ALLOW_TOLERANT=1`;
without it the CLI fails with the `TolerantModeDisabled` safety-net variant.
When a tolerant path is actually reachable and the run has a suspect, the CLI
prints to stderr:

```text
warning: tolerant mode downgrades suspect leaks; deprecated v0.9, removal candidate v0.10.
```

## Audit rows

Each action writes one audit row per suspect, never the suspect's bytes. The
`decided_by` column names what decided:

| `decided_by` | Written when | `action` |
|---|---|---|
| `resolve` | a suspect is tokenized by the resolve pass | `Tokenize` |
| `redact` | `redact` mode replaces a suspect | `Redact` |
| `fallback` | the fallback decides a residual suspect | `Redact` under the `redact` fallback; `Preserve` under `strict` or `tolerant` |

Fallback rows are loser rows (`conflict_loser = true`) and carry the reason in
`fallback_triggered`. Under the `strict` fallback the row is written before the
document is rejected, so it survives for forensic replay even though stdout is
empty. Suspect metadata also goes to the
[`safety_net_log` table](safety-nets.md#safety_net_log-audit-table) when an audit
database is configured.

## Restore and the redaction marker

The marker is not a token. `gaze restore` returns it unchanged, and those bytes
cannot be recovered; this is the one place where Gaze gives up reversibility,
and it does so visibly. Every token Gaze emitted still restores. Marker format
and rules: [the redaction marker](safety-nets.md#the-redaction-marker).

## Structured documents

A `RawDocument::Structured` accepts only `strict` or `tolerant`. `resolve` and
`redact` return `Error::UnsupportedSafetyNetModeForStructured` before any field
is tokenized; see
[structured documents](safety-nets.md#the-structured-path-is-observer-only-and-says-so).

## Design history

The v0.8 proposal that introduced `redact`, `resolve`, and the fallback flag,
with its alternatives and open questions, is kept as a
[historical design record](safety-net-modes-design.md). It lists where the
shipped behaviour differs from the proposal.
