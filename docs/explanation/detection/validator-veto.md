# Validator veto

Validator-backed recognizers emit shape matches. Core validation decides which
reach conflict resolution and records typed audit reasons.

## Contract

Inside `RecognizerRegistry::detect_all_resolved`, veto runs after locale and
minimum-score filtering, before `resolver::resolve_candidates`.

| Validation | Result |
| --- | --- |
| No `validator_kind()` | Keep; no `NotApplicable` audit row |
| `Pass { canonical_form }` | Keep; fill absent candidate canonical form |
| `Fail { reason }` | Drop by default; return `VetoedCandidate` for audit |
| Explicit supported `on_fail = "record"` | Keep failure with typed reason and `EvidenceKind::Learned` |

Lookup uses `candidate.recognizer_id`; validation re-slices normalized matching
input. Record mode supports only `iban_mod97`, `luhn`, `de_steuer_id_mod1110`,
`bsn_mod11`, `cpf_mod11`, `uk_nhs_mod11`, `e164_phone`, and
`e164_phone_national_us`. Phone validators require `phone-parser`.

## Recorded failures

IBAN/card shapes can be sensitive even with failed checksums. Their precision
comes from shape/context:

| Recognizer | What tokenizes without a passing checksum |
| --- | --- |
| `iban.structural` | A registry country code at that country's exact ISO 13616 length, with or without a cue |
| `iban.cued` | A real ISO 3166-1 country code (or `UK`) outside the IBAN registry, two digits and a BBAN (up to four letters then 6 to 26 digits compact, or three to eight digit-bearing groups) within 32 characters after the word `IBAN` on the same line through the shared cue window (one `:`, `,` or `=` right after the cue, one nested JSON key such as `{"number": "`, or one `label:` after a copula or parenthetical; any other `.`, `;`, `!`, `?`, `:`, `,` or `=` ends it). Registry countries stay with `iban.structural`, which knows their exact length, so a registry IBAN with a dropped digit is not covered |
| `card.cued` | A card layout within 32 characters after a card cue (`card` family, German card compounds, a bare `Karte` only with `Nummer`/`Nr`, card brands) on the same line through the shared cue window (one `:`, `,` or `=` right after the cue, one nested JSON key such as `{"number": "`, or one `label:` after a copula or parenthetical; any other `.`, `;`, `!`, `?`, `:`, `,` or `=` ends it): 4-4-4-4-3 (whole), 4-4-4-4, 4-6-5, 4-6-4, compact 16 to 19 digits starting 2-6, or compact 14 to 15 digits starting 3. Compact phone numbers and epoch-millisecond timestamps do not qualify. A span that holds a card stays whole, so a cued 4-4-4-4-3 number is one token even when its first 16 digits pass Luhn (without a cue `card.structural` still keeps a valid card's CVV outside). A Luhn-failing 13- or 15-digit compact card not starting with 3 stays raw (phone and timestamp tradeoff) |

Steuer-ID, BSN, CPF, and NHS require class-specific labels; failed checksums
remain tokens with typed reasons. All-zero Steuer-ID/BSN/NHS placeholders are
excluded. `Fahrzeug-Identifikationsnummer` cannot trigger Steuer-ID/national-ID.
Cued phone rules keep regional parser failures only after a same-line English
or loaded-locale phone label, capturing only the number. German national
numbers without `+49` remain under `phone.national.de` and its parser veto.

`card.structural` still vetoes failed Luhn because uncued digit runs can be
orders/vouchers/tracking IDs. `RegexDetector::with_validator_on_fail` refuses
record mode for this card-run recognizer. Unsupported validator record mode
fails load with `RulepackError::UnsupportedValidatorOnFail`. Uncued phone and
other checksum rules retain their vetoes.

Kept failures have winner audit rows (`conflict_loser: false`) and `Learned`
evidence; sweeps cannot copy them to uncued text. Validation requires canonical
form AND no recorded failure; canonical form alone is insufficient. Same-span
merges retain failures. Restore-side `structural_findings` still reports only
mod-97-valid IBANs and Luhn-valid cards.

`ConflictTier::Validator` is the same-class containment tie-breaker under the
byte-coverage safeguard. `ValidatorVeto` identifies pre-resolver drops only.

## Loopback addresses

`ipv4_parse_non_documentation` / `ipv6_parse_non_documentation` reject
`127.0.0.0/8`, `::1`, mapped `::ffff:127.0.0.1`, and compatible `::127.0.0.1`.
They write `Ipv4LoopbackRange` / `Ipv6LoopbackRange` loser reasons.
Private and link-local addresses stay protected.

Do not veto postcode/phone shapes merely because they resemble amounts, order
tails, or long digit fragments. Missing cues and run length do not prove benign
content. Benign exceptions need an adopter-declared positive policy signal.

## Audit shape

```rust
RedactionEntry {
    conflict_loser: true,
    decided_by: ConflictTier::ValidatorVeto,
    validator_fail_reason: Some(reason),
    ..
}
```

Vetoed candidates create no token/manifest entry. Recorded failures create both
and restore exactly. Audit contains only source, class, action, document kind,
conflict tier, session ID, and typed reason, never matched bytes.

## Type ownership

`gaze-types` owns `ValidatorKind`, `ValidatorOutcome`, and `ValidatorFailReason`.
`gaze-recognizers` re-exports `ValidatorKind` and `Region` for compatibility.
The closed failure reasons are:

- `LuhnFailed`
- `IbanMod97Failed`
- `EmailRfcRejected`
- `PhoneE164Rejected`
- `PhoneNationalRegionMismatch`
- `Ipv4ParseFailed`
- `Ipv6ParseFailed`
- `Ipv4DocumentationRange`
- `Ipv6DocumentationRange`
- `EthEip55ChecksumFailed`
- `AadhaarVerhoeffFailed`
- `FrNirMod97Failed`
- `DeSteuerIdMod1110Failed`
- `BsnMod11Failed`
- `CpfMod11Failed`
- `CnpjMod11Failed`
- `UkNhsMod11Failed`
- `Ipv4LoopbackRange`
- `Ipv6LoopbackRange`

Phone reasons always exist in the type but require `phone-parser` for emission.

## Audit volume

Each failed shape adds a typed loser or opted-in winner row. Log volume grows
with invalid-candidate decisions.

## Non-goals

Observer-only post-clean safety nets do not participate. Resolver signature/order,
document shapes, manifest token format, and restore behavior remain unchanged.
