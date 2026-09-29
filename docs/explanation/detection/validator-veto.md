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
   If the recognizer presents a benign-lookalike grant for its own identity,
   the document (or structured record) carries no cue and no non-Latin
   letter, the span is a match of the audited pattern, and it sits inside
   one of the grant's structures, remove it with its `Benign*` reason
   (see [Benign lookalikes](#benign-lookalikes)). Otherwise continue.
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
   `cpf_mod11`, `e164_phone`, or `e164_phone_national_us`. The last two require
   the `phone-parser` feature. A kept candidate carries
   `validator_fail_reason = Some(reason)` and `EvidenceKind::Learned`.

## Recorded failures

A mistyped or masked IBAN or card number is still someone's financial data,
so Gaze tokenizes an IBAN- or card-shaped span even when mod-97 or Luhn fails
(user ruling 2026-09-27, solo todo 3906). The checksum stops being the
precision; shape and context take its place:

| Recognizer | What tokenizes without a passing checksum |
| --- | --- |
| `iban.structural` | A registry country code at that country's exact ISO 13616 length, with or without a cue |
| `iban.cued` | A real ISO 3166-1 country code (or `UK`) outside the IBAN registry, two digits and a BBAN (up to four letters then 6 to 26 digits compact, or three to eight digit-bearing groups) within 32 characters after the word `IBAN` on the same line through the shared cue window (one `:`, `,` or `=` right after the cue, one nested JSON key such as `{"number": "`, or one `label:` after a copula or parenthetical; any other `.`, `;`, `!`, `?`, `:`, `,` or `=` ends it). Registry countries stay with `iban.structural`, which knows their exact length, so a registry IBAN with a dropped digit is not covered |
| `card.cued` | A card layout within 32 characters after a card cue (`card` family, German card compounds, a bare `Karte` only with `Nummer`/`Nr`, card brands) on the same line through the shared cue window (one `:`, `,` or `=` right after the cue, one nested JSON key such as `{"number": "`, or one `label:` after a copula or parenthetical; any other `.`, `;`, `!`, `?`, `:`, `,` or `=` ends it): 4-4-4-4-3 (whole), 4-4-4-4, 4-6-5, 4-6-4, compact 16 to 19 digits starting 2-6, or compact 14 to 15 digits starting 3. Compact phone numbers and epoch-millisecond timestamps do not qualify. A span that holds a card stays whole, so a cued 4-4-4-4-3 number is one token even when its first 16 digits pass Luhn (without a cue `card.structural` still keeps a valid card's CVV outside, todo 3843). A Luhn-failing 13- or 15-digit compact card not starting with 3 stays raw (phone and timestamp tradeoff) |

Steuer-ID, BSN, and CPF rules already require a class-specific label. Their
checksum failures now stay as tokens with typed failure reasons; all-zero
Steuer-ID and BSN placeholders are excluded. The `Fahrzeug-Identifikationsnummer`
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

## Benign lookalikes

Some weak rules match a shape, not a meaning. A benign-lookalike veto drops
such a candidate only when **the value itself** proves it is not PII,
never because no label was found:

- **Loopback IP addresses** (`127.0.0.0/8`, `::1`, IPv4-mapped loopback)
  never leave the host, so they identify no person or device. The bundled
  IP validators reject them with `ipv4_loopback_range` /
  `ipv6_loopback_range`. Link-local addresses stay protected, because their
  interface ID can be derived from a MAC address.
- **`digit_run_fragment`:** a phone candidate that is a strict part of one
  same-separator digit run holding more than 15 digits cannot be a phone
  number, because no E.164 number is longer. The part of a 16-digit
  product code such as `0593-9506-3395-7573` that looks like a phone
  number is dropped with `benign_digit_run_fragment`. Any other
  recognizer that claims the run (card, IBAN, account) is untouched. The
  bundled `phone.national.de` and `phone.national.us` declare it:

```toml
[recognizers.context]
benign_lookalikes = ["digit_run_fragment"]
```

**Reference numbers and amounts are not vetoed.** The tail of
`ORDER-2026-90210` or the `22186` in `EUR 22186,12` looks like a postcode,
and nothing in the value says it is not one. Only a missing label could
call it benign, and a missing label proves nothing: a finite cue list
misses labels in markup (`Ph<b>one</b>`), zero-width or decomposed
characters, other languages (`Kod pocztowy`, `Puhelinnumero`), typos
(`Phne`) and encodings (`%70%68%6F%6E%65`, base64). Earlier drafts of this
veto skipped those shapes and each review found a labelled value that
leaked. They now tokenize like any other postcode- or phone-shaped value.
The path for tenant-specific reference formats is an adopter-declared
benign pattern in policy: a positive signal from the data owner, not an
inference from absent labels (todo 4005).

The check runs in this stage, before conflict resolution, because a
benign candidate that *won* a conflict could not help: residual admission
re-protects every byte a protective candidate claimed, even inside a
`preserve` winner. These rules keep a real value protected:

- **Only the declaring rule's candidate goes.** Any other candidate over the
  same bytes is untouched and still protects them.
- **A cue anywhere, or any non-Latin letter, still keeps the veto off.** As
  extra caution on top of the value-level proof, validator veto scans the
  whole input with `gaze_types::benign_lookalike::CueEvidence`, and for a
  `RawDocument::Structured` value the whole record (every key and every
  string value at every depth, no length cap). A phone, postal or address
  cue word anywhere, or any letter outside Latin script, disables the
  veto for that document. Before splitting words the scan removes
  markup-like tags (`<b>`, `</span>`) and reads zero-width characters, soft
  hyphens and combining marks as nothing, so `Ph<b>one</b>`, `Ph​one` and a
  decomposed `Tél` count. Encoded text (percent-encoding, base64) is not
  decoded. Cue words are case- and accent-folded, split at non-letters and
  camelCase, and cover English, German, French, Spanish, Italian, Dutch and
  Portuguese stems; the list is shared with the benchmark generator
  (`scripts/bench/lookalike_cue_vocabulary.json`, exact-equality test), and
  every bundled `[locale.phone_labels]` entry must be a phone cue. This
  block never makes a veto safe on its own; the value-level proof does.
- **Only the audited bundled rules are eligible, by exact tuple.** A veto
  needs a `gaze_types::benign_lookalike::BenignLookalikeGrant`. Its only
  constructor, `BenignLookalikeGrant::audited`, mints it when the rule's
  whole tuple (id, class, pattern, capture groups, validator and failure
  mode, locales, locale basis, structures) hashes to one of two compiled-in
  fingerprints: `phone.national.de` and `phone.national.us`, exactly as
  bundled. This is exact-tuple eligibility, not authenticated provenance: a
  caller that builds an identical rule gets a grant, and that rule is then
  exactly the audited one. Validator veto also checks that the recognizer
  presenting the grant has that identity and that the vetoed span is a
  match of the audited pattern in the input. So a rule that reuses an
  audited id with another pattern (even through `Rulepack::parse_bundled`)
  fails to build, and a custom `Recognizer` that borrows a real grant
  vetoes nothing the audited rule would not. The rulepack loader also
  refuses the key in rulepack files and for other ids with
  `RulepackError::IneligibleBenignLookalike`. A custom pack forked from
  `core` must drop its `benign_lookalikes` lines.
- **Every veto is audited.** Each vetoed candidate writes one loser row
  with its reason; a veto that cannot be placed on the source text fails
  the document (`Error::UnauditableVeto`) rather than dropping its row.

A test pins the bundled opt-in set (`phone.national.de` and
`phone.national.us`, `digit_run_fragment` only); extending it needs a
leak-direction review.

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

A benign-lookalike or loopback veto writes the same row with its `Benign*`
or `*LoopbackRange` reason, one row per vetoed candidate.
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
- `BenignDigitRunFragment`

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
