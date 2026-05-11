# Validator Veto

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

`resolver::resolve_candidates` is unchanged. Existing
`ConflictTier::Validator` still means the same-class containment tie-breaker.
`ConflictTier::ValidatorVeto` is only used for this pre-resolver drop.

## Audit Shape

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

## Type Ownership

`ValidatorKind`, `ValidatorOutcome`, and `ValidatorFailReason` live in
`gaze-types` because both recognizers and the core pipeline consume them.
`gaze-recognizers` re-exports `ValidatorKind` and `Region` for source
compatibility.

`ValidatorFailReason` is a closed typed image of current validators:

- `EmailRfcRejected`
- `PhoneE164Rejected`
- `PhoneNationalRegionMismatch`
- `LuhnFailed`
- `IbanMod97Failed`
- `Ipv4ParseFailed`
- `Ipv6ParseFailed`
- `EthEip55ChecksumFailed`

Phone reasons are always present in the type. They are emitted only when the
`phone-parser` feature makes the corresponding validators available.

## North-Star Fit

- **Axis 1, reliability:** invalid validator-backed candidates still fail
  closed before token emission.
- **Axis 2, reversibility:** vetoed candidates never touch the manifest or
  token session, so restore behavior is unchanged.
- **Axis 4, auditability:** previously silent drops now produce typed
  loser-only audit rows.

## Audit Volume

This stage intentionally increases audit volume. Any invalid validator-backed
shape that was previously dropped inside `RegexDetector` now emits one
`validator_veto` row. Adopters with high invalid-candidate rates should expect
redaction logs to grow in proportion to those rejects.

## Non-Goals

- Safety nets remain observer-only and post-clean. They do not participate in
  validator veto.
- The resolver signature and conflict-resolution order are unchanged.
- Raw document shape, clean document shape, manifest token format, and restore
  behavior are unchanged.
