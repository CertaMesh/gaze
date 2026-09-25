# Mandatory anchor resolution

Mandatory anchors are a fail-closed guard for structural recognizers whose shape
alone is not enough to safely emit a precise variant token.

## Declaring a mandatory anchor

A recognizer declares
the requirement in its collision metadata:

```toml
[recognizers.collision]
family = "payment-card-or-iban"
variant = "iban"
precedence = 10
mandatory_anchor = "iban"
```

Locale rulepacks provide cue bundles under `[locale.cues.<key>]`:

```toml
[locale.cues.iban]
names = ["IBAN", "IBAN:", "Account No."]
window_chars = 64
```

## How resolution runs

At runtime, Gaze runs validator veto first, then anchor resolution, then normal
conflict resolution. `AnchorResolver` looks up the candidate's recognizer id in
`FamilyPolicyTable`; when `mandatory_anchor` is present, it scans the active
locale chain for a matching cue bundle and searches a bounded window around the
candidate span. It treats a missing cue bundle as a missing anchor, not as
permission to emit the narrower variant.

When a mandatory anchor is found, the candidate flows normally and can emit its
variant class, for example `custom:iban`. When the anchor is missing, Gaze emits
one family-level token with class `PiiClass::Custom("family:<family>")`, marks
the decision as `ConflictTier::AnchoredContext`, and attaches an
`AmbiguityRecord` with `AmbiguityReason::NoAnchor`.

## Settled spans skip the anchor check

The fallback does not apply to a span whose family collision policy already
settled: when a variant with lower precedence and a mandatory anchor (the
IBAN) beats another variant of its family (the card) on the same bytes, the
policy verdict stands even without a cue. Settlement is tracked separately
from `decided_by`, so a later overlap with an unrelated recognizer, which can
relabel `decided_by` to the rung that decided that pair, does not send the
settled span back through the anchor check. A span that never met a family
rival is anchor-checked as described above.

## One token, one restore mapping

This is HYBRID output, not multiple redactions. The cleaned text receives one
token and the manifest keeps one restore mapping. Audit receives the redaction
entry plus the ambiguity sidecar so adopters can tune cues without weakening
restore semantics.

## Policy action for the family-level token

The family-level token's policy action is resolved by the same first-match walk
as every other class, with one difference: when no reachable rule names the
family class, the token takes the strictest action among its member classes'
resolved actions and its own default (`gaze::rule::resolve`, order in
`gaze_types::Action::strictness_rank`). A policy that names only `custom:iban`
therefore protects the fallback token; an explicit family rule before the
default still overrides. The audit row records the derivation in
`AmbiguityRecord::derived_action`. See
[How a family-level token picks its action](../../reference/policy.md#how-a-family-level-token-picks-its-action).

## Coherence gate

The bundled coherence gate:

```bash
cargo run -p xtask -- locale-cue-bundle-coherence
```

fails if a bundled recognizer declares `mandatory_anchor` without at least one
bundled locale cue block for that key.
