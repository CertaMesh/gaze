# Locale Chain

Locale chain precedence - CLI > policy > rulepack default > system default.

Gaze resolves recognizer locale eligibility from left to right. The CLI
`--locale` value is the highest-precedence operator override. If it is absent,
the policy locale chain applies. If policy has no active locale, the rulepack
`default_locales` apply. If no earlier layer supplies a locale, Gaze uses the
system default `global`.

`global` is universal and intersects every recognizer locale. Other locale tags
are strict: `LocaleTag::Other(_)` matches only the same opaque tag.
