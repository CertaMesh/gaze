# Redaction classes and recognizers

Inventory of embedded `core` (`core-extended` alias) and `secrets`: classes,
recognizers, validators, normalizers, precedence, activation and gaps.
Marked tables are compared against loaded rulepacks and Rust enums by:

```bash
rustup run 1.96.0 cargo test -p xtask --test redaction_classes_doc
```

`core` contains exactly 57 recognizer specs. `core-extended` loads the same bytes but widens
[activation](#shipped-default-activation). `secrets` has `security_token.anchored`
and `password.field`; setup includes it, while library callers and hand-written
policies must select it: `bundled = ["core", "secrets"]` or
`--rulepack-bundled core,secrets`.

No rulepack emits `custom:username`. Optional Nym can report username, licence
plate, building number, tax ID, postal code and date classes;
see [mapping](../explanation/safety-net/safety-nets.md#which-labels-can-fire).

## PII classes and resolver priority

`PiiClass` lives in `gaze-types`. Higher integers win generic partial overlaps.
Containment checks run first; see [resolution order](#full-conflict-resolution-order).

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
stage. A validator can veto a shape match or record a failure on a cued candidate before conflict resolution; a
normalizer changes the canonical value only and never the original restore span.
See [Validator Veto](../explanation/detection/validator-veto.md) and
[Recognizer normalizers preserve the original span](../explanation/detection/recognizer-normalizer-spans.md).

Definitions: `crates/gaze-recognizers/embedded/core.toml` and `secrets.toml`.
`crates/xtask/tests/redaction_classes_doc.rs` checks every table column.

<!-- redaction-classes-gate:recognizers:start -->
| Embedded names | Recognizer id | Matcher | What it matches | Class | Locales | Validator | Normalizer | Safety tier | safe_default | Base | Priority |
|---|---|---|---|---|---|---|---|---|---|---:|---:|
| `core, core-extended` | `email.global` | `regex` | Structurally valid email addresses, including reserved synthetic example domains | `Email` | `global` | `email_rfc` | `email_canonical` | `safe_default` | yes | 0.70 | 90 |
| `core, core-extended` | `email.header.name` | `regex` | Quoted or capitalized display names (all-caps, diacritic, hyphen- or apostrophe-joined parts) before an angle-bracket address in email headers | `Name` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 100 |
| `core, core-extended` | `email.header.name.paren` | `regex` | Parenthesized display names following an email address in headers or address lists | `Name` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 100 |
| `core, core-extended` | `name.forward_marker` | `anchored_match` | Person-name-shaped text after locale-provided forwarded-message cues | `Name` | `de-DE, de-AT, de-CH, en-US, en-GB, en-IE, en-AU, en-CA` | `none` | `none` | `safe_default` | yes | 0.88 | 110 |
| `core, core-extended` | `name.agent_recipient` | `anchored_match` | Person-name-shaped text after locale-provided agent-recipient cues | `Name` | `de-DE, de-AT, de-CH, en-US, en-GB, en-IE, en-AU, en-CA` | `none` | `none` | `safe_default` | yes | 0.88 | 110 |
| `core, core-extended` | `name.auto_footer` | `anchored_match` | Person-name-shaped text after locale-provided footer or sign-off cues | `Name` | `de-DE, de-AT, de-CH, en-US, en-GB, en-IE, en-AU, en-CA` | `none` | `none` | `safe_default` | yes | 0.88 | 110 |
| `core, core-extended` | `phone.structural` | `regex` | Compact international phone candidates beginning with plus and 6 to 15 digits | `custom:phone` | `global` | `e164_phone` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `phone.e164.spaced` | `regex` | Spaced or punctuated international phones, 00 dialing prefixes, optional trunk zeroes and French dotted forms | `custom:phone` | `global` | `phone_number` | `none` | `safe_default` | yes | 0.70 | 79 |
| `core, core-extended` | `phone.e164.spaced.cued` | `regex` | Labelled international or supported national phone forms; validated list neighbours inherit the cue and parser failures retain a typed audit reason | `custom:phone` | `global` | `phone_number` | `none` | `safe_default` | yes | 0.70 | 78 |
| `core, core-extended` | `phone.national.de` | `regex` | German national or plus-49 phone shapes accepted by the German regional parser | `custom:phone` | `de-DE, de-AT, de-CH` | `e164_phone_national_de` | `none` | `locale_gated` | no | 0.82 | 85 |
| `core, core-extended` | `phone.national.us` | `regex` | US NANPA phone shapes, including the documented synthetic 555-01xx range | `custom:phone` | `en-US` | `e164_phone_national_us` | `none` | `safe_default` | yes | 0.82 | 85 |
| `core, core-extended` | `phone.national.us.cued` | `regex` | Same-line US phone candidates after English or loaded locale phone-label buckets; region failures retain a typed audit reason | `custom:phone` | `en-US` | `e164_phone_national_us` | `none` | `safe_default` | yes | 0.82 | 79 |
| `core, core-extended` | `iban.structural` | `regex` | Space-tolerant IBAN shapes at the country's ISO 13616 registry length, tokenized whether or not MOD-97 passes after canonicalization (`on_fail = "record"`: a failure is written on the audit row and the value is never swept); no trailing word boundary in the pattern, the code boundary accepts a glued label and refuses a glued digit or underscore | `custom:iban` | `global` | `iban_mod97` | `iban_canonical` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `iban.cued` | `regex` | IBAN-structured values with a real ISO 3166-1 country code (or `UK`) outside the ISO 13616 registry (two digits, then up to four letters and 6 to 26 digits compact, or three to eight digit-bearing groups after at most one letters-only bank-code group) within 32 characters after the word `IBAN` on the same line through the shared cue window (one `:`, `,` or `=` right after the cue, one nested JSON key such as `{"number": "`, or one `label:` after a copula or parenthetical; any other `.`, `;`, `!`, `?`, `:`, `,` or `=` ends it); registry countries stay with `iban.structural`; MOD-97 failures are recorded, not vetoed | `custom:iban` | `global` | `iban_mod97` | `iban_canonical` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `card.structural` | `regex` | a digit run with optional spaces or dashes; all-zero card windows are excluded; the recognizer finds the Luhn-valid 13 to 19 digit card inside it (the old 13-19 window or a group-aligned card layout: compact, 4-4-4-4, 4-4-4-4-3, 4-6-5, 4-6-4; overlapping Luhn-valid windows are one token over their union), so a touching CVV, expiry or number does not hide it | `custom:credit_card` | `global` | `luhn` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `card.cued` | `regex` | A card layout (4-4-4-4-3 whole, 4-4-4-4, 4-6-5, 4-6-4, compact 16 to 19 digits starting 2-6, compact 14 to 15 digits starting 3; a trailing 3-digit group after the other layouts stays outside the token, further digits refuse the match) within 32 characters after a card cue (`card` with credit/debit/payment/bank/prepaid/gift and number/no/nr/holder, German card compounds, a bare `Karte` only with `Nummer`/`Nr`, card brands) on the same line through the shared cue window (one `:`, `,` or `=` right after the cue, one nested JSON key such as `{"number": "`, or one `label:` after a copula or parenthetical; any other `.`, `;`, `!`, `?`, `:`, `,` or `=` ends it); a span holding a card stays whole (a cued 4-4-4-4-3 is one token); a Luhn-failing 13- or 15-digit compact card not starting with 3 stays raw; Luhn failures are recorded, not vetoed. `card.structural` keeps its Luhn veto | `custom:credit_card` | `global` | `luhn` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `card.cued_short` | `regex` | A 12- to 15-digit number with a Maestro issuer prefix (50, 56 to 69), compact or grouped 4-4-4, directly after a card cue (the `card.cued` card words and `maestro`): only a `:` or `=`, quotes, a copula (`is`, `ist`, `lautet`) or one nested JSON key may stand between them, so `card reader serial` or `card terminal ID is` never reach the number; Luhn failures are recorded, not vetoed | `custom:credit_card` | `global` | `luhn` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `ip.v4` | `regex` | Decimal dotted-quad IPv4 addresses with octets from 0 through 255, excluding RFC 5737 documentation ranges and loopback (`127.0.0.0/8`) | `custom:ip_address` | `global` | `ipv4_parse_non_documentation` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `ip.v6` | `regex` | Full, compressed (including the bare double-colon form), and IPv4-embedded IPv6 textual forms at every locale, excluding RFC 3849 and embedded RFC 5737 documentation ranges and loopback (`::1`, IPv4-mapped or IPv4-compatible `127.0.0.0/8`). The word guard excludes an address adjacent to an identifier character, so Rust and C++ double-colon paths survive; an explicit `Address:`, `Adresse:`, `IP:`, `IPv6:`, `host:` or `addr:` cue admits a glued address after full IPv6 parsing (case-insensitive cues; `global` document-basis activation, superseding #2402). `_2001:db9::1` remains outside the cue rule. A standalone path whose segments are all short hex words and which has no surrounding context is still an address | `custom:ip_address` | `global` | `ipv6_parse_non_documentation` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `eth.address` | `regex` | Forty-hex-digit Ethereum addresses prefixed by 0x and accepted by EIP-55 rules | `custom:eth_address` | `global` | `eth_eip55` | `none` | `safe_default` | yes | 0.70 | 80 |
| `core, core-extended` | `aadhaar.in` | `regex` | Cue-anchored Indian Aadhaar or UID values containing 12 digits and passing Verhoeff | `custom:aadhaar` | `en-IN, hi-IN` | `aadhaar_verhoeff` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `nir.fr` | `regex` | Cue-anchored French NIR social-security values with 15 digits and a valid MOD-97 key | `custom:nir` | `fr-FR` | `fr_nir_mod97` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `steuer_id.de` | `regex` | Cue-anchored German 11-digit Steuer-ID values; broader German connectors require explicit digit grouping to preserve compact-value repeat ownership, excluding all-zero placeholders and vehicle-ID labels; MOD 11,10 failures retain a typed audit reason and never seed the repeat-value sweep | `custom:steuer_id` | `de-DE, de-AT, de-CH` | `de_steuer_id_mod1110` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `vat.de` | `regex` | Cue-anchored German VAT identifiers shaped as DE followed by nine digits | `custom:vat_id` | `de-DE, de-AT, de-CH` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `vat.es` | `regex` | Cue-anchored Spanish VAT, CIF, or NIF identifiers in the ES alphanumeric shape | `custom:vat_id` | `es-ES` | `none` | `none` | `safe_default` | yes | 0.84 | 84 |
| `core, core-extended` | `bsn.nl` | `regex` | Cue-anchored Dutch nine-digit BSN values, excluding all-zero placeholders; 11-test failures retain a typed audit reason and never seed the repeat-value sweep | `custom:bsn` | `nl-NL` | `bsn_mod11` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `cpf.br` | `regex` | Cue-anchored Brazilian CPF values; MOD-11 failures retain a typed audit reason and never seed the repeat-value sweep | `custom:cpf` | `pt-BR` | `cpf_mod11` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `cnpj.br` | `regex` | Cue-anchored Brazilian CNPJ values passing both MOD-11 check digits | `custom:cnpj` | `pt-BR` | `cnpj_mod11` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `nhs.uk` | `regex` | Cue-anchored UK NHS numbers containing 10 digits, excluding all-zero placeholders; MOD-11 failures retain a typed audit reason and never seed the repeat-value sweep | `custom:nhs_number` | `en-GB` | `uk_nhs_mod11` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `ssn.us` | `regex` | Cue-anchored US Social Security numbers in compact or separated three-two-four digit form | `custom:ssn` | `en-US` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `nino.uk` | `regex` | Cue-anchored UK National Insurance numbers with allocation-constrained prefixes | `custom:nino` | `en-GB` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `pan.in` | `regex` | Cue-anchored Indian Permanent Account Numbers in the ten-character PAN shape | `custom:pan` | `en-IN, hi-IN` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `postal.de` | `regex` | Bare five-digit German postal-code shapes | `custom:postal_code` | `de-DE` | `none` | `none` | `locale_gated` | no | 0.70 | 70 |
| `core, core-extended` | `postal.us` | `regex` | US five-digit ZIP or ZIP+4 shapes | `custom:postal_code` | `en-US` | `none` | `none` | `locale_gated` | no | 0.70 | 70 |
| `core, core-extended` | `postal.at_ch` | `regex` | Austrian and Swiss four-digit postal codes, optionally `A-`/`CH-`/`FL-` prefixed, only directly after a postal cue (`PLZ`, `Postleitzahl`, `Postcode`, `ZIP`, including hyphenated labels and `ist`/`lautet`) or directly before a city-shaped token (uppercase start, or `St.` / `St` before a capitalised name) across a space, NO-BREAK SPACE, or NARROW NO-BREAK SPACE; a city-anchored code is not matched when preceded by `#` or directly preceded by an Australian state abbreviation | `custom:postal_code` | `de-AT, de-CH` | `none` | `none` | `locale_gated` | no | 0.70 | 70 |
| `core, core-extended` | `postal.cued_four_digit` | `regex` | Four-digit postcode directly after `postcode`, `postal code`, or `zip`/`zip code`; unlabelled street addresses stay raw because years and amounts have the same shape | `custom:postal_code` | `global` | `none` | `none` | `safe_default` | yes | 0.70 | 70 |
| `core, core-extended` | `postal.cued_short` | `regex` | Swedish `NNN NN`, Polish `NN-NNN`, Brazilian `NNNNN-NNN`, six-digit and three-digit postcodes directly after `zip`/`zip code`, `postcode`, `postal code`, `PLZ`, `Postleitzahl`, `CEP`, `postnummer`, `kod pocztowy`, `code postal` or `póstnúmer` (separators and `is`/`ist`/`lautet` only); unlabelled codes stay raw | `custom:postal_code` | `global` | `none` | `none` | `safe_default` | yes | 0.70 | 70 |
| `core, core-extended` | `postal.au` | `regex` | Australian state or territory abbreviation plus four-digit postcode in Australia Post's published state range, after a capitalized word of three or more letters other than a listed EN/DE function word, or before a terminal field boundary; the state and code are one restorable token | `custom:postal_code` | `en-AU` | `none` | `none` | `locale_gated` | no | 0.70 | 70 |
| `core, core-extended` | `postal.ca` | `regex` | Canadian six-character postal codes alternating letter and digit, starting with a letter; hyphenated, compact, or separated by a space, NO-BREAK SPACE, or NARROW NO-BREAK SPACE; not matched when preceded by `#` | `custom:postal_code` | `en-CA` | `none` | `none` | `safe_default` | yes | 0.80 | 72 |
| `core, core-extended` | `postal.gb` | `regex` | UK postcodes across all six Royal Mail outward forms plus the special GIR outward code, followed by an inward code of one digit and two letters from the official inward alphabet; space, NO-BREAK SPACE, or NARROW NO-BREAK SPACE separator; not matched when preceded by `#` | `custom:postal_code` | `en-GB` | `none` | `none` | `safe_default` | yes | 0.80 | 72 |
| `core, core-extended` | `postal.ie` | `regex` | Irish Eircodes: routing key including `D6W`, plus a four-character identifier over the restricted Eircode alphabet that must carry at least one letter | `custom:postal_code` | `en-IE` | `none` | `none` | `safe_default` | yes | 0.80 | 72 |
| `core, core-extended` | `url.anchored` | `regex` | URLs beginning with an HTTP(S) scheme, with plain or JSON-escaped slashes, or a web prefix, ending before whitespace or a JSON/HTML delimiter (`"` `<` `>` `{` `}`, a backslash that does not escape a slash, a closing single quote) and at the final non-punctuation URL character | `custom:url` | `global` | `none` | `none` | `safe_default` | yes | 0.75 | 85 |
| `core, core-extended` | `ssn.de_cue` | `regex` | Cue-anchored SSN values after German social-insurance cues (Sozialversicherungsnummer, SV-Nummer), AHV, or English social-insurance cues; dashed, dotted, 9 to 11 digit, or three-four-four-two digit form; format basis; DACH provenance describes cue vocabulary | `custom:ssn` | `de-DE, de-AT, de-CH` | `none` | `none` | `safe_default` | yes | 0.88 | 86 |
| `core, core-extended` | `tax_number.cue_anchored` | `regex` | Cue-anchored tax numbers with a three-digit lead and separated digit groups after German or English tax cues; bare digit runs and the checksummed 2-3-3-3 Steuer-ID shape are excluded | `custom:tax_number` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 84 |
| `core, core-extended` | `driver_license.cue_anchored` | `regex` | Letter-led alphanumeric licence numbers after German or English driving-licence cues | `custom:driver_license` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 83 |
| `core, core-extended` | `national_id.cue_anchored` | `regex` | Letter-led, digit-grouped, 9 to 13 digit, or Swiss AHV (`756.dddd.dddd.dd`) identifiers after German or English national-ID / identity-card / AHV cues; excludes vehicle IDs labelled `Fahrzeug-Identifikationsnummer` | `custom:national_id` | `global` | `none` | `none` | `safe_default` | yes | 0.82 | 82 |
| `core, core-extended` | `tax_number.labelled` | `regex` | Class-specific tax labels with grouped or alphanumeric values, including slash-form Steuernummer and explicit English compact tax-identification fields; the complete-value scan extends a bounded capture through the adjacent value and audits length or field boundaries | `custom:tax_number` | `global` | `none` | `none` | `safe_default` | yes | 0.84 | 84 |
| `core, core-extended` | `driver_license.labelled` | `regex` | Class-specific driver-licence labels with grouped or alphanumeric values; the complete-value scan extends a bounded capture through the adjacent value and audits length or field boundaries | `custom:driver_license` | `global` | `none` | `none` | `safe_default` | yes | 0.84 | 84 |
| `core, core-extended` | `id_card.labelled` | `regex` | Class-specific national-ID and identity-card labels with grouped or alphanumeric values; the complete-value scan extends a bounded capture through the adjacent value and audits length or field boundaries | `custom:national_id` | `global` | `none` | `none` | `safe_default` | yes | 0.84 | 84 |
| `core, core-extended` | `passport.cue_anchored` | `regex` | Letter-led alphanumeric, Personalausweis-silhouette, or 9-digit passport numbers after passport / Reisepass cues | `custom:passport` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 84 |
| `core, core-extended` | `birth_date.cue` | `regex` | Numeric and month-name dates after an en/de/fr/nl/pt/da/es birth cue in prose, JSON keys or `key=value` fields; a date without a birth cue stays raw; format recognition only, no calendar-validity claim | `custom:birth_date` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
| `core, core-extended` | `birth_date.answer` | `regex` | A numeric or month-name date given as the answer one sentence after a date-of-birth cue: the sentence break must be followed directly by `it's`, `it is`, `that's`, `es ist` or `das ist`, so a date after any other sentence stays raw | `custom:birth_date` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
| `core, core-extended` | `age.cue` | `regex` | Numeric person age from 0 to 122 in a structured field or a person-framed year phrase; only the number is tokenized | `custom:age` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
| `core, core-extended` | `age.phrase` | `regex` | Numeric person age from 1 to 122 after `turned` with a person word before it and a clause boundary or time word after it, before `geworden` after a person word and a copula, after `at the age of` / `im Alter von` with a person word before it, before `y/o` after a person copula or before a person noun, and before `year old female`/`male` unless an animal noun follows; only the number is tokenized | `custom:age` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
| `secrets` | `security_token.anchored` | `regex` | Cue-anchored credential values (a complete three-segment JWT after a cue included) plus structurally prefixed AWS access keys and three-segment JWTs | `custom:security_token` | `global` | `none` | `none` | `safe_default` | yes | 0.85 | 87 |
| `secrets` | `password.field` | `regex` | Values in explicit EN/DE password or passphrase records; 1 to 256 normalized grammar units, with matching quoted or plain scalar syntax; not a raw-byte ceiling | `custom:password` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
| `core, core-extended` | `customer_id.labelled` | `regex` | Explicit English/German person-linked customer identifier labels in serialized JSON, logs or same-line prose; capture only the complete adjacent value | `custom:customer_id` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
| `core, core-extended` | `employee_id.labelled` | `regex` | Explicit English/German person-linked employee identifier labels in serialized JSON, logs or same-line prose; capture only the complete adjacent value | `custom:employee_id` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
| `core, core-extended` | `record_id.labelled` | `regex` | Explicit English/German person-linked record identifier labels in serialized JSON, logs or same-line prose; capture only the complete adjacent value | `custom:record_id` | `global` | `none` | `none` | `safe_default` | yes | 0.90 | 100 |
<!-- redaction-classes-gate:recognizers:end -->

The three government-ID `*.labelled` rows are safe-default `core` rules. Their
captured value, not the field name, becomes the token. They cooperate with the
older `*.cue_anchored` rows of the same class. Those six rows use
`match.complete_labelled_value` to scan adjacent groups. An older cue rule keeps
its full original capture even when a date-like group appears inside it; a
following uppercase field label remains visible when its value is claimed by a
second recognizer. Otherwise the prior token covers that field as a fail-closed
fallback. Grouped runs
can extend beyond four groups or 40 bytes with a typed audit reason; the
[labelled-identifier explanation](../explanation/detection/labelled-identifiers.md)
describes its stop and audit rules. The `redaction_classes_doc` test pins these
rows and the 57-spec inventory to the loaded rulepack. The
`bundle-tokenization-drift --verify-ack` gate separately checks the bundled
output against its committed corpus snapshot.

The three CRM/HR rules protect `custom:customer_id`, `custom:employee_id` and
`custom:record_id` through the same complete-value scanner. Customer and employee
labels establish their respective classes; person/contact/member identifiers
establish the record class. Bare `id`, `record_id` and `unique_id` do not establish
person linkage and stay outside these rules, as do order, invoice, issue, build,
commit and tracking labels. Connectors must stay on the same line. The rules
accept snake, camel and kebab spelling, English/German labels, numeric and opaque
alphanumeric values (including combining marks and `+/_=-` punctuation), grouped
runs and UUIDs. The original bytes remain in the
manifest for exact restore. An explicit label is the authority; the rules cannot
distinguish a corporate customer from an individual customer. These are text
matchers: serialized JSON retains the semantic key beside the value.
`RawDocument::Structured` walks scalar strings individually, so this route does
not classify an otherwise unlabelled scalar from its structured field name.

The label conventions reflect [Microsoft Graph employeeId](https://learn.microsoft.com/en-us/graph/api/resources/user?view=graph-rest-1.0),
[SAP personnel identifiers](https://help.sap.com/docs/ERP_HCM/d3bc4479e3344626be836c03a5c6962f/4fc7599929c05df3e10000000a42189b.html),
and [membershipNumber](https://schema.org/membershipNumber). Generic
[identifier](https://schema.org/identifier) covers any kind of object, so it is
insufficient by itself. Nym `CUSTOMER_ID` and `EMPLOYEE_ID` remain unsupported by
the closed safety-net class mapping: enabling them needs a class-contract change
and separate measured evidence, rather than only an allowlist edit.

### Cue shapes and group separators

Every cue-anchored identifier rule above (`aadhaar.in` through `pan.in`,
`ssn.de_cue`, and the `*.cue_anchored` rules) accepts its cue as prose
(a labelled nine-digit BSN), as a JSON key in double, single, or backslash-escaped
quotes (a JSON string or numeric `bsn` field), and as a `key=value` or
`key: value` log field. Keys may be snake, camel, or kebab case (`steuer_id`,
`steuerId`, `nhs_number`) and may carry an underscore prefix (`customer_ssn`).
A camelCase prefix (`customerSsn`) is not matched. `birth_date.cue` reads the
same key shapes (a JSON `dob` field or a `date_of_birth` log field) and prose
cues; it is pinned by `crates/gaze-recognizers/tests/birth_date_cues.rs`. The
identifier rules are pinned by
`crates/gaze-recognizers/tests/structured_cue_shapes.rs`.

Detection reads every Unicode space separator (NO-BREAK SPACE, NARROW NO-BREAK
SPACE, THIN SPACE, FIGURE SPACE, and the rest of category Zs) as an ASCII
space, so a grouped IBAN, card, Steuer-ID, or national ID matches and validates
the same way with any of them. Tokens, manifests, and restore keep the original
bytes. Pinned by `crates/gaze-recognizers/tests/unicode_group_separators.rs`.

## Closed validator and normalizer sets

### `ValidatorKind`

`ValidatorKind` is owned by `gaze-types` and currently contains 16 Rust
variants. `E164Phone`, `PhoneNumber` and the parameterized
`E164PhoneNational(Region)` variant are compiled only with `phone-parser`.
The supported `Region` variants cover Germany, the United States, Austria,
Switzerland, the United Kingdom, Ireland, Australia, Canada, New Zealand,
South Africa and France. All other validator variants are always available.

<!-- redaction-classes-gate:validators:start -->
| Rust variant | Rulepack name or names | Feature | Validation | Source |
|---|---|---|---|---|
| `EmailRfc` | `email_rfc` | `always` | Basic email local-part and dotted-domain shape | `crates/gaze-types/src/lib.rs:520-522,566-593` |
| `E164Phone` | `e164_phone` | `phone-parser` | Parser-backed international E.164 validity | `crates/gaze-types/src/lib.rs:523-525,566-593` |
| `PhoneNumber` | `phone_number` | `phone-parser` | Parser-backed international dialing syntax (at most 15 canonical E.164 digits) and national validity across DE, US, AT, CH, GB, IE, AU, CA, NZ, ZA and FR; national values preserve their original form; failed national parsing reports `PhoneNationalRegionMismatch` (no supported region accepted the value) | `crates/gaze-types/src/lib.rs` |
| `E164PhoneNational` | `e164_phone_national_de, e164_phone_national_us` | `phone-parser` | Parser-backed national validity for `Region::De` or `Region::Us` | `crates/gaze-types/src/lib.rs:526-528,555-593` |
| `Luhn` | `luhn` | `always` | Luhn checksum, excluding all-zero candidates | `crates/gaze-types/src/lib.rs:529-530,566-593` |
| `IbanMod97` | `iban_mod97` | `always` | IBAN MOD-97 checksum | `crates/gaze-types/src/lib.rs:531-532,566-593` |
| `Ipv4Parse` | `ipv4_parse` | `always` | Strict decimal dotted-quad IPv4 parse | `crates/gaze-types/src/lib.rs:533-534,566-593` |
| `Ipv6Parse` | `ipv6_parse` | `always` | IPv6 textual parse | `crates/gaze-types/src/lib.rs:535-536,566-593` |
| `Ipv4ParseNonDocumentation` | `ipv4_parse_non_documentation` | `always` | IPv4 parse excluding RFC 5737 documentation ranges and loopback | `crates/gaze-types/src/lib.rs` |
| `Ipv6ParseNonDocumentation` | `ipv6_parse_non_documentation` | `always` | IPv6 parse excluding RFC 3849 and embedded RFC 5737 ranges and loopback (mapped or compatible) | `crates/gaze-types/src/lib.rs` |
| `EthEip55` | `eth_eip55` | `always` | Ethereum EIP-55 checksum rules | `crates/gaze-types/src/lib.rs:537-538,566-593` |
| `AadhaarVerhoeff` | `aadhaar_verhoeff` | `always` | Indian Aadhaar Verhoeff checksum | `crates/gaze-types/src/lib.rs:539-540,566-593` |
| `FrNirMod97` | `fr_nir_mod97` | `always` | French NIR MOD-97 key | `crates/gaze-types/src/lib.rs:541-542,566-593` |
| `DeSteuerIdMod1110` | `de_steuer_id_mod1110` | `always` | German Steuer-ID MOD 11,10 | `crates/gaze-types/src/lib.rs:543-544,566-593` |
| `BsnMod11` | `bsn_mod11` | `always` | Dutch BSN 11-test | `crates/gaze-types/src/lib.rs:545-546,566-593` |
| `CpfMod11` | `cpf_mod11` | `always` | Brazilian CPF check digits | `crates/gaze-types/src/lib.rs:547-548,566-593` |
| `CnpjMod11` | `cnpj_mod11` | `always` | Brazilian CNPJ check digits | `crates/gaze-types/src/lib.rs:549-550,566-593` |
| `UkNhsMod11` | `uk_nhs_mod11` | `always` | UK NHS number MOD-11 | `crates/gaze-types/src/lib.rs:551-552,566-593` |
<!-- redaction-classes-gate:validators:end -->

A validator vetoes a failing candidate by default. `[recognizers.validator]`
accepts `on_fail = "record"` only with `iban_mod97`, `luhn`,
`de_steuer_id_mod1110`, `bsn_mod11`, `cpf_mod11`, `uk_nhs_mod11`, `e164_phone`,
`phone_number`, or `e164_phone_national_us`. The phone kinds require `phone-parser` when the
recognizer is built. A kept candidate's audit row carries
`validator_fail_reason`, and its value is never swept to other copies (see
[validator veto](../explanation/detection/validator-veto.md#recorded-failures)).
Any other validator with `on_fail = "record"`, and any value other than `veto`
or `record`, fails rulepack loading with
`RulepackError::UnsupportedValidatorOnFail`.

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

After validator veto, lower family precedence wins before generic class
priority (`crates/gaze/src/registry.rs`). IBAN 10 beats card 20; government-ID
cues order SSN 10, tax 20, national ID 30.

<!-- redaction-classes-gate:collisions:start -->
| Family | Recognizer id | Variant | Precedence | Mandatory anchor | Source |
|---|---|---|---:|---|---|
| `payment-card-or-iban` | `iban.structural` | `iban` | 10 | `iban` | `crates/gaze-recognizers/embedded/core.toml:346-350` |
| `payment-card-or-iban` | `card.structural` | `pan` | 20 | `none` | `crates/gaze-recognizers/embedded/core.toml:373-376` |
| `phone-or-imei` | `phone.structural` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:182-185` |
| `phone-or-imei` | `phone.e164.spaced` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:212-215` |
| `phone-or-imei` | `phone.e164.spaced.cued` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml` |
| `phone-or-imei` | `phone.national.de` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:246-249` |
| `phone-or-imei` | `phone.national.us` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:275-278` |
| `phone-or-imei` | `phone.national.us.cued` | `phone` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml` |
| `government-id` | `ssn.de_cue` | `ssn` | 10 | `none` | `crates/gaze-recognizers/embedded/core.toml:957-960` |
| `government-id` | `tax_number.cue_anchored` | `tax-number` | 20 | `none` | `crates/gaze-recognizers/embedded/core.toml:1024-1027` |
| `government-id` | `tax_number.labelled` | `tax-number` | 20 | `none` | `crates/gaze-recognizers/embedded/core.toml` |
| `government-id` | `national_id.cue_anchored` | `national-id` | 30 | `none` | `crates/gaze-recognizers/embedded/core.toml:1105-1108` |
| `government-id` | `id_card.labelled` | `national-id` | 30 | `none` | `crates/gaze-recognizers/embedded/core.toml` |
| `government-id` | `passport.cue_anchored` | `passport` | 15 | `none` | `crates/gaze-recognizers/embedded/core.toml` (`variant = "passport"`) |
<!-- redaction-classes-gate:collisions:end -->

For the full family-level ambiguity contract, including equal-precedence
fallback, see [Collision-Family Policy](../explanation/detection/collision-family.md).
For mandatory-anchor lookup and family-level fallback, see
[Mandatory Anchor Resolution](../explanation/detection/anchor-resolution.md).

## Full conflict-resolution order

The resolver (`crates/gaze/src/resolver.rs`) follows this order:

1. Collect locale-eligible, minimum-score candidates; veto validator failures.
2. Prefer strict same-class containers. Keep prior arbitration if the new
   selection exposes any previously covered byte; retain its audit events too.
   Exact and partial overlaps use normal arbitration.
3. Resolve other overlaps by collision-family precedence, mandatory anchors,
   containment precedence, structured containment, then generic tiers.
4. Containment precedence gives a different-class container the whole span when
   its evidence is at least the inner span's: validator-passed > anchored or
   cue-structured > plain regex/dictionary > learned NER. Ties favor the
   container. Resolve nested chains outermost-first, independent of arrival.
   Inner candidates become merged sources and loser audit rows under
   `ContainmentPrecedence`.
5. If that guard refuses, a custom-class container over a builtin
   (`Email`, `Name`, `Organization`, `Location`) still wins under
   `StructuredContainment`. Other refused containers use generic tiers.
6. Generic tiers: class priority > rule priority > score > span length >
   lexicographically smaller recognizer ID. In the prior same-class fallback,
   canonical form can decide as `Validator`, distinct from `ValidatorVeto`.
7. Remove every overlap with each winner until selections are disjoint.
8. Convert surviving candidates without required anchors to family fallbacks.

Same-class preference records `SameClassContainment`. See
[validator veto](../explanation/detection/validator-veto.md),
[collision policy](../explanation/detection/collision-family.md) and
[anchor resolution](../explanation/detection/anchor-resolution.md).

## Deterministic floor and NER-only mass

No standalone deterministic recognizer detects arbitrary streets, cities,
states, companies, first names or surnames. Rule-based `Name` coverage is limited
to email display names and locale cues; `Location` and `Organization` are NER-only.
The archived no-OPF audit found `STREET`, `CITY`, `SURNAME`, `FIRSTNAME`, `STATE`
and `COMPANYNAME` all 0-covered and 0-overlapped at the rule floor
([gap analysis](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.8-kiji-class-gap.md#L32-L58)).
Broad capitalization or common-word rules would add false positives.

## Shipped default activation

With default features (`phone-parser`) and no policy, Gaze loads `core` under
`global`, without safety nets. Setup instead enables `secrets`, Davlan NER and
Nym; OPF remains opt-in (`--safety-net openai-filter`). Eligibility still depends
on shape, cues and validation.

Format-basis recognizers run regardless of locale. Document-basis rules use
locale intersection. The deprecated `core-extended` alias auto-activates enabled,
document-basis `locale_gated` rows. `gaze_assembly::locale_gated_activation_locales`
computes the union excluding `global`, ordered `en-US`, `de-DE`, `de-AT`, `de-CH`,
then canonical tags; core also contributes `en-AU`. Custom packs can extend it.
Clean, daemon and `CorePipelineConfig` share this function.

<!-- redaction-classes-gate:default-activation:start -->
| Bundle selection | Effective locale chain | Auto-activate locale-gated | Active recognizer ids | Source |
|---|---|---|---|---|
| `core` | `global` | no | `aadhaar.in, age.cue, age.phrase, birth_date.answer, birth_date.cue, bsn.nl, card.cued, card.cued_short, card.structural, cnpj.br, cpf.br, customer_id.labelled, driver_license.cue_anchored, driver_license.labelled, email.global, email.header.name, email.header.name.paren, employee_id.labelled, eth.address, iban.cued, iban.structural, id_card.labelled, ip.v4, ip.v6, national_id.cue_anchored, nhs.uk, nino.uk, nir.fr, pan.in, passport.cue_anchored, phone.e164.spaced, phone.e164.spaced.cued, phone.national.us, phone.national.us.cued, phone.structural, postal.ca, postal.cued_four_digit, postal.cued_short, postal.gb, postal.ie, record_id.labelled, ssn.de_cue, ssn.us, steuer_id.de, tax_number.cue_anchored, tax_number.labelled, url.anchored, vat.de, vat.es` | `crates/gaze-recognizers/embedded/core.toml`; `crates/gaze-assembly/src/defaults.rs:45-77` |
| `core-extended compatibility alias` | `global, en-US, de-DE, de-AT, de-CH, en-AU` | yes | `aadhaar.in, age.cue, age.phrase, birth_date.answer, birth_date.cue, bsn.nl, card.cued, card.cued_short, card.structural, cnpj.br, cpf.br, customer_id.labelled, driver_license.cue_anchored, driver_license.labelled, email.global, email.header.name, email.header.name.paren, employee_id.labelled, eth.address, iban.cued, iban.structural, id_card.labelled, ip.v4, ip.v6, name.agent_recipient, name.auto_footer, name.forward_marker, national_id.cue_anchored, nhs.uk, nino.uk, nir.fr, pan.in, passport.cue_anchored, phone.e164.spaced, phone.e164.spaced.cued, phone.national.de, phone.national.us, phone.national.us.cued, phone.structural, postal.at_ch, postal.au, postal.ca, postal.cued_four_digit, postal.cued_short, postal.de, postal.gb, postal.ie, postal.us, record_id.labelled, ssn.de_cue, ssn.us, steuer_id.de, tax_number.cue_anchored, tax_number.labelled, url.anchored, vat.de, vat.es` | `crates/gaze-assembly/src/locale.rs` (`locale_gated_activation_locales`); `crates/gaze-assembly/src/defaults.rs:45-77`; `crates/gaze-cli/src/pipeline/run.rs:137-146,712-728` |
<!-- redaction-classes-gate:default-activation:end -->

Use `--locale=global` or a narrower policy to avoid compatibility expansion
of document-basis rules. Format-basis rules cannot be suppressed by locale;
disable the recognizer instead.

The two postal groups differ on purpose. `postal.de` and `postal.us` match bare
five-digit strings, a shape carrying no structural signal, so they stay
document-basis and locale-gated and appear only in the second row.
Their Rust-side boundary guard leaves numeric pieces inside a connected `SKU-`
stock-keeping identifier raw (ASCII case-insensitive) only when the prefix after
`SKU-` contains an alphabetic product component. Numeric-only `SKU-` forms keep
detection, as do country prefixes, postal labels and hyphenated place names.
The prefix scan is bounded
to 256 characters; a longer uncertain prefix keeps the detection. If SKU values
are part of an adopter's PII contract, protect them with a dictionary or custom
recognizer for that identifier rather than relying on incidental postal matches.
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
`postal.au` uses the Australian state or territory abbreviation and
[Australia Post's published postcode ranges](https://collectables.auspost.com.au/community-and-events/articles/postcodes-turn-50).
A preceding capitalised word of three or more letters admits prose after the
code. A listed English or German function word alone (`The`, `Our`, `Die`,
`Der`, and others) is refused, but nouns such as `Model` and `Modell` still
qualify. Without that context, punctuation, a table boundary, or the end of
input must follow. Thus CSV and table cells such as `NSW,1234` and
`| NSW | 2024 |` can be tokenized even when the number is a count or year.
Its token contains both state and postcode, which protects both parts of an
address and restores them together. Strict same-class containment prefers the AU
token under either shipped locale chain order when prior byte coverage is
preserved. `postal.at_ch` still
protects an out-of-range code or a code in a chain without `en-AU`; the state
can remain raw in those cases. The rule is document-basis `en-AU`: it runs when `en-AU` is in the
effective locale chain, including for every document under the broad setup
policy or the no-policy `core-extended` compatibility chain. An explicit
`global` or `en-NZ` only chain does not run it. New Zealand needs a separate rule because
its postcode has no Australian state abbreviation. The state range filter can
miss postcodes used across state boundaries; a `#` before a locality can still
leave an inner state and postcode match. A capitalized word before a valid
state and range can also make a non-address count look like an address, such as
`Melbourne VIC 3000 people`.
`postal.ca`, `postal.gb`, and `postal.ie` interleave letters and digits in
positions ordinary prose and identifiers do not produce, so they are
format-basis and run at every locale including `--locale=global`. An adopter
who must not tokenize Canadian, UK, or Irish postal codes cannot suppress them
with a locale chain and has to disable the recognizer.

`iban.structural` has a leading word boundary and no trailing one. After the
registry-length candidate, `word_run_extends_identifier` accepts an empty or
Unicode letters-only word run (for example a glued `BIC` label); any digit or
underscore drops the candidate as a possible longer identifier. IBANs glued to
digits or underscores can therefore stay raw. A valid prefix occurs in about
1/97 random registry-shaped values; accepting every prefix would add false
positives. Synthetic evidence:
`scripts/bench/iban_trailing_word_enumeration.py` (200k tokens; the holdout,
A4 and docs were byte-identical under either boundary).

A spaced German IBAN glued to `BIC` can split around `phone.national.de`'s
`0532 0130` match: tokenize phone to cover every byte with residual fragments;
preserve phone and the IBAN may stay raw. Compact German forms tokenize whole.

## Residual coverage

`PipelineBuilder::build` enables residual coverage by default for library,
assembly, clean and daemon pipelines. No activation flag is needed.

### What it covers

After resolution, residual coverage replaces protected candidates' bytes that
no protective winner covers (`crates/gaze/src/pipeline/residual.rs`).

- Admit each original independently only when its class and standalone fallback
  (including family fallback) both preview a protective action.
- A preserved winner leaves other bytes raw, but protected claims inside it emit
  with `decided_by: protection_override`. Its represented originals, same-span
  merges and tied family rivals cannot override it; explicit family `preserve`
  still leaves ambiguity raw.
- Merge adjacent cells of one representative/class into one fragment per run.
- Apply the claimant's own action: tokenize/format-preserve make a reversible
  class token; redact writes `[REDACTED:<class>]`; generalize writes a class label.
- Bytes outside the original evidence union remain raw. For a password original
  at `0..21` and Name winner at `0..15`, residual protects `15..21`, not a closing
  quote at byte 21. The real-pack geometry is pinned by
  [explicit_field_collision_control.rs](../../crates/gaze-recognizers/tests/explicit_field_collision_control.rs).

Rules with no static preview (an adopter `Rule` impl that answers at runtime
only) stay on the legacy path: such a selection blocks the sweep and its
runtime verdict is not second-guessed; such an original is not admitted.

### What changes in the token stream

One value can produce several replacements in partial overlaps or inside a
preserved winner. Manifest and `BundleReport::pii_token_count` counts therefore
measure replacements, not entities. Check
[`EmittedTokenOrigin`](metrics.md#52-per-call-output) before counting entities.
Tokenize/format-preserve fragments restore exactly; redact/generalize are one-way.

The current `bundle-tokenization-drift` corpus has only contained overlaps, so
residual coverage does not move its snapshot. Adding partial overlaps requires
`--verify-ack`.

### Provenance

A residual is traceable like any other emission. Its audit row carries the
representative parent original's `source` and `recognizer_id` /
`recognizer_version_id`, with `provenance_stage = "primary_pipeline.residual"`
(`log_residual_entry` in `crates/gaze/src/pipeline.rs`). In the protection trace
it projects to the existing `primary_pipeline` / `policy` / `tokenize` tuple,
which does not on its own certify a whole entity.
