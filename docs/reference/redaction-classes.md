# Redaction classes and recognizers

This is the canonical inventory of what Gaze can detect through the embedded
`core` and `core-extended` names and the opt-in `secrets` bundle. It covers the emitted classes, every bundled
recognizer, validator and normalizer support, collision precedence, conflict
resolution, deterministic gaps, and no-policy activation.

The inventory is source-backed. A normal workspace test loads both embedded
rulepacks through `Rulepack::load`, instantiates the real validator and
normalizer enums, structurally reads the Rust enum definitions, and compares the
marked tables below as sets. Run it directly with:

```bash
rustup run 1.96.0 cargo test -p xtask --test redaction_classes_doc
```

`core-extended` does not contain a second rulepack. It is a deprecated
compatibility name for the same embedded `core.toml` bytes
(`crates/gaze-recognizers/src/lib.rs:45-55`,
`crates/gaze-cli/src/pipeline/run.rs:718-733`). Its difference is activation
policy, described under [Shipped default activation](#shipped-default-activation).
The shared payload currently contains exactly 39 recognizer specs
(`crates/gaze-recognizers/src/lib.rs`, `embedded()`).

The opt-in `secrets` bundle (`crates/gaze-recognizers/embedded/secrets.toml`)
carries the two credential recognizers, `security_token.anchored` and
`password.field`. Credentials are not PII, so `secrets` is never part of a
default activation: its rows below are inert until a caller loads it by name
with `[policy.rulepacks] bundled = ["core", "secrets"]` or
`--rulepack-bundled core,secrets`. The former `username.field` recognizer was
removed in core 0.6.0; no rulepack emits `custom:username`. The opt-in Nym
safety net emits `custom:username`, `custom:license_plate`,
`custom:building_number`, `custom:tax_id`, `custom:postal_code` and
`custom:date` suspects when enabled
([mapping](../explanation/safety-net/safety-nets.md#which-labels-can-fire));
those are safety-net classes, not recognizer rows, so they are not in the tables
below.

## PII classes and resolver priority

`PiiClass` is the closed class vocabulary at
`crates/gaze-types/src/lib.rs:62-96`. During generic overlap resolution, a
higher class-priority integer wins a partial overlap
(`compare_base_ladder` and `class_priority` in `crates/gaze/src/resolver.rs`).
Two rungs decide containment before the generic tiers: **containment
precedence** hands a span that wholly contains a differently-classed span the
whole span as one token when its evidence tier is at least the contained
span's (`containment_precedence` in `crates/gaze/src/resolver.rs`,
`ConflictTier::ContainmentPrecedence`), and where that guard refuses, a
custom-class structured span that strictly encloses a builtin-class span still
keeps the slot (`structured_containment`, `ConflictTier::StructuredContainment`),
so an NER token inside a URL, IBAN or credential cannot split the identifier.

<!-- redaction-classes-gate:pii-classes:start -->
| Rust variant | Policy spelling | Class priority | Source |
|---|---|---:|---|
| `Email` | `email` | 90 | `crates/gaze-types/src/lib.rs:82-92`; `crates/gaze/src/resolver.rs` (`class_priority`) |
| `Name` | `name` | 80 | `crates/gaze-types/src/lib.rs:82-92`; `crates/gaze/src/resolver.rs` (`class_priority`) |
| `Organization` | `organization` | 70 | `crates/gaze-types/src/lib.rs:82-92`; `crates/gaze/src/resolver.rs` (`class_priority`) |
| `Location` | `location` | 60 | `crates/gaze-types/src/lib.rs:82-92`; `crates/gaze/src/resolver.rs` (`class_priority`) |
| `Custom` | `custom:<name>` | 50 | `crates/gaze-types/src/lib.rs:82-92,233-246`; `crates/gaze/src/resolver.rs` (`class_priority`) |
<!-- redaction-classes-gate:pii-classes:end -->

`Custom(String)` is parametric: the table's `custom:<name>` denotes every
tenant, identifier, and ambiguity-family class, not one literal class.

## Embedded recognizers

The table describes the loaded `RecognizerSpec` values, not a transcription of
the TOML parser's defaults. `safe_default = yes` means exactly
`SafetyTier::SafeDefault`; locale intersection still controls activation.
`locale_gated` recognizers require explicit compatible locale activation or the
deprecated `core-extended` compatibility behavior. The tier contract is at
`crates/gaze-types/src/lib.rs:2407-2447`.

Validator and normalizer `none` means the recognizer intentionally has no such
stage. A validator can veto a shape match before conflict resolution; a
normalizer changes the canonical value only and never the original restore span.
See [Validator Veto](../explanation/detection/validator-veto.md) and
[Recognizer normalizers preserve the original span](../explanation/detection/recognizer-normalizer-spans.md).

Definitions live in `crates/gaze-recognizers/embedded/core.toml` (loaded under
both the `core` and `core-extended` names) and `secrets.toml`; search for the
`id = "..."` line. The table carried a per-recognizer `<file>:<lo>-<hi>`
citation until v0.15, but no gate verified it and 35 of 37 ranges had drifted,
some by more than 150 lines, so it was removed rather than re-verified. Every
remaining column is checked against the loaded rulepack by
`crates/xtask/tests/redaction_classes_doc.rs`.

<!-- redaction-classes-gate:recognizers:start -->
| Embedded names | Recognizer id | Matcher | What it matches | Class | Locales | Validator | Normalizer | Safety tier | safe_default | Base | Priority |
|---|---|---|---|---|---|---|---|---|---|---:|---:|
| `core, core-extended` | `email.global` | `regex` | Structurally valid email addresses, including reserved synthetic example domains | `Email` | `global` | `email_rfc` | `email_canonical` | `safe_default` | yes | 0.70 | 90 |
| `core, core-extended` | `email.header.name` | `regex` | Quoted or capitalized display names before an angle-bracket address in email headers | `Name` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 100 |
| `core, core-extended` | `email.header.name.paren` | `regex` | Parenthesized display names following an email address in headers or address lists | `Name` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 100 |
| `core, core-extended` | `name.forward_marker` | `anchored_match` | Person-name-shaped text after locale-provided forwarded-message cues | `Name` | `de-DE, de-AT, de-CH, en-US, en-GB, en-IE, en-AU, en-CA` | `none` | `none` | `safe_default` | yes | 0.88 | 110 |
| `core, core-extended` | `name.agent_recipient` | `anchored_match` | Person-name-shaped text after locale-provided agent-recipient cues | `Name` | `de-DE, de-AT, de-CH, en-US, en-GB, en-IE, en-AU, en-CA` | `none` | `none` | `safe_default` | yes | 0.88 | 110 |
| `core, core-extended` | `name.auto_footer` | `anchored_match` | Person-name-shaped text after locale-provided footer or sign-off cues | `Name` | `de-DE, de-AT, de-CH, en-US, en-GB, en-IE, en-AU, en-CA` | `none` | `none` | `safe_default` | yes | 0.88 | 110 |
| `core, core-extended` | `phone.structural` | `regex` | Compact international phone candidates beginning with plus and 6 to 15 digits | `custom:phone` | `global` | `e164_phone` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `phone.e164.spaced` | `regex` | Spaced or punctuated international phone candidates outside the US and German branches | `custom:phone` | `global` | `e164_phone` | `none` | `safe_default` | yes | 0.70 | 79 |
| `core, core-extended` | `phone.national.de` | `regex` | German national or plus-49 phone shapes accepted by the German regional parser | `custom:phone` | `de-DE, de-AT, de-CH` | `e164_phone_national_de` | `none` | `locale_gated` | no | 0.82 | 85 |
| `core, core-extended` | `phone.national.us` | `regex` | US NANPA phone shapes, including the documented synthetic 555-01xx range | `custom:phone` | `en-US` | `e164_phone_national_us` | `none` | `safe_default` | yes | 0.82 | 85 |
| `core, core-extended` | `iban.structural` | `regex` | Space-tolerant IBAN shapes at the country's ISO 13616 registry length that pass MOD-97 after canonicalization; no trailing word boundary in the pattern, the code boundary accepts a glued label and refuses a glued digit or underscore | `custom:iban` | `global` | `iban_mod97` | `iban_canonical` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `card.structural` | `regex` | 13 to 19 digit payment-card shapes with optional spaces or dashes that pass Luhn | `custom:credit_card` | `global` | `luhn` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `ip.v4` | `regex` | Decimal dotted-quad IPv4 addresses with octets from 0 through 255 | `custom:ip_address` | `global` | `ipv4_parse` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `ip.v6` | `regex` | Full, compressed (including bare `::`), and IPv4-embedded IPv6 textual forms at every locale. The word guard excludes an address adjacent to an identifier character, so Rust and C++ `::` paths survive; an explicit `Address:`, `Adresse:`, `IP:`, `IPv6:`, `host:` or `addr:` cue admits a glued address after full IPv6 parsing (case-insensitive cues; `global` document-basis activation; todos #3710 and #3762, superseding #2402). `_2001:db8::1` remains outside the cue rule. A standalone path whose segments are all short hex words and which has no surrounding context (`a::b`, `abc::def`) is still an address | `custom:ip_address` | `global` | `ipv6_parse` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `eth.address` | `regex` | Forty-hex-digit Ethereum addresses prefixed by 0x and accepted by EIP-55 rules | `custom:eth_address` | `global` | `eth_eip55` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `aadhaar.in` | `regex` | Cue-anchored Indian Aadhaar or UID values containing 12 digits and passing Verhoeff | `custom:aadhaar` | `en-IN, hi-IN` | `aadhaar_verhoeff` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `nir.fr` | `regex` | Cue-anchored French NIR social-security values with 15 digits and a valid MOD-97 key | `custom:nir` | `fr-FR` | `fr_nir_mod97` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `steuer_id.de` | `regex` | Cue-anchored German 11-digit Steuer-ID values passing MOD 11,10 | `custom:steuer_id` | `de-DE, de-AT, de-CH` | `de_steuer_id_mod1110` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `vat.de` | `regex` | Cue-anchored German VAT identifiers shaped as DE followed by nine digits | `custom:vat_id` | `de-DE, de-AT, de-CH` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `vat.es` | `regex` | Cue-anchored Spanish VAT, CIF, or NIF identifiers in the ES alphanumeric shape | `custom:vat_id` | `es-ES` | `none` | `none` | `safe_default` | yes | 0.84 | 84 |
| `core, core-extended` | `bsn.nl` | `regex` | Cue-anchored Dutch nine-digit BSN values passing the 11-test | `custom:bsn` | `nl-NL` | `bsn_mod11` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `cpf.br` | `regex` | Cue-anchored Brazilian CPF values passing both MOD-11 check digits | `custom:cpf` | `pt-BR` | `cpf_mod11` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `cnpj.br` | `regex` | Cue-anchored Brazilian CNPJ values passing both MOD-11 check digits | `custom:cnpj` | `pt-BR` | `cnpj_mod11` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `nhs.uk` | `regex` | Cue-anchored UK NHS numbers containing 10 digits and passing MOD-11 | `custom:nhs_number` | `en-GB` | `uk_nhs_mod11` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `ssn.us` | `regex` | Cue-anchored US Social Security numbers in dashed or nine-digit form | `custom:ssn` | `en-US` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `nino.uk` | `regex` | Cue-anchored UK National Insurance numbers with allocation-constrained prefixes | `custom:nino` | `en-GB` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `pan.in` | `regex` | Cue-anchored Indian Permanent Account Numbers in the ten-character PAN shape | `custom:pan` | `en-IN, hi-IN` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `postal.de` | `regex` | Bare five-digit German postal-code shapes | `custom:postal_code` | `de-DE` | `none` | `none` | `locale_gated` | no | 0.70 | 70 |
| `core, core-extended` | `postal.us` | `regex` | US five-digit ZIP or ZIP+4 shapes | `custom:postal_code` | `en-US` | `none` | `none` | `locale_gated` | no | 0.70 | 70 |
| `core, core-extended` | `postal.at_ch` | `regex` | Austrian and Swiss four-digit postal codes, optionally `A-`/`CH-`/`FL-` prefixed, only directly after a postal cue (`PLZ`, `Postleitzahl`, `Postcode`, `ZIP`) or directly before a city-shaped token (uppercase start, or `St.` / `St` before a capitalised name) across a space, NO-BREAK SPACE, or NARROW NO-BREAK SPACE; a city-anchored code is not matched when preceded by `#` | `custom:postal_code` | `de-AT, de-CH` | `none` | `none` | `locale_gated` | no | 0.70 | 70 |
| `core, core-extended` | `postal.ca` | `regex` | Canadian `A9A 9A9` postal codes, hyphenated, compact, or separated by a space, NO-BREAK SPACE, or NARROW NO-BREAK SPACE; not matched when preceded by `#` | `custom:postal_code` | `en-CA` | `none` | `none` | `safe_default` | yes | 0.80 | 72 |
| `core, core-extended` | `postal.gb` | `regex` | UK postcodes across all six Royal Mail outward forms plus `GIR 0AA`, followed by a `9AA` inward code over the official inward alphabet; space, NO-BREAK SPACE, or NARROW NO-BREAK SPACE separator; not matched when preceded by `#` | `custom:postal_code` | `en-GB` | `none` | `none` | `safe_default` | yes | 0.80 | 72 |
| `core, core-extended` | `postal.ie` | `regex` | Irish Eircodes: routing key including `D6W`, plus a four-character identifier over the restricted Eircode alphabet that must carry at least one letter | `custom:postal_code` | `en-IE` | `none` | `none` | `safe_default` | yes | 0.80 | 72 |
| `core, core-extended` | `url.anchored` | `regex` | URLs beginning with `http://`, `https://`, or `www.` through the final non-punctuation URL character | `custom:url` | `global` | `none` | `none` | `safe_default` | yes | 0.75 | 85 |
| `core, core-extended` | `ssn.de_cue` | `regex` | Cue-anchored SSN values after German social-insurance cues (Sozialversicherungsnummer, SV-Nummer) in dashed, dotted, or 9 to 11 digit form; format basis; DACH provenance describes cue vocabulary until native SVNR/AHV shapes ship in #2926 | `custom:ssn` | `de-DE, de-AT, de-CH` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `tax_number.cue_anchored` | `regex` | Cue-anchored tax numbers with a three-digit lead and separated digit groups after German or English tax cues; bare digit runs and the checksummed 2-3-3-3 Steuer-ID shape are excluded | `custom:tax_number` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 84 |
| `core, core-extended` | `driver_license.cue_anchored` | `regex` | Letter-led alphanumeric licence numbers after German or English driving-licence cues | `custom:driver_license` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 83 |
| `core, core-extended` | `national_id.cue_anchored` | `regex` | Letter-led, digit-grouped, 9 to 13 digit, or Swiss AHV (`756.dddd.dddd.dd`) identifiers after German or English national-ID / identity-card / AHV cues | `custom:national_id` | `global` | `none` | `none` | `safe_default` | yes | 0.82 | 82 |
| `core, core-extended` | `passport.cue_anchored` | `regex` | Letter-led alphanumeric, Personalausweis-silhouette, or 9-digit passport numbers after passport / Reisepass cues | `custom:passport` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 84 |
| `core, core-extended` | `birth_date.cue` | `regex` | Date-shaped values after explicit EN/DE birth-date field labels or born-on cues; format recognition only, no calendar-validity claim | `custom:birth_date` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
| `secrets` | `security_token.anchored` | `regex` | Cue-anchored credential values plus structurally prefixed AWS access keys and three-segment JWTs | `custom:security_token` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 87 |
| `secrets` | `password.field` | `regex` | Values in explicit EN/DE password or passphrase records; 1 to 256 normalized grammar units, with matching quoted or plain scalar syntax; not a raw-byte ceiling | `custom:password` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
<!-- redaction-classes-gate:recognizers:end -->

## Closed validator and normalizer sets

### `ValidatorKind`

`ValidatorKind` is owned by `gaze-types`; the source currently contains 15 Rust
variants
(`crates/gaze-types/src/lib.rs:505-553`). `E164Phone` and the parameterized
`E164PhoneNational(Region)` variant are compiled only with `phone-parser`; the
current closed `Region` set is Germany and the United States
(`crates/gaze-types/src/lib.rs:555-564`). All other variants are always
available.

<!-- redaction-classes-gate:validators:start -->
| Rust variant | Rulepack name or names | Feature | Validation | Source |
|---|---|---|---|---|
| `EmailRfc` | `email_rfc` | `always` | Basic email local-part and dotted-domain shape | `crates/gaze-types/src/lib.rs:520-522,566-593` |
| `E164Phone` | `e164_phone` | `phone-parser` | Parser-backed international E.164 validity | `crates/gaze-types/src/lib.rs:523-525,566-593` |
| `E164PhoneNational` | `e164_phone_national_de, e164_phone_national_us` | `phone-parser` | Parser-backed national validity for `Region::De` or `Region::Us` | `crates/gaze-types/src/lib.rs:526-528,555-593` |
| `Luhn` | `luhn` | `always` | Luhn checksum | `crates/gaze-types/src/lib.rs:529-530,566-593` |
| `IbanMod97` | `iban_mod97` | `always` | IBAN MOD-97 checksum | `crates/gaze-types/src/lib.rs:531-532,566-593` |
| `Ipv4Parse` | `ipv4_parse` | `always` | Strict decimal dotted-quad IPv4 parse | `crates/gaze-types/src/lib.rs:533-534,566-593` |
| `Ipv6Parse` | `ipv6_parse` | `always` | IPv6 textual parse | `crates/gaze-types/src/lib.rs:535-536,566-593` |
| `EthEip55` | `eth_eip55` | `always` | Ethereum EIP-55 checksum rules | `crates/gaze-types/src/lib.rs:537-538,566-593` |
| `AadhaarVerhoeff` | `aadhaar_verhoeff` | `always` | Indian Aadhaar Verhoeff checksum | `crates/gaze-types/src/lib.rs:539-540,566-593` |
| `FrNirMod97` | `fr_nir_mod97` | `always` | French NIR MOD-97 key | `crates/gaze-types/src/lib.rs:541-542,566-593` |
| `DeSteuerIdMod1110` | `de_steuer_id_mod1110` | `always` | German Steuer-ID MOD 11,10 | `crates/gaze-types/src/lib.rs:543-544,566-593` |
| `BsnMod11` | `bsn_mod11` | `always` | Dutch BSN 11-test | `crates/gaze-types/src/lib.rs:545-546,566-593` |
| `CpfMod11` | `cpf_mod11` | `always` | Brazilian CPF check digits | `crates/gaze-types/src/lib.rs:547-548,566-593` |
| `CnpjMod11` | `cnpj_mod11` | `always` | Brazilian CNPJ check digits | `crates/gaze-types/src/lib.rs:549-550,566-593` |
| `UkNhsMod11` | `uk_nhs_mod11` | `always` | UK NHS number MOD-11 | `crates/gaze-types/src/lib.rs:551-552,566-593` |
<!-- redaction-classes-gate:validators:end -->

### `NormalizerKind`

`NormalizerKind` is owned by `gaze-recognizers`, not `gaze-types`
(`crates/gaze-recognizers/src/regex.rs:9-33`). Neither current variant is
feature-gated.

<!-- redaction-classes-gate:normalizers:start -->
| Rust variant | Rulepack name or names | Feature | Normalization | Source |
|---|---|---|---|---|
| `EmailCanonical` | `email_canonical` | `always` | ASCII lowercase | `crates/gaze-recognizers/src/regex.rs:9-30` |
| `IbanCanonical` | `iban_canonical` | `always` | Remove ASCII whitespace and uppercase | `crates/gaze-recognizers/src/regex.rs:9-30,245-250` |
<!-- redaction-classes-gate:normalizers:end -->

Unknown names fail closed, but the exact stage matters. Raw `Rulepack::load`
deserializes validator and normalizer names into `ValidatorSpec` and
`NormalizerSpec` (`crates/gaze/src/rulepack.rs:288-303,578-643`). During
recognizer wiring, `ValidatorKind::parse` and `NormalizerKind::parse` reject
unknown or feature-disabled names
(`crates/gaze-assembly/src/detector_wiring.rs:114-150`). The CLI maps those
typed recognizer errors to `RulepackError::UnsupportedValidator` and
`RulepackError::UnsupportedNormalizer`
(`crates/gaze-cli/src/pipeline/build.rs:209-213`). There is no silent fallback.

## Collision families and precedence

Collision-family policy runs after validator veto and before the generic class
priority chain. **A numerically lower precedence wins.** The implementation is:

```text
match a_precedence.cmp(&b_precedence) {
    Ordering::Less => Some(true),
    Ordering::Greater => Some(false),
    Ordering::Equal => None,
}
```

Source: `crates/gaze/src/registry.rs:88-119`. Therefore, when
`iban.structural` overlaps `card.structural`, IBAN precedence 10 defeats PAN
precedence 20 even though 10 is numerically smaller. The result is decided by
`ConflictTier::CollisionPolicy`, before either class reaches the generic
`Custom(_)` priority tie. The `government-id` family orders its numeric
variants by cue specificity the same way: an SSN cue (10) beats a tax cue (20),
which beats the vaguer national-ID cues (30).

<!-- redaction-classes-gate:collisions:start -->
| Family | Recognizer id | Variant | Precedence | Mandatory anchor | Source |
|---|---|---|---:|---|---|
| `payment-card-or-iban` | `iban.structural` | `iban` | 10 | `iban` | `crates/gaze-recognizers/embedded/core.toml:346-350` |
| `payment-card-or-iban` | `card.structural` | `pan` | 20 | `none` | `crates/gaze-recognizers/embedded/core.toml:373-376` |
| `phone-or-imei` | `phone.structural` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:182-185` |
| `phone-or-imei` | `phone.e164.spaced` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:212-215` |
| `phone-or-imei` | `phone.national.de` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:246-249` |
| `phone-or-imei` | `phone.national.us` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:275-278` |
| `government-id` | `ssn.de_cue` | `ssn` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:957-960` |
| `government-id` | `tax_number.cue_anchored` | `tax-number` | 20 | `none` | `crates/gaze-recognizers/embedded/core.toml:1024-1027` |
| `government-id` | `national_id.cue_anchored` | `national-id` | 30 | `none` | `crates/gaze-recognizers/embedded/core.toml:1105-1108` |
| `government-id` | `passport.cue_anchored` | `passport` | 15 | `none` | `crates/gaze-recognizers/embedded/core.toml` (`variant = "passport"`) |
<!-- redaction-classes-gate:collisions:end -->

For the full family-level ambiguity contract, including equal-precedence
fallback, see [Collision-Family Policy](../explanation/detection/collision-family.md).
For mandatory-anchor lookup and family-level fallback, see
[Mandatory Anchor Resolution](../explanation/detection/anchor-resolution.md).

## Full conflict-resolution order

The end-to-end order is:

1. Locale and minimum-score filtering collect candidates
   (`crates/gaze/src/registry.rs:342-380`).
2. Validator veto removes validator-backed failures before any overlap is
   resolved (`crates/gaze/src/registry.rs:382-390`). The detailed typed audit
   contract is [Validator Veto](../explanation/detection/validator-veto.md).
3. For an overlap, collision-family precedence is consulted first, then
   mandatory-anchor context, then containment precedence, then structured
   containment, then the generic tiers (`arbitrate` in
   `crates/gaze/src/resolver.rs`).
4. Containment precedence (one entity, one token): when a span wholly
   contains a span of a different class, the container wins the whole span
   and the contained candidate is recorded as a merged source, unless the
   container's evidence tier is below the contained candidate's. The tiers
   are read from what a candidate already carries: **validator passed**
   (mod-97, Luhn, RFC email, E.164; a canonical form is present) >
   **anchored or cue-structured match** (a `structural.*` source, or a
   mandatory anchor found in context) > **plain regex or dictionary term** >
   **learned NER** (the `ner` recognizer). Equal tiers go to the container:
   a validated German phone shape inside a validated IBAN is folded into one
   IBAN token, whatever its rule priority or score. Audit tier
   `ConflictTier::ContainmentPrecedence` on the winner; the swallowed
   candidates keep loser rows (`containment_precedence` in
   `crates/gaze/src/resolver.rs`). Geometry and tiers decide, never arrival
   order. Partial overlaps keep the rungs below; nested chains resolve
   outermost-first; same-class containment keeps step 7. Because the rung
   sits after collision-family policy and the anchor rung, a declared
   rivalry (card inside IBAN) keeps its family verdict and a cue-anchored
   identifier inside an adopter regex keeps its own token.
5. Structured containment: when the guard above refuses a custom-class span
   that strictly encloses a builtin-class (`Email`/`Name`/`Organization`/
   `Location`) span (a plain URL regex over an RFC-validated email), the
   enclosing span still wins and the enclosed candidate is recorded as a
   merged source (`structured_containment` in `crates/gaze/src/resolver.rs`,
   audit tier `ConflictTier::StructuredContainment`). Builtin containers over
   custom spans and builtin-inside-builtin pairs the guard refuses fall
   through to the generic tiers.
6. The generic tiers are **class priority > rule priority > score > span length
   > lexicographically smaller recognizer id**
   (`compare_base_ladder` in `crates/gaze/src/resolver.rs`).
7. Same-class containment has one extra check before those generic tiers: a
   candidate with a validator-produced canonical form defeats an otherwise
   equivalent unvalidated candidate (the same-class containment branch of
   `arbitrate` in `crates/gaze/src/resolver.rs`). This is
   `ConflictTier::Validator`, distinct from the pre-resolver
   `ValidatorVeto`.
8. Replacement removes every overlap with the winner, so multi-overlap inputs
   converge to a disjoint fixed point rather than leaving a candidate that
   overlapped an earlier loser (`insert_candidate` and `remove_overlaps` in
   `crates/gaze/src/resolver.rs`).
9. After pairwise resolution, a surviving candidate that requires but lacks a
   mandatory anchor is converted to its family-level fallback
   (`resolve_candidates_inner` and `apply_missing_anchor_fallback` in
   `crates/gaze/src/resolver.rs`).

## Deterministic floor and NER-only mass

The embedded table above is the derivation: no deterministic recognizer emits
`Location` or `Organization`, and there is no standalone deterministic
recognizer for arbitrary first names, surnames, streets, cities, states, or
company names. Deterministic `Name` coverage is deliberately limited to email
display names and locale-cue-anchored person-name shapes
(`crates/gaze-recognizers/embedded/core.toml:51-155`).

The no-OPF measurement supplied for todo #2419 found all six corresponding
benchmark labels **0-covered and 0-overlapped** at the deterministic rule floor:

- `STREET`
- `CITY`
- `SURNAME`
- `FIRSTNAME`
- `STATE`
- `COMPANYNAME`

The archived v0.8 class-taxonomy gap analysis independently classifies company
and street extraction as safety-net/NER gaps and explains why first-name and
surname labels are not checksum-validatable
([v0.8 class-taxonomy gap, lines 32-58](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.8-kiji-class-gap.md#L32-L58),
archived at the `v0.13.0` tag).

A broad deterministic rule for these free-text classes would be a
false-positive catastrophe. Their benchmark mass is an NER problem, not an
invitation to turn capitalization, dictionary membership, or common words into
unanchored redaction rules. In `PiiClass` terms, the entirely NER-only built-in
classes are `Location` and `Organization`; `Name` has narrow deterministic
email/cue coverage but its unanchored `FIRSTNAME` and `SURNAME` mass remains
NER-only.

## Shipped default activation

The following assumes shipped default features, including `phone-parser`, and
no policy or explicit locale. It describes which recognizers are registered and
eligible to match; whether a particular input produces a candidate still
depends on its shape, cues, and validator outcome.

No safety net runs by default. The opt-in OpenAI Privacy Filter
(`--safety-net openai-filter`) and Nym-small (`--safety-net nym`) nets run only
when selected, and `gaze setup` wires only the pinned Davlan mBERT NER model into
the policy it writes.

The plain `core` default locale chain is `global`
(`crates/gaze-recognizers/embedded/core.toml:1-1373`), so the document-basis
recognizers that activate are exactly the global `safe_default` ones. Every
`locale_basis = "format"` recognizer activates regardless of the chain
(`crates/gaze-assembly/src/detector_wiring.rs:271-301`); see
[Locale Chain](../explanation/policy/locale-chain.md) for the mixed-basis
model. For the CLI, `normalize_rulepack_bundles`
rewrites the deprecated `core-extended` selection to `core` while returning an
`auto_activate_locale_gated` bit
(`crates/gaze-cli/src/pipeline/run.rs:712-728`). `CleanOverrides::apply_to`
carries that bit into `Policy::rulepacks`
(`crates/gaze-cli/src/clean_overrides.rs:48-62`). Pipeline construction then
adds the auto-activation locales to the compatibility locale chain. That set is
derived from the loaded rulepacks by
`gaze_assembly::locale_gated_activation_locales`
(`crates/gaze-assembly/src/locale.rs`): the union of `locales` over enabled,
document-basis `safety_tier = "locale_gated"` recognizers, minus `global`,
ordered compatibility-first (`en-US`, `de-DE`, `de-AT`, `de-CH`) then by
canonical tag. For the bundled `core` recognizers that is exactly `en-US`,
`de-DE`, `de-AT`, `de-CH`; an adopter path rulepack with a locale-gated
recognizer for another locale extends the chain automatically. `gaze clean`
(`crates/gaze-cli/src/pipeline/run.rs`), `gaze daemon`
(`crates/gaze-cli/src/commands/daemon.rs`), and the library's
`CorePipelineConfig` (`crates/gaze-assembly/src/defaults.rs`) all call that one
function, and recognizer wiring admits locale-gated rows under that policy and
locale intersection (`crates/gaze-assembly/src/detector_wiring.rs`).

<!-- redaction-classes-gate:default-activation:start -->
| Bundle selection | Effective locale chain | Auto-activate locale-gated | Active recognizer ids | Source |
|---|---|---|---|---|
| `core` | `global` | no | `aadhaar.in, birth_date.cue, bsn.nl, card.structural, cnpj.br, cpf.br, driver_license.cue_anchored, email.global, email.header.name, email.header.name.paren, eth.address, iban.structural, ip.v4, ip.v6, national_id.cue_anchored, nhs.uk, nino.uk, nir.fr, pan.in, passport.cue_anchored, phone.e164.spaced, phone.national.us, phone.structural, postal.ca, postal.gb, postal.ie, ssn.de_cue, ssn.us, steuer_id.de, tax_number.cue_anchored, url.anchored, vat.de, vat.es` | `crates/gaze-recognizers/embedded/core.toml:1-1373`; `crates/gaze-assembly/src/defaults.rs:45-77` |
| `core-extended compatibility alias` | `global, en-US, de-DE, de-AT, de-CH` | yes | `aadhaar.in, birth_date.cue, bsn.nl, card.structural, cnpj.br, cpf.br, driver_license.cue_anchored, email.global, email.header.name, email.header.name.paren, eth.address, iban.structural, ip.v4, ip.v6, name.agent_recipient, name.auto_footer, name.forward_marker, national_id.cue_anchored, nhs.uk, nino.uk, nir.fr, pan.in, passport.cue_anchored, phone.e164.spaced, phone.national.de, phone.national.us, phone.structural, postal.at_ch, postal.ca, postal.de, postal.gb, postal.ie, postal.us, ssn.de_cue, ssn.us, steuer_id.de, tax_number.cue_anchored, url.anchored, vat.de, vat.es` | `crates/gaze-assembly/src/locale.rs` (`locale_gated_activation_locales`); `crates/gaze-assembly/src/defaults.rs:45-77`; `crates/gaze-cli/src/pipeline/run.rs:137-146,712-728` |
<!-- redaction-classes-gate:default-activation:end -->

The v0.6+ compatibility behavior therefore does activate
`phone.national.de`, `postal.us`, `postal.de`, and `postal.at_ch` with
`--rulepack-bundled core-extended` and no policy. The complete second row is
authoritative: the widened US/German compatibility locale chain also makes the
listed document-basis cue-anchored and locale-specific recognizers eligible.
Pass `--locale=global`, or use an explicit policy with narrower locale gating,
to avoid that document-basis compatibility expansion. Format-basis identifiers
(`ssn.us`, `ssn.de_cue`, `steuer_id.de`, `phone.national.us`, the alphanumeric
postal rules `postal.ca`, `postal.gb`, and `postal.ie`, and the other format
rows in the coverage matrix) are active in both rows; the locale chain is not a
suppression mechanism for them, so an adopter that must not tokenize one of them
has to disable that recognizer.

The two postal groups differ on purpose. `postal.de` and `postal.us` match bare
five-digit strings, a shape carrying no structural signal, so they stay
document-basis and locale-gated and appear only in the second row.
`postal.at_ch` is in the same group: a four-digit string carries even less
signal, so it matches only directly after a postal cue or directly before a
city-shaped token, and only for `de-AT` and `de-CH` documents. Document-basis
rules of one class resolve per span across the chain
([Locale Chain](../explanation/policy/locale-chain.md)), so under `de-AT, de-DE`
both `postal.at_ch` and `postal.de` tokenize their own codes in one document. In
the second row's chain `postal.at_ch` therefore runs on every document and keeps
each match that no `postal.us` or `postal.de` candidate overlaps.
Its measured false-positive class is a four-digit number followed by a
capitalised German noun (`1500 Euro`); every such token restores losslessly.
`postal.ca`, `postal.gb`, and `postal.ie` interleave letters and digits in
positions ordinary prose and identifiers do not produce, so they are
format-basis and run at every locale including `--locale=global`. An adopter
who must not tokenize Canadian, UK, or Irish postal codes cannot suppress them
with a locale chain and has to disable the recognizer.

`iban.structural` carries a leading word boundary but no trailing one. A compact IBAN
glued to the next label (`IBAN AT611904300234573201BIC`, the dense footer
`IBAN:<value>BIC:<value>`) is therefore a candidate, and the trailing boundary
is decided in code by `gaze_types::word_run_extends_identifier`, which reads
the word run after a validated registry-length candidate with the same word
predicate as `is_inside_word`: an empty run or a run of letters only (Unicode
`is_alphabetic`) is a glued label or word and the IBAN tokenizes whole; a run
holding a digit or an underscore could be more identifier and the candidate is
dropped, so `ref AT611904300234573201XQ7 end` yields no token over its
checksum-valid prefix. The disclosed gap is the other side of that rule: an
IBAN glued to a digit or an underscore (`…32011234`, `…3201_x`) stays raw,
because it cannot be told from a longer opaque identifier. The trade was
measured, not assumed: a random registry-shaped prefix passes mod-97 1.02 % of
the time (1 in 97, 200k tokens), so accepting every validated prefix would
tokenize 1 % of every registry-shaped upper-case token regardless of length,
while the letters-only rule's false-accept is 1 % × (26/36)^k for a glued
upper-case alphanumeric run of k characters (0.7 % at k = 1, 0.07 % at k = 8).
The Dataiku EN/DE holdout, the A4 negative corpus and `docs/**/*.md` are
byte-identical under either rule (the A4 corpus contains no registry-shaped
mod-97-valid token), so the evidence for the rule is the synthetic enumeration
in `scripts/bench/iban_trailing_word_enumeration.py` (solo todo #3756).
One related shape is only partly covered: a label glued to a spaced German
IBAN (`IBAN DE89 3704 0044 0532 0130 00BIC`) is a candidate, but
`phone.national.de` (priority 85) still claims the `0532 0130` sub-run, because
its 22-character IBAN-consuming branch keeps its trailing `\b` and stops
consuming at the glued label; with `custom:phone` tokenized every byte is
covered as `<iban_1><phone_1><iban_2>`, with it preserved the IBAN stays raw
as before (solo todo #3764). Compact German IBANs glued to a label tokenize whole.

## Residual coverage

Residual coverage is **on by default** for every pipeline built through
`Pipeline::builder()` (`crates/gaze/src/pipeline.rs`, `PipelineBuilder::build`
sets `residual_coverage: true`). There is no flag to turn it on; it is the
shipped behavior, and `gaze clean`, `gaze daemon`, `gaze-assembly`, and the
library API all get it.

### What it covers

Conflict resolution picks one winning selection per overlap and discards the
losers. When a losing original covered raw bytes that the winner does not, those
bytes previously survived into the clean text **in the clear**. Residual coverage
emits a second replacement over them.

The invariant is per character: **every byte claimed by a candidate of a class
the policy protects leaves the process protected.** A class is protected when
its resolved action is protective (`Action::is_protective`: anything but
`preserve`). Concretely (`crates/gaze/src/pipeline/residual.rs`):

- Admission is **per original**, never per overlap component. An original is
  admitted when its own class and its standalone fallback class (the family
  class, for a cue-less collision-family member) both preview protective. A
  preserved, redacted or unknown neighbour in the same overlap group cannot
  switch another claimant's coverage off.
- A **`preserve` winner does not shield the bytes a protected class claimed.**
  Only protective selections block the sweep; inside a preserved selection,
  bytes an admitted original claimed become a cell of the highest-ranked such
  claimant, and the preserved winner keeps every other byte raw. The cell's
  audit row says `decided_by: protection_override`. The candidates a
  preserved selection *represents* (its own original, a same-span merge, the
  rivals of a precedence tie) never override it: an explicit
  `custom:family:<name> = preserve` rule still leaves the ambiguous span raw.
- **One claimant, one fragment per uncovered run.** Adjacent cells of the same
  representative and class merge even where an inner candidate starts or
  ends, so an email inside a preserved URL leaves as one `<Email_1>`.
- A cell emits under **its claimant's own action**: `tokenize` and
  `format_preserve` mint a reversible class token (a fragment has no format
  to preserve), `redact` writes the one-way `[REDACTED:<class>]` marker,
  `generalize` the class placeholder. A neighbour's action never changes what
  a fragment becomes.
- Bytes that **no** original evidenced are still not protected. For
  `password: "left right"` with a `password.field` original matching `0..21` and
  a Name selection winning `0..15`, the residual covers `15..21` (`" right"`).
  The closing quote at byte 21 sits outside the union and stays in the clear.
  See
  [`crates/gaze-recognizers/tests/explicit_field_collision_control.rs`](../../crates/gaze-recognizers/tests/explicit_field_collision_control.rs),
  which pins exactly that geometry on the real `core` and opt-in `secrets` rulepacks.

Rules with no static preview (an adopter `Rule` impl that answers at runtime
only) stay on the legacy path: such a selection blocks the sweep and its
runtime verdict is not second-guessed; such an original is not admitted.

### What changes in the token stream

**One recognized value can produce more than one replacement**, although
containment precedence now folds a wholly contained rival into the container
(the reference letter `IBAN PL56 0942 8981 7280 5663 2200 4500 BIC` is one
IBAN token; fragments remain for partial overlaps and for claims inside a
preserved winner). Adopters counting manifest entries are counting
*replacements*, not distinct recognized values. See
[`EmittedTokenOrigin`](metrics.md#52-per-call-output) for how to tell the two
apart, and `BundleReport::pii_token_count` in `gaze-document` for the same
distinction on the bundle side.

Restore: a `tokenize` or `format_preserve` fragment is an ordinary reversible
token, and `Session::restore_strict_text` round-trips a document containing
one; a `redact` or `generalize` fragment is one-way exactly where the adopter
chose a one-way action for that class.

Activation does **not** move the bundled `core` tokenization snapshot: the
`bundle-tokenization-drift` corpus contains only contained overlaps, never a
partial one, so it produces no residual and
`crates/xtask/snapshots/core-no-policy.json` is unchanged. A future corpus edit
that introduces a *partial* overlap will move that snapshot and will need the
gate's `--verify-ack` acknowledgement.

### Provenance

A residual is traceable like any other emission. Its audit row carries the
representative parent original's `source` and `recognizer_id` /
`recognizer_version_id`, with `provenance_stage = "primary_pipeline.residual"`
(`log_residual_entry` in `crates/gaze/src/pipeline.rs`). In the protection trace
it projects to the existing `primary_pipeline` / `policy` / `tokenize` tuple,
which does not on its own certify a whole entity.
