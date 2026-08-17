# Locale Chain

Locale chain precedence - CLI > policy > rulepack default > system default.

Gaze resolves the document locale from left to right. The CLI `--locale` value
is the highest-precedence operator override. If it is absent, the policy locale
chain applies. If policy has no active locale, the rulepack `default_locales`
apply. If no earlier layer supplies a locale, Gaze uses the system default
`global`.

Recognizer eligibility then depends on `locale_basis`:

- `document` (the default when an external rulepack omits the field) treats
  `locales` as an eligibility gate. A recognizer tagged `global` is eligible for
  every document locale. Other locale tags are strict:
  `LocaleTag::Other(_)` matches only the same opaque tag.
- `format` treats `locales` as format provenance, not a document-language gate.
  The recognizer runs once regardless of the document locale and its candidates
  join the document-basis candidates before the normal conflict resolver runs.
  `enabled` and `safety_tier` still apply.

Bundled rulepacks declare the basis explicitly for every recognizer. External
and adopter rulepacks retain the legacy `document` behavior unless they opt in
to `locale_basis = "format"`.

This is a deliberate breaking behavior change for the bundled format-basis
identifiers. `--locale=global` and narrow locale chains no longer suppress
them. To restore the old output, disable the recognizer itself (for example,
select an adopter rulepack copy with `enabled = false`); changing the locale is
no longer a suppression mechanism.

The mixed-basis implementation does not resolve todo #2411. A synthetic
`[LocaleTag::Global]` chain is now correct for format-basis recognizers only; it
still suppresses document-basis `name.*`, `phone.national.de`, both postal
recognizers, and legacy/custom rulepacks. Direct/codec primary and residual
passes therefore still need the shared `ProxyConfig::locale_chain`.

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

This matrix lists bundled recognizers shipped in `gaze-recognizers`. For
document-basis rows, supported locales are eligibility projections. For
format-basis rows, they record the identifier's format provenance.

| Bundle | Recognizer ID | Class | Locale basis | Supported locales / provenance | ValidatorKind |
|---|---|---|---|---|---|
| `core` | `email.global` | `Email` | document | `global` | `EmailRfc` |
| `core` | `email.header.name` | `Name` | document | `global` | None |
| `core` | `email.header.name.paren` | `Name` | document | `global` | None |
| `core` | `name.forward_marker` | `Name` | document | `de-DE`, `de-AT`, `de-CH`, `en-US`, `en-GB`, `en-IE`, `en-AU`, `en-CA` | None |
| `core` | `name.agent_recipient` | `Name` | document | `de-DE`, `de-AT`, `de-CH`, `en-US`, `en-GB`, `en-IE`, `en-AU`, `en-CA` | None |
| `core` | `name.auto_footer` | `Name` | document | `de-DE`, `de-AT`, `de-CH`, `en-US`, `en-GB`, `en-IE`, `en-AU`, `en-CA` | None |
| `core-extended` | `phone.structural` | `custom:phone` | document | `global` | `E164Phone` |
| `core-extended` | `phone.e164.spaced` | `custom:phone` | document | `global` | `E164Phone` |
| `core-extended` | `phone.national.de` | `custom:phone` | document | `de-DE`, `de-AT`, `de-CH` | `E164PhoneNational(De)` |
| `core-extended` | `phone.national.us` | `custom:phone` | format | `en-US` | `E164PhoneNational(Us)` |
| `core-extended` | `iban.structural` | `custom:iban` | document | `global` | `IbanMod97` |
| `core-extended` | `card.structural` | `custom:credit_card` | document | `global` | `Luhn` |
| `core-extended` | `ip.v4` | `custom:ip_address` | document | `global` | `Ipv4Parse` |
| `core-extended` | `ip.v6` | `custom:ip_address` | document | `global` | `Ipv6Parse` |
| `core-extended` | `eth.address` | `custom:eth_address` | document | `global` | `EthEip55` |
| `core` | `aadhaar.in` | `custom:aadhaar` | format | `en-IN`, `hi-IN` | `AadhaarVerhoeff` |
| `core` | `nir.fr` | `custom:nir` | format | `fr-FR` | `FrNirMod97` |
| `core` | `steuer_id.de` | `custom:steuer_id` | format | `de-DE`, `de-AT`, `de-CH` | `DeSteuerIdMod1110` |
| `core` | `vat.de` | `custom:vat_id` | format | `de-DE`, `de-AT`, `de-CH` | None |
| `core` | `vat.es` | `custom:vat_id` | format | `es-ES` | None |
| `core` | `bsn.nl` | `custom:bsn` | format | `nl-NL` | `BsnMod11` |
| `core` | `cpf.br` | `custom:cpf` | format | `pt-BR` | `CpfMod11` |
| `core` | `cnpj.br` | `custom:cnpj` | format | `pt-BR` | `CnpjMod11` |
| `core` | `nhs.uk` | `custom:nhs_number` | format | `en-GB` | `UkNhsMod11` |
| `core` | `ssn.us` | `custom:ssn` | format | `en-US` | None |
| `core` | `nino.uk` | `custom:nino` | format | `en-GB` | None |
| `core` | `pan.in` | `custom:pan` | format | `en-IN`, `hi-IN` | None |
| `core-extended` | `postal.de` | `custom:postal_code` | document | `de-DE` | None |
| `core-extended` | `postal.us` | `custom:postal_code` | document | `en-US` | None |
| `core` | `url.anchored` | `custom:url` | document | `global` | None |
| NER artifact | `ner` | `Name` | document | Policy-selected NER locale, or any locale when unset | None |

The NER recognizer keeps semantic `recognizer_id = "ner"` for registry
compatibility. Audit rows additionally carry a versioned
`recognizer_version_id` in the form `ner.<model>.<vN>` when the loaded artifact
declares model metadata, or `ner.unknown.v0` when it does not.
