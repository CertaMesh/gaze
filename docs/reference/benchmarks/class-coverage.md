# Shipped-class benchmark coverage

Generator v13 adds 680 project-authored synthetic documents per partition:
250 positive documents in layer A, 30 name-thread documents in layer R and
400 benign counterweights in layer D. The generator, templates, seeds and gold
span construction live in `scripts/bench/agentic_layers.py`. The data is
released under CC0-1.0. No external personal records or working credentials
are used. Synthetic checksum validity does not imply allocation to a person,
company or account; these values must never be used as real identifiers.

The six previously empty classes are Aadhaar, CNPJ, Ethereum address, French
NIR, Indian PAN and VAT ID. VAT has separate German and Spanish families.
Each family has ten values in prose, log fields and JSON, paired with an
inventory/batch reference carrying the same shape. Aadhaar, CNPJ, NIR and
Ethereum additionally have same-length checksum-failing layer D twins.
The benign context, rather than the presence of digits, defines the reference
counterweights. Invalid checksum twins are benign here because these four
classes use strict validator vetoes; this does not change the existing
invalid-gold or gate-credit contracts for IBAN, cards or cued numeric IDs.

The added field cells cover explicit passwords, fake AWS and GitHub access
credentials and complete three-segment JWTs, German postcodes and spaced, cued Ofcom drama-range phones.
Password policy examples, incomplete credential fragments, batch codes and
unassignable country-code versions provide their benign twins. Names in
forwarded messages, agent recipient instructions and parenthesized sender
headers are exact given-name/surname gold, repeated in a response line;
roles and automated queues are benign. Email values use `.invalid` domains.
German postcodes use the unassigned `00xxx` range. Phone values use the
Ofcom `+44-7700-900xxx` drama reservation; alternate cued gold values replace
its country code with the unassigned `+28` to measure recovery of explicitly
labelled, erroneous personal numbers. These are unchecked phone gold, not
claims of valid E.164 allocation, and remain scored under both gate contracts.

## Independent format sources

Values are derived from public definitions, not production recognizer regexes.
Exact citation URLs are pinned in [class-coverage-sources.json](class-coverage-sources.json):

- Aadhaar: UIDAI authentication specification, twelve digits with Verhoeff error detection. The arithmetic implementation includes the independent `2363` checksum example.
- Numeric CNPJ: Receita Federal MOD-11 specification, twelve payload digits and two check digits, in bare and conventional `XX.XXX.XXX/XXXX-XX` presentations (the latter also specified by ANVISA public form inputs). This family covers the shipped numeric class; the newer alphanumeric CNPJ format is a separate coverage extension.
- Ethereum: ERC-55, twenty bytes rendered as forty hexadecimal characters, with Keccak-256 case checksum. Seeded addresses include mixed, all-lower and all-upper case. They are invented test-network addresses, never copied account addresses. Tests verify the arithmetic against ERC-55's published vectors.
- NIR: Service Public's field definition and the government's MOD-97 key specification. The fictional foreign birthplace `99000` avoids copying an allocated person record.
- PAN: Income Tax Department's ten-character format. These are generated layout examples, not claims of issuer-valid PAN checksums.
- VAT: BZSt's EU format table and Agencia Tributaria's corporate NIF definition. German `DE` plus nine digits is layout-only, as the shipped rule has no validator. Spanish corporate NIF includes its numeric control digit.
- Email headers: RFC 5322, mailbox comments and display names. Forwarding and recipient wording comes from the shipped declarative locale cue definitions, rather than their matching regexes.
- Credentials: AWS access key prefixes, GitHub token prefixes and RFC 7519. JWT claim JSON and non-working signature bytes are generated locally.
- Phones: Ofcom drama reservations and the ITU-T E.164 list marking the `28x` codes as spare.

## Measurement and updates

The agentic scored-label contract declares every added label, including
credentials. Main-corpus contracts v1–v4 retain their existing rules; the
agentic contract scores these new gold values in each combined run. Do not
change historical corpus files or relabel prior measurements. Versions 3–12
are rebuilt by filtering the `coverage_` surface prefix; their byte hashes
and generator-v12 contract are frozen and tested in both partitions.

A document's German language metadata does not override a policy's locale
order. Measure `postal.de` source attribution separately with a copy of the
release's setup policy whose locale order starts with `de-DE`; retain the
ordinary setup policy for comparable release headline measurements. An
`en-US`-first policy can assign the same ZIP span to `postal.us` before
`postal.de` becomes a candidate. Record this activation limit explicitly.

The reproducible value-free attribution probe is
`scripts/bench/probe_postal_coverage.py --binary <clean_for_bench> --policy
<setup.toml> --de-policy <de-first.toml> --output <evidence.json>`. It checks
that the two policies differ only in locale order, rejects refused or missing
responses and records offsets and source IDs for ten positives and ten twins.
Add `--all-new-cells` to record all 680 new cells under the ordinary setup
policy as well as the German-first postal subset. Credentials require a
policy that enables secrets; zero source attribution under an older release
policy is reported as an activation limit.
Attribution is separate from recall and precision: a postcode-shaped batch
reference can still be protected and count as a false positive.

For any future extension: use an independent format source, allocate templates
and values to dev/test before generation, add positive and benign twins, bump
the version, freeze the preceding contract, and verify every old corpus hash.
Remeasure all displayed release tags with the current harness and each tag's
own detection code, retaining observations and scorecards. Detection changes
remain subject to the paired benchmark gain gate; adding data is not evidence
that a recognizer successfully covers it. A separate per-rule CI coverage gate
is outside this generator change.
