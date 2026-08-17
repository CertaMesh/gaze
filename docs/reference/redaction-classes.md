# Redaction classes and recognizers

This is the canonical inventory of what Gaze can detect through the embedded
`core` and `core-extended` names. It covers the emitted classes, every bundled
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
The shared payload currently contains exactly 35 recognizer specs
(`crates/gaze-recognizers/src/lib.rs:62-110`).

## PII classes and resolver priority

`PiiClass` is the closed class vocabulary at
`crates/gaze-types/src/lib.rs:62-96`. During generic overlap resolution, a
higher class-priority integer wins containment
(`crates/gaze/src/resolver.rs:202-243,385-392`).

<!-- redaction-classes-gate:pii-classes:start -->
| Rust variant | Policy spelling | Class priority | Source |
|---|---|---:|---|
| `Email` | `email` | 90 | `crates/gaze-types/src/lib.rs:82-92`; `crates/gaze/src/resolver.rs:385-392` |
| `Name` | `name` | 80 | `crates/gaze-types/src/lib.rs:82-92`; `crates/gaze/src/resolver.rs:385-392` |
| `Organization` | `organization` | 70 | `crates/gaze-types/src/lib.rs:82-92`; `crates/gaze/src/resolver.rs:385-392` |
| `Location` | `location` | 60 | `crates/gaze-types/src/lib.rs:82-92`; `crates/gaze/src/resolver.rs:385-392` |
| `Custom` | `custom:<name>` | 50 | `crates/gaze-types/src/lib.rs:82-92,233-246`; `crates/gaze/src/resolver.rs:385-392` |
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

<!-- redaction-classes-gate:recognizers:start -->
| Embedded names | Recognizer id | Matcher | What it matches | Class | Locales | Validator | Normalizer | Safety tier | safe_default | Base | Priority | Source |
|---|---|---|---|---|---|---|---|---|---|---:|---:|---|
| `core, core-extended` | `email.global` | `regex` | Structurally valid email addresses, including reserved synthetic example domains | `Email` | `global` | `email_rfc` | `email_canonical` | `safe_default` | yes | 0.70 | 90 | `crates/gaze-recognizers/embedded/core.toml:9-49` |
| `core, core-extended` | `email.header.name` | `regex` | Quoted or capitalized display names before an angle-bracket address in email headers | `Name` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 100 | `crates/gaze-recognizers/embedded/core.toml:51-66` |
| `core, core-extended` | `email.header.name.paren` | `regex` | Parenthesized display names following an email address in headers or address lists | `Name` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 100 | `crates/gaze-recognizers/embedded/core.toml:68-86` |
| `core, core-extended` | `name.forward_marker` | `anchored_match` | Person-name-shaped text after locale-provided forwarded-message cues | `Name` | `de-DE, de-AT, de-CH, en-US, en-GB, en-IE, en-AU, en-CA` | `none` | `none` | `safe_default` | yes | 0.88 | 110 | `crates/gaze-recognizers/embedded/core.toml:88-109` |
| `core, core-extended` | `name.agent_recipient` | `anchored_match` | Person-name-shaped text after locale-provided agent-recipient cues | `Name` | `de-DE, de-AT, de-CH, en-US, en-GB, en-IE, en-AU, en-CA` | `none` | `none` | `safe_default` | yes | 0.88 | 110 | `crates/gaze-recognizers/embedded/core.toml:111-132` |
| `core, core-extended` | `name.auto_footer` | `anchored_match` | Person-name-shaped text after locale-provided footer or sign-off cues | `Name` | `de-DE, de-AT, de-CH, en-US, en-GB, en-IE, en-AU, en-CA` | `none` | `none` | `safe_default` | yes | 0.88 | 110 | `crates/gaze-recognizers/embedded/core.toml:134-155` |
| `core, core-extended` | `phone.structural` | `regex` | Compact international phone candidates beginning with plus and 6 to 15 digits | `custom:phone` | `global` | `e164_phone` | `none` | `safe_default` | yes | 0.70 | 80 | `crates/gaze-recognizers/embedded/core.toml:157-185` |
| `core, core-extended` | `phone.e164.spaced` | `regex` | Spaced or punctuated international phone candidates outside the US and German branches | `custom:phone` | `global` | `e164_phone` | `none` | `safe_default` | yes | 0.70 | 79 | `crates/gaze-recognizers/embedded/core.toml:187-215` |
| `core, core-extended` | `phone.national.de` | `regex` | German national or plus-49 phone shapes accepted by the German regional parser | `custom:phone` | `de-DE, de-AT, de-CH` | `e164_phone_national_de` | `none` | `locale_gated` | no | 0.82 | 85 | `crates/gaze-recognizers/embedded/core.toml:217-249` |
| `core, core-extended` | `phone.national.us` | `regex` | US NANPA phone shapes, including the documented synthetic 555-01xx range | `custom:phone` | `en-US` | `e164_phone_national_us` | `none` | `safe_default` | yes | 0.82 | 85 | `crates/gaze-recognizers/embedded/core.toml:251-278` |
| `core, core-extended` | `iban.structural` | `regex` | Space-tolerant IBAN shapes that pass MOD-97 after canonicalization | `custom:iban` | `global` | `iban_mod97` | `iban_canonical` | `safe_default` | yes | 0.70 | 80 | `crates/gaze-recognizers/embedded/core.toml:280-308` |
| `core, core-extended` | `card.structural` | `regex` | 13 to 19 digit payment-card shapes with optional spaces or dashes that pass Luhn | `custom:credit_card` | `global` | `luhn` | `none` | `safe_default` | yes | 0.70 | 80 | `crates/gaze-recognizers/embedded/core.toml:310-333` |
| `core, core-extended` | `ip.v4` | `regex` | Decimal dotted-quad IPv4 addresses with octets from 0 through 255 | `custom:ip_address` | `global` | `ipv4_parse` | `none` | `safe_default` | yes | 0.70 | 80 | `crates/gaze-recognizers/embedded/core.toml:335-354` |
| `core, core-extended` | `ip.v6` | `regex` | Full, compressed (including bare `::`), and IPv4-embedded IPv6 textual forms; bare `::` also matches the scope separator inside Rust and C++ paths at every locale (todo #2402) | `custom:ip_address` | `global` | `ipv6_parse` | `none` | `safe_default` | yes | 0.70 | 80 | `crates/gaze-recognizers/embedded/core.toml:356-405` |
| `core, core-extended` | `eth.address` | `regex` | Forty-hex-digit Ethereum addresses prefixed by 0x and accepted by EIP-55 rules | `custom:eth_address` | `global` | `eth_eip55` | `none` | `safe_default` | yes | 0.70 | 80 | `crates/gaze-recognizers/embedded/core.toml:407-425` |
| `core, core-extended` | `aadhaar.in` | `regex` | Cue-anchored Indian Aadhaar or UID values containing 12 digits and passing Verhoeff | `custom:aadhaar` | `en-IN, hi-IN` | `aadhaar_verhoeff` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:427-446` |
| `core, core-extended` | `nir.fr` | `regex` | Cue-anchored French NIR social-security values with 15 digits and a valid MOD-97 key | `custom:nir` | `fr-FR` | `fr_nir_mod97` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:448-467` |
| `core, core-extended` | `steuer_id.de` | `regex` | Cue-anchored German 11-digit Steuer-ID values passing MOD 11,10 | `custom:steuer_id` | `de-DE, de-AT, de-CH` | `de_steuer_id_mod1110` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:469-488` |
| `core, core-extended` | `vat.de` | `regex` | Cue-anchored German VAT identifiers shaped as DE followed by nine digits | `custom:vat_id` | `de-DE, de-AT, de-CH` | `none` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:490-509` |
| `core, core-extended` | `vat.es` | `regex` | Cue-anchored Spanish VAT, CIF, or NIF identifiers in the ES alphanumeric shape | `custom:vat_id` | `es-ES` | `none` | `none` | `safe_default` | yes | 0.84 | 84 | `crates/gaze-recognizers/embedded/core.toml:511-530` |
| `core, core-extended` | `bsn.nl` | `regex` | Cue-anchored Dutch nine-digit BSN values passing the 11-test | `custom:bsn` | `nl-NL` | `bsn_mod11` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:532-551` |
| `core, core-extended` | `cpf.br` | `regex` | Cue-anchored Brazilian CPF values passing both MOD-11 check digits | `custom:cpf` | `pt-BR` | `cpf_mod11` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:553-572` |
| `core, core-extended` | `cnpj.br` | `regex` | Cue-anchored Brazilian CNPJ values passing both MOD-11 check digits | `custom:cnpj` | `pt-BR` | `cnpj_mod11` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:574-593` |
| `core, core-extended` | `nhs.uk` | `regex` | Cue-anchored UK NHS numbers containing 10 digits and passing MOD-11 | `custom:nhs_number` | `en-GB` | `uk_nhs_mod11` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:595-614` |
| `core, core-extended` | `ssn.us` | `regex` | Cue-anchored US Social Security numbers in dashed or nine-digit form | `custom:ssn` | `en-US` | `none` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:616-632` |
| `core, core-extended` | `nino.uk` | `regex` | Cue-anchored UK National Insurance numbers with allocation-constrained prefixes | `custom:nino` | `en-GB` | `none` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:634-653` |
| `core, core-extended` | `pan.in` | `regex` | Cue-anchored Indian Permanent Account Numbers in the ten-character PAN shape | `custom:pan` | `en-IN, hi-IN` | `none` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:655-671` |
| `core, core-extended` | `postal.de` | `regex` | Bare five-digit German postal-code shapes | `custom:postal_code` | `de-DE` | `none` | `none` | `locale_gated` | no | 0.70 | 70 | `crates/gaze-recognizers/embedded/core.toml:673-695` |
| `core, core-extended` | `postal.us` | `regex` | US five-digit ZIP or ZIP+4 shapes | `custom:postal_code` | `en-US` | `none` | `none` | `locale_gated` | no | 0.70 | 70 | `crates/gaze-recognizers/embedded/core.toml:697-719` |
| `core, core-extended` | `url.anchored` | `regex` | URLs beginning with `http://`, `https://`, or `www.` through the final non-punctuation URL character | `custom:url` | `global` | `none` | `none` | `safe_default` | yes | 0.75 | 85 | `crates/gaze-recognizers/embedded/core.toml:721-760` |
| `core, core-extended` | `security_token.anchored` | `regex` | Cue-anchored credential values plus structurally prefixed AWS access keys and three-segment JWTs | `custom:security_token` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 87 | `crates/gaze-recognizers/embedded/core.toml:762-847` |
| `core, core-extended` | `ssn.de_cue` | `regex` | Cue-anchored SSN values after German social-insurance cues (Sozialversicherungsnummer, SV-Nummer) in dashed, dotted, or 9 to 11 digit form; format basis, DACH provenance | `custom:ssn` | `de-DE, de-AT, de-CH` | `none` | `none` | `safe_default` | yes | 0.88 | 86 | `crates/gaze-recognizers/embedded/core.toml:936-963` |
| `core, core-extended` | `tax_number.cue_anchored` | `regex` | Cue-anchored tax numbers with a three-digit lead and separated digit groups after German or English tax cues; bare digit runs and the checksummed 2-3-3-3 Steuer-ID shape are excluded | `custom:tax_number` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 84 | `crates/gaze-recognizers/embedded/core.toml:1004-1030` |
| `core, core-extended` | `driver_license.cue_anchored` | `regex` | Letter-led alphanumeric licence numbers after German or English driving-licence cues | `custom:driver_license` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 83 | `crates/gaze-recognizers/embedded/core.toml:1044-1065` |
| `core, core-extended` | `national_id.cue_anchored` | `regex` | Letter-led, digit-grouped, or 9 to 11 digit identifiers after German or English national-ID cues | `custom:national_id` | `global` | `none` | `none` | `safe_default` | yes | 0.82 | 82 | `crates/gaze-recognizers/embedded/core.toml:1085-1111` |
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
| `payment-card-or-iban` | `iban.structural` | `iban` | 10 | `iban` | `crates/gaze-recognizers/embedded/core.toml:304-308` |
| `payment-card-or-iban` | `card.structural` | `pan` | 20 | `none` | `crates/gaze-recognizers/embedded/core.toml:330-333` |
| `phone-or-imei` | `phone.structural` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:182-185` |
| `phone-or-imei` | `phone.e164.spaced` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:212-215` |
| `phone-or-imei` | `phone.national.de` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:246-249` |
| `phone-or-imei` | `phone.national.us` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:275-278` |
| `government-id` | `ssn.de_cue` | `ssn` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:956-959` |
| `government-id` | `tax_number.cue_anchored` | `tax-number` | 20 | `none` | `crates/gaze-recognizers/embedded/core.toml:1023-1026` |
| `government-id` | `national_id.cue_anchored` | `national-id` | 30 | `none` | `crates/gaze-recognizers/embedded/core.toml:1104-1107` |
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
   mandatory-anchor context, then the generic tiers
   (`crates/gaze/src/resolver.rs:254-297`).
4. The generic tiers are **class priority > rule priority > score > span length
   > lexicographically smaller recognizer id**
   (`crates/gaze/src/resolver.rs:277-297`).
5. Same-class containment has one extra check before those generic tiers: a
   candidate with a validator-produced canonical form defeats an otherwise
   equivalent unvalidated candidate
   (`crates/gaze/src/resolver.rs:202-240`). This is
   `ConflictTier::Validator`, distinct from the pre-resolver
   `ValidatorVeto`.
6. Replacement removes every overlap with the winner, so multi-overlap inputs
   converge to a disjoint fixed point rather than leaving a candidate that
   overlapped an earlier loser (`crates/gaze/src/resolver.rs:73-166,366-383`).
7. After pairwise resolution, a surviving candidate that requires but lacks a
   mandatory anchor is converted to its family-level fallback
   (`crates/gaze/src/resolver.rs:63-68,325-363`).

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

The tracked Kiji gap analysis independently classifies company and street
extraction as safety-net/NER gaps and explains why first-name and surname labels
are not checksum-validatable
(`docs/reference/benchmarks/v0.8-kiji-class-gap.md:32-58`).

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

The plain `core` default locale chain is `global`
(`crates/gaze-recognizers/embedded/core.toml:1-4`), so the document-basis
recognizers that activate are exactly the global `safe_default` ones. Every
`locale_basis = "format"` recognizer activates regardless of the chain
(`crates/gaze-assembly/src/detector_wiring.rs:271-301`); see
[Locale Chain](../explanation/policy/locale-chain.md) for the mixed-basis
model. For the CLI, `normalize_rulepack_bundles`
rewrites the deprecated `core-extended` selection to `core` while returning an
`auto_activate_locale_gated` bit
(`crates/gaze-cli/src/pipeline/run.rs:712-728`). `CleanOverrides::apply_to`
carries that bit into `Policy::rulepacks`
(`crates/gaze-cli/src/clean_overrides.rs:48-63`). Pipeline construction then
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
| `core` | `global` | no | `aadhaar.in, bsn.nl, card.structural, cnpj.br, cpf.br, driver_license.cue_anchored, email.global, email.header.name, email.header.name.paren, eth.address, iban.structural, ip.v4, ip.v6, national_id.cue_anchored, nhs.uk, nino.uk, nir.fr, pan.in, phone.e164.spaced, phone.national.us, phone.structural, security_token.anchored, ssn.de_cue, ssn.us, steuer_id.de, tax_number.cue_anchored, url.anchored, vat.de, vat.es` | `crates/gaze-recognizers/embedded/core.toml:1-1111`; `crates/gaze-assembly/src/defaults.rs:45-77` |
| `core-extended compatibility alias` | `global, en-US, de-DE, de-AT, de-CH` | yes | `aadhaar.in, bsn.nl, card.structural, cnpj.br, cpf.br, driver_license.cue_anchored, email.global, email.header.name, email.header.name.paren, eth.address, iban.structural, ip.v4, ip.v6, name.agent_recipient, name.auto_footer, name.forward_marker, national_id.cue_anchored, nhs.uk, nino.uk, nir.fr, pan.in, phone.e164.spaced, phone.national.de, phone.national.us, phone.structural, postal.de, postal.us, security_token.anchored, ssn.de_cue, ssn.us, steuer_id.de, tax_number.cue_anchored, url.anchored, vat.de, vat.es` | `crates/gaze-assembly/src/locale.rs` (`locale_gated_activation_locales`); `crates/gaze-assembly/src/defaults.rs:45-77`; `crates/gaze-cli/src/pipeline/run.rs:137-146,712-728` |
<!-- redaction-classes-gate:default-activation:end -->

The v0.6+ compatibility behavior therefore does activate
`phone.national.de`, `postal.us`, and `postal.de` with
`--rulepack-bundled core-extended` and no policy. The complete second row is
authoritative: the widened US/German compatibility locale chain also makes the
listed document-basis cue-anchored and locale-specific recognizers eligible.
Pass `--locale=global`, or use an explicit policy with narrower locale gating,
to avoid that document-basis compatibility expansion. Format-basis identifiers
(`ssn.us`, `ssn.de_cue`, `steuer_id.de`, `phone.national.us`, and the other
format rows in the coverage matrix) are active in both rows; the locale chain
is not a suppression mechanism for them, so an adopter that must not tokenize
one of them has to disable that recognizer.
