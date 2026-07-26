# English/German negative-corpus annotation contract

## Purpose

This corpus measures false positives in Gaze's English/German whole-pipeline
benchmark. It contains only documents whose oracle contains zero PII spans.
For scoring, every document is an independent text input and any span protected
by Gaze is a false positive.

The committed corpus is
[`crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl`](../../../crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl).
It is project-authored synthetic material released as CC0-1.0; no text is
copied from a person, customer record, production system, or third-party
dataset.

## Annotation boundary

A span is personal PII only when the text presents it as identifying, locating,
contacting, authenticating, or linking to a natural person. An identifier shape
alone is not enough: the document must supply a personal referent or a valid
person-linked value. This corpus deliberately supplies neither.

The following are annotated as non-PII:

- public cities, countries, standards bodies, and other organizations when the
  sentence refers only to the public entity;
- generic titles and roles such as a reviewer or technical role when no
  individual is named or implied;
- dates, times, prices, measurements, software versions, source syntax, and log
  syntax that describe a benchmark sample rather than a person;
- reserved documentation network resources and example domains that cannot
  designate a deployed personal endpoint;
- synthetic workflow identifiers that have no customer, account, device, or
  other personal linkage; and
- syntax lookalikes that are deliberately invalid under the relevant validator
  and are explicitly unassigned.

The annotation would change if future editing added a personal referent, a real
contact endpoint, a valid person-linked account value, or any external data.
Such an edit requires replacing the fixture or adding an oracle span; it must
never be silently accepted as negative-only data.

## Why every hard-negative family has zero PII

| Category | Covered hard-negative families | Zero-PII rationale |
|---|---|---|
| `temporal_numeric` | Dates, times, prices, measurements, version numbers | Values describe an explicitly numbered benchmark measurement and software package, with no natural person or personal event. |
| `documentation_network` | RFC 5737 IPv4, RFC 3849 IPv6, example domains | The IP ranges and domains are reserved for documentation. The sentence explicitly treats them as documentation, not as a person or deployed endpoint. |
| `code_log_syntax` | Source code, logs, UUID-shaped trace values, hashes, package versions | Values are generated from the corpus seed and label only a synthetic cache component or package. They have no user, device, or production-record association. |
| `public_entities` | Public city, country, and organization names | Sentences assert only public geographic or organizational facts and explicitly deny personal affiliation. A public entity is not a natural-person identifier in this context. |
| `generic_roles` | Titles, professions, operational roles | Each role denotes a function shared by an unspecified actor; no name, contact value, or unique individual appears. |
| `commerce_identifiers` | Order-, invoice-, stock-, and batch-like identifiers | Keys are marked synthetic and unassigned and have no customer, recipient, account, delivery, or device linkage. The benchmark policy therefore treats them as non-PII workflow syntax. |
| `unicode_mixed_language` | Punctuation, Unicode, diacritics, mixed English/German words | Text consists of generic vocabulary and punctuation. Diacritics and language mixing do not create a personal referent. |
| `invalid_identifiers` | Phone-, postal-, payment-, tax-, and account-like strings | Every value contains an invalid component or checksum and is explicitly unassigned. Phone lookalikes use only the approved synthetic ranges with an alphabetic invalidator: NANPA `+1-555-01X0`, Ofcom `+44-7700-900X00`, and German `+49 1555 01122X3`. No value can be called or used as a valid postal, payment, tax, or account identifier. |

The invalid-identifier category is intentionally adversarial. A detector may
recognize its surface syntax, but the validator and surrounding non-personal
context must prevent protection. These are negative examples, not masked
positive examples.

## Record schema and stability

The file is UTF-8 JSON Lines with exactly one JSON object and one terminating
newline per document. Fields are emitted in this fixed order:

| Field | Contract |
|---|---|
| `id` | Stable unique identifier derived from language, category, and zero-padded ordinal. It does not depend on the seed. |
| `generator_id` | Versioned generator contract: `gaze-en-de-negative-v1`. |
| `seed` | Unsigned integer supplied to the generator; the committed corpus uses `0`. |
| `language` | `en` or `de`. |
| `category` | One of the eight category names in the table above. |
| `license_origin` | `project-authored-synthetic-CC0-1.0`. |
| `text` | One synthetic negative-only document. |
| `oracle_spans` | Always `[]`. A non-empty value violates this corpus contract. |

The generator emits 64 documents for every language/category pair: 1,024 total,
512 English, 512 German, and 128 in each category. Fixed iteration order, fixed
JSON field order, a trailing newline per record, and the inline SplitMix64 PRNG
make identical seeds byte-identical without an external randomness dependency.

Regenerate the canonical fixture:

```console
$ cargo run -p xtask -- generate-negative-corpus --seed 0
```

Verify it without writing:

```console
$ cargo run -p xtask -- generate-negative-corpus --verify --seed 0
```

Invariant tests reject non-empty oracle spans, unstable or duplicate IDs,
non-deterministic bytes, language imbalance, category shortfalls, and incomplete
metadata.
