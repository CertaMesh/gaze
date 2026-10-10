# gaze-recognizers

[![Crates.io](https://img.shields.io/crates/v/gaze-recognizers.svg)](https://crates.io/crates/gaze-recognizers)
[![docs.rs](https://docs.rs/gaze-recognizers/badge.svg)](https://docs.rs/gaze-recognizers)
[![License](https://img.shields.io/crates/l/gaze-recognizers.svg)](https://github.com/CertaMesh/gaze#license)

Built-in `gaze::Recognizer` backends: regex, dictionary, anchored names, and ONNX NER.

## Cargo

```toml
[dependencies]
gaze-pii = "0.16.0"
gaze-recognizers = "0.16.0"
```

Library users that need parser-backed E.164 phone validation must opt in to the
`phone-parser` feature:

```toml
[dependencies]
gaze-recognizers = { version = "0.16.0", features = ["phone-parser"] }
```

`gaze-cli` enables `phone-parser` by default. Without the feature, the
rulepack loader rejects `e164_phone` at load time with
`RulepackError::UnsupportedValidator`, preserving the axis-1 fail-closed
posture rather than silently degrading to shape-only matching.

## Public entry points

The public surface is re-exported from [`src/lib.rs`](src/lib.rs).

| Backend | Public types |
|---------|--------------|
| Regex | `RegexDetector`, `NormalizerKind`, `ValidatorKind` |
| Dictionary | `DictionaryRecognizer` |
| Anchored match | `AnchoredMatchRecognizer`, `AnchoredBoundary`, `NameShape`, `CuePosition` |
| NER | `NerRecognizer`, `NerDetector`, `NerOptions`, `NerLoadError`, `NerBackendKind`, `LabelMap`, `VerifiedArtifacts` |
| Rulepacks | `embedded(name)` |

## Regex backend

`RegexDetector` can be constructed directly:

```rust
use gaze_recognizers::RegexDetector;

let recognizer = RegexDetector::emails()?;
```

Use `RegexDetector::emails()` for the built-in email recognizer. Rulepack
assembly uses `RegexDetector::with_rulepack_fields` so locale tags, scores,
priorities, token families, capture groups, exclusions, validators, and
normalizers can flow from TOML rulepacks into the registry.
Optional rulepack-only `context.reject_match_regex` checks the original regex capture and
label, never value groups found later by the scanner. A guard match produces an audit veto row;
an invalid guard fails pipeline assembly. Optional regex `match.complete_labelled_value` extends
a captured identifier through adjacent value groups. It stops at dates, field cues, and values
claimed by another class. The bundled tax, driver-licence and national-ID fallback rules enable
it. `labelled_value_scan_reason` records `limit_exceeded`, `date_boundary`, `label_boundary`, or
`other_class_boundary` in the redaction audit and SQLite export; it contains no value bytes.
Ambiguous `US`/`UK` prefixes stay in the protected value when no second recognizer claims the
following digits, so a missing locale arm does not leave an identifier raw.

`ValidatorKind` and `NormalizerKind` are closed sets. The complete variant
lists, rulepack spellings, feature gates and fail-closed wiring stage are in
[Closed validator and normalizer sets](../../docs/reference/redaction-classes.md#closed-validator-and-normalizer-sets).
Examples: `ValidatorKind::Luhn` (Mod 10 checksum, used by `card.structural`),
`ValidatorKind::IbanMod97` (ISO 7064 mod-97, used by `iban.structural`) and
`ValidatorKind::E164Phone` (requires the `phone-parser` feature).

`E164Phone` is implemented via the `phonenumber` crate. It preserves valid E.164
matches such as the fictional UK `+44 7700 900123`
while rejecting regex-passing but unassigned shapes such as `+99999999`.

## Anchored match backend

`AnchoredMatchRecognizer` is the v0.6 structural-context backend for
cue-anchored person-name detection. Bundled `core` uses it for
`name.forward_marker`, `name.agent_recipient`, and `name.auto_footer`; the
`locale-de` and `locale-en` bundles provide the `forward_markers`,
`agent_recipient_cues`, and `footer_cues` cue buckets.

Use it when a name appears near deterministic prompt or email structure, not
as a replacement for NER over general prose. Known limits and adopter migration
notes live in
[`docs/reference/policy.md#known-limits---ner-and-prompt-shape`](../../docs/reference/policy.md#known-limits---ner-and-prompt-shape).

## Dictionary backend

`DictionaryRecognizer` detects tenant or rulepack dictionaries supplied through
`gaze::DictionaryBundle` and `gaze::DetectContext`.

Use it for bounded adopter-specific PII such as order IDs, account handles,
internal project names, song titles, or artist names. The recognizer stores a
dictionary name and reads the actual terms from runtime context, policy, or
rulepack assembly.

Use adopter custom recognizers instead when the detector needs external
services, private model code, or domain-specific scoring that should not ship
as a built-in backend.

## NER backend

`NerRecognizer` loads a verified ONNX bundle with `NerOptions`; `NerDetector` exposes the backend. Dependencies are `ort`, `tokenizers`, and `ndarray`. Policy `[ner].model_dir` enables loading through `gaze-assembly`; loading failures are CLI policy errors. The production model family is Davlan mBERT NER.

## Embedded rulepacks

`embedded(name)` returns bundled TOML contents for known rulepack names:

| Name | File | Purpose |
|------|------|---------|
| `core` | [`embedded/core.toml`](embedded/core.toml) | Unified bundled recognizer set. Email/name, parser-backed phone, IBAN, payment-card, IP, ETH, and postal recognizers now live in one bundle. Each recognizer declares `safety_tier = "safe_default"`, `"locale_gated"`, or `"opt_in"` and `locale_basis = "document"` or `"format"`. Format-basis recognizers ignore the document locale for eligibility. |
| `core-extended` | alias of `core` | Deprecated since v0.8.0 and still accepted. CLI use emits a warning and preserves v0.8.x compatibility by auto-activating locale-gated recognizers. |
| `secrets` | [`embedded/secrets.toml`](embedded/secrets.toml) | Credential recognizers (`security_token.anchored`, `password.field`). `gaze setup` includes this bundle; direct library callers load it by name next to `core`. |
| `locale-de` | [`embedded/locale-de.toml`](embedded/locale-de.toml) | DACH locale metadata such as German email headers. |
| `locale-en` | [`embedded/locale-en.toml`](embedded/locale-en.toml) | English locale metadata such as English email headers. |

The loader returns `None` for unknown names. Policy/CLI callers should treat
unknown bundled names as configuration errors.

Official bundled recognizers must declare `locale_basis` explicitly. External
rulepacks that omit it keep the legacy `document` default. With `format`,
`locales` records identifier-format provenance; it is not an eligibility gate.
Disable a format-basis recognizer itself when suppression is intentional.

## Recognizer-level metadata (v0.7.2)

### Collision-family policy

```toml
[recognizers.collision]
family = "payment-card-or-iban"
variant = "iban"
precedence = 10
```

Adopter custom recognizers use `[policy.custom_recognizers.collision]`. `FamilyPolicyTable` keys entries by stable recognizer id. Validator veto runs first; `ConflictTier::CollisionPolicy` resolves same-family, different-variant overlaps. Equal precedence emits `PiiClass::Custom("family:<name>")` with `AmbiguityRecord::PrecedenceTie`. Adopters cannot claim reserved bundled family names. [Contract](../../docs/explanation/detection/collision-family.md).

### Mandatory-anchor resolution

Add `mandatory_anchor = "iban"` to the collision table. Locale bundles supply:

```toml
[locale.cues.iban]
names = ["IBAN", "IBAN:", "Account No."]
window_chars = 64
```

A bounded cue permits the precise variant. Missing cue or cue key emits one family token and restore mapping, `ConflictTier::AnchoredContext`, and `AmbiguityReason::NoAnchor`. `cargo run -p xtask -- locale-cue-bundle-coherence` checks bundled cue completeness. [Contract](../../docs/explanation/detection/anchor-resolution.md).

## Explicit birth-date, age, postcode and credential fields

The embedded `gaze-core` rulepack version 0.6.0 contains 50 recognizers.
`birth_date.cue` and `age.cue` are global `safe_default` rules in `core`.
`postal.cued_four_digit` is a global `safe_default` rule that needs an explicit postal label.
`password.field` ships in the separate
`secrets` bundle; `gaze setup` includes it, while hand-authored policies load it with
`bundled = ["core", "secrets"]`. The former `username.field` rule was removed
in core 0.6.0.

| Rule / custom class | Bundle | Cues and frames |
| --- | --- | --- |
| `birth_date.cue` / `custom:birth_date` | `core` | en `DOB`, `D.O.B.`, `date of birth`, `birth date`, `birthday`, `born`; de `Geburtsdatum`, `Geb.-Datum`, `geb.`, `Geburtstag`, `geboren`, trailing `am <date> geboren`; fr `né`/`née`, `date de naissance`; nl `geboortedatum`; pt `data de nascimento`; da `fødselsdato`, `fødselsdag`, `født`; es `fecha de nacimiento`, `nacido`/`nacida`, `nació` |
| `age.cue` / `custom:age` | `core` | Structured `age`, `Alter`/`alter`, `âge`, `idade`, `leeftijd` fields, person-framed `aged N`, `N years old`, `N-year-old person`, `N Jahre alt`, `âgé de N`, or a complete Portuguese `tem N anos` line; only the number is tokenized |
| `password.field` / `custom:password` | `secrets` | `password`, `passphrase`, `passwort`, `kennwort` |

Bare `aged N`, bare `N-year-old`, and mid-line `age Ny` without a person cue
remain raw because objects and durations use those shapes too. Lowercase
`alter` is accepted only as a structured key, so English `alter N lines` stays raw.

`postal.cued_four_digit` recognizes a four-digit value directly after
`postcode`, `postal code`, or `zip`/`zip code`, including structured keys.
Unlabelled street addresses and bare four-digit numbers stay raw because a
year or amount can have the same shape.

### Supported grammar

`password.field` reads a record that occupies a complete line: optional ASCII spaces/tabs, a complete cue,
optional spaces/tabs, `:` or `=`, optional spaces/tabs, a nonempty value,
optional spaces/tabs, then LF, CRLF or EOF. Adjacent records work independently.
Multiword cues use the literal spaces shown above. This is string recognition,
not JSON/schema parsing: `DetectContext.fields` remains a reserved unit value.

Password values are either an unquoted atom without whitespace,
quotes or backslashes, or paired single/double quotes around the full value.
Quoted values permit spaces and only two escape forms: escaped matching quote
and escaped backslash. Quotes and line separators stay outside the capture;
value spelling is preserved exactly, with no unescaping. Terminal punctuation
belongs to an unquoted value. Empty, malformed, multiline and overbound records
produce no candidate from these rules, never a partial value prefix. Other
recognizers can still detect content in an unsupported record.

Credential values are bounded to 1–256 normalized grammar units. A unit is
one permitted plain Unicode scalar or one supported two-scalar escape. Quoted
values can therefore contain up to 512 normalized scalars. This is neither
a raw-source size bound nor a bound on the cost of scanning a document.

`birth_date.cue` is cue-anchored, not line-anchored: a date is captured only
after a birth cue, and a date without one stays raw. The cue may be prose
(`geboren am` followed by the date), a JSON key in plain, single or backslash-escaped
quotes (a JSON `dob` key with a day-first date), or a `key=value` / `key: value` log field.
Keys may be snake, camel or kebab case (`date_of_birth`, `dateOfBirth`,
`birth-date`) and may carry an underscore prefix (`customer_dob`). Up to eight
filler tokens may sit between cue and date: separators, quotes, and short link
words such as `is`, `ist der`, `est le`, `er`, `op`, `el`, `am`, `den`, `on`.
Line breaks count as separators, so a form label on one line and the date on
the next (`Date of birth:` on one line, the date on the next) is still read as one field.
`anniversaire` and `since` are not cues: both name other dates as often as a
birth date.

Accepted dates: `YYYY-MM-DD`, `YYYY/MM/DD`, `YYYY.MM.DD`, compact `YYYYMMDD`
(19xx/20xx), `D.M.YYYY`, `D-M-YYYY`, slash dates in either day/month order,
two-digit years in those numeric forms, and month names in en/de/fr/nl/da/es
(`14 March 1987`, `March 14, 1987`, `14. März 1987`, `1er mars 1984`,
`12 de marzo de 1987`, `12-Mar-1984`). Spaces inside the date may be NBSP or
NARROW NBSP. Month/day components are structurally bounded to 1–12 / 1–31
without inferring the ordering. Calendar validity is not asserted: a declared
`31.02.1990` is still sensitive. A date glued to a longer word run (`1987x`,
`1987_2`) is not captured. Year-only and year-less dates are outside this
grammar. Pinned by `tests/birth_date_cues.rs`, which also enumerates the
retired field-record grammar and requires every value it captured to be
captured with the same span.

### Protection and policy limits

**Whole-value protection is conditional on the existing conflict winner and
caller policy.** Default assembly tokenizes these custom classes using ordinary
counter tokens. Their priority 100 exceeds embedded custom substring rules
(maximum 87). Strictly contained builtin fragments normally yield to the custom
container, but collision families and anchors are considered earlier. Same-span
Email, partial builtin overlaps and builtin containers can win. Arbitration is
unchanged; a different winner can select a different caller action.

A reproducible synthetic limitation is `password: "left right"` with a competing
builtin Name span over `password: "left`. Existing arbitration selects raw bytes
`0..15`, leaving ` right"` raw, both before and after adding these rules. The
standalone [collision control](tests/explicit_field_collision_control.rs) checks
the exact source range, manifest, merged trace sources and remaining suffix.
Collision repair is a separate scope; restoring the original text does not prove
that every sensitive byte was protected.

The unchanged pipeline normalizer removes U+200C/U+200D and maps fullwidth ASCII
before matching. Removed leading/trailing joiners may remain outside a token;
arbitrarily many interior joiners can lie inside its translated raw span.
All-joiner values normalize to empty and yield no field candidate. Captured
original bytes still restore exactly. There is no claim that all raw field bytes
are protected or that translated captures have a maximum raw length.

Explicit `Preserve`, `Redact`, `Generalize` and `FormatPreserve` policies remain
authoritative. Preserve can leave a field raw and strict admission can reject;
Redact/Generalize remain one-way and unsupported by the protection-trace entry
point. FormatPreserve relies on actual session ownership. Failed transactions
must be discarded. Configured safety nets still inspect the final output.
There is no promise of zero new strict denials or reversal of one-way actions.

Short values such as `password: x` and ambiguous declarations such as
`password: required` intentionally match. Quoted templates/prose can match too;
no entropy threshold, minimum length or stopword list establishes authenticity.
An independent declared-password candidate does not bypass a card/phone
candidate's own validator veto.

These rules add deterministic coverage for supported text fields. Synthetic
proofs do not establish a production leaked-byte reduction, zero rejects or
latency nonregression. Additional regex work has not been benchmarked here.

## Adding recognizers here

Built-in recognizers implement `gaze::Recognizer` with stable id, class, locale eligibility, score/priority, token family, validated canonical form, and audit source labels. Tenant/private-schema recognizers belong outside this crate.

```rust
fn detect(&self, input: &str, ctx: &DetectContext<'_>)
    -> Result<Vec<Candidate>, gaze_types::DetectError>;
```

Return `DetectError::backend(self.id(), message)` on failure, never an empty candidate list. The registry stops on error; `Error::RecognizerDetect` aborts outbound cleaning. Infallible detectors return `Ok(candidates)`. [Fail-closed contract](../../docs/explanation/detection/ner-failclosed.md).

Metadata, safety-net metrics, and candidate/audit linkage: [Metrics](../../docs/reference/metrics.md#4-recognizer-surface-gaze-recognizers--gaze).

## Test support

The crate has a `test-support` feature for tests that need additional support
surface without making it part of the default public runtime.
