# Validator veto

Validator veto is the pre-resolver stage that turns validator-backed recognizer
failures into typed audit metadata. It replaces the old regex self-drop path:
recognizers now emit shape matches, and the core registry decides whether a
validator-backed candidate survives to conflict resolution.

## Contract

The stage runs inside `RecognizerRegistry::detect_all_resolved` after locale and
minimum-score filtering and before `resolver::resolve_candidates`. Its input is
the candidate list, the registry, and the normalized text used for matching.

For each candidate:

1. Look up `candidate.recognizer_id` in the registry's recognizer-id index.
2. Call `Recognizer::validator_kind()`.
3. If the recognizer has no validator, keep the candidate. No audit row is
   emitted for this `NotApplicable` path.
4. If a validator exists, re-slice the matched bytes from normalized input and
   call `ValidatorKind::validate`.
5. `ValidatorOutcome::Pass { canonical_form }` keeps the candidate and fills
   `candidate.canonical_form` only when it was absent.
6. By default, `ValidatorOutcome::Fail { reason }` removes the candidate before conflict
   resolution and returns `VetoedCandidate { candidate, reason }` for audit
   emission.
7. An explicit `on_fail = "record"` (`Recognizer::validator_on_fail`) keeps
   failures only for `iban_mod97`, `luhn`, `de_steuer_id_mod1110`, `bsn_mod11`,
   `cpf_mod11`, `uk_nhs_mod11`, `e164_phone`, or `e164_phone_national_us`. The last two require
   the `phone-parser` feature. A kept candidate carries
   `validator_fail_reason = Some(reason)` and `EvidenceKind::Learned`.

## Recorded failures

A mistyped or masked IBAN or card number is still someone's financial data,
so Gaze tokenizes an IBAN- or card-shaped span even when mod-97 or Luhn fails
(user ruling 2026-09-27). The checksum stops being the
precision; shape and context take its place:

| Recognizer | What tokenizes without a passing checksum |
| --- | --- |
| `iban.structural` | A registry country code at that country's exact ISO 13616 length, with or without a cue |
| `iban.cued` | A real ISO 3166-1 country code (or `UK`) outside the IBAN registry, two digits and a BBAN (up to four letters then 6 to 26 digits compact, or three to eight digit-bearing groups) within 32 characters after the word `IBAN` on the same line through the shared cue window (one `:`, `,` or `=` right after the cue, one nested JSON key such as `{"number": "`, or one `label:` after a copula or parenthetical; any other `.`, `;`, `!`, `?`, `:`, `,` or `=` ends it). Registry countries stay with `iban.structural`, which knows their exact length, so a registry IBAN with a dropped digit is not covered |
| `card.cued` | A card layout within 32 characters after a card cue (`card` family, German card compounds, a bare `Karte` only with `Nummer`/`Nr`, card brands) on the same line through the shared cue window (one `:`, `,` or `=` right after the cue, one nested JSON key such as `{"number": "`, or one `label:` after a copula or parenthetical; any other `.`, `;`, `!`, `?`, `:`, `,` or `=` ends it): 4-4-4-4-3 (whole), 4-4-4-4, 4-6-5, 4-6-4, compact 16 to 19 digits starting 2-6, or compact 14 to 15 digits starting 3. Compact phone numbers and epoch-millisecond timestamps do not qualify. A span that holds a card stays whole, so a cued 4-4-4-4-3 number is one token even when its first 16 digits pass Luhn (without a cue `card.structural` still keeps a valid card's CVV outside). A Luhn-failing 13- or 15-digit compact card not starting with 3 stays raw (phone and timestamp tradeoff) |

Steuer-ID, BSN, CPF, and NHS rules require a class-specific label. Their
checksum failures now stay as tokens with typed failure reasons; all-zero
Steuer-ID, BSN, and NHS placeholders are excluded. The `Fahrzeug-Identifikationsnummer`
vehicle label cannot trigger the Steuer-ID or national-ID rule. The two cued
phone rules require a same-line phone label and capture only the number. They
keep regional parser failures under English and loaded locale phone-label
buckets. A German national number without `+49` still uses
`phone.national.de` and keeps its parser veto. There is no separate cued
relaxation for that rule.

`card.structural` keeps vetoing a Luhn failure. It offers every digit run in
the text, and without a cue a 16-digit run is as likely an order, voucher or
tracking number. `RegexDetector::with_validator_on_fail` refuses
`on_fail = "record"` on such a card-run recognizer. The rulepack loader uses
the explicit validator allowlist above (`RulepackError::UnsupportedValidatorOnFail`).
The bundled uncued phone and other checksum-backed rules keep their vetoes.

A kept failure stays traceable and contained:

- The winner's audit row carries its typed `validator_fail_reason` with
  `conflict_loser: false`.
- Its evidence is `Learned`, so the repeat-value sweep never copies the value
  to an uncued occurrence. The resolver and the sweep treat a candidate as
  validated only when it has a canonical form and no recorded failure; the
  IBAN normalizer's canonical form alone proves nothing.
- A merge of two candidates over the same span keeps the recorded failure.

The restore-boundary outbound scan (`structural_findings` in `session.rs`)
still reports only mod-97-valid IBANs and Luhn-valid cards.

`ConflictTier::Validator` means the same-class containment tie-breaker when
the byte-coverage safeguard retains prior arbitration.
`ConflictTier::ValidatorVeto` is only used for this pre-resolver drop.

## Loopback addresses

The bundled IP validators (`ipv4_parse_non_documentation`,
`ipv6_parse_non_documentation`) also reject loopback addresses:
`127.0.0.0/8`, `::1`, and IPv4-mapped (`::ffff:127.0.0.1`) or
IPv4-compatible (`::127.0.0.1`) loopback. A loopback address never leaves
the host, so it identifies no person or device. Each rejection writes the
usual loser row with `Ipv4LoopbackRange` or `Ipv6LoopbackRange`.
Link-local and private addresses stay protected.

**Why there are no other lookalike vetoes.** Skipping a postcode- or
phone-shaped value because it *looks* benign (an order-number tail, an
amount, a fragment of a long digit run) needs proof that the value is not
PII. A missing label proves nothing: markup, invisible characters, other
languages, typos and encodings all hide labels from a finite cue list, and
two valid phone numbers joined in one long digit run defeat a run-length
bound. Such vetoes were tried and removed. The future path is an
adopter-declared benign reference pattern in policy, a positive signal from
the data owner.

## Audit shape

The pipeline logs one loser-only `RedactionEntry` per vetoed candidate:

```rust
RedactionEntry {
    conflict_loser: true,
    decided_by: ConflictTier::ValidatorVeto,
    validator_fail_reason: Some(reason),
    ..
}
```

For a vetoed candidate, no token or manifest entry is created. A recorded
failure instead emits a token and a manifest entry that restores exactly; its
winner audit row carries the typed reason. Audit rows are metadata-only: source, class, action,
document kind, conflict tier, session id, and typed failure reason. Raw matched
bytes never enter the audit entry.

## Type ownership

`ValidatorKind`, `ValidatorOutcome`, and `ValidatorFailReason` live in
`gaze-types` because both recognizers and the core pipeline consume them.
`gaze-recognizers` re-exports `ValidatorKind` and `Region` for source
compatibility.

`ValidatorFailReason` is a closed typed image of current validators:

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

Phone reasons are always present in the type. They are emitted only when the
`phone-parser` feature makes the corresponding validators available.

## North-star fit

- **Axis 1, reliability:** the cued rules protect failed values while uncued
  scanners retain their vetoes.
- **Axis 2, reversibility:** kept failures restore exactly from the manifest;
  vetoed candidates never enter it.
- **Axis 4, auditability:** vetoes have typed loser rows and kept failures have
  typed winner rows.

## Audit volume

This stage intentionally increases audit volume. A failed validator-backed
shape emits a typed loser row when vetoed or a typed winner row when an opted-in
rule keeps it. Adopters with high invalid-candidate rates should expect
redaction logs to grow in proportion to those decisions.

## Non-goals

- Safety nets remain observer-only and post-clean. They do not participate in
  validator veto.
- The resolver signature and conflict-resolution order are unchanged.
- Raw document shape, clean document shape, manifest token format, and restore
  behavior are unchanged.
