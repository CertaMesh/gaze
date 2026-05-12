# Locale Chain

Locale chain precedence - CLI > policy > rulepack default > system default.

Gaze resolves recognizer locale eligibility from left to right. The CLI
`--locale` value is the highest-precedence operator override. If it is absent,
the policy locale chain applies. If policy has no active locale, the rulepack
`default_locales` apply. If no earlier layer supplies a locale, Gaze uses the
system default `global`.

`global` is universal and intersects every recognizer locale. Other locale tags
are strict: `LocaleTag::Other(_)` matches only the same opaque tag.

## v0.7.x anchor and collision-family interaction

Locale packs can also provide mandatory-anchor cue buckets under
`[locale.cues.<key>]`. Collision-family recognizers declare
`mandatory_anchor = "<key>"`; during resolution, the active locale chain selects
which cue bundles are available for that key.

If no active locale supplies the required anchor, Gaze fails closed to a
family-level token with `ConflictTier::AnchoredContext` and
`AmbiguityReason::NoAnchor`. The
`locale-cue-bundle-coherence` xtask gate keeps mandatory-anchor declarations in
the bundled core rulepacks aligned with the embedded `locale-de` and
`locale-en` cue bundles.
