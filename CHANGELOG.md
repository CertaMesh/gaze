# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- **Dates of birth after a birth cue are tokenized** (solo todo #3651).
  Every release up to and including v0.15.1 sent these raw through
  `gaze clean` and `gaze proxy` alike: `Geburtsdatum 30.05.1971`,
  `{"dob": "30.05.1971"}` in a tool result, `née le 02/11/1992`, and any
  month-name or two-digit-year date. `birth_date.cue` only read a line-start
  field record (`DOB: 1990-02-03`) and `born on` / `geboren am`.
- **House numbers beside a street the NER model found are tokenized**
  (solo todo #3670). Every release up to and including v0.15.1 tokenized
  `Musterweg` in `Musterweg 17b` and `Example Street` in `17 Example Street`,
  but sent the house number raw, because the location span ends at the
  street word.

### Changed

- **A NER street licenses the house number beside it.** After conflict
  resolution, a winning NER location whose last word is a street word of an
  active locale tokenizes the adjacent house number (`17`, `17b`, `9A`,
  `12-14`, `12/3`) as its own `location` token with recognizer id
  `address.house_number.street_corroborated`. German writes the number after
  a street ending (`-straße`, `-weg`, `-platz`, …, from
  `[locale.street_suffixes_number_after]` in `locale-de`); English writes it
  before a street type (`Street`, `Road`, `Drive`, …, from
  `[locale.street_types_number_before]` in `locale-en`). A city, a bare street
  word, a number across a line break, tab or table border, a five-digit
  number, and a decimal or time never qualify. Only policies that load a
  locale pack with these lists and run NER change: `core` alone, or a policy
  without `[ner]`, tokenizes exactly what it did before. Known limit: the
  lexicon cannot tell a street from a title the NER model mislabels as a
  location (`Chapter 12 Civil Court`), and a year right before an English
  street (`In 2019 Abbey Road …`) is tokenized.

- **`birth_date.cue` reads birth cues in prose, tool-call JSON and
  `key=value` logs.** Cues cover en, de, fr, nl, da and es (`DOB`,
  `date of birth`, `born`, `Geburtsdatum`, `geb.`, `geboren am`,
  `am … geboren`, `née le`, `date de naissance`, `geboortedatum`,
  `født den`, `fecha de nacimiento`, and more); JSON keys may be snake, camel
  or kebab case with an underscore prefix (`customer_dob`, `dateOfBirth`,
  `birth-date`). Dates may be ISO, year-first with `/` or `.`, compact
  `YYYYMMDD`, day-first with `.` or `-`, slash in either order, two-digit
  years, or month names in those six languages. A date without a birth cue
  is still left alone, so invoice, log and release dates are unchanged. Every
  value the old rule captured is still captured with the same span.

## [0.15.1] - 2026-09-26

v0.15.1 closes the v0.15.0 known limitation for payment cards next to other
digits and makes `gaze proxy` tokenize what the safety net flags instead of
refusing the request. The curated summary below leads; the full entries follow
in Keep a Changelog form.

**Security.** Every release up to and including v0.15.0 sent a payment card
number raw to the model when digits touched it: a CVV or expiry after it, an
order or year number before it, or digits glued on by normalization. This was
the v0.15.0 known limitation (solo todo #3843). The card is now tokenized on
the forward path, and the restore-boundary DLP check finds it in model output
through the same code (PR #658).

**Highlights.**

- **`gaze proxy` tokenizes safety-net findings** (PR #660). Under the policy
  `gaze setup` writes (Nym on), the proxy refused every request that held a
  date, such as `Invoice date 1971-05-30.`, with an opaque
  `500 {"error":"Pipeline"}`, while `gaze clean` tokenized the same date. The
  proxy now runs the Resolve step of
  `gaze clean --safety-net-fallback strict` on request text: a flagged span is
  forwarded as a token and restored in the response. What that step cannot
  tokenize is still refused, never deleted.
- **Proxy refusals say why** (PR #660): `422` with the typed
  `ProtectionError` variant, the fallback reason and the suspect classes,
  never the text, plus one line on the proxy's stderr.

What #660 does not change, stated so nobody reads more into it:

- Spans that no net flags and no rule detects, such as a `DD.MM.YYYY` date
  without a cue, still reach the provider raw, exactly as `gaze clean` prints
  them. The old behaviour blocked some of them only because it refused the
  whole request whenever any other date in it was flagged.
- The proxy is not plain `gaze clean`. Clean's default `redact` fallback runs
  a second reversible batch and then deletes what is left one way; the proxy
  refuses such a request instead.
- Each request pays one more Nym pass per surfaced string on the legacy
  adapters. The PR measured about +7 ms per request; the proxy latency row
  under Performance is this release's measurement.

**Breaking changes** (each has a full entry below):

- `gaze proxy` refusals are now `422`. Legacy OpenAI and Gemini adapters
  answer `{"error":"Refused","refusal":{…}}` instead of
  `500 {"error":"Pipeline"}`; the Anthropic direct profile answers code
  `ProtectionRefused` instead of `502 InvalidToken`. Clients that matched the
  old status or error name must match the new ones (PR #660).
- `gaze_proxy::DirectProxyError` is `Clone` but no longer `Copy` (PR #660).
- A policy that acts on `custom:iban` differently from the
  `family:payment-card-or-iban` class applies to about 7 % more IBANs, because
  a card candidate inside a BBAN now settles the family to `custom:iban`
  (PR #658).

**Known limitations.** These gaps ship in this release:

- **A repeat of a tokenized name can reach the model raw** (solo todo #3849).
  Gaze tokenizes each occurrence only where a recognizer fires; it does not
  carry a value it already tokenized to that value's other occurrences.
  Without NER (no policy, `core` only), a name found in an email header stays
  raw when the same document repeats it in prose. With NER (the `gaze setup`
  policy), prose repeats are caught, but names written in lowercase can leak
  whole or in fragments.
- **UK national-format phone numbers are not detected** (solo todo #3848).
  Numbers written with a leading `0` and a UK area code reach the model raw
  under every setup, the Nym net included; `+44` numbers are covered.
- **Some dates of birth pass raw** (solo todo #3651). Dates without a
  birth-date cue in non-ISO formats, a German `Geburtsdatum` followed by a
  `DD.MM.YYYY` date, and a `DD.MM.YYYY` value under a `dob` key in a JSON
  tool result reach the model
  raw on every path, `gaze clean` included.
- **Two `gaze proxy` restore gaps from v0.15.0 remain** (solo todos #3841,
  #3842). A token split across streaming events is not restored on the legacy
  OpenAI chat and Gemini streaming paths, and the Anthropic path can restore a
  JSON-escaped spelling. Both fail toward pseudonymized or escaped output,
  never toward a leak.
- **The MCP chokepoint still refuses what the safety net flags** (solo todo
  #3850). `gaze clean`, `gaze daemon` and `gaze proxy` tokenize a net finding;
  MCP tool calls refuse it. That fails closed; parity needs a contract
  decision.

**Performance.**
Measured with [`scripts/bench/cli-latency.py`](scripts/bench/cli-latency.py)
on the release code (built at `f769f823`) over 30 benchmark documents after
one warm-up, on a quiet MacBook Pro (Apple M5 Max, 18 cores, 64 GB, macOS 26.5;
1-minute load 1.77 at start, no other build or benchmark running, one ONNX
Runtime thread). The script now also measures `gaze proxy`. Evidence:
[`latency-v0.15.1.json`](docs/reference/benchmarks/latency-v0.15.1.json).

| Warm per document or request | Median | p95 |
|---|---:|---:|
| v0.15.0 rules + NER | 20.0 ms | 32.7 ms |
| v0.15.1 rules + NER | 19.7 ms | 32.8 ms |
| v0.15.1 `gaze setup` policy, no net (`--safety-net none`) | 20.4 ms | 33.3 ms |
| v0.15.1 `gaze setup` policy with Nym (the default) | 69.4 ms | 138.9 ms |
| `gaze proxy` request, setup policy, no net | 27.2 ms | 39.6 ms |
| `gaze proxy` request, setup policy with Nym | 112.1 ms | 215.6 ms |

The pipeline rows match v0.15.0 within noise. The proxy rows are one OpenAI
chat request per document through `gaze proxy serve` to a local upstream that
echoes the text back, so they hold the proxy's own time and no provider time.
All 30 requests on each policy were accepted and every reply restored to the
sent document exactly. A proxy request took about 7 ms more at the median
than the same document through a warm `gaze daemon` without a net, and about
43 ms more with Nym (daemon: 69.5 ms). With a net the proxy runs the Resolve
step and then its admission scan on each surfaced string (PR #660). This run
does not isolate what PR #660 added; the PR's own probes measured about 7 ms
per request against v0.15.0. One-shot `gaze clean` takes 772 ms per document
for the setup policy and 2,140 ms with Nym; a warm `gaze daemon` answers in
20.3 ms and 69.5 ms after a first cold request of 763 ms and 2,121 ms, and a
proxy start takes 773 ms and 2,068 ms before the first request. In a 10-turn
`gaze daemon` conversation that re-sends the history, a 4.2 KB turn took
183 ms without a net and 1,019 ms with Nym.

**Benchmark.**
Measured on the release commit (`f769f823`, clean tree) with
[`scripts/bench/run_no_opf_benchmark.py`](scripts/bench/run_no_opf_benchmark.py)
(`full` profile, seed 20260710) on the same 2,910 documents and 130,282 gold
PII bytes as v0.15.0, on a MacBook Pro (Apple M5 Max, 18 cores, 64 GB,
macOS 26.5). The arm is the exact policy `gaze setup --non-interactive`
writes, Nym on, byte-identical to the v0.15.0 policy (SHA-256 `f909a23a…`)
([`scorecard-v0.15.1.json`](docs/reference/benchmarks/scorecard-v0.15.1.json)).

| Setup | Refused | Leaked, all processed docs | Leaked, common set | False-positive bytes | Exact restores |
|---|---:|---:|---:|---:|---:|
| v0.15.1 `gaze setup` policy (rules + NER + Nym) | 0 | 19,556 (15.0%) | 19,556 (15.0%) | 30,073 | 2,910 / 2,910 |
| v0.15.0 `gaze setup` policy (rules + NER + Nym) | 0 | 19,556 (15.0%) | 19,556 (15.0%) | 30,073 | 2,910 / 2,910 |

Both rows use scored-label contract v1, which scores every corpus label. Under
scored-label contract v2 (PASSWORD and SECURITYTOKEN out of contract, gold
123,621 B) the same release code, policy, seed and host leak 13,319 B
(10.77%), as in v0.15.0; that run adds `--scored-labels
docs/reference/benchmarks/scored-labels-v2.json`, and its scorecard is not
committed. The totals do not move because 96 of the corpus's 126 card
numbers fail the Luhn check and the other 30 were already tokenized. The
card fix shows only in the benchmark's shape-only probe, which ignores the
Luhn check: it now covers 124 of the 126 card spans instead of 99. The #660
proxy change does not touch this benchmark, which drives the `gaze clean`
pipeline.

### Fixed

- **`gaze proxy` tokenizes what the safety net flags instead of refusing the
  request.** Under the policy `gaze setup` writes (Nym enabled), the proxy
  answered `500 {"error":"Pipeline"}` to any request containing a date, such
  as `Invoice date 1971-05-30.` or `born on 30 May 1971.`, while `gaze clean`
  tokenized the same date. The proxy ran only the primary pipeline and then
  used the net as an admission gate, so every net finding became a refusal.
  Both request paths (the legacy OpenAI and Gemini adapters and the Anthropic
  direct profile) now run the Resolve step of
  `gaze clean --safety-net-fallback strict`, through one shared library
  function (`Pipeline::resolve_boundary_text`), then admission as before. The
  flagged span is forwarded as a token and restored in the response. What
  Resolve cannot tokenize is still refused, never deleted. Plain `gaze clean`
  defaults to the `redact` fallback: when the nets' re-run flags something
  new, it tokenizes that in a second reversible batch and deletes what is left
  one way. The proxy refuses such a request instead (for example
  `user jweber84 born 1984-03-12` under the `gaze setup` policy). Spans that no
  net flags and no rule detects, such as a `DD.MM.YYYY` date without a cue,
  still reach the provider raw, exactly as `gaze clean` prints them. The old
  behaviour blocked some of them only because it refused the whole request
  whenever any other date in it was flagged. No leak shipped: the old
  behaviour failed closed. Affected: v0.15.0 with a configured net (PR #660,
  solo todo #3847).
- **Proxy refusals say why.** A refusal is now `422` with the typed
  `ProtectionError` variant, the fallback reason and the suspect classes,
  never the text, and one line on the proxy's stderr. The legacy adapters
  answer `{"error":"Refused","refusal":{…}}`; the Anthropic direct profile
  answers code `ProtectionRefused` with the same `refusal` object. It was an
  opaque `500 {"error":"Pipeline"}` (legacy) or `502 InvalidToken` (direct)
  with an empty log. The shape is documented in
  `docs/explanation/proxy/proxy-runtime.md`. **Breaking** for clients that
  matched the old status or error name. `gaze_proxy::DirectProxyError` is
  `Clone` but no longer `Copy` (PR #660, solo todo #3847).

### Security

- **A payment card with digits touching it is now tokenized before the text
  reaches the model.** Every release up to and including v0.15.0 sent the
  card raw when a CVV or expiry followed it (for a card number `CARD`:
  `Karte CARD 123`, `CARD 12 28`, a fullwidth `１２３`), when a number
  preceded it (`Nr 7 CARD`, `2024 CARD`, `Order 5678 CARD paid`,
  `Nr 12345 CARD`), or when normalization glued more digits onto it (a
  fullwidth group or a dropped ZERO WIDTH JOINER), with no policy and under
  the `gaze setup` policy alike. `card.structural` took the greedy 13-19 digit
  run and ran Luhn on it once. It now takes the whole digit run
  (`\b\d(?:[\s-]?\d)*\b`) and finds the card inside it. Every window the old
  pattern matched that passes Luhn, and every group-aligned window printed in
  a card layout (compact 13-19, 4-4-4-4, 4-4-4-4-3, 4-6-5, 4-6-4) that passes
  Luhn, is a card candidate; overlapping candidates are tokenized as one span
  covering their union. The digits cannot tell which of two overlapping
  Luhn-valid windows is the card (`0 CARD` passes Luhn both as the
  17-digit window and as `CARD`; a random number before a card makes such a
  window about one time in ten), so the token fails closed and covers both.
  Groups end at separators and, through the new `DetectContext::source_spans`,
  where normalization hid a break; validator veto accepts a union span that
  still holds a card. The restore-boundary DLP check runs the same code
  (`gaze_types::payment_card::scan_card_run`), so both directions agree; on
  the #652 review probe (440,000 texts) it reports every card it reported
  before, plus 13,953 texts with a card it missed. Every 1- to 4-digit number
  written before a 16-digit, 19-digit, Amex or Diners card (separated, glued by
  a ZERO WIDTH JOINER, or in fullwidth digits; 133,320 cases) now leaves no
  card digit raw on either path. On 5,000 generated texts per family (forward
  path, no policy), cards with touching digits went from 1,799 to 4,423 fully
  tokenized; amounts, timestamps, phone numbers, compact long IDs and year or
  order prefixes without a card are unchanged. Valid IBANs stay fully
  tokenized and restore exactly, but the token class can change: about one
  BBAN in ten holds a Luhn-valid 4-4-4-4 window, and that card candidate
  inside the IBAN now settles the family to the narrow `custom:iban` token
  (the #619 settled-narrow rule) instead of the family-level
  `family:payment-card-or-iban` token. On 20,000 generated mod-97-valid
  IBANs, `custom:iban` went from 1,219 to 2,601 and the family token from
  18,781 to 17,399 (audit rows change with them), so a policy that acts on
  `custom:iban` differently from the family class now applies to about 7 %
  more IBANs. The cost, taken
  deliberately (leak safety over false positives), is more card tokens on
  Luhn-passing windows in longer grouped runs: random 13-19 digit groupings
  481 to 489 texts, a Luhn-invalid 4-4-4-4 with a 2-4 digit tail 346 to 503,
  and grouped IDs of five to ten 4-digit groups 748 to 1,840 texts (14,896 to
  39,395 card-token bytes). Every such token restores exactly. The no-OPF
  scorecard (2,910 documents) is unchanged: its card gold fails Luhn. The
  `luhn` validator now also skips any Unicode whitespace and non-ASCII
  digits, as the restore check already did. (PR #658, solo todo #3843)
  This fixes the v0.15.0 Known limitation "A payment card next to other
  digits can reach the model untokenized".
- **The restore-boundary DLP check (PR #652, v0.15.0) now scans the whole
  digit run.** As shipped in v0.15.0, its retry only looked inside the first
  19 digits of the run, so a card after a separated number of four or more
  digits (`2024 CARD`, `Order 5678 CARD`), any number before
  a 19-digit card, and a glued tail of four or more digits still passed
  unreported. It now runs the card scan described above (PR #658).

### Documentation

- The explanation, reference, how-to and tutorial pages are restructured,
  and stale facts in them are corrected against the code, among them the
  `ValidatorFailReason` variant list, the proxy's shipped version, the
  dashboard activation receipt and the document bundle files. The old
  safety-net modes page is kept verbatim as a design record with a banner
  listing where the code differs (PRs #657, #659; #657 merged before the
  v0.15.0 tag but was not listed there).

## [0.15.0] - 2026-09-25

v0.15.0 makes the policy that `gaze setup` writes protect every detected class
and turns the Nym safety net on in it by default, while keeping Gaze's
contract: fail closed, preserve reversibility, and keep PII out of
agent-visible surfaces. The curated summary below leads; the full entries
follow in Keep a Changelog form.

**Security.** Policies written by `gaze setup` in v0.11.2 through v0.14.0 set
the default rule to `preserve`, so detected phone numbers, IBANs, payment card
numbers and IP addresses left the process raw with a success exit. The
generated policy now tokenizes by default (PR #635). **Remediation:** back up
any custom rules in the existing policy, then run `gaze setup --force` to
regenerate it; the manual repair is in the Security entry below. `gaze clean`,
`gaze daemon` and `gaze proxy` now warn on stderr when a loaded policy sends a
detected class through raw, whether by an explicit `preserve` default or an
omitted default, and name reachable one-way `generalize` rules (PR #641).
Existing policies stay valid. Other shipped leaks closed in this release each
have an entry under Security or Fixed, which names the affected releases where
the history proves them. Among them: `gaze clean` without `--policy` ran an
email-only pipeline (v0.3.0–v0.14.0, PR #618); `gaze index` ran without the
`core` floor (v0.11.0–v0.14.0, PR #620); `gaze proxy` never ran a configured
safety net on request text (v0.13.0–v0.14.0, PR #585) and forwarded raw bytes
after a safety-net fallback deletion (v0.13.0–v0.14.0, PR #593); the opt-in
prefix cache could return raw PII from a stale decision (v0.9.0–v0.14.0,
PR #579); a Resolve+Redact fallback deletion could leave newly detectable raw
text behind (v0.8.1–v0.14.0, PR #584); several IBAN and collision-family
shapes shipped raw (PRs #622, #624, #626, #627, #628); and so did national IDs
under JSON keys and `key=value` log fields and identifiers grouped with
non-breaking spaces, in every release through v0.14.0 (PR #647).

**Highlights.**

- **Nym on by default in `gaze setup`** (PR #642). Setup installs the
  SHA-pinned Nym-small bundle, writes an activating policy, and proves in the
  doctor check that Nym catches a synthetic plate. It prints the model card
  licence (MIT), the pinned upstream revision, the open training-data licence
  review, and the opt-out: `gaze setup --safety-net none`. OPF stays opt-in.
- **A policy can activate a safety net** (PR #636). `[safety_net] backend =
  "nym"` turns Nym on for both the CLI and `gaze-assembly`;
  `[safety_net.nym] model_dir` locates the bundle. `--safety-net` is
  repeatable, so nets stack for one run and replace the policy's selection;
  `--safety-net none` disables them for one run with a notice.
- **The setup policy loads every bundled PII rulepack except `secrets`**, plus
  the locales those packs declare, with `en-US` first (PR #635). API keys and
  tokens stay opt-in through `secrets`.
- **The benchmark scores the exact setup policy** (PR #643). The scorecard
  harness builds its pipeline through the same policy resolution as
  `gaze clean --policy`, and an equivalence check proves the two agree.
- **One entity, one token** (PR #628): a candidate that wholly contains
  another class's candidate wins the whole span, and a `preserve` winner no
  longer shields bytes a protected class claimed.
- **The Kiji DistilBERT safety net is removed** (PR #612): it recovered 1,831
  leaked gold bytes for +169,657 false-positive bytes on the 2026-09-16
  leaderboard.

**Breaking changes** (each has a full entry and migration below; see also
[UPGRADE.md](UPGRADE.md)):

- `gaze setup --safety-net ner` is removed; use `--safety-net none` for the
  former NER-only policy (PR #642).
- `--safety-net-backend nym` needs one explicit `--safety-net nym` (PR #636).
- `gaze setup` installs the pinned Davlan mBERT NER model; re-run it (PR #612).
- `gaze index ingest` requires the pinned NER model through `--ner-model-dir`
  or `GAZE_NER_MODEL_DIR` (PR #612).
- Credential recognizers moved from `core` to the opt-in `secrets` rulepack;
  `username.field` is removed (PR #607).
- The Kiji safety net, its flags, features, environment variables and API are
  removed (PR #612).
- Custom rulepack paths keep the `core` floor unless `bundled = []` or
  `--rulepack-bundled=none` (PR #632).
- Containment precedence and per-character residual coverage change the token
  stream (PR #628).
- A collision-family token takes the strictest member action instead of the
  `default` rule (PR #624).
- The safety net writes a one-way `[REDACTED:<class>]` marker instead of
  deleting bytes (PR #623).
- `gaze-mcp-rmcp`, `gaze-mcp-bridge` and `gaze-document` move to rmcp 2.x
  (PR #616).
- `gaze_document::extract::pdf::rasterize_first_page` is removed; use
  `extract_pages` (PR #650).
- Custom `gaze-proxy` adapters that build `PiiSurface` values must set the new
  `syntax` field (PR #656).
- A policy with `schema_version = "0.1"` no longer loads; write `"0.1.0"`. The
  `PolicySchemaUnsupported` error's `supported` field now reads `"0.1."`
  (PR #576).
- `gaze-mcp-bridge` caps its session cache at `session.max_sessions` (default
  1,000) and refuses a new session at the cap when none can be evicted
  (PR #578).
- `gaze-token-bridge` fingerprints of custom entities that contain repeated or
  non-space whitespace change; re-ingest them (PR #553).
- `gaze proxy` runs configured safety nets on request text, so a request in
  which a net finds raw text outside the tokens, or a net that errors, now
  refuses the request before it reaches the provider (PR #585).

**Known limitations.** One detection gap and two `gaze proxy` restore gaps
ship in this release:

- **A payment card next to other digits can reach the model untokenized**
  (solo todo #3843). A card followed by a separated CVV or expiry
  (`4111 1111 1111 1111 123`) or preceded by other digits fails the Luhn check
  as one run, so the forward path does not tokenize it. The restore-boundary
  check reports the same shape in model output (PR #652). A fix is in progress
  (PR #658) and lands after this release.

The two restore gaps fail toward pseudonymized or escaped output, never toward
a leak, and both are planned for v0.16:

- **A token split across streaming events is not restored** on the legacy
  OpenAI chat and Gemini streaming paths (solo todo #3841). The proxy restores
  each server-sent event on its own, and upstream streams usually split a Gaze
  token over several events, so the client can receive the placeholder instead
  of the original value in streamed text and tool-call arguments. Non-streaming
  responses and the Anthropic path, which accumulates per content block, are
  not affected.
- **The Anthropic path can restore a JSON-escaped spelling** (solo todo #3842).
  When a value was captured inside a JSON string in a text block, the manifest
  holds its escaped source spelling, so restoring it into `tool_use.input` or
  into plain text keeps the escapes: a literal backslash before a quote, or a
  `\u` escape in place of a non-ASCII letter. The legacy adapters handle
  JSON-destination restores correctly as of PR #656.

**Performance.**
Measured with [`scripts/bench/cli-latency.py`](scripts/bench/cli-latency.py)
on the release code (built at `6fcba31a`) over 30 benchmark documents after one
warm-up, on a quiet MacBook Pro (Apple M5 Max, 18 cores, 64 GB, macOS 26.5; 1-minute
load 1.74 at start, no other build or benchmark running, one ONNX Runtime
thread). Evidence:
[`latency-v0.15.0.json`](docs/reference/benchmarks/latency-v0.15.0.json).

| Pipeline, warm per document | Median | p95 |
|---|---:|---:|
| v0.14.0 rules + NER | 28.4 ms | 40.7 ms |
| v0.15.0 rules + NER | 19.9 ms | 33.1 ms |
| v0.15.0 `gaze setup` policy, no net (`--safety-net none`) | 20.3 ms | 33.2 ms |
| v0.15.0 `gaze setup` policy with Nym (the default) | 69.8 ms | 139.8 ms |

Rules + NER got faster than v0.14.0 because NER now runs once per document
(PR #653); without that change the setup policy's 15-step locale chain ran NER
15 times. Nym adds about 50 ms per document at the median and raises peak
memory from about 590 MiB to about 1,050 MiB. One-shot `gaze clean` pays process
start and model load on every call: 772 ms per document for the setup policy
and 2,149 ms with Nym. A warm `gaze daemon` answers in 21.3 ms and 69.0 ms,
after a first cold request of 762 ms and 2,120 ms. Latency grows with text
length: in a 10-turn `gaze daemon` conversation that re-sends the history, a
4.2 KB turn took 185 ms without a net and 1,022 ms with Nym. v0.16 is the
performance release.

**Benchmark.**
Measured on the release commit with
[`scripts/bench/run_no_opf_benchmark.py`](scripts/bench/run_no_opf_benchmark.py)
(`full` profile, seed 20260710, scored-label contract v1) on the same 2,910
documents and 130,282 gold PII bytes as v0.14.0, on a MacBook Pro (Apple M5
Max, 18 cores, 64 GB, macOS 26.5). The arm is the exact policy
`gaze setup --non-interactive` writes, Nym on
([`scorecard-v0.15.0.json`](docs/reference/benchmarks/scorecard-v0.15.0.json)).

| Setup | Refused | Leaked, all processed docs | Leaked, common set | False-positive bytes | Exact restores |
|---|---:|---:|---:|---:|---:|
| v0.15.0 `gaze setup` policy (rules + NER + Nym) | 0 | 19,556 (15.0%) | 19,556 (15.0%) | 30,073 | 2,910 / 2,910 |
| v0.14.0 default (rules + NER + Kiji) | 0 | 25,179 (19.3%) | 25,179 (19.3%) | 168,276 | 2,282 / 2,910 |

Neither setup refused a document, so the common set is all 2,910. Leaked PII
bytes fell 22.3% and false-positive bytes 82.1%. Both rows use scored-label
contract v1, which scores every corpus label. Under scored-label contract v2
(PASSWORD and SECURITYTOKEN out of contract, gold 123,621 B) the same run
leaks 13,319 B (10.77%): the same release code (`6fcba31a`), policy (SHA-256
`f909a23a…`), seed and host, rerun with `--scored-labels
docs/reference/benchmarks/scored-labels-v2.json`. That v2 scorecard is not
committed; the v1 row stays the version's benchmark figure.

### Security

- **The restore-boundary DLP check now flags NBSP-grouped and fullwidth IBANs
  and cards, and cards with digits touching them.** This deterministic outbound
  check scans model output at the restore boundary (before tokens are
  restored) for structural identifiers the manifest did not authorize. It used
  its own patterns on the raw text, and the IBAN pattern accepted only an ASCII
  space between groups, so `GB82\u00A0WEST\u00A0…` (NBSP, NARROW NBSP, THIN
  SPACE or any other Unicode space separator) or a card written in fullwidth
  digits passed unreported. The scan now runs on the same normalized view as
  detection and maps findings back to exact raw byte offsets. A manifest value
  and its echo now compare equal whatever separator either side used, so a
  Zs-grouped echo of a manifest value reports `ManifestBypass`, not
  `FreshPiiDetected`. A card followed by a CVV or expiry
  (`4111 1111 1111 1111 123`), preceded by another number, or joined to more
  digits by a fullwidth group or a dropped ZERO WIDTH JOINER failed Luhn as one
  run and also passed unreported, in ASCII text too. The check now retries the
  group-aligned sub-runs printed in a card layout (compact, 4-4-4-4,
  4-4-4-4-3, 4-6-5, 4-6-4) and reports the card at its exact offsets. Other
  ASCII input scans unchanged (PR #652).

- **National IDs in tool-call JSON and `key=value` logs are now tokenized.**
  Every release up to and including v0.14.0 matched cue-anchored identifiers
  (BSN, Steuer-ID, CPF, CNPJ, NHS, SSN, NINO, PAN, Aadhaar, NIR, VAT ID,
  passport, national ID, driver licence, tax number) only in prose such as
  `BSN: <9 digits>`. A JSON key (`{"bsn":"<9 digits>"}`), a log field
  (`bsn=<9 digits>`) or a snake, camel or kebab key (`steuer_id`, `steuerId`,
  `nhs_number`, `customer_ssn`) passed the value through raw, including on the
  `gaze proxy` tool-call argument path. The `core` rulepack patterns now accept
  quoted, single-quoted and escaped JSON keys, `=` and `:` log forms, and those
  key spellings. A camelCase prefix before the cue (`customerSsn`) is not yet
  matched, and CSV header-to-column association is not covered: a CSV column
  headed `bsn` with bare values is not tokenized by this change (PR #647,
  solo todo #3818; CSV is solo todo #3829).
- **Identifiers grouped with non-breaking or thin spaces are now tokenized.**
  Every release up to and including v0.14.0 missed IBANs, payment cards,
  Steuer-IDs and other grouped identifiers whose groups were separated by
  NO-BREAK SPACE, NARROW NO-BREAK SPACE, THIN SPACE or another Unicode space,
  as PDFs and banking UIs write them. The IBAN and Steuer-ID patterns accepted
  only ASCII spaces, and the Luhn check vetoed a card holding a non-ASCII
  separator, so each value shipped raw; an NBSP-grouped Steuer-ID leaked its
  first two digits next to a `phone` token. Detection now reads every Unicode space
  separator as an ASCII space; tokens, manifests and restore keep the original
  bytes (PR #647, solo todo #3819).
- **`gaze setup` policies now tokenize every detected class.** Generated policies
  in v0.11.2–v0.14.0 preserved unmatched classes, allowing detected phone,
  IBAN, payment card, and IP address values to pass through raw. The generated
  policy now loads every bundled PII pack and its locales while keeping
  `secrets` opt-in. Back up custom rules, then run `gaze setup --force` to
  regenerate an existing policy. For a manual repair, set the `[[rule]]`
  default action to `"tokenize"`, delete the old per-class rules (the
  `location = generalize` rule emits a one-way marker), and enable the
  additional bundled packs and locales.
- **rustls 0.23.45 and rustls-webpki 0.103.15.** The lockfile moves rustls from
  0.23.40 to 0.23.45 for RUSTSEC-2026-0285 (TLS 1.3 handshake messages were
  accepted across encryption-level boundaries; the advisory does not let a
  network attacker alter or complete an authenticated handshake), and
  rustls-webpki from 0.103.13 to 0.103.15, which the new rustls requires.
  `gaze-proxy`, `gaze-model-setup` and the `gaze` CLI link rustls (PR #580).

### Added

- **Policy fall-through warnings in `gaze clean`, `gaze daemon`, and `gaze proxy`.**
  After successful processing or startup, Gaze names registered detection
  classes that an explicit `preserve` default or omitted default sends through
  raw. It also identifies reachable per-class `generalize` rules as one-way.
  Existing policies remain valid; back up custom rules and run
  `gaze setup --force`, or set the default action to `"tokenize"`.
- **The scorecard harness benchmarks the exact `gaze setup` policy** (PR #643).
  A policy-file config builds the benchmark pipeline from a policy TOML through
  the same resolution `gaze clean --policy` uses, now shared as
  `gaze_assembly::{resolve_policy_inputs, ResolvedPolicyInputs}`.
  `scripts/bench/check_policy_equivalence.py` proves the release binary and
  the benchmark agree document by document and refuses an empty or truncated
  sample; a model-free six-case sample runs in CI. Scorecards record the
  policy file SHA-256 and the model bundle pins. `Candidate` (non-exhaustive)
  gained `source_recognizer_ids`, so protection traces carry each original
  recognizer id and a custom id containing `+` is no longer split.
- `[safety_net].backend = "nym"` activates Nym from a policy in both CLI and
  `gaze-assembly`; `[safety_net.nym].model_dir` supplies its optional bundle
  location. `gaze-assembly/safety-net-nym` forwards the backend feature.
  Repeatable `--safety-net` values stack CLI nets and replace policy selection;
  `--safety-net none` disables them for one run with a notice.
- **`ConflictTier::ContainmentPrecedence`** (audit string
  `containment_precedence`): a candidate that wholly contains a candidate of
  another class won the whole span as one token; the swallowed candidate is a
  merged source and its loser row carries the tier. **`ConflictTier::
  ProtectionOverride`** (`protection_override`): a residual fragment's row
  when the fragment replaced bytes inside a `preserve` selection because a
  protected class claimed them.
- **`Pipeline::registry`**, **`FamilyPolicyTable::anchored_families`** and
  **`RegexDetector::with_base_score`**; `gaze` re-exports `AmbiguityRecord`,
  `AmbiguityReason`, `LosingCandidate` and `DerivedFamilyAction`, the types
  behind `RedactionEntry::ambiguity_record`. `gaze-assembly` pins that the
  registry's anchored-family member map equals the one
  `uncovered_collision_family_classes` derives from policy and rulepacks
  (the validation solo todo 3761 asked for; the single-source refactor is
  not done).
- **`Action::strictness_rank` and `Action::strictest`** in `gaze-types`: the
  fail-closed order over the closed action set (`redact` > `tokenize` >
  `generalize` > `format_preserve` > `preserve`), and `Action` now serializes
  with its canonical audit spelling.
- **`AmbiguityRecord::derived_action`** (`DerivedFamilyAction { action,
  member_class }`): a family-level token's audit row records which member's
  explicit rule set its action when no rule named the family class;
  `member_class` is `null` when the family's own default applied. Serialized
  only when present, so existing rows and fixtures are unchanged.
- **`Action::is_protective`** in `gaze-types`: every action but `preserve`,
  the admission test residual coverage uses.

- **Benchmark gold-gap diagnostic (scored-label contract v3).**
  `docs/reference/benchmarks/scored-labels-v3.json` keeps v2's labels and adds
  a `gold_gap` rule: a predicted span that repeats a scored gold value
  byte-for-byte in the same document (ASCII-trimmed, class-compatible, on a
  word boundary, no gold or ignored overlap) is reported as
  `gold_gap_protected_bytes` with a per-label breakdown and an adjusted
  precision. It is a diagnostic column; the v2 headline is unchanged, v1/v2
  never run the step, and a malformed `gold_gap` block fails closed. On the
  saved `pass2-ner` predictions it credits 1,683 ranges (11,188 bytes) and
  moves no leaked byte. It stays diagnostic until the committed 200-candidate
  human audit (`gold-gap-sample-v3.json`) passes.
- **`gaze_types::iban_registry_length`.** The ISO 13616 IBAN Registry length
  for a country code, previously private to the `iban_mod97` validator. It is
  now the workspace's one source of truth for IBAN length: the validator gates
  on it and the `iban.structural` pattern's length branches are pinned to it by
  a drift test in `gaze-recognizers`.
- **Nym warm-latency script.** `scripts/bench/nym-warm-latency.py` times the
  production pipeline (`clean_for_bench`) warm, per document, for `pass2-ner`
  and `full-stack-nym-resolve` over the coverage-loop corpus plus 512- and
  1,024-piece synthetic documents, and prints p50, p95 and mean with a
  hardware line (chip, cores, RAM, OS, ort version, bundle SHA) and the host
  load average. No latency row is recorded until it runs on a quiet host.
- **Benchmark shape-recall column.** Each scorecard run now splits every
  validator-backed label's surviving bytes into gold that passes its validator
  and gold that fails it (`production_recall_by_gold_validity`), next to the
  existing validator-backed and shape-only recall. The headline leaked bytes
  are unchanged; the benchmark document renders the table for the shipped
  default arm from the next measured release on.
- **Opt-in Nym-small safety net** (`--safety-net nym`, feature
  `safety-net-nym`, on in the default `gaze-cli` build). Runs
  `Wismut/nym-pii-multilingual-small` v3 int8 in process through ONNX Runtime.
  It is not a default: no safety net runs by default, and nothing loads
  unless `nym` is selected.
  - `gaze setup --safety-net nym` fetches the bundle at revision `4348999c`
    and verifies it against `NYM_SMALL_INT8_BUNDLE_SHA256`; the backend
    re-verifies digests, modes and the `id2label` table before loading, and
    `gaze mcp doctor` reports the bundle.
  - Only labels with a Gaze class can fire, each with an explicit threshold.
    The default is op-B: `BUILDING_NUMBER`, `LICENSE_PLATE`, `USERNAME` at 0.5
    and `DATE_OF_BIRTH` at 0.9, mapped to `custom:building_number`,
    `custom:license_plate`, `custom:username` and `custom:date`. `TAX_ID` and
    `ZIP_CODE` exist but are off. The other 34 labels, including
    `GIVEN_NAME`, can never be enabled.
  - New policy table `[safety_net.nym]` (`labels` plus `threshold`) configures
    the allowlist and fails at load on an unknown or unmapped label, a missing
    or stray threshold, or a threshold outside `(0, 1]`. It configures and
    never activates: a policy declaring it while another net (or none) runs is
    refused.
  - Spans are whole words (the pipeline's sub-word rule, now one shared
    `gaze_types::is_inside_word`), tokenizer character offsets become UTF-8
    byte offsets, and input longer than 512 pieces is scanned in overlapping
    windows; an unscored piece or uncovered character is a typed error.
  - Audit rows carry `safety_net_id = "nym-small-int8"`, the score, and
    `raw_label = "LABEL>=THRESHOLD"`. Nym is refused through
    `--safety-net-registry`, which would drop the label and threshold.
  - New benchmark arm `full-stack-nym-resolve`. On the 2,910-document
    population it removes 6,154 leaked gold bytes under scored-label contract v2
    (20,727 to 14,573) for 526 false-positive bytes, action precision 0.891,
    one one-way deletion, 2,909 of 2,910 exact restores; timings provisional.
  - Three items were open before any default change; this release turns Nym on
    in the `gaze setup` policy with each one stated. The address-context guard
    for room and seat numbers is still a documented known gap. The licence
    review of the Wikipedia-derived (CC-BY-SA) training data is still open, and
    setup's notice names it. Latency was measured quietly for rules plus NER
    with and without Nym; the release numbers for the setup policy are in the
    Performance summary above. See
    [Known gaps and open review items](docs/explanation/safety-net/safety-nets.md#known-gaps-and-open-review-items).

- **Anchored four-digit postal codes for Austria and Switzerland**
  (`postal.at_ch`). Four-digit codes in `de-AT` and `de-CH` documents had no
  recognizer: 366 gold ZIP entities (1,470 bytes) in the EN/DE holdout leaked in
  full. A bare four-digit string carries no structural signal (unanchored
  `\d{4}` is 19% precise on the holdout and fires across 62.5% of the negative
  corpus), so the rule matches only in two positions:

  * directly after a postal cue: `PLZ`, `Postleitzahl`, `Postcode`, `ZIP`,
    `Zip code`, optionally followed by `:`, `#` or `.`;
  * directly before a city-shaped token: an uppercase letter and at least two
    more letters or hyphens, or `St.` / `St` followed by a capitalised name,
    after an optional comma and one to three spaces, NO-BREAK SPACEs or NARROW
    NO-BREAK SPACEs.

  An `A-`, `CH-` or `FL-` country prefix is part of the token. The rule is
  `locale_basis = "document"`, `locales = ["de-AT", "de-CH"]`,
  `safety_tier = "locale_gated"`: German (`de-DE`) documents gain no four-digit
  tokens.

  Measured on the full 2,910-document population, both scored-label contracts,
  base `095ddaff`: ZIP entities fully covered rise from 548 to 862 (+314), ZIP
  byte recall on the rule floor from 59.7% to 83.0%, and total leaked bytes fall
  by **1,270** (contract v2 rule floor 87,647 to 86,377; `pass2-ner` 20,727 to
  19,457). No document failed closed; every restore stayed exact.

  **Known cost, disclosed on purpose.** 28 tokens on the 567 `de-AT` / `de-CH`
  holdout documents overlap no gold span (+112 false-positive bytes, 28 more
  documents with a false positive). 16 of them follow a postal cue and are most
  likely unannotated codes; 12 follow a capitalised word. German capitalises
  every noun, so `1500 Euro` or `3000 Mitarbeiter` looks like `1500 Musterstadt`
  to this anchor, and the `regex` crate has no negative lookahead to hold a
  stop-list. The same shape covers years, versions, flight and train numbers,
  ports and error codes before a capitalised word (`Am 12.03.2024 Treffen`
  tokenizes the year), and English text under the `core-extended` chain. Every such token restores losslessly. 3 more tokens cover the year
  of a date of birth. Zero matches on the 1,024 A4 negative documents, both as
  committed and with every document forced to `de-AT` or `de-CH`.

  Choices measured rather than assumed: codes 1900 to 2099 are kept (they are
  assigned in both countries; excluding them removes one false positive); no
  guard against a digit group before the code (it removes no false positive and
  would leak `Musterweg 12 4020 Musterstadt`). Two narrowings found by
  out-of-corpus enumeration, each costing zero gold: a bare `St.` is refused
  because `1500 St.` means "1500 pieces", and a city-anchored code preceded by
  `#` is refused because `#4711 Fehler beheben` is an issue reference. A
  302,400-input differential enumeration against base (codes, cues, prefixes,
  separators, followers, seven locale chains) found no input where base
  protected a byte the candidate leaks, no change at all outside `de-AT` /
  `de-CH` chains, and every restore exact.

  **Locale chains.** Document-basis rules of one class resolve per span
  across the chain, so under `de-AT, de-DE`, `de-CH, de-DE` or `de-AT, en-US`
  a four-digit match does not switch off `postal.de` / `postal.us` for a
  five-digit code elsewhere in the same document (pinned in
  `postal_at_ch.rs`). The no-policy `core-extended` compatibility chain
  (`global, en-US, de-DE, de-AT, de-CH`) therefore runs this rule on every
  document and keeps each match no US or German candidate overlaps; forced onto the
  1,319 other holdout documents it produced 54 gold ZIP, 141 other-gold and 10
  no-gold tokens. Pass `--locale=global` or a narrower policy chain to avoid it.

  `en-AU` and `en-NZ` stay uncovered: their postcode follows the locality, so
  this anchor reaches only about a third of them and needs its own design.

- **Postal-code coverage for Canada, the UK, and Ireland** (`postal.ca`,
  `postal.gb`, `postal.ie`). `custom:postal_code` was previously served only by
  `postal.de` (`de-DE`) and `postal.us` (`en-US`), both
  `locale_basis = "document"`, so seven of the nine document locales in the
  EN/DE holdout had no postal recognizer at all and ZIP was the largest single
  leak bucket. Measured gold ZIP recall was 334 of 1,090 entities, which is the
  locale-gated population to within 2 entities of incidental overlap.

  The three new rules are `locale_basis = "format"` and `safety_tier =
  "safe_default"`, matching the treatment `nhs.uk`, `nino.uk`, `ssn.us`,
  `nir.fr`, `bsn.nl`, and `cpf.br` already receive: letter/digit interleaving is
  itself the precision mechanism, so they need no document-locale gate and run
  at every locale, including `--locale=global`. **Adopters who must not tokenize
  Canadian, UK, or Irish postal codes cannot suppress them with a locale chain
  and have to disable the recognizer.**

  Measured on the rule-floor arm over the 1,886-document holdout: gold ZIP byte
  recall rises from 30.8% to 59.7%, recovering **1,567 gold bytes**; per locale,
  `en-CA` 97.1%, `en-GB` 96.9%, `en-IE` 87.9%. Precision cost is one false
  positive across all 1,886 documents (an uppercase letter-letter-digit, digit-letter-letter token in lowercase
  prose, whose outward code is a real assigned UK district) and **zero across all
  1,024 committed A4 negative documents**. The bundle tokenization drift snapshot
  is unchanged: none of the three patterns match the drift corpus.

  Review hardening, all costing **zero** of the 71 / 83 / 60 measured gold
  entities and verified by a per-entity gold census over the full holdout:

  * The Eircode identifier must now carry at least one letter. Allowing all four
    characters to be digits made `postal.ie` tokenize the ordinary
    `LETTER + 2 digits + 4 digits` business reference layout — `ORDER A12 3456`,
    `TICKET D45 6789`, `JOB F90 1234` — as postal codes at every locale. The A4
    negative corpus contains no token of that shape, so its 0/1024 score could
    not see the class.
  * `postal.ca` and `postal.gb` no longer match when the preceding character is
    `#`. `\b` gave no protection there, so `#D3D3D3` (`lightgray`) and `#A9A9A9`
    (`darkgray`) tokenized as postal codes; a measured 1.31% / 2.87% of uniform
    `#RRGGBB` values matched. The corpus contains no `#RRGGBB` literal.
  * All three accept NO-BREAK SPACE, NARROW NO-BREAK SPACE, and a doubled space
    between the two halves. `[ ]?` matched U+0020 only, so a postcode pasted out
    of a PDF or rendered HTML leaked in full — the same failure class as the
    `ssn.us` NBSP regression.
  * `postal.gb` covers the Girobank pseudo-postcode (`GIR`, then `0AA`) and restricts the
    inward code to the official Royal Mail alphabet (never `C I K M O V`), which
    also drops matches overlapping a different gold label from 3 to 1. The AREA
    letters stay wide: encoding the official `Q V X` / `I J Z` exclusions was
    measured and LOST a gold entity on this holdout.
  * `postal.ie` covers the `D6W` Dublin 6W routing key, the one assigned routing
    key that is not `LETTER + 2 digits`. Every D6W address leaked in full before.

  The 4-digit locales (`de-AT`, `de-CH`, `en-AU`, `en-NZ`) were deliberately not
  covered by this change. Unanchored `\d{4}` is 19% precise on the holdout (516
  of 2,723 runs are gold ZIP) and fires 1,717 times across 62.5% of the negative
  corpus, so it requires an anchor; `de-AT` / `de-CH` now have one
  (`postal.at_ch`, above).

- **`birth_date.cue` in `core`** (`custom:birth_date`, every locale): a date of
  birth in a field record (`date of birth:`, `birth date:`, `birthdate:`,
  `DOB:`, `Geburtsdatum:`, with `:` or `=` at line start) or after `born on` /
  `geboren am`, in ISO, `DD.MM.YYYY` or slash form. The same change recovers a
  whole candidate that an explicit-field rule suppressed and then lost to a
  later rival, so its bytes are tokenized instead of left raw (PR #589).
- **`gaze-mcp-core` untrusted-invocation request mode.**
  `RequestMode::UntrustedInvocation`, `PiiEnvelope::dispatch_request` and
  `ToolCtx::invocation_args()` let an opted-in tool receive its execution
  arguments unchanged while the envelope audits a fixed omission marker and
  still protects the response. Both entry points reject a descriptor whose
  mode does not match before authorization or audit, and the new mode refuses
  operator response bypass. `request_mode` is not serialized, so wire metadata
  cannot opt in. `BeginCallContext.args_audit` is new: `None` for existing
  calls, a versioned constant for the new mode, which hosts that adopt it must
  persist. `SessionTransaction::restore_strict_text_bounded` checks the exact
  expanded UTF-8 size against a limit before it reserves output, then
  substitutes once without detecting, minting mappings or committing
  (PR #590).
- **`gaze::DetectError` at the crate root**, so an out-of-crate `Recognizer`
  implementation can name its error type without `gaze::registry` or a direct
  `gaze-types` dependency. The `gaze::registry` docs carry a complete
  out-of-crate example (PR #601).
- **`gaze-mcp-bridge` bounds its session cache** with `session.max_sessions`
  (default 1,000; `0` is a config error, also through
  `BridgeSessionStore::from_config`). At the cap a file store persists the
  least recently used inactive session before evicting it; an ephemeral store
  refuses the new session. A session that any caller still holds, even through
  a weak handle, is never evicted: admission fails with
  `BridgeError::LimitExceeded`, and a persistence failure with
  `BridgeError::SessionStore` (PR #578).
- **`Pipeline::admit_safety_nets` and `admit_safety_nets_transaction`** run
  every configured safety net over already pseudonymized text, with token
  coverage built from the session's owned values. `gaze proxy` uses them at
  request admission (PR #585).

### Changed

- **Breaking (custom `gaze-proxy` adapters):** `PiiSurface` has a new
  `syntax: SurfaceSyntax` field (`Text`, `Json`, or `ModelOutput`). Restore uses
  it to choose the escaping for each surface. `ProviderAdapter` has a new
  provided method, `requests_json_output(request)`, which defaults to `false`.
  If an adapter builds `PiiSurface` values directly, set `syntax:
  SurfaceSyntax::Text` to keep the previous verbatim restore. Use `Json` for
  fields that hold serialized JSON (PR #656).

- **Breaking:** `gaze setup --safety-net ner` is removed. Use `--safety-net none`
  for the former NER-only policy. `gaze setup` now installs Nym by default and
  writes an activating policy with the pinned model path. OPF remains opt-in
  and can be stacked by the printed command. The doctor proves Nym catches a
  synthetic plate; setup prints the model card MIT licence, pinned upstream
  revision, open training-data licence review, and opt-out.

- **Breaking:** `--safety-net-backend nym` now requires one explicit
  `--safety-net nym` selection. Add `--safety-net nym` to existing commands
  that used only the backend selector; the old form succeeded without
  activating a safety net.

- **Custom rulepack paths keep the `core` detection floor by default** (solo
  todo #3712; breaking in 0.x). Since the v0.4.0 rulepack policy loader, a
  `[policy.rulepacks]` table with `paths` but no `bundled` key silently selected
  no bundled packs. The same happened with policy-less `gaze clean
  --rulepack-path`. Omission now means `["core"]` on both surfaces. Explicit
  `bundled = []` and `--rulepack-bundled=none` keep custom-only behavior. A
  successful build prints a one-line stderr notice whenever the resolved
  bundled selection omits `core` and its `core-extended` alias, even if another
  bundled pack is selected without a custom path.
- **One entity, one token: containment precedence** (solo todo #3740,
  concept v2 approved 2026-09-23; breaking in 0.x). When one candidate
  wholly contains a candidate of a different class, the container wins the
  whole span as one token with its own class and action, unless its
  evidence tier is below the contained candidate's (validator passed >
  anchored or cue-structured match > plain regex or dictionary > learned
  NER; ties go to the container). The rung sits after collision-family
  policy and the mandatory-anchor rung and before the structured-containment
  rung, which it generalises and which remains for the containers the guard
  refuses. `IBAN PL56 0942 … 4500 BIC` (a spaced Polish IBAN, de-AT) is now
  `IBAN <iban_n> BIC` instead of five tokens; a Luhn-valid card whose tail is
  a German phone shape is one card token. Partial overlaps keep today's
  rules (solo todo #3769). Measured on the 98,256-document IBAN enumeration
  (3 locales): split IBANs 8,955 → 423, wrong-class IBAN tokens 11,196 →
  2,706, leaked and false-positive bytes unchanged; 4 of 1,886 real holdout
  documents change (`<credit_card_n><phone_n>` → `<credit_card_n>`); 0 of
  1,024 negative documents change.
- **Protection beats preservation: per-character residual coverage** (solo
  todo #3740; breaking in 0.x). Residual admission is per original, not per
  overlap component; a `preserve` winner no longer shields bytes a protected
  class claimed (they leave as a fragment of the highest-ranked claimant with
  `decided_by: protection_override`); adjacent fragments of one claimant
  merge into one; and a fragment emits under its claimant's own action
  (`tokenize`/`format_preserve` → class token, `redact` → one-way
  `[REDACTED:<class>]` marker, `generalize` → placeholder). This closes the
  hole where `custom:postal_code = preserve` shipped 20 raw IBAN bytes on
  the letter above (8,295 raw bytes across 480 enumeration documents) and
  where `custom:url = preserve` shipped an email inside the URL raw
  (the URL now carries an email token in place of the address). The candidates a preserved
  selection represents never override it, so an explicit
  `custom:family:<name> = preserve` rule still leaves the ambiguous span raw.
  Supersedes the interim "any protective action" admission from the
  strictest-member-action change. See
  [Residual coverage](docs/reference/redaction-classes.md#residual-coverage)
  and [UPGRADE.md](UPGRADE.md).

- **Audit rows of a policy-regex collision family name the members.** A
  loser row now carries the losing member's own class (it carried the
  winner's family class), and a family token's
  `ambiguity_record.losing_candidates` lists every member with its class (it
  was `[]`), because both are resolved through the registry by recognizer id.
  `recognizer_id` itself is unchanged: it was already the policy `name`.

- **Drift corpus: a Rust scope-separator line.** `[bundle-tokenization-drift]`
  snapshots for the `core` and `secrets` bundles change in `fixtures_sha256`
  only. The corpus had no code-shaped path, so the gate could not see the
  `ip.v6` word boundary at all; the new line must stay untokenized. Detections
  are unchanged at 12 for `core` and 2 for `secrets`, and no byte span or token
  shape moved, because the line is appended and tokenizes nothing.

- **The safety net no longer deletes: it writes a one-way `[REDACTED:<class>]`
  marker.** `SafetyNetMode::Redact` and the `Resolve` + `Redact` fallback used
  to replace a flagged span with the empty string, so the bytes vanished and
  nothing downstream could tell a redaction from a typo. They now write a
  visible marker (`[REDACTED:name]`, `[REDACTED:custom:phone]`), recorded as an
  ordinary non-owned manifest entry with `Action::Redact` -- the same shape the
  primary pass has always emitted for a redacting policy -- carrying the raw
  span and the ids of every suspect that drove it. **Which spans get redacted
  is unchanged; only what is written in their place changes.** The marker is
  deliberately outside the token grammar: restore passes it through as ordinary
  text, the strict restore scan does not flag it, and the class path renders
  lowercased with every non-alphanumeric byte except `:` mapped to `-`. Mapping
  `_` is what stops a custom class named `address_2` making the marker parse as
  the bare token shape `custom:address_2`; mapping the rest is what keeps the
  emitter and the predicate from drifting apart, because `PiiClass::Custom` is a
  public variant an adopter can build directly and `PiiClass::family` does not
  normalise. `gaze` re-exports `is_redaction_marker` as the single predicate
  every consumer should ask, and
  `is_redaction_marker(redaction_marker(class))` now holds for every class.
  **Behaviour change for adopters using `redact` (including the default
  `Resolve` + `Redact` fallback):** clean output now contains marker text where
  bytes previously disappeared, so it is longer, not shorter, for those spans.
  See `docs/explanation/safety-net/safety-nets.md#the-redaction-marker`.

- **BREAKING (behaviour): a collision-family token no longer falls to the
  `default` rule.** When no reachable rule names `custom:family:<name>`, the
  token takes the strictest action among its member classes' resolved actions
  and its own default (`Action::strictness_rank`); an explicit family rule
  before the default still wins verbatim. Member-only policies
  (`tokenize custom:iban`, `default preserve`) now protect no-cue IBANs and
  card-run collisions instead of shipping them raw. Adopters who relied on a
  family token falling to a `preserve` default must add an explicit
  `custom:family:<name> = preserve` rule before their default rule; see
  [How a family-level token picks its action](docs/reference/policy.md#how-a-family-level-token-picks-its-action).
  The `gaze clean` load-time notice and
  `gaze_assembly::uncovered_collision_family_classes` now fire when a policy
  names a member class (before or after the default rule) or a dead
  post-default family rule without a reachable family rule, whatever the
  default, and no longer claim a leak.
  Under an active protection trace (the MCP and proxy chokepoints) only
  `tokenize` and `preserve` are executable, so a family token that now derives
  `redact`, `generalize` or `format_preserve` fails closed there with
  `UnsupportedActionVariant`, the same error an explicit rule with that action
  on a member class already produced; nothing is emitted.
- **Nym suspects no longer carry JSON syntax at their edges.** Quotes,
  colons, commas, brackets, braces and whitespace are trimmed from both ends
  of a decoded span, and a span of syntax alone is dropped. Trimming only
  narrows a span, so a tool-call value like `"name": "Anna Müller",` yields
  `Anna Müller`, not `"Anna Müller",`.
- **BREAKING (`gaze-mcp-rmcp`): rmcp 2.x.** `gaze-mcp-rmcp`,
  `gaze-mcp-bridge` and `gaze-document` move from rmcp 1.6 to rmcp 2.x, whose
  `ContentBlock` replaces `Content` / `RawContent`. Adopters that name rmcp
  types next to `gaze-mcp-rmcp` must upgrade rmcp with it. MSRV stays 1.89.
  Because rmcp 2.x marks `ContentBlock`, `TextContent` and `Annotations`
  `#[non_exhaustive]`, bridge ingress now refuses any non-text content variant
  through a wildcard arm, and refuses a text block or its `annotations` object
  when either serializes a field it does not redact
  (`unsupported_content_field`), so a later rmcp release cannot widen what
  reaches the agent unredacted.

- **BREAKING: `gaze setup` installs the benchmarked NER model.** The default
  (`--safety-net ner`) used to install the Kiji distilbert-NER bundle as the
  primary `[ner]` model in the written policy. It now downloads and verifies
  the pinned Davlan mBERT bundle, the model the benchmark scores:
  `onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` at commit
  `cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8`, `SHA256SUMS` digest
  `7b0b9d0d200bf7f3a39654257f8723998316600852edff8404834eb7edfc5c16`, into
  `$XDG_DATA_HOME/gaze/models/davlan-mbert-ner-hrl` (else
  `~/.local/share/gaze/models/davlan-mbert-ner-hrl`). **Adopters who ran
  `gaze setup` before must re-run it** to get the benchmarked model. Setup now
  prints `For gaze index: export GAZE_NER_MODEL_DIR=<dir>`. New API:
  `gaze_model_setup::{install_ner_bundle, install_ner_bundle_with_fetcher,
  default_ner_model_dir}`, `gaze_recognizers::verify_davlan_ner_bundle` and the
  `DAVLAN_NER_*` constants, `NerRecognizer::load_pinned_davlan`, and
  `NerLoadError::PinnedBundle`.
- **BREAKING: `gaze index ingest` requires the pinned NER model.** The index
  used the Kiji net as its only prose name and organization detector. Ingest
  now requires `--ner-model-dir <dir>` or `GAZE_NER_MODEL_DIR`, and the
  directory must verify against the pinned Davlan digests. An absent or
  unpinned directory fails closed with the typed `IndexNerModelMissing` error
  (exit 2) and nothing is written. A safety net is optional for ingest and
  required for search (TokenBridge refuses a search without an output net, so
  `gaze index search` without one fails closed with `SafetyNetConfig`):
  `gaze index --safety-net {openai-filter|nym}` with `--opf-command` (or
  `GAZE_OPENAI_FILTER_OPF`), `--opf-checkpoint` (or `OPF_CHECKPOINT`),
  `--nym-model-dir` (or `GAZE_NYM_MODEL_DIR`) and `--safety-net-timeout-ms`.
  When configured it checks ingest output and search snippets, and residual
  suspects still redact or fail closed per `--on-residual {redact,strict}`.
- **BREAKING: credentials are no longer detected by default.** Credentials are
  not PII, so the two credential recognizers leave the `core` rulepack (now
  version 0.6.0) for a new opt-in bundled rulepack, `secrets`:
  `security_token.anchored` (`custom:security_token`) and `password.field`
  (`custom:password`) moved verbatim, with the same ids, classes, patterns,
  scoring and sources. `secrets` is never part of a default activation, not even
  when `[policy.rulepacks]` is omitted. To keep tokenizing API keys, access
  tokens, JWTs and `password:` records, load it next to `core` with
  `[policy.rulepacks] bundled = ["core", "secrets"]` or
  `--rulepack-bundled core,secrets`. `username.field` (`custom:username`) is
  removed outright: a line-start `username:` record rarely occurs in prose, and
  its measured rule-floor byte recall was 0.9 % of 1,034 gold bytes. Nothing
  emits `custom:username` any more. See UPGRADE.md.

  The v0.15.0 release benchmark shows what that means for a setup policy,
  which does not load `secrets` (`scorecard-v0.15.0.json`,
  `per_label_recall`, scored-label contract v1): 2,020 of 2,322 `PASSWORD`
  gold bytes, 4,220 of 4,342 `SECURITYTOKEN` bytes and 72 of 1,034 `USERNAME`
  bytes stay raw. Load `secrets` when credentials must be tokenized.

- [bundle-tokenization-drift] The `core` snapshot records rulepack version 0.6.0 and the extended drift corpus hash; its detection entries are unchanged, which proves the new credential fixture lines stay inert under `core`.

- [bundle-tokenization-drift] The new `secrets` snapshot pins exactly one `security_token.anchored` and one `password.field` detection on the credential fixture lines appended to the drift corpus.

- `cooperates_with` is now symmetric across all five `custom:postal_code`
  recognizers, and the stale research-855 collision comment in `core.toml` —
  which described a two-rule world — is replaced by a two-group policy note
  explaining why the numeric rules stay locale-gated and the alphanumeric ones
  do not.

- [docs] The `core.toml` line-citation column is **removed** from the recognizer
  coverage matrix in `docs/reference/redaction-classes.md`, and the doc gate now
  expects twelve columns. The column was documentation cosmetics that no code
  depended on and no gate verified: 35 of 37 rows had drifted, some by more than
  150 lines. Regenerating it (as an earlier entry in this cycle did) only reset
  a clock that would drift again on the next `core.toml` edit, so the column is
  gone rather than re-verified. The gate keeps checking every remaining column
  against the loaded rulepack. Recognizer definitions are found by searching
  `core.toml` (loaded as both `core` and `core-extended`) or `secrets.toml` for
  the `id = "..."` line.

- [bundle-tokenization-drift] The `core` snapshot records rulepack version0.5.3; detection entries, spans, classes, sources, token shapes and counts are unchanged.

- **A safety-net fallback document now gets one reversible round before it can
  be denied.** Under `SafetyNetMode::Resolve` with `SafetyNetFallback::Redact`,
  the terminal scan that runs after a fallback deletion used to deny the
  document on *any* unprotected suspect it reported. That scan is the fourth
  full model pass, and the deletion changes the whole input string, so it
  routinely reports a 1–5 byte sub-word span the three earlier passes read and
  accepted — a finding no stage was permitted to act on, over bytes the denial
  protected no better than completing would have. The terminal report now gets:
  one reversible round that tokenizes what it can (restore-exact, never
  deleted), one bounded deletion of a suspect that **contains** a deletion seam
  — a shape the fallback itself manufactured by joining two fragments — and then
  a typed admission. **Denials are now named:** a suspect covering bytes the
  fallback's own audit rows say it removed, a second seam-manufactured shape, a
  suspect that names no real range of the document, or a round the resolver
  refuses. Everything else is merged into the returned `LeakReport` and the
  document completes carrying it, exactly as completing documents already ship
  their own final report.
  **What this costs:** a fallback document that reports something at the
  terminal scan now runs one extra model pass, and a fresh finding that appears
  only *after* that round ships raw in the output with an honest report, because
  both bounds are spent. Measured on the v0.15 production corpus: the terminal
  scans reported 42 bytes across 16 spans that used to deny, of which 37 bytes
  are now tokenized reversibly and 5 bytes are the one seam-manufactured span
  the bounded deletion removed; **2 bytes, in one document, ship raw** after the
  round, and they overlap 0 gold. Those are measurements on that corpus, seed and
  model bundle, not a bound for other documents, and a shipped byte that overlaps
  no gold is not proof it is not PII — only that the benchmark does not count it.
  Admission is strictly wider than before, so no document that completed under
  v0.14 can start denying.
  Audit rows for the extra round are `decided_by: resolve`, `action: tokenize`
  with the fallback reason attached — the combination that distinguishes them
  from the second batch's rows. The protection trace projects them as an
  ordinary `("safety_net", "resolve", "tokenize")`; no new wire keys.
- **Clean-to-raw mapping understands deletions.** `map_clean_span_to_raw` and
  `validate_clean_manifest` previously assumed every untokenized clean run stood
  for an equal-length raw run, which a fallback deletion breaks. They now
  reconstruct the document's layout from the deletion ledger's raw coordinates
  and reconcile it against the manifest, so a manifest that disagrees with its
  own deletion ledger fails closed instead of mapping onto the wrong bytes. A
  document with no deletions takes the unchanged affine path. A resolution gap
  must now map to exactly as many raw bytes as it has clean bytes, which is what
  stops a token from ever standing for bytes on both sides of a deletion.

- **Residual coverage is on by default.** Every pipeline built through
  `Pipeline::builder()` now protects raw bytes that admitted originals evidenced
  but conflict resolution did not keep, instead of leaving them in the clear.
  One recognized value can therefore contribute more than one replacement, so a
  manifest span count is a count of *replacements*, not of distinct recognized
  values. Restore is unaffected. Bytes that no original evidenced remain
  uncovered. See
  [Residual coverage](docs/reference/redaction-classes.md#residual-coverage).
  The bundled `core` tokenization snapshot is unchanged.
- `EmittedTokenSpan` gained `origin: EmittedTokenOrigin` (`Whole` |
  `ResidualFragment`) so a consumer can tell a whole selection from a residual
  fragment before it indexes, canonicalizes or counts. `Whole` is the default
  and is omitted on the wire, so existing whole-span JSON is byte-identical and
  pre-v0.15 JSON reads back as `Whole`. `EmittedTokenSpan::new` keeps its
  signature; `EmittedTokenSpan::residual_fragment` is the new constructor.
  **Unmigrated readers:** there is no `deny_unknown_fields`, so a consumer built
  before v0.15 ignores the new key and counts a fragment as a whole span, and no
  version field distinguishes the two. Rebuild entity-counting consumers against
  v0.15.
- `gaze-document` `BundleReport::pii_token_count`, `pii_tokens_by_class` and
  `ClassCount::count` are documented as replacement counts, not entity counts.
  `bundle_version` is unchanged.
- `gaze-token-bridge` protects a residual fragment **by location**: a
  class-derived placeholder in the stored snippet, and no `CanonicalEntity`, no
  `IndexEntity` and no posting. Fragment raw bytes no longer reach the
  persistent index. Documented consequence: a residual fragment is **protected
  but unsearchable**. Whole entities remain searchable exactly as before.
- **Breaking: the policy schema gate matches `0.1.` instead of `0.1`.** The
  old prefix also accepted `0.10.0` and any later two-digit minor. Now `0.1.x`
  loads, and `0.10.0`, `0.2.0` and a bare `"0.1"` fail closed with
  `PolicySchemaUnsupported`, whose `supported` field reads `"0.1."`. Policies
  written by `gaze setup` already say `"0.1.0"`; change a hand-written
  `schema_version = "0.1"` to `"0.1.0"` (PR #576).
- **The prefix cache no longer skips detection.** `enable_prefix_cache()`,
  `PipelineOptimizationConfig::with_prefix_cache(true)` and both
  `PrefixCacheWriteMode` values stay source-compatible, but every input is now
  rescanned in full under its current field, locale, dictionaries, recognizers
  and rules, and no prefix is stored. Adopters who enabled it lose its speedup
  on growing inputs and should budget full-scan latency. Audit rows carry the
  real recognizer and rule decisions instead of `prefix_cache` provenance, and
  the test-support prefix counters return zero. The leak this closes is under
  Fixed (PR #579).
- **Locale chains fall through per span, not per document.** A class's rules
  at a later chain locale used to switch off for the whole document as soon as
  an earlier locale produced any candidate of that class. Now a later locale's
  candidate joins where no earlier-locale candidate of the same class overlaps
  it, so the earlier locale still wins per span. Under `[global, de-DE]`, a
  document with an international mobile number and a national Berlin number
  now tokenizes both instead of only the first, which switched
  `phone.national.de` off. Expect more tokens under multi-locale chains
  (PR #614).
- **Safety-net Resolve plans more before it falls back.** A truthful
  `PartialBleed` report with raw text on both sides of an owned token used to
  pick `OverlapConflict` and the configured fallback; every gap in the report
  is now planned against the original manifest without retokenizing owned
  entries (PR #588). Under Resolve+Redact a successful first resolve followed
  by actionable raw gaps gets one more complete reversible batch before any
  deletion, so residual bytes can become tokens instead of being deleted. A
  text policy runs at most four safety-net sweeps per call. Malformed
  follow-up metadata (invalid classes, ranges or UTF-8, a false gap claim,
  inconsistent coordinates) now refuses the document before any effect, and a
  later failure can leave the extra mappings and audit attempts in place
  (PR #591). The terminal behaviour after a fallback is described in the
  entry above.
- **Malformed primary geometry is refused.** Invalid or overlapping raw
  mappings from the primary pass now return `InvalidOutput` before policy,
  audit or token allocation, a new refusal for output that used to be accepted
  (PR #589).
- **`gaze-token-bridge` collapses internal whitespace in custom entities**
  before the HMAC projection, as its canonicalization contract documents:
  `Case  123` and `Case 123` now share a fingerprint. Re-ingest custom entities
  that contain repeated or non-space whitespace (PR #553).
- **`gaze mcp serve` writes terminal outcomes to `{call_id}.terminal.json`.**
  The start record `{call_id}.json` is no longer overwritten, so principal,
  tool, external session, redacted arguments and start time survive success
  and failure and join on `call_id` (PR #582).
- **Dictionary terms stop at hyphenated identifiers.** A hyphen counts as a
  connector only when an identifier character sits on its other side, so a
  term no longer matches one part of `AAA-BBB`, while `-AAA` and `AAA-` still
  match. A dictionary that relied on partial matches inside hyphenated words
  must list the full form (PR #568).
- **Byte-adjacent NER entities of one class stay separate.** Two entities that
  touch without sharing a byte used to merge into one pseudonym; each now gets
  its own, and overlapping chunk results still merge (PR #564).
- **`gaze daemon` reports failed eviction audit writes on stderr** as JSON with
  the session's generated `audit_session_id`, the eviction reason and a closed
  `Sqlite`/`Backend`/`Unknown` detail code, instead of dropping them silently.
  The caller's session ID and backend error text are never printed (PR #570).

### Removed

- **BREAKING: the Kiji DistilBERT safety net is removed.** On the
  2,910-document benchmark (2026-09-16 safety-net leaderboard) it recovered
  1,831 leaked gold bytes under scored-label contract v2 for +169,657
  false-positive bytes, an action precision of 2.5%. No safety net runs
  without a policy that selects one; the rules plus the pinned Davlan mBERT NER
  model are benchmark arm `pass2-ner`, and the policy `gaze setup` writes in
  this release adds Nym (see Changed). The `SafetyNet` trait, the
  `resolve` / `redact` / `strict` modes and fallback ladder, terminal
  admission, and the sub-word guard stay; the OpenAI Privacy Filter and
  Nym-small nets use them. Removed surface:
  - `gaze clean` / `gaze daemon` flags: `--safety-net kiji-distilbert`,
    `--safety-net-backend kiji-distilbert`, `--safety-net-add kiji-distilbert`,
    `--kiji-backend {subprocess,ort,tract,candle}`,
    `--kiji-distilbert-precision {fp32,int8}`, `--kiji-distilbert-command`,
    `--kiji-distilbert-model-dir`, `--kiji-distilbert-locales`.
    `--safety-net-registry` stays; `openai-filter` is now its only
    registry-capable backend.
  - Cargo features `safety-net-kiji`, `runtime-tract` and `runtime-candle` on
    `gaze-recognizers` and `gaze-cli`, and the `tract-onnx` / `candle`
    dependencies. The musl-static deployment path through `tract` is gone.
  - Rust API: the `gaze_recognizers::safety_net::kiji_distilbert` module
    (`KijiDistilbertSafetyNet`, `KijiBackendKind`, `KijiDistilbertPrecision`,
    `OrtKijiBackend`, `SubprocessKijiBackend`, `KIJI_DISTILBERT_BUNDLE_SHA256`,
    the int8 bundle pin, `verify_kiji_bundle`). In `gaze-model-setup`:
    `install_kiji_bundle*`, `InstallOptions`, `default_kiji_model_dir`, and the
    `KijiDistilbertPrecision` re-export.
  - Environment variables `GAZE_KIJI_DISTILBERT_COMMAND`,
    `GAZE_KIJI_DISTILBERT_MODEL_DIR`, `GAZE_KIJI_DISTILBERT_PRECISION`.
  - Scripts `scripts/fetch/fetch-kiji-safetynet-model.sh`,
    `scripts/bench/kiji-runner.py`, `scripts/bench/kiji-bench-scorer.py`,
    `scripts/bench/quantize-kiji-int8.py`.
  - The NER loader no longer accepts the Kiji structured `labels.json`
    manifest.
  - Benchmark arms `full-stack-kiji-resolve`, `pass3-kiji` and
    `pass3-locale-aware`. Committed release rows (v0.14.0) keep their Kiji
    measurements; the opt-in `full-stack-opf-resolve` and
    `full-stack-nym-resolve` arms stay.

  **Migration.** For a second opinion after the deterministic passes, use
  `--safety-net openai-filter` or `--safety-net nym` (install with
  `gaze setup --safety-net nym`). **Re-run `gaze setup`:** it previously
  installed the Kiji distilbert-NER bundle as the primary `[ner]` model in the
  policy it wrote. `gaze index ingest` now requires `--ner-model-dir` or
  `GAZE_NER_MODEL_DIR` (see Changed).

- **BREAKING (`gaze-document`, `pdf-input` feature):
  `gaze_document::extract::pdf::rasterize_first_page` is removed** (PR #650).
  It has had no callers since layout report v2 (#219) moved PDF ingestion to
  `extract_pages`. Use `extract_pages(path, PdfRasterConfig::new())`, which
  returns one `PdfPagePayload` per page: `VectorText` for pages with
  selectable text and `Raster(RasterizedPage)` for image-only pages. It has no
  single-page mode and does not rasterize pages that have selectable text.

### Fixed

- **`gaze-proxy` restores raw values into JSON documents as valid JSON.** The
  legacy OpenAI and Gemini adapters pasted raw values verbatim into answer
  fields that hold serialized JSON: Chat Completions
  `tool_calls[].function.arguments`, Responses `function_call` and `mcp_call`
  `arguments`, and JSON-mode answer text. JSON mode means OpenAI `json_object` or
  `json_schema`, or Gemini `responseMimeType: application/json`. Streaming
  deltas had the same problem. A value holding `"`, `\`, or a control
  character made the agent's tool call or structured answer fail to parse.
  Some values still parsed but changed silently: the UNC path
  `\\fileserver\new_hires` decoded as `\fileserver`, a newline, and `ew_hires`.
  Restore now JSON-escapes raw values in these fields and writes plain text byte
  for byte as before. A value captured inside a JSON string of the request, such
  as a field of a JSON tool result, is already escaped in the manifest. It is
  still written into these fields as it is, so it is not escaped twice. The
  Anthropic Messages codec and Gemini `functionCall.args` were already exact and
  are pinned by the same end-to-end suite (PR #656, solo todo #3837).

- Safety nets now scan manifest-owned and session-verified placeholders with a stable eight-byte
  surrogate prefix derived from the placeholder shape after removing the random
  session hex. Nym, OPF, and registry backends no longer change detections
  when a fresh session chooses a different random prefix. The scan view
  preserves byte offsets; observable clean output and restore mappings retain
  the original token bytes. Findings wholly inside a verified placeholder are
  discarded; findings that cross one are clipped to exposed bytes before
  policy or fallback can act, so fallback cannot replace an owned placeholder
  with a one-way redaction marker (PR #644).

  On the 2,910-document scored-labels-v2 Nym/NER replay with fresh random
  sessions, three pre-fix runs leaked 14,071–14,088 bytes (mean 14,078),
  produced 28,645–28,676 false-positive bytes (mean 28,659), and restored
  2,909–2,910 documents exactly. Each variant passed a 60-document replay
  across five fresh CLI sessions. The selected shape-derived hex mapping was
  scored again after merging main with identical results:

  | Scan prefix | Unstable documents / 60 | Leaked bytes | False-positive bytes | Exact restores | Redaction actions | Post-policy suspects |
  |---|---:|---:|---:|---:|---:|---:|
  | `00000000` | 0 | 14,116 | 28,638 | 2,907 | 4 | 0 |
  | `xxxxxxxx` | 0 | 14,706 | 28,598 | 2,909 | 1 | 0 |
  | No prefix, mapped offsets | 0 | 14,471 | 28,642 | 2,910 | 0 | 0 |
  | Shape-derived hex (selected) | 0 | 13,991 | 28,652 | 2,910 | 0 | 0 |

- **IPv6 after a glued address cue no longer ships raw.** The `core` `ip.v6`
  recognizer now accepts a fully parsed address immediately after `Address:`,
  `Adresse:`, `IP:`, `IPv6:`, `host:` or `addr:` (case-insensitively at every
  locale). The existing word guard remains
  in force, so Rust and C++ double-colon paths, including `Address::new`, stay untouched.
  The identifier-glued form `_2001:db8::1` remains outside this cue rule
  (solo todo #3762).

- **Security: a compact IBAN glued to the next word shipped raw.**
  `IBAN AT6119…3201BIC` and the dense footer `IBAN:AT6119…3201BIC:BKAUATWW`
  (a compact Austrian IBAN, elided here) cleaned to themselves with
  `detections: 0`, an empty leak report and a success exit, in every release
  from v0.4.3-rc.1 (#48) through v0.14.0 and on main after #622. The
  `iban.structural` pattern ended in `\b`, so a candidate immediately followed
  by a letter or digit was never a candidate at all. The boundary was also
  load-bearing the other way: without it the exact-length branches match a
  checksum-valid prefix of a longer opaque token (`ref AT611904300234573201XQ7
  end`), and Rust's `regex` has no lookahead to say "not followed by more
  identifier". The trailing boundary now lives in code, next to
  `is_inside_word`: `gaze_types::word_run_extends_identifier` reads the word
  run after a validated registry-length candidate with the same word predicate,
  accepts it when the run is empty or letters only (Unicode `is_alphabetic`, so
  `…3201und` and `…3201Überweisung` behave alike), and rejects it when the run
  holds a digit or an underscore. `RegexDetector` applies it to every
  `iban_mod97`-validated recognizer, and the pattern's trailing `\b` is gone.
  One shape is recovered only in part: a label glued to a SPACED German IBAN
  (`IBAN DE89 3704 … 0130 00BIC`, the German example IBAN) is now a candidate, but
  `phone.national.de` (priority 85) still wins the `0532 0130` sub-run, because
  its 22-character IBAN-consuming branch ends in `\b` and stops consuming at the
  glued label. Under a policy that tokenizes `custom:phone` every byte is
  covered (`<iban_n><phone_n><iban_m>`, where main left 18 bytes raw beside one
  phone token); under a phone-preserving policy it stays raw as on main.
  Dropping that `\b` too was measured and rejected: it makes the branch consume
  the first 22 characters of every longer spaced IBAN, which repairs 1,866
  fragmented documents per German policy but uncovers 7,212 bytes that an
  accidental phone token had hidden on digit-glued documents, so it is a
  separate change with its own trade (solo todo #3764).
  **Residual gap, by design:** an IBAN glued to a digit or an underscore
  (`…32011234`, `…3201_x`) stays raw exactly as before, because it is
  indistinguishable from a longer opaque identifier; accepting every validated
  prefix would tokenize 1 % of every registry-shaped upper-case token of any
  length (mod-97 false-accept, measured 1.02 % over 200k random tokens), while
  the letters-only rule's false-accept decays with the glued run's length
  (1 % × (26/36)^k for an upper-case alphanumeric run of k characters, so 0.7 %
  at k = 1 and 0.07 % at k = 8). The Dataiku EN/DE holdout, the A4 negative
  corpus and `docs/**/*.md` are byte-identical under the old and new boundary
  (the A4 corpus holds no registry-shaped mod-97-valid token at all), so the
  evidence is the synthetic enumeration: base vs fix over
  `scripts/bench/iban_trailing_word_enumeration.py`, now 98,256 documents with
  ten glued trailers, scored on output bytes. Fixtures in
  `crates/gaze-recognizers/tests/iban_trailing_group.rs` pin both directions
  and the spaced-German phone interplay; the enumeration script now reads the registry
  length table out of `crates/gaze-types/src/lib.rs` instead of carrying a
  third hand-copied table. Solo todo #3756.
- **Security: a collision family of policy regex recognizers shipped its
  family token raw under a `preserve` default.** A `kind = "regex"`
  `[[policy.custom_recognizers]]` rule registered through the `Detector`
  wrapper, whose `Recognizer::id()` is the constant `legacy-detector`, so the
  registry could not find it by the policy `name` its
  `[policy.custom_recognizers.collision]` membership is filed under. The
  candidate side always carried the policy `name`: precedence decided, an
  equal precedence emitted `custom:family:<name>`, and a missing
  `mandatory_anchor` cue fell back to it, as documented. But
  `RecognizerRegistry::family_member_classes` saw no member, so the
  strictest-member derivation credited nothing and the family token took the
  policy default: `ticket CASE-0001 open` left `gaze clean` unchanged with
  zero detections and exit 0 under two `tokenize` members and
  `default = "preserve"`; the same two rules without collision metadata
  tokenized it. Dictionary rules (`dict/<name>`) were never affected. The
  mismatch dates from v0.7.1 and was invisible until the derivation landed in
  this cycle (#624), because every family token took the default before.
  Policy regex rules now register as `Recognizer`s under `detector.name`, at
  the score the wrapper hard-coded (1.0) and on the format basis, so the
  conflict ladder and the per-locale candidate pool see the same candidates
  as before; only the registry lookups change. Pinned through
  `gaze_assembly::build_pipeline` and the `gaze` binary for the tie, the
  strictest-member (`redact`) tie, both precedence directions of the
  `docs/reference/policy.md` example, and the anchored member with and
  without its cue. Measured both directions with
  `scripts/bench/policy_regex_collision_matrix.py` (36 arms, synthetic set
  plus the Dataiku en/de holdout): 1,907 documents, 658 expected spans, `lost_bytes = 0` and `lost_values = 0` in every arm; the only changed documents are 427 family tokens (395 documents, +2,162 protected bytes) that base shipped raw under a `preserve` default and head protects, and the same tokens written as `[REDACTED]` where a `redact` member outranks a `tokenize` default; every collision-off and preserve-member arm is byte-identical, so no conflict winner moved. Solo todo 3757.

- **Security: a custom policy naming only member classes shipped no-cue IBANs
  raw.** The mandatory-anchor fallback and precedence-tie family token
  (`custom:family:payment-card-or-iban`, emitted since v0.7.1 whenever no IBAN
  cue is in range or a Luhn-valid card run collides with the IBAN) resolved its
  action by its own class, which member-only policies never name, so it fell to
  a `preserve` default and the whole IBAN left the process with a success exit
  (`Überweisung DE89 3704 … 0130 00`; `Bitte überweisen auf FO14 5878 …
  1234`, IBANs elided here). Documented as a footgun with a stderr warning since
  v0.11; the north star does not let protection depend on reading a warning.
  The action is now derived from the member rules (see Changed), through the
  one resolver every surface shares (`gaze clean`, `gaze daemon`, proxy, MCP,
  index). The residual-coverage cell path, which re-implemented the lookup
  inline, now uses the same resolver: under de-DE a long no-cue IBAN whose
  sub-run `phone.national.de` wins previously left the rest raw, and with the
  derivation alone would have failed the whole document closed
  (`residual policy preview mismatch`); its remaining bytes now carry
  family-class residual tokens. Solo todo #3746.
- **Security: residual coverage switched off under any action but `tokenize`.**
  The cells that cover a losing candidate's remaining bytes beside an
  overlapping winner were planned only when every previewed action in the
  overlap was exactly `tokenize`. A stricter action anywhere in the component
  (a `redact` member rule reaching the family class through the derivation
  above, or an explicit `default = redact`, which reached the same gate before
  this release) silently dropped every cell, and the loser's bytes left the
  process raw with a success exit: under de-DE, `custom:credit_card = redact`
  with a tokenize default shipped `AD56 7551` and `9893` of a no-cue IBAN
  beside the phone token that won its middle (872 of 18,556 family documents
  in the review matrix). Admission is now "every action in the component is
  protective" (`Action::is_protective`) at the planner, the plan's runtime
  check and the residual cell's own lookup; admitted cells keep emitting
  tokens. The same gate also reaches losers whose own rule is `redact`,
  `generalize` or `format_preserve`: a `redact` name overlapped by a winning
  email used to leave its remaining bytes raw beside the email token and now
  leaves them as a name token, with a residual audit row
  (`provenance_stage = "primary_pipeline.residual"`). Found by the review of
  the derivation change; the fix and the derivation ship together, so no
  release carries the regression.
- **Precision: `ip.v6` tokenized Rust and C++ double-colon paths mid-identifier.**
  The double-colon shorthand makes a great many path segments legal IPv6
  addresses: the colons plus `a` inside `CleanOverrides::apply_to`, the colons
  plus `defa` inside `Policy::default()`, `d`, the colons and `f` inside
  `std::fs::read`, and the bare colon pair wherever a path has no hex on either
  side.
  The `ipv6_parse` validator accepts every one of them, because they really are
  RFC 4291 addresses. The rule's guard class excluded hex digits only, so any
  other identifier character satisfied it and the candidate fired inside the
  word, rewriting `gaze::rule::resolve` as `gaz<...>rul<...>resolve`. Six or
  more pull-request bodies were mangled this cycle. The guard is now a
  word-character class, the same edge rule as `gaze_types::is_inside_word`. It
  is a strict subset of the old class, so the change can only remove matches:
  a differential enumeration over 256 address forms x 24 prefixes x 24 suffixes
  finds no case where the new rule matches and the old one did not, and no
  whole address lost in a context whose delimiters are not identifier
  characters. Across this repository's own `docs/**/*.md` the class drops from
  176 matches (616 bytes) to 6 (47 bytes), of which five are IPv4 loopbacks and
  one is a literal double-colon example. A standalone all-hex path with no context
  either side (two hex letters joined by a double colon) is still read as the address it is. No detection is
  added; this is a precision fix, not a leak fix.

- **Security: an IBAN followed by an upper-case word could match nothing at
  all.** Whichever of two outcomes an adopter got depended only on whether the
  IBAN's digits happened to be Luhn-valid: for BE, and for any IBAN whose BBAN
  is not a Luhn-valid card run, the WHOLE IBAN shipped raw with `detections: 0`,
  an empty leak report and a success exit; for AT, EE, LT, LU, CZ, PL, HU and LC
  shapes whose digits are Luhn-valid, `card.structural` claimed the digits and
  the country code and check digits leaked raw beside a `custom:credit_card`
  token (5 raw bytes at length 20, 10 at 24, 15 at 28, 20 at 32).
  Shipped defect in every release from v0.4.3-rc.1 (#48) through v0.14.0.
  `iban.structural` matched
  `\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){2,7} ?[A-Z0-9]{1,4}\b`. Both the repeated
  four-character group and the mandatory one-to-four character tail accept an
  optional LEADING space, so an upper-case or digit word written after the IBAN
  was absorbed into the candidate — ` SWIF` as a whole group plus `T` as the
  tail, or ` BIC` as the tail alone. The over-long candidate then failed
  `iban_mod97`, which gates on the country's registry length, so validator veto
  dropped it. `IBAN … BIC: …` is the standard European invoice and
  e-mail footer layout, so this fired on ordinary documents:
  `IBAN AT61 1904 … 3201 BIC: BKAUATWW` (a spaced Austrian IBAN, elided here)
  cleaned to `IBAN AT61 <…:Custom:credit_card_n> BIC: BKAUATWW`, and
  `IBAN BE62 … 9627 SWIFT GEBABEBB` (Belgian) cleaned to itself. The pattern now
  carries one alternation branch per ISO 13616 registry length, with exact
  repetition counts only, so the candidate stops at the country's real IBAN
  length. This is a strict narrowing that costs no recall: every candidate the
  new pattern declines to match was already rejected by the validator's length
  gate, so it could never have produced a token. Measured base vs fix over
  55,536 documents (89 registry countries × 2 BBAN alphabets × 4 seeded valid
  IBANs × spaced/compact × 3 prefixes × 13 trailing contexts,
  `scripts/bench/iban_trailing_word_enumeration.py`, 5 policies): about 7,400
  documents per policy go from leaking to fully covered, and no IBAN byte is lost
  that main protected on the same IBAN with no trailing word. The remaining
  losses (275 B under de-DE, 1,715 B under de-AT, all in documents with no IBAN
  cue) are the pre-existing family-fallback class tracked as todo 3746: with
  `custom:family:payment-card-or-iban` left to a policy's default `preserve`,
  main's extra coverage in those documents came only from the swallowed word.
  Fixtures in `crates/gaze-recognizers/tests/iban_trailing_group.rs`
  cover every registry country, and
  `iban_pattern_branch_lengths_match_the_validator_registry` pins the pattern's
  length branches to `gaze_types::iban_registry_length` so the two tables
  cannot drift apart.

- **Security: `gaze index ingest` now runs the `core` rulepack, so
  `gaze index search` no longer prints identifiers raw.** Shipped defect in
  every release from v0.11.0 through v0.14.0: ingest built its own pipeline
  (email regex, `Label: value` fields, NER, default rule preserve) without
  `core`. Credit card numbers, IBANs, IP addresses, phone numbers and every
  other `core` class stayed raw inside the stored snippet, and
  `gaze index search` printed them to the agent-facing output under the
  footer "raw PII never shown", with the required output net in place (the
  Nym net does not flag these shapes). The encrypted store never held them in
  plaintext at rest. Ingest now resolves the same policy as a policy-less
  `gaze clean` (bundled `core`, tokenize default rule) and adds the pinned NER
  bundle, the field detector and the optional net on top, so the two verbs
  share one deterministic floor. **Re-run `gaze index ingest` on every
  existing index**: stored snippets keep the raw values until then. No
  workaround exists on older releases; do not pass their search output to an
  agent for documents with structured identifiers. `gaze-assembly` gains
  `build_pipeline_builder` and `CorpusIngestor` gains `with_dictionaries`
  (solo todo #3711).
- **Security: `gaze clean` without `--policy` now runs the `core` rulepack.**
  Shipped defect in every release from v0.3.0 through v0.14.0: with neither
  `--policy` nor `--rulepack-bundled`/`--rulepack-path`, `gaze clean` ran a
  stub pipeline that tokenized only email addresses. Credit card numbers,
  IBANs, IP addresses, phone numbers, national IDs and every other `core`
  class went out raw, while the run reported success. The documented default
  (`["core"]` when `[policy.rulepacks]` is omitted) held only for policy files.
  A policy-less run now resolves the same policy as `--rulepack-bundled core`,
  with a tokenize default rule, which also keeps `--context-json` dictionary
  terms tokenized. **Adopters calling `gaze clean` without a policy now get
  tokens for values that used to pass through; this is intended.** Workaround
  on older releases: pass `--rulepack-bundled core`. `gaze daemon` always
  required a policy; `gaze mcp serve` and policy-less `gaze proxy` already ran
  `core`. The now unreachable `UnsupportedSessionScope` CLI error variant is
  removed (solo todo #3706).

- **A family settled by collision policy stays settled when a later,
  unrelated overlap is decided.** Shipped defect in v0.14.0 (since the
  resolver began relabelling the incumbent with the deciding rung,
  3878c5f9): when `iban.structural` beat `card.structural` by collision policy
  and a lower-priority recognizer outside the family then overlapped the
  IBAN (for example a four-digit group plus the next capitalised word), the
  base-ladder rung overwrote `decided_by`. The missing-anchor fallback keyed
  on that label, so the settled IBAN was anchor-checked again and, with no
  cue in range, became the `family:payment-card-or-iban` token.
  - Axis 1: under a policy that tokenizes `custom:iban` and
    `custom:credit_card` with a preserve default and no family rule,
    `Zahlung an AT61 1904 … 3201 Kontoinhaber Max` (IBAN elided here) shipped the IBAN
    raw (reproduced on main with a custom recognizer as the unrelated overlap,
    and with `postal.at_ch` from #613).
  - Axis 4: the IBAN's class depended on whether an unrelated overlap existed.
  The resolver now records collision-policy settlement as its own internal
  state, set on a collision-policy win or a precedence-tie family token and
  kept across later ladder wins, merges and collateral removals, and the
  fallback keys on it. `decided_by` keeps its audit meaning (the last rung that
  touched the span), so a settled IBAN that later beat an unrelated rival on
  rule priority is audited as `RulePriority`. An IBAN whose family was never
  settled by policy still takes the missing-anchor fallback. Enumeration
  script: `scripts/bench/collision_settled_enumeration.py`.

- **The OpenAI Privacy Filter safety net now reads OPF span offsets as
  characters, not bytes.** Shipped defect since the `openai_filter` backend
  landed in v0.6.0: OPF reports `start`/`end` as Python string indices (Unicode
  characters), and the subprocess adapter used them as UTF-8 byte offsets into
  the clean text. Any multibyte character before a span (an umlaut, `ß`, `€`, an
  en dash, an NBSP) shifted it left. The shifted span then either failed closed
  (`opf returned out-of-bounds span`, or a clean-to-raw mapping failure when it
  landed inside a Gaze token) or, **when it happened to land on valid
  boundaries, silently checked and protected the wrong bytes**. On the
  full EN/DE population 624 of 655 OPF-arm refusals were on documents with
  non-ASCII text. The adapter now converts character offsets to byte offsets
  once, against the exact text sent to OPF; an offset past the last character
  still fails closed as `InvalidOutput`. The Python OPF bench scorer
  (`scripts/bench/safety_net_bench_lib.py`) had the same defect and is fixed.

  Measured on the fixed 300-document EN/DE subset (`mode-opf-resolve-redact`,
  contract v2): refusals fall from **75 to 5**, completed documents from 225
  to 295. On the 225 documents that completed both before and after, leaked
  bytes fall from 373 to 350 and false-positive bytes from 2,669 to 2,628,
  which is the wrong-bytes effect going away. The 70 newly completed documents
  leak 128 of 4,289 gold bytes. No document went from completed to refused.
  The 5 remaining refusals include pure-ASCII documents and have a separate
  cause. The line-by-line stdin limitation noted in review is fixed in the next
  entry.

- **The OpenAI Privacy Filter safety net now has the stock `opf` CLI analyse the
  whole clean text as one input.** Shipped defect since v0.6.0, verified against
  the pinned CLI (`privacy-filter` @ `f7f00ca7`): piped stdin is read one line
  at a time, blank and whitespace-only lines are skipped, and every line gets
  its own JSON result with offsets relative to that line. Clean text with two
  non-blank lines failed closed as `opf stdout was not valid JSON`. **Clean text
  whose only non-blank line followed blank lines came back as one valid result
  whose spans landed too early, so the wrong bytes were checked and protected
  with no refusal.** The CLI also prints an ANSI colour section after the JSON
  unless `--no-print-color-coded-text` is passed, so with Gaze's default
  arguments every call to the stock CLI failed closed; the silent case needed
  that flag in the configured arguments. The benchmark daemon bridge sends the
  whole text and was unaffected.

  The adapter now appends `--no-print-color-coded-text --text-file /dev/stdin`
  after the configured arguments, so the text still travels over the pipe and
  never touches disk. `opf` reads that file in Python text mode, which turns
  `\r\n` and a lone `\r` into `\n`; the adapter maps OPF's offsets back through
  that translation to UTF-8 bytes. It also refuses, as `InvalidOutput`, any
  result whose echoed `text` differs from the text it sent (`opf analysed a
  different text than the one sent`), more than one JSON document, and output
  without the echoed `text` (a bare span array is no longer accepted). Empty
  clean text returns no spans without starting `opf`, which prints nothing for
  an empty file. **Adopters wrapping `opf` in their own command must accept
  `--no-print-color-coded-text --text-file <path>` and echo the analysed text.**
  On Windows there is no `/dev/stdin`; multi-line text is refused there instead
  of mis-mapped. The Python OPF bench scorer uses the same whole-text input and
  fails loudly instead of scoring only the first line of a multi-line fixture.

- **The Kiji safety net no longer tokenizes or deletes parts of words.** Shipped
  defect since at least v0.14.0: the shared Kiji decoder (ORT, tract, candle)
  merged BIO labels per WordPiece, so the pinned English model's piece-level
  firings on German text became suspects such as `G`/`em`/`ä` and the resolve
  path emitted `<Name_n>wort` for `Passwort` and three adjacent name tokens for
  `IBAN`. On the 80 explorer documents, 732 of 961 safety-net tokens were
  mid-word on v0.14.0 and 749 of 977 on 9a3a788. Spans are now assembled from
  whole words: any labelled piece labels its word, so byte coverage is a
  superset of the old output. As defense in depth for every net, a name,
  location or organization suspect that starts or ends inside a word is never
  acted on by any `Resolve` or `Redact` stage; it stays in the report with a new
  `LeakReportTelemetry::UnactionableSubword` row. Same 80 documents, same
  binary flags: safety-net tokens 977 → 587, mid-word 749 → 0, leaked gold bytes
  1,537 → 1,418 with no document rising, refusals 0 → 0, documents reaching the
  one-way `Redact` fallback 13 → 7. **Behaviour change:** whole words the model
  mislabels (`verpflichtet`, `Hauptniederlassung`) are now tokenized whole
  instead of in pieces, and the fallback deletes whole mislabelled words
  (80 bytes, none gold) instead of pieces (54 bytes). Model precision on German
  is unchanged and tracked separately. **Known limitations:** a net that does
  not decode whole words (OPF, the Kiji subprocess backend, adopter nets) can
  still report a sub-word name, location or organization suspect; under
  `Resolve` with the `Redact` fallback and in `Redact` mode it now ships raw
  with a `Preserve` audit row and an `UnactionableSubword` row where earlier
  releases tokenized or deleted part of the word, and under the `Strict`
  fallback the document is refused. The Kiji tokenizer truncates input at 512
  word pieces and the net does not chunk, so text past that point is not
  checked by the net and no telemetry says so.
- Custom class names that normalize to empty (for example `custom:!!!`) are now
  a typed load-time error. `PiiClass::custom` returns `Result<PiiClass,
  EmptyCustomClassName>`; callers must handle invalid names. Live and staged
  sessions also reject empty custom classes constructed directly through the
  enum before changing session state. Valid session tokens continue to
  round-trip through the token bridge's strict parser (#507).
- **Security: the opt-in prefix cache could return raw PII.** Affected v0.9.0,
  where the cache shipped (PR #252), through v0.14.0, when the prefix cache
  was enabled. A repeated or extended input replayed stored detection
  decisions, so after a locale, dictionary, custom rule, custom recognizer or
  pipeline policy change, or when an appended suffix completed a value
  (`alice@` growing into a full address), bytes the current configuration
  protects left the process raw. Every input is now rescanned in full; the
  cost is under Changed (PR #579).
- **Security: `gaze proxy` never ran a configured safety net on request
  text.** Affected v0.13.0, the first release whose proxy accepts a safety
  net, through v0.14.0. Surfaced request text reached the Anthropic and OpenAI
  providers after primary pseudonymization alone, so a residual only a net
  would catch was forwarded raw. Requests now pass safety-net admission after
  primary pseudonymization and before the provider call: a raw gap, a
  malformed suspect or a net error refuses the request, and a net that
  re-flags text inside an owned token is allowed. Admission ignores observer
  skip optimizations and runs every backend the locale chain selects, which
  adds inference time to each request. Responses and proxies without a net
  are unchanged (PR #585).
- **Security: `gaze proxy` forwarded raw bytes after a safety-net fallback
  deletion.** Affected v0.13.0 through v0.14.0. Both residual checks read only
  the surviving manifest entries, and a Redact fallback deletion leaves no
  entry, so a net-only span the fallback deleted looked like "no PII" while
  the checks still held the original bytes; the request, or a buffered JSON or
  SSE response, went out with `Ok`. Both checks now fail closed when a fallback deletion
  leaves them nothing to check (PR #593).
- **Security: a Resolve+Redact fallback deletion could leave newly detectable
  raw text.** Affected v0.8.1, where the fallback modes shipped (PR #223),
  through v0.14.0. Resolve could tokenize a suspect, delete a follow-up
  residual through the Redact fallback and return `Ok` without scanning the
  changed text, so raw text the deletion exposed shipped. A terminal scan now
  checks the final output, and malformed registry spans are refused before
  manifest correlation could drop them; how that scan admits and refuses is
  described under Changed (PR #584, PR #586).
- **Security: strict protection ran only the first chain locale's safety
  net.** Affected v0.13.0, where strict protection shipped, through v0.14.0.
  Under `[en-US, de-DE]` a benign global fallback ran for English and the
  German-only net was skipped, so an IBAN-shaped residual came back unchanged,
  and validation made the same first-locale choice and accepted the
  configuration. Validation and dispatch now resolve backends over the whole
  chain, and a chain locale with no covering backend is refused at validation
  time. Observer mode keeps first-match selection (PR #574).
- **Security: `--rulepack-path` without `--policy` preserved the classes its
  rulepacks detected.** Affected at least v0.4.5, where the synthesized
  policy's class rules first shipped, through v0.14.0; earlier releases were
  not checked. The synthesized policy generated tokenize rules only from
  bundled rulepacks, so custom PII found by a path rulepack left raw. Every
  bundled and path rulepack now contributes its classes (PR #545).
- **Security: a skipped optional-cue recognizer could hand a collision family
  to a `preserve` rule.** Affected v0.7.1, where collision families shipped,
  through v0.14.0. Collision metadata was registered before the recognizer was
  built, so an optional cue recognizer that was then skipped still lowered a
  live variant's precedence and the wrong `preserve` variant won. Metadata is now
  registered only after construction succeeds (PR #558).
- **Security: `gaze proxy` could return restored PII in a carrier assembled
  across Anthropic text blocks.** Affected v0.13.0, where the carrier guard
  shipped, through v0.14.0. The guard checked each text block on its own; it
  now also checks the joined restored text in JSON, NDJSON and SSE responses
  before residual validation (PR #544).
- **Security: OCR email repair closed only the first gap in a multi-label
  domain.** Affected v0.9.0, where the repair shipped, through v0.14.0.
  `user@mail. corp. example. invalid` left the address tail outside
  detection in `gaze document clean`; the repair now repeats until no gap is
  left (PR #565).
- **`gaze proxy` no longer copies upstream response headers on the legacy
  path.** Cookies, hop-by-hop headers and infrastructure metadata from the
  provider reached the client. The response is rebuilt with only the canonical
  JSON or SSE content type for the transformed body (PR #549).
- **`gaze proxy` pseudonymizes structured Responses API message text.**
  `input_text` and `output_text` parts inside message content were not
  surfaced, so the residual scan refused requests that contained PII there
  (PR #548).
- **Known session tokens restore after a leading word character.**
  `rec_<prefix>:name_1` restores to `rec_` plus the value instead of failing
  strict restore with `UnknownToken`; a known family token no longer swallows
  a longer unknown one (PR #581). Family namespace tokens also round-trip in
  prose restore, matched as a whole rather than at their tail (PR #552).
- **`gaze_read_file` accepts ordinary filenames** such as `scan_1.png`; token
  syntax in the path is validated by the central session scanner and malformed,
  nested, foreign and legacy placeholders still fail closed (PR #571).
- **`gaze index search` without `--class` searches every class the index
  domain declares**, so indexed organizations and custom entities are found;
  an explicitly disallowed class is still refused (PR #569).
- **Verbose safety-net subprocess diagnostics no longer abort inference.** A
  healthy OPF child that wrote more than 256 bytes to stderr with diagnostics
  on used to fail; the adapter keeps a bounded prefix and drains the rest, and
  the shared redactor no longer shows an unfinished raw token at the cut. On
  Windows the adapter uses non-blocking pipe writes and reads only available
  bytes, with diagnostics on or off (PR #580).
- **Pipeline and resolver fixes:** an empty optional model registry no longer
  raises a coverage error at runtime when custom nets run (PR #561); an inline
  TOML comment after `strict_locale_overlap = true` no longer disables the
  strict check (PR #562); name spans with a trailing particle after multibyte
  whitespace no longer panic (PR #563); regex exclusions match regardless of
  ASCII case (PR #567); family-level candidates keep earlier losing
  recognizers in their audit provenance (PR #566).
- **Audit fixes:** `gaze audit` JSONL export carries all seven restore
  telemetry fields (PR #555); ingress-blocked `gaze-mcp-bridge` results are
  audited as `Blocked` with the deciding rule instead of `Allowed` (PR #554);
  `BundleReport.clean_char_count` counts the final `clean.md`, header and
  trailing newline included (PR #556).
- **Concurrency and lifecycle fixes:** concurrent `gaze-mcp-core` dispatches
  with unchanged argument mappings no longer conflict (PR #546); `gaze proxy`
  daemon cleanup deletes a pidfile only if it still is the file it locked, and
  reports lock I/O errors (PR #572); the proxy dashboard accepts normal browser
  navigation headers (PR #547), keeps browser purge notifications on their own
  socket (PR #550), stays up when idle (PR #551), and waits for the child's
  ready message before pairing completes (PR #592).

### Performance

- NER now runs once per document instead of once per locale-chain step. Since the
  per-span locale fall-through, the registry called every document-basis recognizer at
  every chain step, so the 15-step `gaze setup` chain ran the same NER inference 15
  times per document and the two-step rules+NER chain ran it twice. Recognizers whose
  output ignores the step locale (NER, regex, anchored-match, dictionary) now declare it
  through the new `Recognizer::detect_is_locale_invariant` method (default `false`), and
  the registry reuses their first result at later steps. Per-span claiming is unchanged,
  and clean text, manifests, and audit rows are byte-identical. Custom recognizers keep
  one call per step unless they opt in (PR #653). Measured latency is in the
  Performance summary at the top of this section.
- NER chunk planning borrows the tokenizer when truncation is already off
  instead of cloning it and its WordPiece vocabulary on every call; configured
  truncation keeps the clone. Output is unchanged (PR #587).
- The occurrence ledger's origin-agreement guard runs once per record instead
  of once per segment, which was quadratic in the number of carried tokens on
  the restore-boundary protection paths. The guard also runs on a ledger with
  no segments, where it used to be skipped; no production path reached that
  case (PR #602).

## [0.14.0] - 2026-09-11

### Fixed

- **The ORT NER backend fails closed on missing, malformed, and nonfinite model
  output** (#474). A missing output tensor, a tensor whose rank or dimensions are
  not `[1, seq_len, num_labels]`, a flat buffer whose length does not match those
  dimensions, and any non-finite (`NaN` or `Inf`) logit each raise a typed
  `NerRuntimeError::Output`, which the recognizer boundary surfaces as
  `DetectError::Backend` and the pipeline surfaces as `Error::RecognizerDetect`.
  The first two cases previously returned `Ok` with an empty span list and the
  third was never checked, so a corrupt model output was indistinguishable from a
  document that contains no PII, and raw PII was forwarded downstream with no
  error and no audit trail. `NaN` was the sharpest case: it loses every comparison
  in the argmax fold, so a fully corrupt logit row decoded as label `O` at maximum
  apparent confidence. The backend also validates `shape[2]` against `id2label`
  instead of trusting it, removing an out-of-bounds slice. Detection is fail-closed
  again on this path. See
  [NER fail-closed](docs/explanation/detection/ner-failclosed.md).
- **Strict restore no longer falsely fails on identifier-shaped literals** (#473).
  A byte-exact inverse containing `Kunde_7`, `ORDER_12345`, or `FOO_12` previously
  reported `failed` because one post-restore lexical count was assigned to both
  `unknown_token_count` and `manifest_bypass_count`. Core pipeline telemetry,
  Session strict restore, the MCP `restore_strict` tool, and CLI restore now share
  one provenance-aware classifier: bare identifier-shaped literals outside
  authorized ranges are audit-only `manifest_bypass`, while every unmapped
  canonical placeholder (own-prefix, foreign-prefix, legacy wrapped) and every
  incomplete prefixed wrapper still blocks. Legacy emitted formats remain
  blocking. The change deliberately narrows the heuristic rejection boundary and
  keeps the release aligned with the project contract: fail closed, preserve
  reversibility, keep PII out of agent-visible surfaces. Serde-defaulted
  `trap_shape_count` telemetry and nullable `restore_trap_shape_count` audit
  metadata are additive. Snapshot formats and token grammar are unchanged. Some
  historical `failed` decisions become `success` on new runs; byte-exact restore
  and PII detection metrics are independent.

### Changed

- **`RecognizerRegistryBuilder` dropped its private always-empty `validators` and
  `canonicalizers` maps** (#469) and initializes both once in `build()`. The
  public surface is preserved: `Validator`, `Canonicalizer`, `ValidationResult`,
  both the root and `gaze::registry` import paths, registry storage, both typed
  map accessors, and the README contract. An external integration regression
  implements the public traits and pins the existing empty-map behaviour.
- **The benchmark scorer and validator-probe calls bounded their subprocess I/O**
  (#455). Both use bounded, nonblocking JSONL pipes, discard child stderr in
  memory, and return closed errors after cleaning up the direct child.
  Malformed, incomplete, oversized, or out-of-order responses abort the cell
  instead of producing a scorecard. The `diagnostics_dir` argument is still
  accepted, but new results omit `stderr_log` and `stderr_bytes`; historical
  scorecards remain readable.
- **PyArrow moved to 23.0.1 and pytest to 9.0.3** (#466), with a regression that
  writes a synthetic Parquet file through real PyArrow and drives the existing
  `dataiku_en_de_gaze_bench.load_documents` integrity path. The new PyArrow lock
  keeps `manylinux_2_28` wheels and drops `manylinux_2_17`, so prebuilt wheels on
  glibc-based Linux require glibc 2.28 or newer. Production loader code is
  unchanged.
- **`quinn-proto` moved from 0.11.14 to 0.11.16** (#470) as a lock-only update.

### Added

- **Offline evidence contracts and a scored synthetic MCP evaluation** (#454).
  A strict aggregate receipt validator, a private grouped paired evaluator,
  fixture-only class commitments, and one scored real rmcp duplex route keep
  planned inventory, observed counts, missing observations, provenance, and gate
  results distinct. Missing observations never become measured zero, and
  incomplete checks cannot produce PASS. The addition is synthetic harness
  capability only; no production source, detector, rulepack, workflow, or
  dependency changed.
- **A synthetic bridge connecting scored MCP observations to paired evaluation**
  (#456), built on the same Rust scorer, parent-owned occurrence slots, and an
  in-memory evaluator. The bridge rejects inconsistent identities, missing
  measurement coverage, and invalid outcome rows; protocol, processing, deadline,
  or child-cleanup failures abort the cell before success can be returned.
- **Verified isolated builds for synthetic producer artifacts** (#459). A
  parent-owned build session creates a verified Git snapshot and empty target,
  validates Cargo's selected artifact, retains its file descriptor, runs that
  exact path through the bridge, and re-checks identity and bytes before cleanup
  can report success.

### Removed

- **`crates/gaze/build.rs` and its Cargo registration** (#467). The pinned
  `ort-sys` native-link implementation already owns the Darwin clang runtime
  search path and link library, so the removal retires duplicate native-link
  ownership rather than an absent transitive dependency.
- **Two unreachable `not(feature = "ocr-tesseract")` fallback functions in the
  `gaze-document` MCP module** (#468). The `mcp` feature requires
  `ocr-tesseract` and the module compiles only for `mcp`, so neither fallback
  can exist in a supported feature graph.

### Documentation

- AGENTS.md requires signed commits and no longer requires a message prefix (#463).
- The benchmark index gained rows for `evidence-protocol-v1.md` and
  `class-commitments-v1.json` (#464).
- The MCP chokepoint documentation describes carrier preflight and staged
  protection, and places response snapshot computation before transaction commit
  and `finish_call`; operator-tier `BypassByOperator` is distinguished from the
  protected agent path (#465). Runtime code is unchanged.
- The benchmark README dropped the stale `logs/` artifact row and its
  stderr-log review step (#472).

### Benchmarks

- Benchmarks: see `docs/reference/benchmarks` (v0.14.0 scorecard).

## [0.13.0] - 2026-09-09

### Added

- **Opt-in local proxy inspection dashboard** (#397). The default-off
  `gaze-cli` `dashboard` feature adds an isolated, killable child runtime with
  bounded, memory-only inspection. Pairing, authentication, registration-bound
  activation, purge, and disable are fail-closed boundaries. Inspection may
  expose owner-side content to the authenticated local operator; it is not an
  agent-facing surface. See the [local dashboard guide](docs/how-to/dashboard/run-local-dashboard.md)
  and [trust boundary](docs/explanation/dashboard/trust-boundary.md).
- **Strict Anthropic Messages direct profile** (#397, #399). The proxy validates
  supported request and response carriers, stages reversible substitutions,
  and rejects unsupported opaque or numeric schema carriers before forwarding.
  Limits, tool-schema handling, buffering, and unsupported features are explicit
  in the [Anthropic contract](docs/explanation/proxy/anthropic-messages-contract.md).
  These guarantees are profile-specific, not a blanket promise for every
  provider or every SDK extension.
- **Reusable pinned model setup library** (#366–#368). `gaze-model-setup` and
  the public Kiji bundle verifier provide the installation path used by
  `gaze setup`, with verified artifacts and typed installation outcomes.

- **`passport.cue_anchored` recognizer and an extended `national_id.cue_anchored`
  expand passport / national-ID / ID-card detection on the shipped
  default** (solo todo #3025, slice A). A new `custom:passport` recognizer
  (government-ID collision family, precedence 15 — a passport cue beats a
  tax-number or national-ID cue but yields to an SSN cue) covers passport /
  Reisepass numbers, and `national_id.cue_anchored` gains cue vocabulary
  (Identitätskarte, Personalnummer, Staatsbürgerschaftsnummer, AHV, the
  hyphenated `national-id` forms) and shapes (Swiss AHV `756.dddd.dddd.dd`,
  longer alphanumerics). Both carry the byte-identical shared connector, so
  passport is the sixth member of the drift-guarded connector family. Measured
  on the pinned EN/DE corpus at candidate `cfb3aed` against `def702a`, the
  full-stack Kiji resolve cell removes 3,480 leaked labelled bytes: PASSPORTID
  −1,810, NATIONALID −1,027, IDCARDNUM −586, plus incidental BUILDINGNUM −26.
  These are slice-specific measurements, not a v0.12.0-to-v0.13.0 comparison.
  The deterministic cells add 12 false-positive bytes from one unlabelled
  national-ID span; the rule floor adds one false-positive document. A4 negative
  results are unchanged. Kiji also loses coverage on LICENSEPLATENUM, ZIP, and
  PASSWORD, and the scorecard's strict regression comparison does not pass.
  See the [measured scorecard, hardware, and limits](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-3025a-cfb3aed-scorecard.md)
  and [benchmark runner](scripts/bench/run_no_opf_benchmark.py). The passport check digit and the Swiss AHV
  EAN-13 check digit are real validators, but the synthetic corpus carries no
  valid check digits, so no validator is attached. The passport shape is a single
  capture and the prefixed Swiss AHV form (`CH-756.dddd.dddd.dd`) is a dedicated
  pattern alternative, so a `ValidatorKind` can attach without redesign once a
  checksum-valid generator exists (gated on #2427; the AHV EAN-13 validator lands
  in a dedicated recognizer per #2926). The unprefixed `756.dddd.dddd.dd` form is
  covered by the generic DACH digit-group arm.

- **`gaze daemon` gained the eight safety-net and detection-surface flags only
  `gaze clean` accepted** (#446): `--safety-net-registry`, `--safety-net-add`,
  `--opf-locales`, `--opf-command`, `--opf-checkpoint`,
  `--kiji-distilbert-precision`, `--rulepack-bundled`, and `--rulepack-path`.
  None of these has a policy.toml equivalent — `gaze::Policy` carries no
  safety-net section — so under an identical policy the daemon chokepoint could
  previously only ever run a *single* Pass-3 backend, never the locale-aware
  multi-backend registry, with no configuration able to reach the stronger
  setup. The daemon now refuses a document whose locale no registered backend
  covers, where a single backend would scan it with a model that does not cover
  it and report nothing. This was a capability gap, not a leak: the flags were
  rejected by clap and the daemon failed closed, so nothing was silently
  ignored. The five flags `clean` still has that `daemon` does not
  (`--format`, `--max-bytes`, `--context-json`, `--session-scope`,
  `--session-ttl`) are deliberate and each is justified where its value is set.

- **`gaze_proxy::ProxyConfig::with_dictionaries` installs one immutable
  dictionary source for every proxy detection pass.** Omitting the builder keeps
  the existing empty-bundle default. `ProxyConfig` was already
  `#[non_exhaustive]` and the stored field is private, so this addition does not
  break external struct construction.

- **Scheme- and `www.`-anchored URL detection at the deterministic rule floor**
  (todo #2254). The new `url.anchored` recognizer in the embedded `core` bundle
  tokenizes `http://`, `https://`, and `www.`-prefixed URLs as
  `custom:url`, covering the whole span rather than a fragment. It is
  `safety_tier = "safe_default"` with `locales = ["global"]`, so it is active for
  every adopter of the default bundle, not only for configurations that
  auto-activate locale-gated recognizers.

  The [consolidated scorecard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-consolidated-post-wave-scorecard.md)
  records the measured EN/DE coverage and A4 negative results. Bare-host URLs
  without a scheme or `www.` prefix remain outside this rule's scope because
  they also occur in the negative corpus.

  **Documentation, repository, and example URLs are tokenized.** This is
  intentional: the benchmark includes reference-host shaped gold spans, so it treats a reference URL inside a
  data-owner document as PII to protect. Tokens stay restorable through the
  manifest, so an over-tokenized public URL is a recoverable ergonomics cost
  (axis 5) while an under-tokenized private one is a leak (axis 1).

- **Cue-anchored bearer-credential detection at the deterministic rule floor**
  (todo #2318). The new `security_token.anchored` recognizer is one two-arm,
  `safe_default`, global rule in the embedded `core` bundle. It protects
  structurally typed AWS access-key/JWT shapes and high-entropy values adjacent
  to explicit English or German credential cues, emitting the reversible
  `custom:security_token` class.

  The [consolidated EN/DE comparison](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-consolidated-post-wave-scorecard.md)
  measures this rule together with URL detection and discloses both increased
  false positives and class-level Kiji regressions. Its subtractive baseline
  is specific to that recognizer wave, not the previous release.

  Arm 2 requires at least one unambiguous delimiter between cue and value.
  Whitespace and `:`, `=`, or `#` qualify; `_` and a directly abutting `-` do
  not. This prevents the safe default from splitting cue-prefixed snake_case
  identifiers such as tokenization helper names. A4 does not contain this
  identifier class; post-fix dogfooding across project documentation and Rust
  source produced zero SECURITYTOKEN detections.

  **The shipped no-policy `CorePipelineConfig` default now tokenizes
  credential-shaped strings in ordinary adopter text.** Git tokens in logs,
  API keys in documentation, and similar cue-anchored secrets are protected
  and remain restorable through the manifest. This is a deliberate,
  recoverable axis-5 ergonomics cost in service of axis-1 reliability. The CLI's
  separate no-policy stub path is unchanged.

- **Corpus-informed government-ID recognizers at the deterministic rule floor**
  (todos #2318 follow-on, #2923). Four cue-anchored `safe_default` recognizers
  join the embedded `core` bundle: `ssn.de_cue` (German social-insurance cues
  such as `Sozialversicherungsnummer` and `SV-Nummer` before dashed, dotted, or
  9 to 11 digit values, class `custom:ssn`), `tax_number.cue_anchored`
  (`custom:tax_number`), `driver_license.cue_anchored`
  (`custom:driver_license`), and `national_id.cue_anchored`
  (`custom:national_id`). Every chosen shape was picked by measured sweep against
  looser drafts and matches 0 of the 1,024 committed A4 negative documents.

  Locale basis follows the mixed model from #414: `ssn.de_cue` is a
  format-basis sibling of `ssn.us` (same class, same national identifier
  shapes, German cue vocabulary, DACH provenance) and is an explicit addition
  to the ratified format-basis promotion set; the three bilingual cue-anchored
  recognizers are document-basis `global`, like `security_token.anchored`. All
  four therefore fire under every locale chain, including `--locale=global`.
  `ssn.us` keeps its pattern, locales, and basis unchanged; the only edit to it
  is the symmetric `cooperates_with` metadata line. Cross-class overlaps between
  the numeric shapes are resolved by the new `government-id` collision family
  (`ssn` 10 beats `tax-number` 20 beats `national-id` 30; lower wins).

  The [shipped scorecard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-government-id-scorecard.md)
  measures the deterministic cells at 3,194 fewer leaked bytes each and 9 more
  false-positive bytes: rule floor adds 1 false-positive document, while pass2
  adds none. The full-stack Kiji `resolve` cell removes 3,093 leaked bytes and
  613 false-positive bytes with no change in false-positive documents.
  **Disclosed regression:** the Kiji cell loses 6 covered `PASSWORD` bytes, the
  downstream safety-net interaction tracked as todo #2491 (mechanism #2420),
  not a resolver decision and unaffected by collision precedence.

  `tax_number.cue_anchored` deliberately requires a three-digit lead and
  internal separators: it cedes the checksummed 2-3-3-3 Steuer-ID shape to
  `steuer_id.de` and excludes A4's bare-digit invalid identifiers, so its
  measured 16.8% coverage is a precision choice, not a detection deficiency.
  `national_id.cue_anchored` ships with a known, bounded gap: 77 bare German
  NATIONALID spans carry no cue and are structurally unreachable by any
  cue-anchored rule.

### Changed

- **`gaze setup` now verifies strict ownership and private file modes even
  when an existing Kiji bundle lets it skip downloading** (#366–#368).
  Only a complete tree owned by the current effective user may have loose
  modes repaired, followed by mandatory verification. Foreign-owned or
  otherwise invalid non-empty installs fail closed. Reinstall into a private
  directory owned by the account that runs Gaze, or have the owner correct
  ownership and permissions before retrying; changing the working directory
  cannot change the trusted owner. The coordinated publish plan places
  `gaze-recognizers` 0.13.0 before `gaze-model-setup` 0.13.0.

- **MCP protection intentionally changes runtime compatibility:** custom tools
  must declare trusted object-member and non-sensitive numeric carriers;
  empty primary pipelines now fail closed. Configure the bundled pipeline
  and locale chain with `gaze-assembly::CorePipelineConfig`. Stricter safety-net
  coverage, residual, and token-provenance checks can reject previously accepted
  calls. Transport tool errors now expose only their class, with details kept
  owner-side. See `crates/gaze-mcp-core/README.md` for migration requirements.
  `SearchDocumentsTool` supplies its bounded carrier declarations, and
  `gaze_read_file` restores protected paths owner-side before file validation.
  All publishable workspace crates and their internal dependency minimums
  move together to 0.13.0 in this release (#452).


- **`gaze-cli` declares each shared flag group once** (audit 7201 S11-F1, solo
  todo #2368). `gaze clean` and `gaze daemon` each declared their flags inline
  in `Cmd`, restated them in a 33- and 23-field destructure, and rebuilt them
  into a runtime `Args` struct — five mirrors of one list. `gaze proxy serve`
  and `gaze proxy start` carried two byte-identical 31-line dashboard blocks
  plus two hand-written 10-field mappings. The flags now live in one
  `#[derive(clap::Args)]` struct per group, flattened with `#[command(flatten)]`,
  and the OpenAI-filter subprocess group and the safety-net budget group
  (`--safety-net-timeout-ms`, `--safety-net-input-limit-bytes`,
  `--safety-net-mode`, `--safety-net-fallback`) are owned once in
  `commands::shared_args` and shared by `clean` and `daemon`, so a verb cannot
  quietly lose one and run a weaker Pass-3 safety net than its sibling.

  **The shared-argument refactor preserved the CLI surface at that commit.** `scripts/verify/cli-help-surface.sh`
  builds the base revision and the working tree in one run and diffs `--help`
  for the root command and all 32 subcommands; all 33 captures are byte-identical
  across the change, and the captures are committed under
  `crates/gaze-cli/tests/fixtures/cli-help/`.

  The refactor preceded #446, which adds the eight daemon flags listed above.
  Its help-surface comparison describes the refactor alone, not the cumulative
  v0.13.0 release.

- **One documented safety-net default across the library and the CLI** (audit
  7201 S01-F1, solo todo #2949). `Pipeline::clean_with_safety_net` and
  `clean_with_safety_net_detect_context` — the policy-less convenience entry
  points — previously hard-coded `SafetyNetMode::Strict` + `Redact`, which
  contradicted `SafetyNetPolicy::default()` (`Resolve` + `Redact`, the shipped
  production default and the CLI default since v0.8.1). They now use
  `SafetyNetPolicy::default()`. **Adopters calling these entry points with a
  registered safety net now get enforcement instead of observation**: suspects
  are tokenized reversibly and any residual takes the `Redact` fallback, rather
  than being reported and shipped. This strengthens axis 1 and preserves
  axis 2 (the promoted spans stay restorable). For the previous behaviour pass
  an explicit policy to `clean_with_safety_net_policy_detect_context`, or use
  `Pipeline::scan_safety_nets` for a report-only pass. In-tree pipelines that
  register no safety net are unaffected.

- **The three structured-document walkers are one `walk_structured` with a
  `LeafOp`** (audit 7201 S01-F2, solo todo #2950). The pseudonymize,
  clean-and-scan, and scan-only traversals of `RawDocument::Structured` were
  three near-identical recursive copies that had already drifted. They are now
  one function parameterized by `LeafOp { Pseudonymize, CleanAndScan, ScanOnly }`,
  with every intentional difference — empty-string skipping, whether scalar
  leaves are scanned, whether the document is rebuilt, and the root field-path
  prefix — declared once on the op and documented there. Behaviour is unchanged
  on all three paths, including the pre-existing divergence where
  `scan_safety_nets_structured` reports bare-key field paths (`profile.email`)
  and `clean_with_safety_net*` reports JSONPath-style ones (`$.profile.email`),
  which is preserved deliberately and tracked as solo todo #2958.

- **`SafetyNetMode` x `SafetyNetFallback` is lowered once to a total
  `SafetyNetDecision`** (audit 7201 S01-F1, solo todo #2949). The two public
  fields spell twelve pairs; the runtime has six behaviours, and seven pairs
  previously differed only in a field nothing read. `SafetyNetPolicy::decision()`
  is now the single, total lowering to
  `SafetyNetDecision { Observe { strict }, Redact, Resolve { on_residual } }`,
  and every pipeline arm, the skip-gating optimizer, and the CLI boundary read
  the decision instead of re-deriving the lattice from the pair. `SafetyNetDecision`
  is exported from `gaze`. The public `mode` and `fallback` fields are unchanged.
  The full twelve-pair behaviour table is pinned by
  `safety_net_policy_lowering_covers_all_twelve_representable_pairs`.

- **`gaze clean` no longer warns that `--safety-net-fallback` is "ignored when
  `--safety-net-mode` is terminal"** (audit 7201 S01-F1, solo todo #2949). The
  lowering documents which pairs consult the fallback, so the runtime warning is
  redundant. Relatedly, the tolerant-deprecation warning now fires only where a
  tolerant disposition is reachable — `--safety-net-mode tolerant`, or a tolerant
  fallback under `--safety-net-mode resolve`. It no longer fires for
  `--safety-net-mode redact --safety-net-fallback tolerant`, where the fallback
  is never consulted. The `GAZE_ALLOW_TOLERANT` gate is unchanged and still
  rejects a tolerant flag in any position.

- **Persistent owner-side corpus index schema v2: each document is stored
  once, postings are derived on load, and re-ingest upserts** (audit 7201
  S17-F1, todo #2936). `gaze_token_bridge::persistent::FileCorpusIndexStore`
  previously wrote one record per distinct fingerprint, each carrying a full
  copy of the document (snippet plus every entity's raw value), so a document
  with E entities was duplicated E times on disk, and re-ingesting a `doc_id`
  without clearing the domain left the stale copy first in scan order (the
  stale snippet won). Raw document snippets and entity values are now persisted
  exactly once per `(domain_id, doc_id)`; the fingerprint-to-document postings
  map is rebuilt on load and never written to disk; `insert_hit` replaces an
  existing `(domain_id, doc_id)` outright, and `clear_domain` + `save` leaves
  no document bytes in the sealed payload.

  **Breaking for existing local index files.** `SCHEMA_VERSION` is now `2`.
  Loading a v1 `index.json` fails closed with the typed
  `unsupported owner-side index schema 1; supported 2` error (the version is
  checked before the payload shape, so there is no partial or reinterpreted
  load), and a v2 payload carrying duplicate document keys is rejected the
  same way. Preserve the old encrypted index and its key for recovery, then
  rebuild into a fresh private directory with
  `gaze index ingest <dir> --index-path <new-private-directory>`. Reusing the
  old path fails during load before ingest can clear it. Verify the new index
  before pointing search and other consumers at its path.
  The `entities: N` metric printed by `gaze index ingest` keeps its meaning
  (indexed document/fingerprint pairs). `FileCorpusIndexStore::hit_count_for_domain`
  was removed (it had no callers). The AEAD/key layer and the sealed-file
  envelope are unchanged.
- **Audit-row enums now own one canonical string form** (audit S05-F2, solo todo
  #2935). `Action`, `ConflictTier`, `DocumentKind`, and `FallbackReason` expose
  matching `as_str` / `from_canonical_str` methods, and SQLite plus CLI consumers
  delegate to them instead of maintaining panic-prone copies. `FallbackReason`
  JSON now serializes as snake_case to match SQLite; every former PascalCase
  spelling remains accepted as a serde alias.

- **`gaze_assembly::build_pipeline` derives its `NoRecognizers` guard from
  actual registration and uses one locale predicate** (audit 7201 S10-F1,
  todo #2928). The guard now fails closed when zero recognizers were
  registered, instead of re-deriving eligibility from policy and rulepack
  metadata. Two behavioural consequences for adopters:
  - Rulepack recognizers with an **empty locale list** (a pack that omits both
    `default_locales` and per-recognizer `locales`) now register and run under
    every document locale chain, matching the detect-time
    `LocaleChain::intersects` semantics that already treated an empty list as
    matching. Previously assembly silently dropped them (a missed detection).
    Bundled rulepacks are unaffected (every bundled recognizer declares
    locales).
  - An `anchored_match`-only rulepack whose optional builtin cue bucket
    (`forward_markers`, `agent_recipient_cues`, `footer_cues`) is not present
    under the active locale chain now fails with `NoRecognizers` instead of
    building a zero-recognizer pipeline that preserved every byte.
  NER model loading now runs before the guard; error precedence is unchanged
  for reachable configurations (a configured `model_dir` that fails to load
  still surfaces as `NerLoad`).
- **Locale-gated auto-activation is derived from the loaded rulepacks**
  (audit 7201 S10-F2, todo #2929). The `auto_activate_locale_gated` locale set
  (`core-extended` compatibility alias) is now computed by the new public
  `gaze_assembly::locale_gated_activation_locales(&[Rulepack])` — the union of
  `locales` over enabled, document-basis `safety_tier = "locale_gated"`
  recognizers, minus `global`, ordered compatibility-first
  (`en-US, de-DE, de-AT, de-CH`) then by canonical tag — instead of a literal
  `[en-US, de-DE, de-AT, de-CH]` list that was triplicated across
  `CorePipelineConfig`, `gaze clean`, and `gaze daemon`. For the bundled `core`
  recognizers the derived set equals the old literal, so shipped behaviour is
  unchanged (the compatibility chain stays `global, en-US, de-DE, de-AT,
  de-CH`). Behavioural widening for adopters: a path rulepack whose
  document-basis locale-gated recognizer declares another locale (for example
  `es-ES`) now auto-activates under the alias without an explicit `--locale`
  or policy locale; previously it silently never activated. Any future bundled
  locale-gated recognizer joins the activation set automatically. The
  `--locale`/policy locale override precedence is unchanged.
- `[bundle-tokenization-drift]` snapshot for bundle `core` regenerated: the new
  `url.anchored` recognizer adds one `custom:url` detection to the drift corpus
  (11 -> 12 detections). No existing detection changed class, span, or shape.
- `[bundle-tokenization-drift]` snapshot for bundle `core` regenerated for the
  mixed locale-basis rulepack version bump (`0.5.1` -> `0.5.2`). Detection count
  remains 12; no detection changed class, span, or shape.
- **Bundled identifier recognizers now use explicit mixed locale semantics**
  (todo #2417). Rulepacks gain additive
  `locale_basis = "document" | "format"` metadata. External and adopter
  rulepacks that omit it retain the legacy `document` default. Bundled
  recognizers declare it explicitly.

  The A4-clean format recognizers `aadhaar.in`, `bsn.nl`, `cnpj.br`, `cpf.br`,
  `nhs.uk`, `nino.uk`, `nir.fr`, `pan.in`, `phone.national.us`, `ssn.us`,
  `steuer_id.de`, `vat.de`, and `vat.es` now run once regardless of the
  document locale. Their `locales` values record identifier-format provenance,
  and their candidates join document-basis candidates before the unchanged
  conflict resolver runs. Linguistic `name.*` recognizers remain
  document-basis. `phone.national.de`, `postal.de`, and `postal.us` remain
  temporarily document-gated pending precision hardening; postal promotion
  depends on todo #2424.

  **Breaking behavior:** `--locale=global` and narrow locale chains no longer
  suppress the format-basis identifiers. Adopters relying on that suppression
  can receive new tokens and changed snapshots. To restore the old output,
  disable the affected recognizer outright, for example by selecting an
  adopter rulepack copy with `enabled = false`; locale mismatch is no longer a
  suppression mechanism.

  This deliberately trades axis 5 (snapshot compatibility and configuration
  convenience) for axis 1 (never leak a foreign-format identifier merely
  because the surrounding document uses another locale). The known remaining
  rule-coverage debt is 27 target spans: 14 DE national-phone and 13 postal
  spans. The proxy transport debt is now closed by #2411: direct/codec primary
  and residual passes receive the shared `ProxyConfig::locale_chain`; #2403
  previously fixed the legacy path.

- **A provably corrupt clean-text manifest now hard-errors in every safety-net
  fallback mode, including `Tolerant`** (#403). The safety-net RESOLVE path checks
  that the manifest describes a monotonic, gap-preserving clean/raw alignment
  before it makes any manifest-derived decision. A manifest that fails that check
  contradicts the document it describes, so it fails closed as
  `SafetyNetError::InvalidOutput` with a `manifest-integrity:` message prefix
  rather than taking the configured fallback — `Tolerant` no longer returns
  `Ok(())` and `Redact` does not attempt redaction. This is deliberate: redaction
  derives its deletion spans from manifest coordinates, so a manifest that
  misdescribes the document makes the redactor delete the wrong bytes and can
  leave the flagged PII in place. That is a leak, not a degraded-but-safe
  document, and `Tolerant` was contracted to tolerate residual suspects, never an
  internally inconsistent manifest. Axis 1 (never leak) over axis 5 (adopter
  ergonomics).

  **Not reachable from the public surface today.** `redact_text_with_manifest`
  emits an `EmittedTokenSpan` for every replacing action — `Tokenize`, `Redact`,
  `Generalize`, `FormatPreserve` — so every clean/raw divergence is described by a
  manifest entry and a pipeline-produced manifest satisfies the check by
  construction. The checks are defense in depth against a future change to the
  primary pass, not a response to a reachable failure. No new `Error` or
  `FallbackReason` variant; `gaze-types` is unchanged.

### Fixed

- **A builtin-class sub-token can no longer split a structured identifier**
  (solo todo #3025, slice U). The base conflict ladder ranks `Email`, `Name`,
  `Organization` and `Location` above every `Custom` class, so an NER
  organisation or name token that sat *inside* a rule-recognised span
  (`url.anchored`, `security_token.anchored`, …) won on `ClassPriority`,
  `remove_overlaps` dropped the whole container, and the clean text carried a
  mid-word token with the head and tail of the identifier raw — for example
  a URL split around an embedded NER name, or an `AKIA…` credential
  split around its prefix. The
  [structured-containment scorecard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-3025u-bfcf264-scorecard.md)
  measures 1,490 fewer leaked URL bytes and 44 fewer credential bytes in the
  full-stack Kiji resolve cell, with 2 more leaked PASSWORD bytes and 2 fewer
  ZIP bytes from downstream re-segmentation. The rule floor is unchanged. `resolve_candidates` now
  carries a structured-containment rung (`ConflictTier::StructuredContainment`,
  audit string `structured_containment`): when a custom-class span strictly
  encloses a builtin-class span, the enclosing span keeps the slot and the
  enclosed span is recorded as a merged source. The rung is containment-only
  and geometry-decided (permutation-invariant); partial overlaps,
  custom-inside-custom, builtin-inside-builtin and builtin containers over
  custom spans keep their existing rungs.

- **The five SSN / government-ID recognizers share one cue-to-value connector grammar, so
  multi-word phrasing between the cue and the value is covered** (solo todo #3025, slice G).
  `ssn.us`, `ssn.de_cue`, `tax_number.cue_anchored`, `driver_license.cue_anchored` and
  `national_id.cue_anchored` previously allowed a single optional keyword plus one punctuation
  mark between the cue and the value, so real phrasing such as a licence cue followed by a comma,
  a German social-insurance cue followed by a relative clause, and a tax cue followed by `as`
  leaked. All five now carry one byte-identical connector fragment (up to four closed-vocabulary
  tokens with Unicode-aware, unbounded whitespace — matching `ssn.us`'s prior `\s*` tolerance, so
  aligned columns, long padding and non-breaking spaces stay covered — and one optional separator).
  This is a grammar-only change: cue
  vocabulary and value shapes are unchanged, so the set of eligible value shapes did not move.
  Measured in the [connector scorecard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-3025g-edfb167-scorecard.md), on the shipped default (`full-stack-kiji-resolve`),
  against the slice-U merge (`56e1a3d`): 1,802 fewer leaked labelled bytes — SSN −526,
  DRIVERLICENSENUM −582, IDCARDNUM −384, NATIONALID −205, TAXNUM −95. On the deterministic rule
  floor, 190 more entities fully covered for zero false-positive movement (rule and pass2
  false-positive bytes unchanged), and zero A4 negative movement in every category and cell. A
  drift-guard test (`shared_connector_grammar_is_byte_identical_across_the_family`) fails if one
  of the five copies is edited alone.

- **Audit rows now name the conflict tier that actually decided the overlap**
  (audit S02-F1, solo todo #2948). The resolver probed its comparator twice —
  once per direction — and reused the second answer as the winner's
  `decided_by` label. The mandatory-anchor rung is not antisymmetric (it
  inspects one candidate and ignores the rival), so when an anchored incumbent
  such as `iban.structural` kept its span by score, span length, or recognizer
  id, the reverse probe stamped `ConflictTier::AnchoredContext` on it — an audit
  row claiming a missing-anchor fallback decided a conflict the base ladder
  decided. A fully tied overlap kept whatever tier an earlier overlap had left
  behind instead of naming its own. Both are corrected: the anchor rung is
  consulted once per pair and no longer labels a ladder-decided overlap, and a
  retained incumbent always carries a definite tier. Detection outcomes are
  unchanged — which spans get tokenized, their classes, and the restore
  contract are byte-identical; only `decided_by` (and the `ambiguity_record`
  derived from it in `gaze-audit` / JSON exports) moves.

- **The `mcp-tier-isolation` gate now actually fails when the agent/operator
  tier boundary is violated** (audit 7359 §6-F1, solo todo #2993). **The tier
  partition itself was, and remains, enforced by rustc:** the operator surface
  sits behind `#[cfg(feature = "operator-tier")]` in
  `crates/gaze-mcp-core/src/{lib.rs,tools/mod.rs}`, and no agent-tier build has
  ever linked it. What was missing was any check that this keeps holding. The
  gate's agent-tier assertion was `assert!(true)`, deferring the real guarantee
  to the `dylint-gate`, whose `dylint.toml` carries only `gaze_audit` rules and
  nothing about tiers — so un-gating the operator restore surface and running
  `cargo run -p xtask -- mcp-tier-isolation` exited 0. **No adopter was exposed
  by this; the alarm on the boundary was, until now, the only thing that was
  not real.**

  The vacuous assertion is replaced by `trybuild` compile-fail fixtures in
  `crates/gaze-mcp-core/tests/ui/tier/`, one per gated surface (`tools::export`,
  `tools::restore`, `tools::restore_strict`, and the `operator_tools`
  re-export). Each is compiled as an external crate against the same agent-tier
  feature graph the test binary was built with, so rustc — not another gate's
  configuration — is the enforcer. Removing any `cfg` gate makes the matching
  fixture compile, which fails the gate and names the surface. The gate
  additionally requires each feature graph to report its tier tests as passing,
  since `cargo test` exits 0 for zero tests, and the fixtures are enrolled in
  the `trybuild-fixture-hygiene` inventory so deleting one is also a failure.
  Each graph further declares whether it is driven with `cargo test` or
  `cargo check`, and the gate refuses a `cargo test` graph that names no test,
  so an empty expectation cannot silently mean both "nothing executes here" and
  "nobody filled this in".

- **Structured documents no longer accept a safety-net enforcement request and
  silently perform observation** (audit 7201 S01-F2, solo todo #2950). The
  structured arm of `clean_with_safety_net_policy_detect_context` cleaned each
  field, ran the nets over the result, and returned `Ok` — with no enforcement
  stage anywhere on the path. A caller passing `SafetyNetMode::Redact` or
  `SafetyNetMode::Resolve` with a `RawDocument::Structured` therefore got
  observer-only behaviour and a success return, with the suspect bytes still in
  the document. It now fails closed with the new
  `Error::UnsupportedSafetyNetModeForStructured { mode }` before any field is
  tokenized. **Adopters passing structured documents with an enforcing mode now
  get an error** — the intended correction; pass `SafetyNetMode::Strict` (or
  `Tolerant`) for the observer behaviour they were actually receiving, or use
  `Pipeline::scan_safety_nets_structured`. Note that the policy-less
  `clean_with_safety_net*` entry points default to `Resolve`, so structured
  documents must go through `clean_with_safety_net_policy_detect_context`.
  Text documents are unaffected. Axis 1.

- **Safety-net `resolve` fallback acted on the primary report instead of the
  residual one, destroying tokens and shipping residual PII under the default
  policy** (audit 7201 S01-F1, solo todos #2949 and #2956). When the resolve
  pass converged and the post-resolution re-run flagged a residual suspect, the
  fallback was handed the *primary* `LeakReport`, which by then described
  pre-resolve coordinates.

  With a **non-empty** primary report — the broadly reachable case, since any
  deterministic safety net produces one — the redactor was pointed at stale
  pre-resolve spans. Those spans had since become part of a token the resolve
  pass minted, so the fallback deleted the token, dropped its manifest entry,
  and left the actual residual in the document: an axis-1 leak and an axis-2
  restore break in the same operation, returned as `Ok`.

  With an **empty** primary report the fallback had nothing to act on at all, so
  the residual shipped and no fallback audit row was written. (Reaching that
  shape requires a backend whose verdict differs across byte-identical text,
  since a converged resolve leaves the document unchanged.)

  Only the `strict` fallback was safe, because it rejects the document without
  consulting the report. The fallback now receives the report that produced the
  reason, and acts only on the suspects in it that are not already protected by
  a live token — a protected suspect is audited as a `Preserve` no-op instead of
  being redacted, which also closes a gap where such suspects were left out of
  the audit entirely. Pinned by
  `resolve_fallback_does_not_redact_stale_pre_resolve_spans`,
  `resolve_fallback_redacts_the_residual_report_not_the_stale_primary_report`,
  `resolve_fallback_redacts_the_residual_without_deleting_protected_live_tokens`
  (both fallback reasons), and the twelve-pair lowering table; the first three
  are required by name in the `safety-net-sanity` gate.

- **A residual found only by the post-resolution re-run was invisible in the
  returned `LeakReport`** (solo todo #2959). The report handed back to the
  caller is the first pass's, so a boundary that decides on it — the CLI's
  tolerant-mode deprecation warning, or an adopter's "did anything leak?" check
  — was told nothing was found while the residual shipped under `tolerant` or
  was destroyed one-way under `redact`. The suspects the fallback acted on are
  now merged into the returned report. A converged resolve merges nothing, so a
  deterministic net that re-reports the same suspect does not produce
  duplicates.

- **Fallback audit rows now state what happened to the suspect's bytes**
  (audit 7201 S01-F1, solo todo #2949). `fallback_action` was renamed to
  `fallback_row_action` and documented as the row's claim about the bytes:
  `Action::Redact` only when the residual span is actually deleted,
  `Action::Preserve` when it is left in place (shipped under `tolerant`,
  rejected under `strict`). With the residual-report fix above, a
  `decided_by: Fallback` row now also names the residual suspect that drove it
  rather than a stale primary-pass suspect.
- **A detached `gaze proxy start --policy prod.toml` now runs the policy instead
  of the bundled `core` pipeline** (solo todo #2965). `start` persisted the
  policy into its daemon config and then spawned the serving child with only
  `--bind` and `--session-ttl`, so the detached daemon resolved
  `build_pipeline(None, "core")`: no policy rules, no custom recognizers, no
  dictionaries, and no policy locale tier. The configured `--rulepack` and all
  three `--upstream-*` overrides were dropped the same way, which is why
  `gaze proxy status` could print upstreams the running daemon never used. The
  child's argument list is now derived from the daemon config as a whole, so the
  daemonized proxy resolves the same pipeline as `gaze clean`. **The previous
  entry for `gaze proxy` (solo todo #2937, PR #437) covered `gaze proxy serve`
  only**; adopters running the daemon were unaffected by that fix. `restart`
  carried the same defect and is fixed by the same change.

- **`gaze proxy start` now fails when the daemon dies during startup instead of
  reporting success** (found by the red test for #2965). The liveness probe used
  `kill(pid, 0)`, which cannot distinguish a running child from one that exited
  and has not been reaped, so a child that failed closed on an unloadable policy
  was reported as `gaze-proxy started` with exit code 0. `start` now reports the
  new `ProxyError::DaemonExitedEarly`, naming the child's exit code and the
  stderr log to read, and removes the empty pidfile that would otherwise fail
  every later start as stale. A startup failure slower than the 250 ms probe
  window is still reported as started; that daemon is dead rather than serving
  unprotected.

- **`custom:family:*` policy classes now preserve the collision-family namespace**
  (audit S05-F1, solo todo #2934). `PiiClass::from_policy_name` previously
  normalized the reserved `family:` separator and hyphenated family name, so a
  protective class rule could silently miss a family-level ambiguity token and
  preserve the original value. `PiiClass::family` and `as_family_name` now model
  that namespace once, and policy and rulepack parsing share the same
  non-normalizing path. The enum and manifest wire shape are unchanged.
- **`gaze proxy` now resolves the same rulepacks, dictionaries, and
  auto-activated locales as `gaze clean` for the same policy** (audit 7201
  S11-F2, solo todo #2937). Previously the proxy assembled a narrower pipeline
  that skipped dictionary values and locale-gated auto-activation.
- **Proxy request protection and fail-closed residual validation now read the
  same configured `DictionaryBundle`.** This covers direct/codec JSON and SSE
  response validation plus the legacy primary and residual request passes; the
  residual can no longer know fewer dictionary terms than the primary pass.
- **`gaze-proxy` direct/codec primary and residual passes now use the resolved
  locale chain instead of a pinned Global chain** (solo todo #2411). This
  closes the direct/codec half after #2403 fixed the legacy path, and keeps both
  passes aligned with the same configured dictionaries and document locales.

- **The ORT NER backend now hands the BIO decoder the document text, not its
  provenance label** (audit S07-F1, solo todo #2902). `OrtBackend::detect` passed
  the constant `"ner/ort"` where `merge_bio_span_results` expected the string
  the tokenizer offsets index into, so in production (a) joiner bridging read
  the bytes between tokens from `"ner/ort"` and was dead — hyphenated or dotted
  names such as `Anne-Marie` / `john.doe` decoded as two spans — and (b) any
  input of 7 bytes or fewer had its spans boundary-checked against `"ner/ort"`,
  so short structured field values such as `Anna`, `Alice`, or `Berlin` (the
  tool-call JSON shape axis 3 is built for) lost their NER span entirely. Both
  are recall defects (axis 1). The decoder now receives the real input, the
  decode step is a model-free `decode_logits` seam with red-first tests, and the
  Kiji safety-net decoders (`ort`/`tract`/`candle`) were checked and already
  passed the real text.

  The heuristic `enforce_source_boundaries` flag (which guessed whether the
  argument was text by comparing span ends to its length) and the
  `is_token_boundary_match` suppression it gated were removed rather than
  silently activated by the corrected argument: that suppression never ran in
  production and turning it on is a measured decision (solo todo #2904).
  Production output changes only by adding spans the decoder was designed to
  emit; no span the previous decoder emitted is dropped.

  **API:** `NerDetector::merge_bio_spans` now takes `document_text` and
  `provenance` as separate parameters (was one `source` used for both);
  `NerDetector::merge_bio_span_results`' last parameter is renamed
  `document_text` (same position, same type). Callers that passed a provenance
  string as `source` must pass the tokenizer input instead.

- **Safety-net RESOLVE no longer returns `Ok` on a result it cannot verify**
  (#403). The mode's only post-condition used to be a follow-up net scan, so any
  net whose second pass disagreed with its first — an ML net, a cached net, a
  sampled net — could leave a raw PII fragment in the clean text and emit a
  permanently unrestorable manifest while the call succeeded. Resolutions are now
  planned against the unmutated clean text and rejected as a typed
  `FallbackReason` when they overlap each other, when a suspect's coverage claim
  contradicts the manifest, or when a residual suspect survives the follow-up
  pass; suspects lying wholly inside a live token are audited as reversible
  no-ops instead of destroying the token. `SafetyNetFallback::Redact` keeps its
  existing redact-and-deliver meaning for every fallback reason.

- **Kiji safety net: location and organization suspects were swapped** (PR
  #425, todo #2312, defect todo #2925). Since v0.9.0-rc.1 the in-process Kiji
  DistilBERT decoders (`ort`, `tract`, `candle`) mapped classifier ids 3–6 as
  `B-LOC, I-LOC, B-ORG, I-ORG` while the pinned model actually emits
  `B-ORG, I-ORG, B-LOC, I-LOC`, and the Python subprocess runner used a third,
  MISC-first order. Every real place was therefore reported as `organization`
  (mapped to the `Name` safety-net class) and every real organisation as
  `location`: the `LeakSuspect` class, the class/family of tokens minted by
  RESOLVE mode, `ClassMismatch`/fallback decisions, and the `raw_label` /
  `mapped_class` columns of `safety_net_log` audit rows were all wrong for
  those suspects. Span positions and scores were unaffected. All backends and
  the runner now share one label registry pinned to the upstream
  `onnx-community/distilbert-NER-ONNX` `config.json` (`3a19fe9`), verified by
  decoder-parity tests, and every backend fails the whole request closed with
  a typed `SafetyNetError::InvalidOutput` on a malformed classifier width,
  offset/logit length mismatch, non-finite logits, or missing output tensor
  instead of silently mapping to `O` or returning no spans. The bundle ships no
  `id2label` artifact, so the registry cannot be re-checked at bundle load; the
  SHA-256 bundle pin plus the parity tests are the guard.

### Evidence and known limits

The release includes all changes since v0.12.0, not only MCP #452. Detection
remains dependent on the configured recognizers, dictionaries, locales, and
safety nets. MCP protects supported tool-call carriers; it does not protect
chat UI uploads or pasted user messages outside that path. Operator restore
surfaces remain privileged and may intentionally return raw owner data.

Benchmark numbers above describe the named historical candidate/base pairs.
They are not a fresh measurement of this release head and must not be added
together as an end-to-end release gain. The
[benchmark runner](scripts/bench/run_no_opf_benchmark.py),
[consolidated scorecard and machine specification](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-consolidated-post-wave-scorecard.md),
[post-wave scorecard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-post-wave-a8f7182-scorecard.md),
and linked slice scorecards retain corpus pins, hardware, failed-closed
accounting, false positives, and known class regressions. No universal
PII-detection or exact-restore rate is claimed. Safety-net `Redact` fallback
still deletes residual bytes; strict MCP rejects results that cannot satisfy
its reversible protection contract.

## [0.12.0] - 2026-07-06

### Added

- **Policy-authoring docs now cover the `\b`-next-to-symbol regex pitfall**
  (#361). `\b` adjacent to a non-word character (`€`, `$`, `£`, punctuation)
  never forms a boundary against whitespace, so currency-style patterns like
  `\b(...€|$...)\b` silently fail to match — a fail-open leak in redaction
  policies. Investigated as a suspected 0.5.x → 0.11.x regression and ruled
  out: outputs are byte-identical across versions (standard Rust `regex`
  semantics, engine unchanged). `docs/reference/policy.md` documents the
  failure modes and the explicit-boundary-group rewrite.
- **`gaze clean` now warns when a collision-family fallback class would silently
  leak** (#360). When an active recognizer can emit a `custom:family:<family>`
  class (for example `custom:family:payment-card-or-iban` for an anchor-less
  IBAN) and the policy leaves that class to a non-protective default, the CLI
  prints a `warning:` to stderr naming the exact rule to add. Rust adopters get
  the same list from the new `gaze_assembly::uncovered_collision_family_classes`.

### Changed

- **CI now runs `bundle-tokenization-drift --verify-ack`** (#360). Any change to
  a committed bundle tokenization snapshot — including emitted class renames —
  must carry a `// drift-ack:` comment and a CHANGELOG entry to merge. The v0.8
  `custom:iban` → `custom:family:payment-card-or-iban` snapshot change merged
  without either, which is how the rename reached adopters undocumented.

### Fixed

- **Documented the collision-family fallback class contract so `preserve`-default
  policies stop leaking IBANs** (#360). A policy keyed on `custom:iban` with a
  `preserve` default matched no rule for the anchor-less
  `custom:family:payment-card-or-iban` class and silently preserved the IBAN.
  The class names are now documented as pinnable contract in
  [`docs/reference/policy.md`](docs/reference/policy.md), and setting a covering
  `[[rule]]` (or loading `locale-en`/`locale-de` for the precise `custom:iban`
  class) closes the leak. Restores axis-1 protection for the default `core`-only
  configuration.

## [0.11.3] - 2026-07-02

v0.11.2 crates were never published to crates.io; v0.11.3 supersedes it for
crates.io users.

### Added

- **Restore and manifest property suites now cover round-trip and invariant
  behavior** (#354), raising confidence that reversible pseudonymization stays
  stable across generated inputs.
- **Remote CI now runs the release-critical gates** (#356): MSRV,
  `cargo-deny`, and the `xtask` gate suite.
- **Restore token regexes now use a session cache** (#356), reducing repeated
  parsing work without changing strict-restore behavior.

### Changed

- **The unused daemonization dependency was removed** (#352), narrowing the
  dependency graph shipped to adopters.

### Fixed

- **Email boundary leak fixes, SafetyNet fail-closed behavior, and locale
  fallback fixes were ported to main** (#353), restoring axis-1 protections that
  were absent from v0.10.0 through v0.11.2.
- **Strict restore no longer treats Unicode-digit token ordinals as valid**
  (#355), avoiding a false-positive restore path for non-ASCII token numbers.
- **The crates.io publish workflow now packages the workspace as a unit before
  publishing.** Per-crate pre-flight packaging resolved unpublished internal
  dependencies against the registry and caused the v0.11.2 multi-crate publish
  failure.

### Security

- **The pdfium CI download is pinned and SHA-256 verified** (#352), replacing an
  unverified network fetch in the document test setup.

## [0.11.2] - 2026-06-23

### Added

- **`gaze setup` provides the one-command onboarding path.** The CLI now installs
  and SHA-verifies the pinned NER model, writes a working policy, and runs a
  doctor check so OPF model setup is verified or fails closed.
- **Recognizer coverage expanded for outbound DLP workflows.** EU VAT IDs,
  ISO-length-gated IBANs, and spaced international E.164 phone numbers are now
  detected by default recognizers.

### Changed

- **Owner-side TokenBridge indexes are encrypted at rest.** Index files now use
  ChaCha20-Poly1305 bound to a per-index id, with `GAZE_INDEX_KEY` and optional
  `os-keychain` support, closing the plaintext PII and projection-key material
  exposure from `0.11.1`.
- **`gaze index ingest` defaults residual safety-net hits to redact.** The new
  `--on-residual redact|strict` mode keeps real-document ingestion usable while
  preserving the never-leak contract; operators can still opt into strict
  fail-closed behavior.
- **The README now leads with the one-command quickstart and the correct product
  framing:** deterministic reversible PII pseudonymization and outbound DLP, not
  guardrails, prompt-injection defense, or content-safety filtering.

### Fixed

- **`gaze index` now surfaces real error detail.** Failures that previously
  collapsed into an opaque `PolicyConfig` error now preserve the actionable
  underlying error.
- **Detection NER now loads the Kiji bundle.** The loader accepts optional
  `config.json` metadata and conditionally supplies `token_type_ids`, matching
  the shipped Kiji model bundle.
- **Proxy and structural recognizer hardening.** OpenAI proxy PII surfaces were
  tightened, email structural TLD matching was corrected, and the new phone and
  IBAN recognizers avoid the known false-negative shapes fixed in this release.

## [0.11.1] - 2026-06-20

### Added

- **`gaze-token-bridge` is now published to crates.io.** The crate remains
  experimental and focused on gated index-search; output backstop verification
  is still pending.

### Fixed

- **`gaze-cli` index installs now resolve from crates.io.** The optional
  `gaze-token-bridge` dependency now carries a publishable version so
  `gaze-cli` can publish and install with its `index` feature.

## [0.11.0] - 2026-06-20

### Added

- **`gaze-mcp-bridge`: optional policy-gated MCP bridge.** Gaze can now sit as
  an MCP server toward the agent and an MCP client toward downstream MCP
  servers, with restore-on-egress / redact-on-ingress handling, fail-closed
  errors, and default-deny policy behavior.
- **`gaze-token-bridge`: owner-side gated search over redacted corpora.** This
  experimental crate is not published to crates.io. It keeps search
  authorization and translation owner-side; output never-leak backstop
  verification is still pending.
- **`scan_folder` example for `gaze`.** The new bring-your-own-data redaction
  demo shows how to scan a local folder through the core runtime.

### Changed

- **Documentation now follows a Diátaxis × feature information architecture.**
  The docs were reorganized around task, reference, explanation, and tutorial
  needs while staying anchored to product features.
- **GDPR adopter guidance was substantially expanded.** The new material covers
  per-party identifiability, Article 25, Chapter V transfers, enterprise
  security expectations, and DPO-grade DPIA support.
- **Top-level repository layout was decluttered.** Examples, benches, and assets
  moved into crate-scoped docs, and the internal lint crate moved from `xtask/`
  to `lint/`.
- **Release notes are now sourced from `CHANGELOG.md` plus GitHub generated
  notes.** The committed `dist/release-notes` artifact was removed.

## [0.10.1] - 2026-06-04

### Fixed

- **fix(gaze-952): stop over-redacting camelCase command/argv identifiers + narrow lowerCamel suppression** (PR #302). This patch keeps command and argv-shaped identifiers from being treated as PII while preserving the tighter lowerCamel safety-net suppression.

## [0.10.0] - 2026-06-01

### Changed

- **BREAKING (`gaze-types`, custom recognizer authors): `Recognizer::detect`
  is now fallible** (P0 #908, PR #293). The trait method signature changed from
  the infallible

  ```rust
  fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate>;
  ```

  to

  ```rust
  fn detect(&self, input: &str, ctx: &DetectContext<'_>)
      -> Result<Vec<Candidate>, gaze_types::DetectError>;
  ```

  A recognizer backend can no longer represent "scan failed" as an empty
  candidate list — the only way a leak could previously slip through. The
  shared `DetectError` type lives in `gaze-types` (`DetectError::Backend {
  recognizer_id, message }`). `RecognizerRegistry` aggregation propagates the
  error, and the pipeline surfaces it as the new
  `gaze::pipeline::Error::RecognizerDetect(DetectError)` variant.

  **Migration for custom `Recognizer` impls:** wrap your existing return value
  in `Ok(...)`, and map any backend/runtime failure to
  `DetectError::backend(self.id(), <message>)` instead of swallowing it and
  returning an empty `Vec`. Infallible recognizers (pure regex/dictionary
  logic that cannot fail) simply return `Ok(candidates)`. See the fail-closed
  design contract in
  [`docs/architecture/p0-908-ner-failclosed.md`](docs/architecture/p0-908-ner-failclosed.md).

### Fixed

- **Byte-exact restore for adjacent and path-like tokens** (P0 #923, PR #295).
  Restore no longer inserts stray whitespace between adjacent spans, so
  path-like and back-to-back token sequences round-trip byte-for-byte. (Axis 2
  reversibility.)
- **Recognizer spans respect token boundaries** (P0 #923, PR #295). A
  recognizer no longer matches a substring inside a larger token — e.g.
  `Artist` is not tokenized inside `Artistfy`. A single-token common word such
  as `Workspace` is no longer promoted to an `Organization` span. (Axis 4
  determinism, fewer false-positive leaks of surrounding context.)

### Security

- **NER detection fails closed on backend error** (P0 #908, PR #293).
  Previously a NER backend runtime failure mapped to an empty detection set,
  silently passing raw text through unredacted — a critical PII-leak path. The
  failure now propagates as `DetectError::Backend` and the pipeline aborts
  outbound redaction (`Error::RecognizerDetect`) rather than emitting partially
  cleaned output. (Axis 1 never-leak.)
- **Long NER inputs are chunked into bounded, overlapping tokenizer-token
  windows** (P0 #908, PR #293). Inputs longer than the model's 512-token
  ceiling are scanned in overlapping WordPiece-token windows (480-token payload
  budget, 30-token overlap) so a long document can no longer slip past the
  model unscanned. The overlap is a documented security invariant —
  `overlap_tokens >= longest detectable entity + margin` — not a throughput
  knob; spans are remapped to original byte offsets before de-duplication.
  Contract:
  [`docs/architecture/p0-908-ner-failclosed.md`](docs/architecture/p0-908-ner-failclosed.md).
- **Release pre-flight now scrubs public text for local path/PII leaks** (PR #294).
  The new `scrub-public-text` gate scans release-facing docs and notes before
  publication, making accidental workspace-path or fixture leaks a release
  blocker rather than a post-release cleanup. (Axis 1 never-leak, Axis 4 trust.)

## [0.9.1] - 2026-05-29

v0.9.1 is a reliability and adopter-trust patch. The headline is an Axis-1
never-leak fix: NER `detect()` now fails **closed**. Previously a detector-backend
error returned an empty detection set, silently passing the raw text through
unredacted — a critical leak path that let PII reach an LLM outside the manifest
contract. The backend error now propagates as a typed failure instead of an empty
result, and inputs longer than the NER window (>512 tokens) are chunked so long
documents can no longer slip past the model unscanned. Manifest restore semantics
and the signed snapshot wire format are unchanged from v0.9.0.

### Added

- **Accessibility-aware CLI output gate** (PR #287): the CLI honours `NO_COLOR`
  and `CLICOLOR_FORCE` and performs TTY detection; informational output is never
  conveyed by colour alone. (Axis 5 ergonomics.)

### Changed

- **Daemon-mode docs reframed as stdio server.** `gaze daemon` is now documented
  as a long-lived stdio server in the LSP / MCP / language-server-protocol
  tradition rather than a Unix daemon in the strict sense. The subcommand verb is
  unchanged through v0.9.x; a `gaze serve` canonical alias is planned for v0.10
  (todo #486). External adopter feedback prompted the reframe. (Axis 4 trust,
  Axis 5 ergonomics.)
- **`gaze document clean` bundle layout splits into agent + owner paths** (axis 1
  enforcement). `Bundle::write` now requires distinct `AgentBundleDir` and
  `OwnerBundleDir` newtypes; the writer rejects equal or nested paths with typed
  `DocumentError::BundleLayoutInvalid`. The CLI gains `--agent-out` + `--owner-out`
  and retains `--out` as a shorthand that auto-creates `<PATH>/agent` +
  `<PATH>/owner` subdirs.
- **CI: DCO sign-off is now enforced on pull requests** (PR #288), and the Rust
  toolchain is pinned to 1.96.0 for reproducible trybuild output (PR #289).
  Contributor-facing; no adopter API change. (Axis 4 trust.)

### Fixed

- **Axis-1 bundle leak risk** (closes todo #489): `gaze document clean` previously
  wrote `manifest.json` next to `clean.md` in a single caller-selected `out_dir`,
  with no runtime enforcement of the agent / owner partition. Adopters following
  the README who uploaded the bundle directory to an LLM workspace leaked
  restorable manifest material. The new split-path bundle layout enforces the
  agent / owner partition at type and path-validation level. Original two-directory
  `manifest.bin` signed-envelope binding (the v0.7.0 architectural spec in
  `docs/architecture/document-extension.md`) remains a v0.11+ follow-up.

### Security

- **NER fail-closed never-leak fix** (PR #290): a recognizer/NER-backend error no
  longer returns an empty detection set that passes raw text through unredacted;
  the error now propagates and inputs exceeding the NER window (>512 tokens) are
  chunked. Any byte of PII reaching an LLM outside the manifest contract is a
  critical defect — this closes a detector-error bypass of the redaction pipeline.
  (Axis 1 reliability.)

## [0.9.0] - 2026-05-16

v0.9.0 is the performance-wave final release: Kiji int8 ORT warm p50 lands at
1.849ms in the committed model leaderboard snapshot, int8 preserves F1 recall
with a 0.000 delta across the safety-net matrix, opt-in pipeline skip/capitals
gates reduce the synthetic numeric bench from 300 SafetyNet calls to 0, and the
documented prefix-cache run reduces detector bytes by 52.7% and latency by
50.8%. `gaze daemon` removes full binary fork + model-load overhead for repeated
calls, and `tract` provides the new opt-in static-binary path. Manifest restore
semantics and the signed snapshot wire format are unchanged from v0.8.1.

Measured on: Apple M5 Max / macOS 26.5 hosts in the committed v0.9 snapshots
and final rc revalidation. Methodology, runnable commands, fixture SHAs, and
model pins: [`docs/benchmarks.md`](docs/benchmarks.md).

### Added

- **In-process Kiji ORT backend** (PR #250 `4b8db66`): Kiji DistilBERT now runs
  inside the process instead of through the Python subprocess path; the final
  public latency claims are tied to the committed benchmark snapshots and
  [`docs/benchmarks.md`](docs/benchmarks.md). (Axis 1 reliability, Axis 5 ergonomics.)
- **Kiji int8 dynamic quantization** (PR #256 `0a35f8e`): shipped the quantized
  Kiji bundle with 1.849ms warm p50 in
  `crates/gaze-recognizers/benches/ner_models_snapshot.json` and a 0.000 F1
  recall delta in
  `crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json`.
  Measured on: Apple M5 Max / macOS 26.5; see
  [`docs/benchmarks.md`](docs/benchmarks.md). (Axis 1 reliability, Axis 4 trust.)
- **`gaze daemon` JSONL stdio mode** (PR #255 `cddbea4`): a persistent,
  multi-session CLI daemon removes per-call binary fork and model-load overhead
  while preserving the same reversible session contract. (Axis 3 agentic-first,
  Axis 5 ergonomics.)
- **Tier 4 pipeline gating and prefix cache** (PR #252 `67374cb`): opt-in skip
  gates, capitals heuristic, prefix cache, and length bucketing reduce avoidable
  SafetyNet work; the synthetic numeric bench drops SafetyNet calls from 300 to
  0, and the documented prefix-cache run reduces detector bytes by 52.7% and
  latency by 50.8%. Measured on: Apple M5 Max / macOS 26.5; see
  [`docs/benchmarks.md`](docs/benchmarks.md) and
  `crates/gaze/benches/tier4_pipeline_gating.rs`. (Axis 1 reliability,
  Axis 5 ergonomics.)
- **Runtime comparison benchmark** (PR #257 `0fd7c8e`): published ORT vs tract
  vs candle results; recommendation is ORT by default and tract for opt-in
  static binaries. (Axis 4 trust.)
- **Tiny-model leaderboard** (PR #258 `3fd3859`): validates Kiji int8 as the
  v0.9 default against the smaller model candidates. (Axis 4 trust, Axis 5 ergonomics.)
- **End-to-end pipeline benchmark** (PR #244 `1bf78f3`): added detection +
  performance measurement for the full Gaze pipeline. (Axis 4 trust.)
- **Python Kiji runner reference wrapper** (PR #236 `c5e623f`): added a reference
  subprocess runner to bridge the older benchmark path and the new in-process
  runtime work. (Axis 4 trust.)
- **CLI plaintext JSON `entries` field** (PR #232 `ac579cd`): `gaze clean
  --format=json` now emits top-level `entries` mirroring session snapshot entries
  while preserving the signed `session_blob`; empty detections emit `entries: []`.
  (Axis 5 ergonomics.)
- **OPF checkpoint trust pin and benchmark cells** (PR #240 `e681361`, PR #241
  `430ba7f`): the OpenAI Privacy Filter backend pins the local checkpoint bundle
  SHA256 and required artifact inventory, and the benchmark snapshot now includes
  measured OPF direct-detector cells. (Axis 4 trust.)
- **Observer-residual safety-net cells and latency snapshot** (PR #248 `7cab754`):
  Kiji and OPF share a direct/observer scorer, observer-residual cells are
  populated across locale buckets, and latency snapshots cover the 150-fixture
  direct-mode corpus. (Axis 4 trust.)
- **Multi-NER leaderboard** (PR #245 `4a4338d`): published
  [`v0.9-ner-model-leaderboard.md`](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9-ner-model-leaderboard.md); Kiji selected as the v0.9
  default per shipped class-map measurement. (Axis 4 trust, Axis 5 ergonomics.)
- **Audit NER-provenance schema migration** (PR #238 `b030038`): `gaze-audit`
  adds eleven nullable provenance columns to `redaction_log` for future NER
  attribution; existing rows read back with `NULL` provenance fields. (Axis 4 trust,
  Axis 5 ergonomics.)
- **Locale-aware Pass-3 SafetyNet dispatcher** (PR #226 `98fa572`, PR #242
  `4b28ebc`): `Pipeline::with_safety_net_registry` routes SafetyNet work through
  `LocaleAwareModelRegistry`; the CLI gains registry and backend-locale override
  flags, and audit rows carry the resolved backend id. (Axis 1 reliability,
  Axis 5 ergonomics.)
- **Metrics single source of truth** (PR #235 `71c8e6c`): added `docs/metrics.md`
  to catalog observable surfaces. (Axis 4 trust.)

### Changed

- **Workspace version pin** `0.8.1` -> `0.9.0` across all ten published
  crates.
- **Synthetic email fixtures** (PR #233 `e48901b`) now use the IANA-reserved
  `@example.invalid` domain instead of reachable `@example.com` examples across
  crates and document fixtures. (Axis 4 trust.)
- **`SqliteLogger` leak-suspect writes now have one canonical verb** (PR #234
  `77f91b9`): call `LeakSuspectLogger::log_leak_suspect`; SQLite schema and
  write behavior are unchanged. (Axis 5 ergonomics.)
- **Pre-1.0 API naming cleanup** (PR #237 `d6a48af`): `redact_text` /
  `redact_with_context` names now use `pseudonymize_*` terminology to match
  reversible pseudonymization. (Axis 4 trust naming.)
- **Safety-net benchmark snapshot schema v2** (PR #230 `1bc60f5`): internal
  benchmark artifacts move from single-backend Kiji fields to a backend x locale
  x mode matrix with mode-independent strict-span leak-rate entries. (Axis 4 trust.)
- **Kiji and OPF benchmark docs** (PR #231 `eba3c03`, PR #245 `4a4338d`, PR #257
  `0fd7c8e`): consolidated v0.9 research docs around pinned artifacts,
  per-locale metrics, observer-residual caveats, runtime tradeoffs, and the Kiji
  int8 default recommendation. (Axis 4 trust.)
- **README and docs scope corrections** (PR #228 `051bbc9`, PR #229 `4bdcaa8`,
  PR #246 `5ff0b1b`, PR #251 `dbc6bdc`, PR #254 `930e38e`): clarified the
  ghostwriter flow, replaced the Mermaid walkthrough with an ASCII flow, removed
  private demo references, and qualified proxy scope as API-key path only.

### Removed

- **Private demo-repo README link** (PR #251 `dbc6bdc`): removed the stale link
  from public docs. (Axis 5 ergonomics.)

### Fixed

- **CI proxy smoke SIGPIPE** (PR #227 `31e00cc`): proxy status capture now uses
  command substitution so smoke tests do not trip SIGPIPE.
- **Ubuntu CI disk-full failure** (PR #249 `d412800`): frees preinstalled bloat
  before the workspace build on `ubuntu-latest`.
- **v0.9.1 follow-up fixups pulled into rc.1** (PR #253 `23d9ddd`): typed
  snapshot accessor, OPF artifact-hash gate, silent-drop telemetry, and
  locale-aware benchmark re-run.

### Release validation notes

- Final validation re-ran the coverage-loop recall pass, the Kiji int8
  direct/observer scorer, the ORT int8 benchmark, and
  `cargo run -p xtask -- ci-feature-matrix` on `origin/main` commit `79ba82f`
  (`v0.9.0-rc.1`) before promoting the release notes. Measured on: Apple M5 Max
  / macOS 26.5; see [`docs/benchmarks.md`](docs/benchmarks.md) and
  [`v0.9.0-rc1-combined-revalidation.md`](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9.0-rc1-combined-revalidation.md).
- The PR #240 checkpoint-pinning caveat remains scoped to benchmark
  reproducibility: the old release checksum URL returned the expected pre-tag
  404, so the validation used the local Kiji cache after checksum verification.
  Kiji int8 observer-residual macro recall held at `0.666667`; the one-shot
  Python scorer's p99 row was an outlier while median and p95 remained inside
  the drift band.
- Reversibility is unchanged: manifest restore semantics and the signed snapshot
  wire format remain compatible with v0.8.1.
- ORT is the recommended runtime default; use the opt-in `tract` path when a
  static binary is the deployment constraint.

## [0.8.1] - 2026-05-15

Reversibility-first SafetyNet defaults, layout-report v2 with vector-PDF + multi-column + table-cell + deskew handling, an `OcrBackend` trait for plug-in OCR drivers, model-SHA integrity for the Kiji backend, and the default release binary now baking `--features proxy`. Schema-level: `BundleReport.bundle_version` bumps `1 → 2` (additive); `gaze-audit` rows gain a typed `fallback_triggered: Option<FallbackReason>` column and `decided_by` gains `Redact`/`Resolve`/`Fallback` variants.

### Added

- **SafetyNet `resolve` + `redact` + `fallback` modes** (PR #223 `167acca`): suspect spans flagged by Pass-3 SafetyNet are now promoted to custom-recognizer matches and rejoin conflict resolution before any irreversible side-effect. On promotion failure the typed fallback path emits a `:Redact_` token and records a `FallbackReason` in the audit log. Closed-enum variants: `FallbackReason::{OverlapConflict, ValidatorVeto, AnchorMissing, ResidualSuspect}`. New `decided_by` variants: `Redact`, `Resolve`, `Fallback`. CLI gains `--safety-net-mode {resolve|strict|tolerant}` and `--safety-net-fallback {redact|none}`. Tolerant remains dev-only behind `GAZE_ALLOW_TOLERANT=1`. (Axis 1 reliability, Axis 2 reversibility, Axis 4 trust.)
- **gaze-document layout report v2** (PR #219 `6acf77e`, PR #222 `9714b41`): `BundleReport.bundle_version` bumps `1 → 2`. New per-page fields: `ocr_source`, `ocr_backend`, `confidence`, `low_confidence`, `column_count`, `page_index`. New top-level field: `low_confidence_threshold`. Vector-PDF text-extraction fallback when PDFs have selectable text; multi-column segmentation in the post-processor; per-page confidence + low-confidence flagging against the threshold; table-cell preservation in markdown output; rotation/deskew preprocessing before OCR. v1 bundles continue to parse on read; emission is always v2. (Axis 1 reliability, Axis 4 trust.)
- **`OcrBackend` trait** (PR #218 `b9f3407`): single trait, single impl (`TesseractBackend`). `gaze-document` now exposes one OCR contract that second-party backends (ocrs, Apple Vision, PaddleOCR) can slot into cleanly. Trait is object-safe; covered by `tests/ocr_backend.rs`. (Axis 4 trust, Axis 5 ergonomics.)
- **Kiji model-SHA integrity** (PR #221 `07cf93d`): `KijiDistilbertSafetyNet` backend now verifies the DistilBERT bundle SHA256 at init and fails closed via `SafetyNetError::ModelIntegrityMismatch { expected, actual }` on mismatch. Direct-vs-observer benchmark harness shipped under `gaze-recognizers/benches/`; published metric fields stay `null` until populated on a machine with the pinned local Kiji runtime (Axis 4 — no uncited benchmark numbers). (Axis 1 reliability, Axis 4 trust.)
- **Safety-net architecture contract** (PR #216 `1cf6732`): `docs/architecture/safety-nets.md` now documents the resolve/redact/fallback semantics, the typed `FallbackReason` set, and how SafetyNet promotion interacts with `ConflictTier`. Companion adopter-facing doc updated in PR #217 `d55af13`.

### Changed

- **`--safety-net-mode resolve` is the new default** (PR #217 `d55af13`, PR #223 `167acca`), replacing `strict`. Reversibility-first; falls back to `redact` on resolve failure. Strict mode remains available for hard-fail deployments via `--safety-net-mode strict`. (Axis 1, Axis 2.)
- **Default release binary now bakes `--features proxy`** (PR #220 `fc00c26`): the published `gaze-v0.8.1-*.tar.gz` artifacts include `gaze proxy {serve,start,stop,status,logs,restart}` out of the box. Adopters who build from source unchanged.
- **Marketing-pass README** (PR #215 `ad22121`): adopter-focused copy refresh; no behavior change.
- **Workspace version pin** `0.8.0 → 0.8.1` across all ten crates.

### Removed

- **Legacy `OcrAdapter` shims** (PR #224 `89aaa4e`): the deprecated v0.7.1 adapter surface is gone. Adopters who plug in custom OCR now implement `OcrBackend` directly. Magic-byte validation (`detect_image_format`) is now mandatory at the `clean_with_ocr_backend` boundary — bare-byte payloads fail closed with `DocumentError::UnsupportedInput`.

### Fixed

- **Table-cell mock-backend test missing PNG magic bytes**: `bundle::tests::clean_with_mock_backend_preserves_table_cell_context` failed on `89aaa4e` after the magic-byte gate landed in PR #224. Test fixture now prepends `\x89PNG\r\n\x1A\n` to the synthetic payload. CI was red on main HEAD; this commit makes it green.

### Migration notes

- If your downstream tooling reads SafeBundle JSON: handle the `bundle_version=2` field. v1 reads work; v2 emission is non-optional.
- If you query the audit log: the new `fallback_triggered` column is nullable on existing rows; the new `decided_by` variants are closed-enum and discriminated.
- If your pipeline expected `--safety-net-mode strict` as default: pass the flag explicitly.
- If you embedded custom OCR via `OcrAdapter`: port to `OcrBackend` (object-safe, same shape).
- v0.7.x → v0.8.x → v0.8.1 multi-hop adopters: read [UPGRADE.md](./UPGRADE.md) before bumping; v0.8.0 already flipped several defaults.

## [0.8.0] - 2026-05-14

Bundle unification + versioned recognizer lineage + Kiji-style defense-in-depth + ten checksum-backed and locale-gated national-ID recognizers across five new locale packs, plus the new `gaze-proxy` crate that puts a PII chokepoint in front of OpenAI / Anthropic / Gemini API traffic. The workspace publish count rises from nine to ten.

### Added

- **Versioned recognizer lineage** (v0.8 Tier 1, PR #203 `3c95304`): `Candidate.recognizer_version_id` + `RedactionEntry.recognizer_id` + `recognizer_version_id` (all `Option<String>`, additive). Audit boundary in `pipeline.rs` now propagates lineage instead of dropping at `source`. SQLite schema gains nullable `recognizer_id` / `recognizer_version_id` columns; pre-migration rows tagged `legacy_unversioned`. NER recognizer emissions versioned as `ner.<model>.<vN>` from artifact config metadata (`ner.unknown.v0` fallback). `docs/architecture/locale-chain.md` gains a coverage matrix listing every bundled recognizer × supported locales × ValidatorKind. (Axis 4 trust/auditable, Axis 5 ergonomics.)
- **`SafetyTier` enum on rulepack recognizers** (v0.8 Tier 1.5, PR #201 `8ab9daf`): `SafeDefault`, `LocaleGated`, `OptIn` with `#[non_exhaustive]`. Closed-enum activation gate replaces the dual-bundle activation model. (Axis 1 reliability, Axis 4 trust.)
- **`KijiDistilbertSafetyNet` backend** (v0.8 Tier 2.5, PR #202 `0cd9ccc`): new `--safety-net-backend kiji-distilbert` flag (default remains `openai-filter` for compat). Pass-3 SafetyNet device with pinned-artifact contract identical to existing OpenAI filter (SHA256SUMS hard-fail on missing). New `CliError::SafetyNetArtifactMissing { backend, path }` typed variant. `scripts/fetch/fetch-kiji-safetynet-model.sh` mirror of existing NER fetcher. (Axis 1 defense-in-depth.)
- **Seven checksum-backed locale validators** (v0.8 Tier 2, PR #207 `16c1fd5`): Aadhaar Verhoeff (IN), French NIR MOD-97 variant (FR), German Steuer-ID MOD 11,10 (DE), Dutch BSN MOD-11 (NL), Brazilian CPF + CNPJ MOD-11 (BR), and UK NHS number MOD-11 (UK). New `ValidatorKind` variants are closed-enum and fail-closed on parse. Five new locale packs ship alongside: `locale-fr`, `locale-nl`, `locale-br`, `locale-in`, `locale-uk`. Every entity ships at `safety_tier = "safe_default"`, so adopters in BR/FR/NL/IN/UK get coverage out of the box once their locale is set. (Axis 1 reliability, Axis 3 agentic-first.)
- **Three locale-gated regex recognizers** (v0.8 Tier 3, PR #208 `7348690`): US SSN, UK NINO, and Indian PAN. All ship at `safety_tier = "locale_gated"` — no bare 9-digit / 10-character shapes activate without explicit locale + cue context. PAN extends the existing `locale-in` pack from Tier 2 in place. (Axis 1 reliability, Axis 4 trust.)
- **Corpus rework v2 implementation** (PR #205 `aa9c5fc`): the 61 stochastic status-quo templates + the `fixture_variants` mechanism are replaced with 150 deliberate scenarios. Each scenario declares its expected emissions including `recognizer_version_id` from day one. `fake` crate added as an xtask-only dev dependency; seed pinned in a documented `COVERAGE_CORPUS_SEED` constant. `baseline.json` fully re-snapped. (Axis 4 trust.)
- **`UPGRADE.md`** (PR #206 `492573d`): per-minor migration guide complementing `CHANGELOG.md`, with v0.7.x → v0.8.0 TL;DR + backfill summaries for v0.4 → v0.7.
- **[`v0.8-kiji-class-gap.md`](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.8-kiji-class-gap.md)** (PR #210 `eba350a`): coverage map of all 26 Kiji PII classes against gaze's recognizers — 6 beat-via-Tier-2, 1 beat-via-Tier-3, 16 observer-only-via-Tier-2.5, 3 parity, 0 deferred.
- **[`v0.8-kiji-benchmark.md`](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.8-kiji-benchmark.md)** (PR #209 `b875381`): two-mode (direct-detector + observer-residual) benchmark methodology headlining strict span leak rate, with a rule-floor snapshot pinned to corpus + Gaze tag. Kiji direct-detector + observer-residual cells deferred (no pinned model SHA yet — tracked as v0.8.x follow-up).
- **`ARCHITECTURE.md`** (PR #211 `fd130ac`): 14.8 KiB repo-root architecture overview of how the ten workspace crates fit together, with eight numbered Key Design Decisions and a one-diagram view of the redact/restore path.
- **`gaze-proxy` crate at v0.8.0** (PR #212 `503d0f9`): new published workspace crate. Multi-provider HTTP proxy with an adapter/driver pattern that serves OpenAI's `/v1/chat/completions`, Anthropic's `/v1/messages`, and Gemini's `/v1beta/models/*:{generateContent,streamGenerateContent}` without translation. SSE streaming and tool-call argument reconstruction wired through `gaze::Pipeline` (chunk-split PII spans inside `tool_calls.function.arguments` are accumulated and redacted before leaving the proxy). Daemon-mode subcommands `gaze proxy {serve,start,stop,status,logs,restart}` plus opt-in `install-launchd` / `install-systemd-user` installers. Feature-gated on `gaze-cli` as `--features proxy`, off by default. (Axis 3 agentic-first, Axis 5 ergonomics.)

### Changed

- **Unified `core` + `core-extended` bundled rulepacks** into single `core` bundle (v0.8 Tier 1.5, PR #201). Each recognizer now declares a `safety_tier` field; no-policy activation gates on `SafeDefault` only. Adopter behavior preserved through alias path described under Deprecated.
- **Workspace version pin** `0.8.0-rc.1` → `0.8.0` across ten crates (was nine; `gaze-proxy` joins this release).
- **[bundle-tokenization-drift] `baseline.json`** fully re-snapped against the corpus rework v2 scenarios.

### Deprecated

- **`--rulepack-bundled core-extended`** is deprecated (v0.8 Tier 1.5, PR #201). Aliases to `--rulepack-bundled core` with auto-activation of `LocaleGated` recognizers + a `tracing::warn!` deprecation notice. Scheduled removal in v0.10.0. Adopters who relied on the v0.4.5 PR #58 no-policy surprise activation for `phone.national.*` / `postal.*` should pass `--locale=de-DE` or `--locale=en-US` explicitly.

### Migration notes

- Existing `RedactionEntry` JSON consumers see no shape change — new fields use `#[serde(skip_serializing_if = "Option::is_none")]` and emit nothing when None.
- Existing SQLite audit DBs are migrated forward: pre-migration redaction rows get `recognizer_id = "legacy_unversioned"`, `recognizer_version_id = NULL`. Migration is idempotent.
- Existing `policy.toml` files unchanged. No new required fields.
- **`gaze-proxy` is opt-in** behind `--features proxy` on `gaze-cli`; existing adopters are unaffected unless they invoke `gaze proxy serve` or `gaze proxy start`.
- All Tier 2 + Tier 3 entity adds are additive. Adopters in BR / FR / NL / IN / UK / US-only / UK-only see no behavior change unless they enable their locale via `--locale=<bcp47>`.

## [0.7.2] - 2026-05-13

Dogfooding-driven point release. Both findings surfaced during a Pulseflow
adopter demo (`EmpireTwo/business:dogfooding/pulseflow-demo-2026-05-13`) and
strengthen the trust + adopter-ergonomics axes of the north star.

### Added

- **Policy schema versioning** (F#6, PR #192 `e698e35`): new top-level
  `schema_version` field in `policy.toml`. The loader gates the `major.minor`
  prefix against the supported version and fails closed with a dedicated
  `{"error":"PolicySchemaUnsupported","exit":2,"found":"...","supported":"0.1"}`
  CLI envelope. Existing 0.6.x/0.7.x policies continue to load via a soft
  default. Public-surface additions: `gaze::SUPPORTED_POLICY_SCHEMA_MAJOR_MINOR`,
  `gaze::DEFAULT_POLICY_SCHEMA_VERSION`, `gaze::PolicyError::PolicySchemaUnsupported`,
  `gaze_cli::CliError::PolicySchemaUnsupported`. (Axis 4 trust, Axis 5 ergonomics.)

### Changed

- **PolicyConfig error envelope now carries detail** (F#5, PR #191 `6ec7afd`):
  every `gaze-cli` `map_err(|_| PolicyConfig)` site now threads the underlying
  loader cause through `CliError::PolicyConfigDetail`. JSON shape stays additive
  — `{"error":"PolicyConfig","exit":2}` unchanged, optional `detail` field now
  populated at every site EXCEPT the bare clap parse fallback (intentional —
  argv noise must not leak through `detail`). (Axis 4 trust, Axis 5 ergonomics.)

## [0.7.1] - 2026-05-12

### Added

- New OSS crate `gaze-document` for document → safe-bundle generation.
  Ships with PNG/JPG/PDF input support via Tesseract subprocess OCR (single-page
  PDF rasterization via pdfium). Output is a `SafeBundle` containing redacted
  Markdown + a restorable `gaze::Manifest` + an OCR/PII report. New `gaze document
  clean <input> --out <dir>` subcommand on `gaze-cli` (opt-in via `--features
  document`).
- Validator-veto pre-resolver phase for validator-backed recognizer failures.
  Invalid candidates are rejected before conflict resolution, then logged as
  loser-only audit rows with `decided_by: ValidatorVeto` and typed
  `validator_fail_reason` metadata. See
  `docs/architecture/validator-veto.md`.
- Collision-family metadata and `FamilyPolicyTable` for cross-class recognizer
  rivalries. Bundled `core-extended` now declares PAN-vs-IBAN and phone-family
  metadata, `ConflictTier::CollisionPolicy` is audit-serializable, adopter
  custom recognizers can declare non-reserved collision families, and
  `xtask family-policy-table-coherence` validates bundled declarations.
- Mandatory-anchor resolution for collision-family recognizers. Bundled locale
  cue blocks under `[locale.cues.<key>]` can keep structural candidates on their
  precise variant; missing anchors emit a family-level
  `PiiClass::Custom("family:<name>")` token plus `AmbiguityReason::NoAnchor`.
  `xtask locale-cue-bundle-coherence` validates bundled cue coverage.
- Ambiguity side-channel and bundled audit migration for v0.7.x collision
  handling. `RedactionEntry` can carry `ValidatorFailReason` and
  `AmbiguityRecord`; `SqliteLogger` migrates `validator_fail_reason`,
  `ambiguity_record`, `collision_family`, and `collision_variant`; CLI audit
  queries can filter ambiguity and collision metadata. See
  `docs/architecture/ambiguity-side-channel.md`.
- `gaze-document` opt-in `mcp` feature exposes `gaze_read_file` and
  `gaze_read_text` Tool impls that route document ingestion through
  `PiiEnvelope::dispatch`. Returns `{ clean_markdown, manifest_id,
  file_metadata }`.
- `gaze-cli` opt-in `mcp` feature exposes `gaze mcp install --client=<name>`,
  `gaze mcp doctor`, and `gaze mcp serve` subcommands. Install writes client
  JSON with the absolute `current_exe()` path and an idempotent marker-fenced
  skill section in `AGENTS.md`. Ships claude-code, claude-desktop, and cursor
  at launch.

### Changed

- [bundle-tokenization-drift] `core-extended` no-policy snapshot refreshed for
  mandatory-anchor fallback on structural IBAN candidates without loaded locale
  cue bundles.

### Deprecated

### Removed

### Fixed

### Security

## [0.7.0] - 2026-05-11

### Added

- `gaze-mcp-core` — transport-free PII chokepoint runtime: `Tool` trait, sealed `ToolCtx`, `ToolRegistry`, `PiiEnvelope::dispatch`, `Frontend`/`DispatchHost`, `ManifestStore`, `AuthHook`, and `SessionIdPolicy`. Public tool structs (`CleanTool`, `TokenizeFieldTool`, `SafetyNetCheckTool`, `ExportSessionTokensTool`, `RestoreTool`, `RestoreStrictTool`) all use `#[non_exhaustive]` with `pub fn new()` constructors per pre-1.0 SemVer policy. (#162)
- `gaze-mcp-rmcp` — rmcp transport sink that binds `gaze-mcp-core`'s transport-free runtime to the rmcp protocol surface. (#174)
- `gaze_pii::Session::export_with_extension(DocumentExtension) -> Result<SensitiveSnapshot>` — opt-in document mode for OCR/PDF/transcript bundles. (#177)
- `gaze_types::DocumentExtension` (signed-envelope-bound integrity hashes for `<base>-agent/` files). (#177)
- `gaze_types::TextOrigin`, `CodecAuditRow`, `CodecCapabilitySet`, `ExtractionDensityPolicy`. (#177)
- `docs/architecture/document-extension.md` — bundle contract + two-dir layout reference. (#177)
- Coverage feedback loop Phase 0+1: xtask `coverage-corpus` + 5-fixture integration test skeleton. (#176)
- Coverage feedback loop (Phase 2-5): full synthetic corpus plus info-only trend gate. (#178)
- CC-8 token-shape shadow guard at policy + rulepack regex paths fails closed on patterns that match emitted token samples. `PolicyError::TokenShapeShadow` + `RulepackError::TokenShapeShadow`. (#162)
- `gaze-audit` columns `snapshot_scheme` (TEXT NOT NULL), `snapshot_alg` (TEXT NOT NULL), `snapshot_key_version` (INTEGER NULL) on the audit row. Pre-existing rows migrate with `"gaze.snapshot.v1.sha256-salted"` / `"SHA-256"` / NULL defaults. Plumbed through `AuditLogRow`, `AuditFilter`, and `build_audit_query_sql`. (#179)
- `Ipv4Parse`, `Ipv6Parse`, and `EthEip55` validator kinds for parser-backed
  IP address validation and EIP-55 Ethereum address checksums. Closes #440.
- `eth.address` in `core-extended`, emitting `custom:eth_address` for
  EIP-55-valid Ethereum addresses.
- New dependency: `sha3 = "0.10"` for Keccak-256 checksum validation.

### Changed

- `gaze_pii::Session` snapshot reference now binds the final emitted byte sequence rather than an earlier semantic object. Operator-bypass mutations post-snapshot are detectable. Pre-existing audit rows continue to verify under the v1 scheme tag. (#179)
- Snapshot envelope: text-only `Session::export()` stays v3; document-extended `Session::export_with_extension()` emits v4. v0.6.x readers fail closed on v4. (#177)
- Workspace bumped 0.6.6 → 0.7.0.
- [bundle-tokenization-drift] `eth.address` and parser-backed IP validator fixtures refreshed `core` and `core-extended` no-policy snapshots.

### Fixed

- `gaze_pii::default_policy` falls back to `Tokenize` (axis-1 fail-closed). (#175)

### Deprecated

### Removed

### Security

## [0.6.6] - 2026-05-09

### Fixed

- Each crates.io page for `gaze-pii`, `gaze-types`, `gaze-audit`, `gaze-recognizers`, `gaze-assembly`, and `gaze-cli` now renders its own per-crate README. Previously, the v0.6.5 placeholder publish mirrored the project root README to all 8 placeholder stubs, so adopters landing on `crates.io/crates/gaze-types` saw the umbrella project README instead of the gaze-types-specific content.

### Changed

- Real workspace crates publish to crates.io at v0.6.6 via the trusted-publisher OIDC workflow. Placeholder stubs at v0.6.5 remain as version history.

### Notes

- `gaze-mcp-core` and `gaze-mcp-rmcp` stay at v0.6.5 placeholder content until their feature branches merge in v0.7. The v0.7.0 release publishes both as real crates with their own per-crate READMEs.
- No code changes vs v0.6.5. Detection contracts, audit-sink isolation, and recognizer behavior are identical.

## [0.6.5] - 2026-05-09

### Added

- `SECURITY.md` — vulnerability disclosure policy with scoped in/out-of-scope
  criteria for the chokepoint runtime, audit-sink isolation, and recognizer
  fail-open regressions.
- `CODE_OF_CONDUCT.md` — Contributor Covenant 2.1.
- `.github/workflows/publish-crates.yml` — crates.io trusted-publisher OIDC
  workflow, with no long-lived token, for workspace publishes on tag push or
  manual dry-run dispatch.
- README badges for crates.io, license, docs.rs, tests, and GitHub stars, plus an
  "Available on crates.io" section listing all published workspace crate names.
- `.github/workflows/test.yml` — fmt + clippy + workspace test suite on PRs
  and main push.
- Placeholder publishes on crates.io at 0.6.5 for `gaze-pii`, `gaze-types`,
  `gaze-audit`, `gaze-recognizers`, `gaze-assembly`, `gaze-cli`,
  `gaze-mcp-core`, and `gaze-mcp-rmcp` to reserve namespace ahead of the v0.7
  real publish. Each placeholder mirrors the canonical project README and
  declares the same internal dependency topology the real workspace will
  publish.

### Changed

- README rewrite: tighter lede, copy-paste build-from-source install snippet
  until v0.7, token format example matched to runtime output, license section,
  and no v0.7 roadmap language in install instructions.
- Repo description changed from "GDPR-compliant debugging proxy between AI
  agents and production data" to "Reversible PII pseudonymization runtime for
  agentic LLM workflows."
- Adopter attribution in CHANGELOG and the `gaze-recognizers` NER module docs
  now uses neutral "an adopter" phrasing instead of named individuals.
- Repository visibility changed from private to public.

### Notes

- No code changes in this release. Detection contracts, audit-sink isolation,
  and recognizer behavior are identical to v0.6.4. Adopters pinned to `^0.6.4`
  resolve to v0.6.5 with no behavioral diff.
- v0.7.0 is the next functional release; it introduces `gaze-mcp-core`
  (chokepoint runtime) and `gaze-mcp-rmcp` (rmcp transport adapter) as full
  implementations.

## [0.6.4] - 2026-04-30

### Added

- `phone.national.de` rulepack class extended with DE 3-digit and 4-digit
  area-code metro alternations. Closes #420.

### Changed

- Removed bogus `891` ONK from the 3-digit alternation; BNetzA
  Vorwahlverzeichnis source URL and as-of date are pinned in test fixture
  comments.
- Test fixtures use synthetic non-reachable subscriber shapes
  (zero-exchange-code per `CONTRIBUTING.md:42`) instead of real-looking
  BNetzA-assigned numbers.
- Pre-push hook gains a docs-only fast-path for allowlisted documentation
  paths, from PR #120 by external contributor @naoray.

### Fixed

- IBAN-shape mod-97-failing input now has test coverage documenting the
  class-misattribution behavior while preserving manifest restore and avoiding
  leaks.

## [0.6.3] - 2026-04-30

### Added

- `phone.national.de`: 10-digit metropolitan landline coverage (Berlin 030,
  Hamburg 040, Frankfurt 069, Munich 089). Previously only matched 11+ digit
  national-significant-numbers, leaking common metro landlines. Closes #414.

### Fixed

- `phone.national.us`: consuming-boundary mirror with `phone.national.de`
  rejects identifier-attached numbers like `Order_15551234567`,
  `Customer+12025550100`. Closes #415.
- `phone.structural`: cross-recognizer leak — applied consuming-boundary class
  so global E.164 candidate respects same identifier-attached rejection as
  national recognizers. Previously `Customer+12025550100` leaked through
  `phone.structural` even after `phone.national.us` rejected it.
- DE phone regex no longer over-matches formatted IBAN tails like
  `DE89 3704 0044 0532 0130 00`.

## [0.6.2] - 2026-04-30

### Fixed

- `ip.v6` recognizer: RFC 4291 §2.2 IPv4-embedded form support
  (`x:x:x:x:x:x:d.d.d.d`, including IPv4-mapped `::ffff:d.d.d.d` and
  IPv4-compatible `::d.d.d.d`). Previously, inputs like
  `::ffff:192.0.2.128` partially tokenized as `::ffff:192`, leaking the
  embedded IPv4 octets. Closes #419.

## [0.6.1] — 2026-04-30

### Added

- `gaze clean --openai-filter-device {auto|cpu|cuda|mps}` selects the
  Pass-3 OpenAI SafetyNet subprocess device. The default `auto` preserves
  v0.6.0 behavior (closes #362).

### Changed

- `phone.national.de` now matches German national phone numbers across
  hyphen, space, slash, and dot separator variants, including `0171-...`,
  `0171 ...`, `0171/...`, and `+49-171-...` shapes (closes #316, refs #92).

### Fixed

- `xtask cargo-metadata-audit-isolation` now fails loud on unknown feature
  names instead of silently ignoring them, with an explicit cross-platform
  allowlist for known optional cargo metadata features (closes #340, closes
  #350).
- Default-feature CLI builds no longer warn on dead OpenAI device-selection
  helper code when `safety-net-openai` is disabled.

## [0.6.0] — 2026-04-29

### Added

- Tracked `.githooks/pre-push` runs full local gate matrix (cargo fmt + tests + xtask gates) before allowing push. Doc-only pushes fast-path. `GAZE_PREPUSH_FAST=1` skips xtask gates when CI is healthy. One-time setup: `git config core.hooksPath .githooks` per clone.
- **v0.6 GH #24 anchored_match recognizer kind:** cue-anchored
  `Name` detection now covers email forward headers, agent reply preambles, and
  auto-footers through deterministic structural rules. The default `core`
  bundle adds `name.forward_marker`, `name.agent_recipient`, and
  `name.auto_footer` with structural audit source labels such as
  `structural.agent_recipient`.
- **v0.6 locale cue buckets:** `locale-de` now ships `forward_markers`,
  `agent_recipient_cues`, and `footer_cues` with German cues plus English
  safety duplicates; `locale-en` ships English-only cue buckets. The synthesis
  matrix and 12-fixture false-positive budget are locked in tests for GH#24.

### Changed

- **Trait method signature changed.** Custom `RedactionLogger` impls must update
  their return type from `gaze::Result<()>` to
  `Result<(), gaze_types::RedactionLogError>`. Import-path source-compat is
  preserved via the permanent `gaze::RedactionLogger` re-export; the canonical
  trait home is `gaze_types::RedactionLogger`.
- **v0.6 RedactionLogger home moved to `gaze-types` (closes #252):**
  `gaze-types` now owns `RedactionLogger` and the closed
  `RedactionLogError` sink-error set. `gaze-audit::SqliteLogger` implements
  the trait directly, and `gaze` converts sink failures at the pipeline
  boundary through `gaze::Error::RedactionLog`.
- **v0.6 closes #114 — generic locale-bucket placeholder syntax adopted in
  bundled `core` rulepack:** the shipped `email.header.name` recognizer now uses
  the canonical `{locale.email_headers}` placeholder. The legacy
  `{locale_email_headers}` underscore alias still parses for back-compat (one
  more rev cycle, scheduled to drop in v0.7) — adopter rulepacks should migrate
  to the dotted form. No detection or token-shape change.
- **v0.6 adopter migration for GH#24:** v0.5.1 adopters can load
  `["core", "locale-de"]` under `[locale].active = ["de-DE"]` to tokenize the
  prompt/header/footer leak shapes reported by adopters without changing existing custom
  recognizers. Mixed German/English templates can load
  `["core", "locale-de", "locale-en"]`; per-tenant cue additions should live in
  custom locale/rulepack data.
- **v0.6 known limits documented:** `anchored_match` still fires inside
  markdown code fences and URLs in v0.6; RegionHint-based `CodeBlock` / `Url`
  exclusion is deferred to v0.7. The docs also call out deferred Subject/Re
  anchors, unanchored scheduling prose, the current `person_name`-only
  `name_shape`, and global rather than per-region NER thresholding.
- **v0.6 audit source-label coverage:** audit-row metadata tests now lock in
  `AUDIT_RESTRICTED_COLUMNS` including `source`, so persisted audit queries can
  explain structural `anchored_match` emissions without adding a
  `recognizer_id` column. References GH #24.
- **v0.6 audit source-label normalization:** `name.auto_footer` now emits the
  structural source label `structural.footer`, matching the `footer_cues`
  bucket wording used by the bundled locale rulepacks. References PR #84 NIT
  #289.

### Deprecated

### Removed

- **BREAKING — audit-sink imports.** Replace `use gaze::SqliteLogger;` with
  `use gaze_audit::SqliteLogger;`. Same for `gaze::AuditFilter`,
  `gaze::AuditLogRow`, `gaze::build_audit_query_sql`, and
  `gaze::AUDIT_RESTRICTED_COLUMNS`. The v0.5 `gaze = { features = ["audit"] }`
  shim is removed.
- **v0.6 audit feature shim removed from `gaze` (closes #315):** removed the
  `audit` feature, the optional normal `gaze-audit` dependency, the cfg-gated
  `gaze::{SqliteLogger, AuditFilter, AuditLogRow, build_audit_query_sql,
  AUDIT_RESTRICTED_COLUMNS}` re-exports, the cargo-deny `gaze.audit` feature
  ban, and the xtask `"gaze audit feature sanity"` cargo-metadata graph.

### Fixed

### Security

### Pass-3 SafetyNet (PR #91 — ships in v0.6.0 alongside the audit-shim drop and the v0.6 anchored_match work)

#### Added

- **Pass-3 observer-only SafetyNet rollup (PR #91):** new
  privacy backend that audits Gaze's clean output for PII the deterministic
  pipeline missed, without ever mutating the manifest, the clean text, or
  the restore path. The shipped backend is the official OpenAI Privacy
  Filter (`opf`) subprocess adapter. North-star fit is explicit: A1 (never
  leak) holds because the upstream `text` and `placeholder` JSON fields
  are stripped at the adapter boundary and never cross into Gaze; A2
  (reversibility) is preserved because the contract is observer-only and
  the manifest is immutable from a backend's perspective; A3
  (agentic-first) is supported by per-field structured-document traversal
  that emits field-pathed suspects for agent tool-call JSON; A4
  (auditable + deterministic) is preserved by the closed `SafetyNetError`
  variant set, the typed `LeakKind` classification (`Uncovered` /
  `PartialBleed` / `ClassMismatch`), and the optional `safety_net_log`
  SQLite table.
- **`gaze-types` SafetyNet trait surface (Phase 1):** new public
  `SafetyNet`, `SafetyNetContext`, `LeakSuspect`, `LeakKind`,
  `LeakReport`, `LeakReportTelemetry`, `SafetyNetPiiClass`,
  `OpenAiPrivateLabel`, and `SafetyNetError` types. The contract is
  byte-free: `SafetyNetContext` is `Copy`, holds borrowed references, and
  exposes only manifest, locale chain, document kind, optional opaque
  session id, and optional structured field path.
- **`Pipeline::clean_with_safety_net_detect_context` (Phase 2):** new
  pipeline entry point that runs deterministic clean, builds the manifest,
  and dispatches per-field structured traversal to registered safety
  nets. Returns `(CleanDocument, LeakReport)`. Locale-skip telemetry is
  recorded per field when the session-level locale chain does not match
  the backend's `supported_locales`.
- **`OpenAiFilterSafetyNet` adapter (Phase 4) at
  `crates/gaze-recognizers/src/safety_net/openai_filter`:** subprocess
  adapter for the official `openai/privacy-filter` `opf` CLI, invoked as
  `opf --format json --output-mode typed`. Adopters bring their own
  pinned upstream Git revision or release. PII-bearing `text` and
  `placeholder` JSON fields are deserialized through a private
  `PrivatePiiString` whose `Drop` clears the buffer and whose `Debug`
  writes `<private-opf-field>`; spans are projected to `RawSpan`
  (start, end, label, score) before any code outside the adapter sees
  them.
- **Subprocess deadline + resource isolation (closes #320, refs #321,
  closes #322):** single deadline covers stdin write, stdout read,
  stderr read, and child wait. Timeout fires `SIGKILL` and reaps the
  process, returns `SafetyNetError::Runtime { message: "opf subprocess
  timed out and was killed" }`, which the CLI maps to exit `3` with
  variant `Timeout`. Stdout/stderr readers are bounded (4 MiB / 256 B).
  Initialization failures are cached in a
  `OnceLock<Result<Arc<...>, Arc<...>>>` so deterministic problems do
  not retry on every clean.
- **Stderr discipline:** default `Stdio::null()`. Opt-in
  `with_stderr_diagnostics(true)` captures up to 256 bytes, replaces
  non-printable bytes with spaces, and sanitizes whitespace-separated
  tokens that contain `@` or seven or more ASCII digits to `<redacted>`
  so backend logs cannot leak emails or phone shapes.
- **Checkpoint perms verification:** `--openai-filter-checkpoint` must
  exist before the subprocess spawns. Files and directories must be
  owned by the current uid, must not be symlinks, and must not be
  group/world writable; directories must be mode `0700`. Missing
  checkpoints produce sanitized `WeightsMissing { path:
  "<missing:<filename>>" }`.
- **`gaze-cli` SafetyNet surface (Phase 6):** new flags `--safety-net`,
  `--openai-filter-command`, `--openai-filter-checkpoint`,
  `--openai-filter-operating-point`, `--safety-net-timeout-ms`,
  `--safety-net-input-limit-bytes`, `--safety-net-mode` (strict |
  tolerant). `clean` JSON output gains a `leak_report` block carrying
  typed stats. Strict mode exits `3` on `Uncovered` / `PartialBleed`
  suspects with variant `SuspectedLeak`; tolerant mode emits a stderr
  `{"warning":"SafetyNet",...}` event and exits `0`. `ClassMismatch`
  always warns and never fails strict mode.
- **Exhaustive `SafetyNetError` -> `CliError::SafetyNetFailure` mapping:**
  stable variant strings (`Unavailable`, `WeightsMissing`,
  `ModelUnavailable`, `InputTooLarge`, `Timeout`, `Runtime`,
  `InvalidOutput`, `SuspectedLeak`) so adopters can branch on the
  failure shape without parsing free-form text.
- **`safety_net_log` audit table (Phase 5, `gaze-audit`):** new table
  on the existing audit DB stores metadata-only suspect rows plus
  `LocaleSkipped` telemetry events. Restricted columns lock that no raw
  upstream payload (text or placeholder bytes) is persisted; the
  `safety_net_log_does_not_persist_suspect_or_placeholder_bytes` test
  pins the invariant. `gaze audit safety-net query --audit-db <path>`
  reads filtered rows back from a read-only connection.
- **`safety-net-sanity` xtask gate (Phase 7):** new behavioral gate
  batched across `gaze`, `gaze-cli`, `gaze-recognizers`, and
  `gaze-audit` that asserts manifest diff invariants, strict/tolerant
  CLI behavior, subprocess boundary safety, and `safety_net_log` schema.
  Enforced by `.githooks/pre-push` through
  `cargo run -p xtask -- safety-net-sanity`.
- **`class-map-override-safety` extension (Phase 7):** the existing gate
  now asserts that `all_official_labels_map_exactly_to_gaze_classes`
  runs and passes, so the closed OPF label allowlist cannot drift
  silently.
- **`ci-feature-matrix` extension (Phase 7):** the matrix now enrolls
  the `safety-net` and `safety-net-openai` feature combos so the gated
  code paths are covered by the local pre-push gate.
- **MockSafetyNet test helper (Phase 3):** `gaze-recognizers` exports
  a `test-support`-gated `MockSafetyNet` so adopter tests can drive
  manifest diffing without spawning a subprocess.
- **Documentation (Phase 8):** new
  [`docs/architecture/safety-nets.md`](docs/architecture/safety-nets.md)
  covers the trait shape, observer-only contract, OPF adapter boundary,
  stderr discipline, structured-doc traversal, replay hash, audit
  table, and CI gate. `crates/gaze-cli/README.md` documents every
  flag, the exit-code map, the latency budget, and synthetic examples
  using only approved fixtures (RFC 6761 `*.invalid` domains, NANPA
  `555-01xx` phones, Ofcom drama ranges). `docs/policy.md` notes that
  SafetyNet activation is CLI / programmatic only and lists the
  requirements any future TOML surface must satisfy (locale gating,
  fail-closed load, default strict mode, CLI override precedence).

#### Changed

- **deny.toml feature scope (Phase 0):** safety-net dependency bans for
  `reqwest`, `hyper`, `tokio`, and `ureq` are scoped to the
  `safety-net-*` feature graphs. The `cargo-metadata-audit-isolation`
  xtask gate is the authoritative enforcer; `cargo-deny` remains a
  belt-and-suspenders check for feature policy.

#### Notes for adopters

- SafetyNet code paths are gated off by default. Build with
  `--features safety-net-openai` on `gaze-cli` (or `gaze-recognizers`
  for programmatic use) to opt in. Existing clean / restore consumers
  see no dependency-graph change.
- Bring-your-own-binary plus bring-your-own-weights: install `opf`
  from a pinned upstream Git revision or release. The adapter does
  not download or update the checkpoint. Pin the install path with
  `GAZE_OPENAI_FILTER_OPF=<path>` or `--openai-filter-command=<path>`.
- Strict mode is the default. Tolerant mode
  (`--safety-net-mode=tolerant`) preserves exit `0` for runs that
  report suspects, but always writes a stderr warning event so
  monitoring can pick it up.
- Activation is **CLI / programmatic only**, not `policy.toml`, in this
  Pass-3 rollup. See `docs/policy.md` for the requirements any future
  TOML surface must satisfy.
- This rollup ships in **v0.6.0** alongside the audit-shim drop
  and the v0.6 `anchored_match` recognizer work. Adopters
  upgrading from v0.5.x see one combined release: switch
  `gaze::SqliteLogger` imports to `gaze_audit::SqliteLogger`, then opt
  into SafetyNet at their own pace via the `safety-net-openai` feature.

#### Deferred to a post-v0.6.0 release

The following SafetyNet items are intentionally out of scope for v0.6.0
and are tracked for a later release:

- **Live-model nightly workflow** with a non-empty synthetic corpus to
  detect FP-rate drift between checkpoint upgrades.
- **Native `ort` backend** with a `weights.rs` SHA-pinned scaffolding
  module that removes the subprocess hop. The `OpenAiFilterBackend`
  trait shape was designed so the same adapter API serves both
  subprocess and in-process implementations.
- **Fetch / download command** (`gaze safety-net fetch`) that pulls a
  pinned `opf` build into a private cache directory and verifies the
  checksum offline. Closes the "first-run requires manual install" gap.
- **Long-lived subprocess / daemon mode** to amortize subprocess
  startup cost when latency budgets tighten.
- **False-positive adjudication dashboard** on top of `gaze audit
  safety-net query` and `audit export` so reviewers can triage
  suspects across runs.

See
[`docs/architecture/safety-nets.md` "Future work"](docs/architecture/safety-nets.md#future-work-deferred-to-a-post-v060-release)
for the same list with its design notes.

## [0.5.2] - 2026-04-29

### Added

- **NER adopter assets (GH issue #90 items 1+4):** promoted the
  Davlan mBERT label contract and canonical NER policy snippet to
  `crates/gaze-recognizers/assets/ner/` for framework adapters and adopters. `crates/gaze-recognizers/assets/ner/README.md`
  documents the BIO tag to Gaze class schema, the `"drop"` sentinel, and the
  future `gaze model fetch <name>` / `gaze policy snippet ner` manifest path.

### Changed

- **Pinned default NER artifact source (GH issue #90 item 2):**
  `scripts/fetch/fetch-ner-model.sh` now installs the pre-quantized int8 ONNX artifact
  from `onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` at commit
  `cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8`, copies the Gaze-authored
  `labels.json` contract, and verifies all installed bytes against the
  repository-root `SHA256SUMS`.
- **Policy docs for NER adopters:** `docs/policy.md` now cites the canonical
  `crates/gaze-recognizers/assets/ner/` contracts, documents `[ner].locale` as a single BCP47 string,
  and calls out Rust-regex inline flags such as `(?i)` in
  `[[policy.custom_recognizers]].pattern`.

## [0.5.1] - 2026-04-29

### Fixed

- **Bundled rulepack version sync:** corrective patch - bundled `core`, `core-extended`, `locale-de`, and `locale-en` rulepacks now report `rulepack_version = "0.5.1"`, restoring the v0.4.6 CHANGELOG contract that bundled rulepacks track `gaze-recognizers`. v0.5.0 release-prep missed the embedded TOMLs; this patch corrects that.

### Changed

- Version bump 0.5.0 -> 0.5.1 across `gaze`, `gaze-types`, `gaze-recognizers`, `gaze-audit`, `gaze-cli`, and `gaze-assembly`.
- [bundle-tokenization-drift] v0.5.1 rulepack_version sync refreshed `core` and `core-extended` no-policy snapshots; only the `rulepack_version` field changed.

## [0.5.0] - 2026-04-27

### Added

- **v0.5 Phase B — `gaze-types` crate (PR #74, commit `4675b79`):** new shared-contract crate hosts `Recognizer`, `Detection`, `PiiClass`, `Action`, `RedactionEntry`, `LocaleTag` / `LocaleChain` / `LocaleError`, `RawDocument`, `CleanDocument`, `DictionaryBundle`, and the token-related value types. Adopters now get a serde-only contract crate without `ort`/`tokenizers`/`ndarray` ML deps in their dependency tree. `gaze` re-exports the contracts under their previous paths for source-compatibility.
- **v0.5 Phase B — `bundled-recognizers` feature gate (PR #74):** `gaze` no longer pulls `ort`/`tokenizers`/`ndarray`/`onig` in `--no-default-features` builds. Default features remain unchanged, so existing CLI / library consumers see no behavior change.
- **v0.5 Phase B — `DictionaryBundleExt` extension trait (PR #74):** `bundle.from_context(&ctx)` now requires `use gaze::DictionaryBundleExt;` (or import from `gaze-types`). The split keeps `gaze-types::DictionaryBundle` a pure value type while preserving the convenience constructor for `gaze` callers.
- **v0.5 Phase B — `DictionaryEntry::try_new` validated construction (PR #74):** empty term lists and non-ASCII case-insensitive entries fail closed at construction time rather than reaching the recognizer registry. `DictionaryEntry::new` is replaced by the validated `try_new`.
- **v0.5 Phase C — `gaze-audit` crate (PR #75, commit `64b6394`):** new passive-sink crate hosts `SqliteLogger`, `AuditFilter`, `AuditLogRow`, `build_audit_query_sql`, and `AUDIT_RESTRICTED_COLUMNS`. `gaze` no longer carries `rusqlite` in its default or `--no-default-features` graphs.
- **v0.5 Phase C — `audit` feature shim on `gaze` (PR #75):** one-minor migration window. `gaze = { features = ["audit"] }` re-exports `gaze::SqliteLogger` and the audit-query symbols by adding `gaze-audit` as a normal dependency. Scheduled to be removed in v0.6 (decision drawer `gaze_decisions_6c60bce3b9f8ed7a4de538d8`).
- **v0.5 Phase C — `cargo-metadata-audit-isolation` xtask gate (PR #75):** parses `cargo metadata --format-version=1` and fails closed if any non-audit-responsible workspace member has a normal-dependency path to `gaze-audit` in default or `--no-default-features` graphs. The audit-responsible allowlist is documented in source; `gaze-cli` is the only allowed consumer because its `audit` subcommands run against the passive sink directly.
- **v0.5 Phase C — `cargo deny` audit-feature ban (PR #75):** denies enabling `gaze`'s `audit` feature outside the dedicated compatibility tests, blocking accidental reintroduction of `gaze-audit` into the protected default graph.
- **v0.5 Phase D — `gaze_module_isolation` Dylint lint (PR #76, commit `3e367d1`):** Dylint late-HIR lint replaces the syn-walker `audit-metadata-only` gate. Resolution runs through `LateContext::qpath_res` against rustc's name resolver, not text matching. `check_item`, `check_expr`, `check_ty`, trait references, struct fields, and macro emission are covered. 18 UI fixtures cover all known bypass classes including macro call-site hygiene, `#[path]` modules, `include!`, type positions, trait bounds, and `extern crate gaze_audit`. Pinned toolchain: `nightly-2025-09-18`, `clippy_utils@20ce69b9...`, `dylint_linting`/`dylint_testing` 5.0. New `dylint` GitHub Actions workflow runs the gate on every push to `main` and PR.
- **v0.5 Phase D — `dylint-gate` xtask command (PR #76):** verifies the `lint/dylint/ui` fixture corpus has exactly 18 enabled fixtures, rejects `*_disabled.rs`, and runs `cargo dylint --workspace --all` when `cargo-dylint` is installed (skips with a clear message locally when absent; CI installs it explicitly).

### Changed

- **v0.5 Phase B / C audit-sink refactor:** `gaze` core no longer carries `rusqlite` in default or `--no-default-features` builds. Library callers that previously imported `gaze::SqliteLogger` should switch to `use gaze_audit::SqliteLogger;` (preferred), or temporarily enable `gaze`'s `audit` feature for the one-minor migration window.
- [bundle-tokenization-drift] Release aggregation refreshed `core` and `core-extended` no-policy snapshots for the v0.4.6 bundled rulepack version bump.

### Removed

- **v0.5 Phase E — legacy `audit-metadata-only` syn walker (PR #77, commit `f4fde12`):** decommissioned. The Dylint gate added in Phase D is now the canonical audit-sink protected-path enforcer. Phase E removed: the inline syn-walker source from `crates/xtask`, the `RESTORE_AUDIT_FORBIDDEN_SYMBOLS` constant, the adversarial walker tests in `crates/xtask/tests/adversarial_audit_metadata_only.rs`, and the `.github/workflows/audit-metadata-only.yml` workflow. Net: `-942` lines of legacy walker code, tests, and workflow.

### Migration notes (adopters)

- `use gaze::SqliteLogger;` → `use gaze_audit::SqliteLogger;` (preferred). One-minor compatibility option: `gaze = { features = ["audit"] }` re-exports the original path; the shim is scheduled to drop in v0.6.
- `bundle.from_context(&ctx)` now requires `use gaze::DictionaryBundleExt;` (or `use gaze_types::DictionaryBundleExt;`). The trait is the explicit migration seam introduced when `DictionaryBundle` moved into `gaze-types`.
- `DictionaryEntry::new(...)` → `DictionaryEntry::try_new(...)?` if the call site cannot statically guarantee a non-empty term list and ASCII case-insensitive entries.
- Workspace tests that reference `gaze::SqliteLogger` via the dev-dependency path should run with `cargo test --workspace --all-features`; the `--all-features` flag enables the `audit` shim that those compatibility tests rely on.

## [0.4.6] - 2026-04-26

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.6`.
- Bundled rulepack versions now track `gaze-recognizers` at `0.4.6`.
- **Bundle-tokenization drift gate:** no-policy `core` and `core-extended` bundled outputs now have checked-in baselines; intentional drift requires an explicit source ACK and changelog marker before release.
- **Fixture-citation lint:** synthetic fixture policy is now enforced by `xtask`, tightening the no-real-PII discipline for examples and tests.
- **Rulepack-derived bundle classes:** bundled class listings are derived from rulepacks instead of hand-maintained metadata, reducing release drift for adopter-facing bundle docs and checks.
- **DE national-phone recall broaden:** `core-extended` recognizes additional documented synthetic German national-phone mobile shapes while preserving parser-backed validation.
- **CI/no-feature matrix:** `xtask ci-feature-matrix` guards the no-default-feature phone parser path so unsupported parser validators continue to fail closed.
- **Homebrew tap decision:** README install guidance remains release-asset first until a public tap exists and the release process publishes to it.

## [0.4.5] - 2026-04-26

### Added

- **Audit retention manual purge (PR #59):** `gaze audit purge --before <iso8601> [--dry-run | --count]` deletes redaction-log rows older than the cutoff. Calendar-aware ISO 8601 validation rejects malformed dates fail-closed with typed `AuditPurgeIso8601` error. Restricted DELETE clause; no policy-level retention default; no background auto-purge.
- **`audit_metadata_only` xtask gate (PR #59):** compile-time enforcement that restore-path code does not import audit metadata symbols. Walker covers file scope `use`, nested `mod`, function/impl/trait-default/const/static block-statement `use`, glob imports, aliased crates, `extern crate`, and `#[path]`-resolved external modules. Known limitations (fully-qualified path references, `include!`, let-else diverge, macro-emit) documented in `docs/architecture/xtask.md`; v0.5 architectural pivot to dylint-based name-resolution lint scheduled.
- **`--session` audit filter (PR #57):** opaque session-scope filter for `gaze audit query` / `gaze audit export` (NOT raw `session_hex`).
- **DE + US national phone recognizers (PR #58):** parser-backed E.164 region-aware validators (`phonenumber` crate) for German and US national phone numbers. Cooperate with structural phone recognizer; gated behind `phone-parser` Cargo feature.
- **ClassMapOverrideSafety extension (PR #55 / S4):** further hardening of class-map override safety gate.
- **Rulepack version bump validation (PR #56 / S5):** rulepack version bump audit + drift-prevention rule.
- **`gaze-assembly` crate restructure (PR #61 / S6):** `lib.rs` split into focused modules by responsibility.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.5`.
- **`core-extended` no-policy locale activation (PR #58):** the bundled `core-extended` rulepack now activates `phone.national.de`, `phone.national.us`, `postal.us`, and `postal.de` recognizers when invoked without a policy via `--rulepack-bundled core-extended`. Previously these required an explicit `--locale` or policy-supplied locale. Adopters using the bundle without a policy will see additional tokenization for German/US national phone numbers AND bare 5-digit numeric strings (matching the postal recognizers). To restore prior behavior, supply an explicit `--locale=global` or pass a policy with narrower locale gating.

### Fixed

- No standalone `fix(...)` commits landed between `v0.4.4` and `v0.4.5`; the bundle is release plumbing plus S1-S6 feature, hardening, and documentation work.

### Documentation

- README catch-up for v0.4.2-v0.4.4 (PR #60).
- README Requirements section with per-OS support matrix (PR #62).
- Org transfer URL sweep from the original org to the then-current org (PR #63).
- New `docs/architecture/xtask.md` documenting `audit_metadata_only` gate coverage, known limitations, and v0.5 roadmap.
- New `v0.5-dylint-audit-gate.md` research stub (now hosted in [PIInuts/business:research/](https://github.com/PIInuts/business/blob/main/research/v0.5-dylint-audit-gate.md)).

## [0.4.4] - 2026-04-26

### Added

- **S1 ClassMapOverrideSafety xtask gate** (#51): the previously scaffolded gate is now active. The behavioral test runner invokes `t20_context_class_map_overrides_policy_dict_class` and `t20a_class_map_override_fails_closed_when_action_rule_uncovered` through `cargo test`, while `.github/workflows/class-map-override-safety.yml` runs the gate on PRs and pushes to `main`. An adversarial in-PR self-test programmatically verifies the gate fails non-zero when a listed test is missing or renamed, following the meta-Potemkin guard captured in drawer `gaze_architecture_12b32d53`.
- **S2 audit schema v2** (#53): `RedactionEntry` now includes `created_at: i64` epoch milliseconds, with an on-open SQLite `ALTER TABLE` migration so legacy DBs without `created_at` remain queryable through a NULL default. `gaze audit query` and `gaze audit export` now accept `--from <iso8601>` and `--to <iso8601>` filters, JSONL export includes `created_at`, and ISO 8601 parse failures emit typed `CliError::PolicyConfig` messages with the offending input quoted. Time-filtered queries omit NULL `created_at` legacy rows by SQL semantics; unfiltered queries still include them. Fixture coverage covers both v0.4.3-shaped and v0.4.4-shaped SQLite DBs.
- **S3a phonenumber-backed `E164Phone` validator** (#52): the `phonenumber` crate is available behind the optional `phone-parser` feature, default-on for `gaze-cli` and opt-in for raw library users. `ValidatorKind::E164Phone` extends the existing `phone.structural` recognizer in `core-extended.toml`, preserving valid E.164 matches such as `+4915550112233` while rejecting regex-passing but unassigned shapes such as `+99999999`. Builds without `phone-parser` reject the `e164_phone` validator at rulepack load time with `RulepackError::UnsupportedValidator`, preserving axis-1 fail-closed behavior rather than silently dropping phone detection at runtime. Audit notes live in [`PIInuts/business:research/v0.4.4-phonenumber-audit.md`](https://github.com/PIInuts/business/blob/main/research/v0.4.4-phonenumber-audit.md).
- **S4 Date posture memo** (#50): [`PIInuts/business:research/v0.4.4-date-posture.md`](https://github.com/PIInuts/business/blob/main/research/v0.4.4-date-posture.md) locks Gaze's Date-as-PII stance. Dates are not PII by default, never ship in default `core` or `core-extended` bundles, and future v0.4.5+ implementation scope is limited to DOB-only structured contexts. General-prose dates require context classification research for v0.5+, and the GH #5 token-spam tradeoff is resolved as no-default-on. The negative corpus covers version strings, IPs, file paths, ID-shaped numerics, year-only strings, and build or CI metadata.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.4`.
- ClassMapOverrideSafety is no longer a scaffold; `cargo run -p xtask -- class-map-override-safety` now executes its named tests and returns a meaningful exit code.
- The audit query path continues to open SQLite read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY`, carrying forward the v0.4.3 S4 hardening.

### Notes for adopters

- The Linux x86_64 binary requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer), the same constraint as v0.4.2 and v0.4.3.
- Phone validation is feature-gated. `gaze-cli` enables `phone-parser` by default; raw library users opt in with `gaze-recognizers = { features = ["phone-parser"] }` when they need parser-backed E.164 validation. Without that feature, `e164_phone` is rejected at rulepack load time.
- Audit time filters accept ISO 8601 timestamps through `--from` and `--to`. Legacy audit DBs without `created_at` are still queryable, but time-filtered queries exclude their NULL timestamp rows by SQL semantics.

### Deferred to v0.4.5

- `--session` audit filtering, deferred from v0.4.4 until the session identifier storage type design is locked.
- DOB-scoped Date recognizer, per the S4 memo and only if an adopter provides a concrete DOB leak fixture.
- S3b national phone recognizers for DE and US, deferred from v0.4.4 due to scope budget.
- ClassMapOverrideSafety coverage for other class-rule paths.
- Audit retention and auto-purge, now unblocked by the v0.4.4 `created_at` foundation.

### Deferred to v0.5

- Open-key `PiiClass` refactor.
- Crate-shape Option B: extract `gaze-types` and collapse `gaze-assembly`.

## [0.4.3] - 2026-04-26

### Added

- **S1 ValidatorKind substrate** (#47): three new validators in `crates/gaze-recognizers/src/regex.rs`: `Luhn` for Mod 10 checksums, `IbanMod97` for ISO 7064 mod-97 validation, and `IbanCanonical` for uppercase-plus-whitespace-stripped normalization.
- **S2 core-extended Phase 2** (#48): two validator-backed recognizers in `core-extended.toml`:
  - `iban.structural` matches IBANs with optional whitespace, applies the `iban_mod97` validator plus `iban_canonical` normalizer, and emits class `custom:iban`.
  - `card.structural` matches broad credit-card shapes with optional space or hyphen separators, applies the `luhn` validator, and emits class `custom:credit_card`.
  - Default `[[rule]]` entries now ship in the rulepack so `--rulepack-bundled core,core-extended` tokenizes these classes out of the box, following the CLI shipping divergence pattern captured in drawer `gaze_architecture_c6eefa4b`.
  - The bundled `core-extended` rulepack version is now `0.4.3`.
- **S3 xtask `no_tenant_knowledge` gate** (#46): production-code lint scanner rejects tenant-pattern strings (`order_id`, `Order_42`, `Song_42`, `User_7`) in `crates/{gaze,gaze-recognizers,gaze-assembly,gaze-cli}/src/`. Allow markers (`// allow(tenant-fixture)`) hard-fail in production scope and remain valid only in `tests/`, `benches/`, `docs/`, and `CONTRIBUTING.md`. CI runs the gate through `.github/workflows/no-tenant-knowledge.yml`, and an adversarial in-PR self-test verifies the scanner actually scans rather than printing success.
- **S4 `gaze audit query/export` CLI** (#45): the existing `commands/audit.rs` stub is now wired into full read-only metadata export from audit SQLite. Filters include `--class`, `--source`, `--action`, and `--document-kind`; JSONL is the default output. A restricted column set defends against extra-column leaks, with cross-version SQLite fixture coverage for current and legacy schemas.
- Tenant numeric ID negative fixtures (`Subscriber_*`, `Order_*`, `Customer_*`, `0815 12345`) are explicitly proven not to fire as IBAN or credit-card matches.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.3`.
- `--audit-db` queries now open the SQLite database read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY` for defense in depth, so the audit CLI cannot write to the DB even if compromised.

### Deferred to v0.4.4

- `--session` and `--from`/`--to` audit filters need a session column and `created_at` schema migration.
- Date recognizer needs an explicit policy-posture brainstorm, including the GH #5 tradeoff considerations.
- National phone patterns need parser-backed per-locale validation because of collision risk with tenant numeric IDs.
- Open-key `PiiClass` refactor plus crate-shape Option B remain targeted for v0.5.

### Notes for adopters

- The Linux x86_64 binary requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer), the same constraint as v0.4.2.
- Phase 2 validator-backed recognizers are opt-in via the `core-extended` rulepack; adopters using only `core` get no behavior change.

## [0.4.2] - 2026-04-25

### Added

- **S4 Linux release artifact:** release CI now publishes `gaze-x86_64-unknown-linux-gnu` from a native `ubuntu-24.04` runner, alongside `gaze-aarch64-apple-darwin`, with `.sha256` files for both artifacts. The Linux artifact requires glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer); older distros should build from source.
- Release artifact smoke now executes the packaged binary for `--version`, `alice@example.invalid` clean/restore reversibility, S1 runtime knob help flags (`--session-scope`, NER, and rulepack surfaces), and `core-extended` bundled rulepack loading with neutral non-real fixture data.
- v0.4.1 Bundle P1 foundation: `gaze-assembly` library entrypoint, `xtask` scaffold, and the `symmetric_potemkin_gate` workflow.
- `token.family` now threads from recognizers into session snapshot entries while preserving the existing emitted token grammar.
- Locale-aware regex `pattern_template` lowering for `{locale_email_headers}` with English and German defaults.
- `capture_groups = [...]` regex span narrowing with first-non-empty semantics.
- `NerRecognizer` public export plus `[ner] threshold` policy knob using min-aggregated span confidence.
- Core `email.header.name` recognizer for RFC822-style header display names, including German `Von:` / `An:` forms.
- Strict rulepack composition validation: same-class recognizer pairs now require explicit `cooperates_with` declarations.
- `Context::fields_typed() -> ContextFieldsRef<'_>` borrowed accessor for context-field consumers.
- `gaze clean --audit-db=<path>` persists the metadata-only SQLite redaction log for pipe-mode invocations.
- **S1 three-surfaces backfill:** `gaze clean` now exposes CLI overrides for existing policy runtime knobs: `--session-scope`, `--ner-model-dir`, `--ner-locale`, `--rulepack-bundled`, and `--rulepack-path`.
- **S2 core-extended rulepack:** opt-in bundled rulepack with Phase 1 shape-only recognizers for E.164 phone numbers, IPv4/IPv6 addresses, and `de-DE`/`en-US` postal codes.
- **S5 v0.5 design:** design doc for open-key `PiiClass` and decision-deferred crate-shape Option B sketch.
- **P3.5 #100 parity audit:** three-surfaces parity audit table for every `policy.toml` field, classifying runtime knobs with CLI/TOML/default coverage and policy-document fields that intentionally remain TOML-only.
- **P3.5 #114 generic placeholder vocab:** rulepack locale `pattern_template` placeholders now support generic `{locale.<bucket>}` expansion from adopter-defined `[locale.<bucket>] names = [...]` tables.

### Changed

- Coordinated version bump across `gaze`, `gaze-recognizers`, `gaze-cli`, and `gaze-assembly` to `0.4.2`.
- **P3.5 #115 CLI split:** split `gaze-cli/src/main.rs` into focused `commands`, `pipeline`, `restore`, `io`, `error`, and `logger` modules with responsibility-based names and no CLI behavior change.
- Snapshot envelope version bumped from 2 to 3; v0.4.1 imports v2 snapshots with default `counter` family, while v0.4.0 rejects v3 snapshots instead of silently collapsing family metadata.
- Dictionary recognizer audit sources now include per-term traceability as `dictionary:{name}[#term_index]`.
- **S3 fixture sweep:** renamed tenant-pattern test and benchmark strings to neutral placeholders, with `CONTRIBUTING.md` documenting tenant class naming policy.
- `{locale_email_headers}` remains supported as a v0.4.2 compatibility alias for `{locale.email_headers}` and is deprecated for removal in the v0.5 cycle.
- **P3.5 #116 NER split:** split the NER recognizer implementation into focused `ner/` submodules without changing public exports or runtime behavior.

### Fixed

- Adopter-reported gap closed: locale-aware email-header recognizer (`Von:` / `An:` plus English defaults) tokenizes header display names and restores them round-trip. See GH #24.
- `[ner] threshold` knob un-deferred from v0.4.2 so adopters can tune the NER confidence floor for prompt-preamble PII.
- Template lowering now preserves regex quantifiers such as `{0,3}` and keeps locale-header alternation non-capturing, so capture-group span narrowing remains stable.

## [0.4.0-rc.1] - 2026-04-24

### Added

- **F3 Rulepack schema** - TOML-defined recognizer bundles with closed validator/normalizer kind registry. Fail-closed on unknown matchers (Dictionary now wired; NER deferred to v0.5).
- **F4 Locale infrastructure** - 4-tier chain (CLI > policy > rulepack defaults > system default). Per-recognizer locale gating via `locales = [...]`. Strict opaque-tag matching.
- **F2-full Resolver** - class-priority > rule-priority > score > span-length > recognizer-id with multi-overlap fixed-point iteration.
- **F5 `.invalid` domain swap** - FPE email shape now uses `email{N}.{session_hex}@gaze-fake.invalid`. Legacy `example.test` Pass 2 trap arm retained for v0.3 manifest restore compatibility.
- **F6 Dictionary detector** - Aho-Corasick-backed recognizer registered through the new Recognizer trait. Adopter-tunable via `[[policy.custom_recognizers]]` or `--context-json` (standalone).
- **Typed Context envelope** - `--context-json` carries tenant fields/dictionaries/class_map through `DetectContext` into per-recognizer detection (no longer parsed-and-dropped).
- **F7.5 Byte-range-skip** - Pass 1 substitution spans tracked; Pass 2 trap scan skips matches fully contained in spans. Closes Pass 1->Pass 2 cascade false-positive (adopter raw values matching trap arms no longer rejected in strict mode).
- **Audit symmetry** - `RedactionEntry.decided_by` ConflictTier enum + merge-loser entries.
- **Schema-drift gating** - `RulepackError::UnsupportedFieldInB1` rejects `token.family`, `token.format`, `context.hotwords`, `context.boost`, `context.window` if set to non-default until consumers ship in v0.4.1.

### Changed

- **Pipeline**: legacy `Detector` trait path removed. All detection routes through `RecognizerRegistry`.
- **Policy surface**: legacy top-level `[[detector]]` rejected with `LegacyDetectorUnsupported` error; migrate to `[[policy.custom_recognizers]]`.
- **Locale tag matching**: `LocaleTag::Other(_)` now strict-equals (no longer universal fallback).

### Fixed

- NER label-map BIO-prefix resolution (already shipped in v0.3.1; folded into rc series for completeness).
- Cascade false-positive on adopter tenant identifiers (`Order_42`, `Song_42`, `User_7`) under strict mode (PR #22).

### Known limits - please test in dogfood

- **GH #24**: NER context-sensitivity gap - names in prompt boilerplate / RFC822 email headers may pass through default davlan-hrl. Workarounds + roadmap in issue #24.
- **token.family / token.format**: parsed + gated; runtime consumers planned for v0.4.1.
- **context.hotwords / boost / window**: parsed + gated; runtime consumers planned for v0.4.1.
- **Per-term traceability** in dictionary detection log: `dictionary:{name}` only; `[#term_index]` extension planned for v0.4.1.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>

## [v0.3.1] — 2026-04-24

### Fixed

- **NER silent no-op with BIO-prefixed labels.json.** `LabelMap::resolve`
  now accepts both BIO-prefixed (`B-PER`, `B-LOC`) and bare (`PER`, `LOC`)
  label keys. Previously, bundles shipping BIO-prefixed labels (the standard
  Davlan/HuggingFace format) produced zero detections silently. Adopters on
  aarch64-apple-darwin were particularly affected. (#19)
- Spec-drift: `[session]` policy.toml key now authoritative over
  `--session-ttl`. (#16)
- Spec-drift: Broken `[ner] model_dir` exits `PolicyConfig` (exit code 2)
  instead of silently degrading. (#16)
- Spec-drift: `kind = "column"` policy rules rejected by `gaze clean` CLI
  load. (#16)

### Added

- `tracing::info!("ner detector registered, N backends")` on NER bootstrap -
  adopters can now confirm whether [ner] block is being picked up. (#19)
- `tracing::warn!` on zero-overlap (NER inference ran but emitted 0 entities
  for input class) - surfaces silent detection failures. (#19)

### Changed

- README hero copy + project north star documentation refresh. (#15)
- Roadmap documentation for v0.4 / v0.4.1 / v0.5. (docs-only,
  offsite-readable)

## [0.3.0] — 2026-04-24

### Changed

- **Counter-family tokens now wrap in angle brackets.** `<{session_hex}:Email_1>`,
  `<{session_hex}:Name_1>`, `<{session_hex}:Custom:order_id_1>`. Format-preserving email tokens
  (`email1.{session_hex}@gaze-fake.invalid`) stay bare — angle brackets defeat the
  format-preserving purpose.

### Added

- **`crate::token_shape` module** exposing `pattern()` +
  `contains_token()`. Centralizes the token grammar the CLI's Pass 2
  hallucination detector uses. Drift-gate fixture forces compile
  errors if `PiiClass` grows without grammar updates.
- **Exhaustive Pass 1 + Pass 2 regex for wrapped tokens.** Pass 1 uses
  a delimiter-sensitive match (angle brackets serve as explicit
  delimiters); Pass 2 whitelists via `contains_token()`.
- **`docs/policy.md`** — user-facing `policy.toml` authoring guide.

### Fixed

- PR #10 follow-up — `Custom:` namespace round-trip + hallucination
  tests.
- **Homebrew formula SHA placeholders replaced** with the real
  `gaze-aarch64-apple-darwin` digest
  (`baa7edb79d84fea5d74377f82877c5069d861381a9f6012aa55af2264a8287f4`)
  once the tag-triggered release workflow published the binary. Closes
  the rc.1 "Known gaps" entry — `brew install Naoray/gaze/gaze` now
  resolves without the cask fallback.

## [0.3.0-rc.2] — 2026-04-23

Same contents as rc.1 — only the release workflow matrix changed
(x86_64-apple-darwin dropped). rc.1 was tagged but its workflow never
published a release: the `macos-13` Intel runner pool could not
allocate a runner for the x86_64 build, leaving the release job blocked
on an unmet dependency. Adopter target is Apple Silicon, so dropping x86_64
for rc unblocks the adapter retarget immediately; Intel + Linux return
in a later rc when runner strategy is worked out.

## [0.3.0-rc.1] — 2026-04-23

First release candidate of the standalone `gaze` CLI. Ships the
subprocess contract that language-specific adapters (e.g.
`gaze-laravel`) target. Library API surface continues to evolve in
parallel — the CLI protocol is the stable seam.

### Added

- **Standalone `gaze` CLI with pipe-mode subcommands.** `gaze clean`
  consumes plaintext on stdin and emits `{text, session_blob}`;
  `gaze restore` consumes `{text, session_blob}` and emits the
  rehydrated original. Adapters shell out rather than linking the
  library.
- **Two-pass restore.** First pass matches exact tokens via
  `Session::tokens()`; second pass runs a shape validator over the
  surviving text to catch reformatted token placeholders. Addresses
  the counselors-review finding that single-pass restore silently
  skipped renders.
- **Session TTL enforcement.** Snapshots carry `issued_at` and
  `Session::import` rejects blobs past the configured TTL with a
  `BlobExpired` error (CLI exit bucket 3). Prevents stale blobs from
  leaking tokens across restarts.
- **Policy TOML loader.** `Policy::load` parses a user-supplied
  `policy.toml`; `Pipeline::from_policy` builds the detection engine
  from it. `gaze --policy path/to/policy.toml` wires the file into the
  CLI.
- **Typed `CliError` variants with exit buckets and stderr JSON
  protocol.** `UnknownToken`, `Tamper`, `VersionByte`, `EmptyInput`,
  `InvalidEncoding`, `BlobExpired`, `MaxBytes`, plus a panic hook that
  funnels unexpected failures into the same structured protocol.
- **`--max-bytes` input size cap.** Rejects oversize input with a
  structured error instead of allocating unbounded buffers.
- **`--session-ttl` flag.** Overrides the default blob lifetime per
  invocation.
- **`--format=json` flag.** Stats output (`{detections, runtime_ms,
  ...}`) for adapter observability.
- **Pipe-mode integration suite.** Roundtrip, canary, `UnknownToken`,
  tamper, version-byte, argv, panic, and stats coverage.
- **Homebrew formula skeleton** at `dist/homebrew/gaze.rb`. SHAs
  filled post-release.
- **GitHub Actions release workflow** at `.github/workflows/release.yml`.
  Tag-triggered macOS builds (darwin-arm64 + darwin-x86_64).

### Changed

- **Workspace refocus: ghostwriter crate removed.** v0.2's
  language-specific `ghostwriter` crate was deleted in favour of the
  channel-agnostic `gaze` CLI. Adapters now consume the subprocess
  contract instead of linking a Rust library.
- **Custom class namespace fix.** Custom-class tokens are emitted as
  `Custom:{name}_N` rather than colliding with built-in class names.
- **`stats.detections` counter excludes `Preserve`.** Preserve-action
  hits are not real detections; they no longer inflate the count.
  Dead `Structured` dispatch branch dropped.

### Fixed

- Session snapshot payload carries an `issued_at` timestamp — previous
  layout had no basis for TTL enforcement.

### Known gaps (deferred)

- **Linux x86_64 binary not built.** The `ort` (ONNX runtime)
  dependency needs bundled system libraries; folded into a later rc
  to avoid blocking adopters on the adapter retarget.
- **Homebrew SHAs are placeholders** until the workflow publishes the
  darwin binaries; follow-up commit fills them.

[Unreleased]: https://github.com/CertaMesh/gaze/compare/v0.14.0...HEAD
[0.14.0]: https://github.com/CertaMesh/gaze/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/CertaMesh/gaze/compare/v0.12.0...v0.13.0
[0.6.4]: https://github.com/EmpireTwo/gaze/compare/v0.6.3...v0.6.4
[0.6.3]: https://github.com/EmpireTwo/gaze/compare/v0.6.2...v0.6.3
[0.6.2]: https://github.com/EmpireTwo/gaze/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/EmpireTwo/gaze/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/EmpireTwo/gaze/compare/v0.5.1...v0.6.0
[0.4.6]: https://github.com/EmpireTwo/gaze/compare/v0.4.5...v0.4.6
[0.4.5]: https://github.com/EmpireTwo/gaze/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/EmpireTwo/gaze/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/EmpireTwo/gaze/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/EmpireTwo/gaze/compare/v0.4.0-rc.1...v0.4.2
[0.4.0-rc.1]: https://github.com/EmpireTwo/gaze/releases/tag/v0.4.0-rc.1
[v0.3.1]: https://github.com/EmpireTwo/gaze/releases/tag/v0.3.1
[0.3.0]: https://github.com/EmpireTwo/gaze/releases/tag/v0.3.0
[0.3.0-rc.2]: https://github.com/EmpireTwo/gaze/releases/tag/v0.3.0-rc.2
[0.3.0-rc.1]: https://github.com/EmpireTwo/gaze/releases/tag/v0.3.0-rc.1
