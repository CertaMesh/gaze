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

## Coverage matrix

This matrix lists bundled recognizers shipped in `gaze-recognizers` and the
locale projections that can activate them. `global` recognizers are eligible for
every locale chain because `global` intersects all locale projections.

| Bundle | Recognizer ID | Class | Supported locales | ValidatorKind |
|---|---|---|---|---|
| `core` | `email.global` | `Email` | `global` | `EmailRfc` |
| `core` | `email.header.name` | `Name` | `global` | None |
| `core` | `email.header.name.paren` | `Name` | `global` | None |
| `core` | `name.forward_marker` | `Name` | `de-DE`, `de-AT`, `de-CH`, `en-US`, `en-GB`, `en-IE`, `en-AU`, `en-CA` | None |
| `core` | `name.agent_recipient` | `Name` | `de-DE`, `de-AT`, `de-CH`, `en-US`, `en-GB`, `en-IE`, `en-AU`, `en-CA` | None |
| `core` | `name.auto_footer` | `Name` | `de-DE`, `de-AT`, `de-CH`, `en-US`, `en-GB`, `en-IE`, `en-AU`, `en-CA` | None |
| `core-extended` | `phone.structural` | `custom:phone` | `global` | `E164Phone` |
| `core-extended` | `phone.national.de` | `custom:phone` | `de-DE`, `de-AT`, `de-CH` | `E164PhoneNational(De)` |
| `core-extended` | `phone.national.us` | `custom:phone` | `en-US` | `E164PhoneNational(Us)` |
| `core-extended` | `iban.structural` | `custom:iban` | `global` | `IbanMod97` |
| `core-extended` | `card.structural` | `custom:credit_card` | `global` | `Luhn` |
| `core-extended` | `ip.v4` | `custom:ip_address` | `global` | `Ipv4Parse` |
| `core-extended` | `ip.v6` | `custom:ip_address` | `global` | `Ipv6Parse` |
| `core-extended` | `eth.address` | `custom:eth_address` | `global` | `EthEip55` |
| `core-extended` | `postal.de` | `custom:postal_code` | `de-DE` | None |
| `core-extended` | `postal.us` | `custom:postal_code` | `en-US` | None |
| NER artifact | `ner` | `Name` | Policy-selected NER locale, or any locale when unset | None |

The NER recognizer keeps semantic `recognizer_id = "ner"` for registry
compatibility. Audit rows additionally carry a versioned
`recognizer_version_id` in the form `ner.<model>.<vN>` when the loaded artifact
declares model metadata, or `ner.unknown.v0` when it does not.
