# Safety-net modes

A safety net reports suspect PII in clean output; only the core acts on it.
The default is `resolve` with `redact` fallback.
`gaze clean` and `gaze daemon` use `--safety-net-mode` /
`--safety-net-fallback`. Rust callers pass `SafetyNetPolicy` to
`Pipeline::clean_with_safety_net_policy_detect_context`; its default is the
same pair. See the [observer contract](safety-nets.md#observer-only-contract).

## The four modes

| Mode | What happens to a suspect | Reversible? | Production use |
|---|---|---|---|
| `resolve` (default) | Tokenize directly in family `safety_net`, scan again, then fallback on residuals. | Yes, for every resolved suspect | Default |
| `redact` | Write `[REDACTED:<class>]` and audit; no fallback. | No, for that span | Opt-in, when you want to skip the resolve pass |
| `strict` | Return report unchanged; CLI exits `3` with empty stdout for `Uncovered` / `PartialBleed`. `ClassMismatch`-only warns and ships. | Tokens retain their mapping; no output on refusal | Opt-in, when an uncovered suspect must stop the run |
| `tolerant` | Warn and ship unchanged suspect bytes. | Yes | Never. Development only |

Findings inside issued placeholders are dropped before any action. Under
`Resolve` and `Redact`, name/location/organization sub-word findings remain raw
with `Preserve` audit rows; strict fallback instead refuses them as residuals.
See [sub-word suspects](safety-nets.md#sub-word-suspects-are-never-acted-on).

## The fallback applies only under resolve

`SafetyNetPolicy::decision()` in `crates/gaze/src/pipeline.rs` turns the
`(mode, fallback)` pair into one runtime decision:

| `--safety-net-mode` | `--safety-net-fallback` | `SafetyNetDecision` | Runtime behaviour |
|---|---|---|---|
| `strict` | any | `Observe { strict: true }` | report only; the CLI boundary exits `3` |
| `tolerant` | any | `Observe { strict: false }` | report only; the CLI boundary warns and ships |
| `redact` | any | `Redact` | replace every suspect span with a marker; no fallback |
| `resolve` | `f` | `Resolve { on_residual: f }` | tokenize, re-run the nets, apply `f` to the residual |

The test `safety_net_policy_lowering_covers_all_twelve_representable_pairs` in
`crates/gaze/tests/safety_net.rs` pins all twelve pairs.

Under `resolve`, a suspect goes to the fallback for one of two reasons:

- `OverlapConflict`: the suspect is a `ClassMismatch` that does not lie
  wholly inside one live token or redaction marker, so tokenizing it would
  re-tokenize a token; or its span does not match the manifest and no complete
  multi-gap plan covers it; or two planned spans overlap.
- `ResidualSuspect`: after the one resolve pass, the re-run still reports a
  suspect; or the suspect span splits a UTF-8 character.

`ValidatorVeto` and `AnchorMissing` exist in the closed `FallbackReason` enum
and are accepted in audit rows, but no code path emits them today: resolve
never runs a validator or an anchor check on a suspect. The CLI also prints a
`ClassMismatch` warning on stderr in every mode. The fallback acts on the report from the re-run, at
post-resolve positions, not on the first report, whose spans may already be
tokenized.

The three fallbacks:

| `--safety-net-fallback` | What happens to a residual suspect |
|---|---|
| `redact` (default) | Tokenized when every residual the post-resolve re-run found can be tokenized reversibly; otherwise all of them are replaced with a one-way `[REDACTED:<class>]` marker. A first-pass refusal is always replaced with the marker. |
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

| Need | Choice |
|---|---|
| Try reversible handling first | Default `resolve` + `redact` |
| Skip the extra resolve scan | `redact`, accepting one-way loss |
| Stop on uncovered suspects | `strict`, or `resolve` + `strict` fallback |
| Measure false positives on known-clean input | Development-only `tolerant` |

## Why resolve is the default

Resolve preserves more originals than direct redaction and avoids strict-mode
stalls for handled suspects. It costs an extra scan when suspects appear.
Sub-word and terminal-admission exceptions may remain raw with an honest
report; [safety nets](safety-nets.md#sub-word-suspects-are-never-acted-on)
defines them. Detection gaps can still leak in every mode.

## Why tolerant is not a production mode

Tolerant sends flagged bytes unchanged. Both tolerant flags require
`GAZE_ALLOW_TOLERANT=1`; otherwise the CLI returns `TolerantModeDisabled`.
When reachable with a suspect, it warns on stderr:

```text
warning: tolerant mode downgrades suspect leaks; deprecated v0.9, removal candidate v0.10.
```

Use only where you own input/output and send neither to a model.

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
cannot be recovered. The marker records the removed class; surviving tokens still restore. Marker format
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
