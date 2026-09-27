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
6. `ValidatorOutcome::Fail { reason }` removes the candidate before conflict
   resolution and returns `VetoedCandidate { candidate, reason }` for audit
   emission.
7. The exception: when the recognizer declares `on_fail = "record"`
   (`Recognizer::validator_on_fail`) and the validator is `iban_mod97` or
   `luhn`, a `Fail` keeps the candidate. It carries
   `validator_fail_reason = Some(reason)` and `EvidenceKind::Learned`.

## Recorded failures: IBAN and payment cards

A mistyped or masked IBAN or card number is still someone's financial data,
so Gaze tokenizes an IBAN- or card-shaped span even when mod-97 or Luhn fails
(user ruling 2026-09-27, solo todo 3906). The checksum stops being the
precision; shape and context take its place:

| Recognizer | What tokenizes without a passing checksum |
| --- | --- |
| `iban.structural` | A registry country code at that country's exact ISO 13616 length, with or without a cue |
| `iban.cued` | A country code outside the IBAN registry, two digits and a BBAN (up to four letters then 6 to 26 digits compact, or three to eight digit-bearing groups) within 32 characters after the word `IBAN` on the same line. Registry countries stay with `iban.structural`, which knows their exact length, so a registry IBAN with a dropped digit is not covered |
| `card.cued` | A card layout (4-4-4-4, 4-6-5, 4-6-4, compact 13 to 19 digits) within 32 characters after a card cue (`card`, `Karte` compounds, card brands) on the same line; one trailing 3- or 4-digit group (a CVV) stays outside the token, and any further digits refuse the match |

`card.structural` keeps vetoing a Luhn failure. It offers every digit run in
the text, and without a cue a 16-digit run is as likely an order, voucher or
tracking number. `RegexDetector::with_validator_on_fail` refuses
`on_fail = "record"` on such a card-run recognizer, and the rulepack loader
refuses it for every validator other than `iban_mod97` and `luhn`
(`RulepackError::UnsupportedValidatorOnFail`), so a tax, national-ID or other
checksum cannot be relaxed by accident.

A kept failure stays traceable and contained:

- The winner's audit row carries `validator_fail_reason`
  (`IbanMod97Failed`, `LuhnFailed`) with `conflict_loser: false`.
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

No token is emitted, no manifest entry is created, and restore round-trip
semantics do not change. The row is metadata-only: source, class, action,
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

Phone reasons are always present in the type. They are emitted only when the
`phone-parser` feature makes the corresponding validators available.

## North-star fit

- **Axis 1, reliability:** invalid validator-backed candidates still fail
  closed before token emission.
- **Axis 2, reversibility:** vetoed candidates never touch the manifest or
  token session, so restore behavior is unchanged.
- **Axis 4, auditability:** previously silent drops now produce typed
  loser-only audit rows.

## Audit volume

This stage intentionally increases audit volume. Any invalid validator-backed
shape that was previously dropped inside `RegexDetector` now emits one
`validator_veto` row. Adopters with high invalid-candidate rates should expect
redaction logs to grow in proportion to those rejects.

## Non-goals

- Safety nets remain observer-only and post-clean. They do not participate in
  validator veto.
- The resolver signature and conflict-resolution order are unchanged.
- Raw document shape, clean document shape, manifest token format, and restore
  behavior are unchanged.
