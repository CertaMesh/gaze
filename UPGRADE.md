# Upgrading Gaze

This file is a per-minor migration guide for adopters of the `gaze-pii`
workspace (the published cargo name; the library is imported as `gaze`).
Pair it with [CHANGELOG.md](CHANGELOG.md): CHANGELOG records what changed,
UPGRADE.md tells you what *you* need to do.

## v0.14.x → v0.15.0

### TL;DR

1. **Regenerate your `gaze setup` policy.** Policies written by v0.11.2
   through v0.14.0 preserve detected phone numbers, IBANs, payment cards and
   IP addresses raw. Back up any custom rules, run `gaze setup --force`, then
   re-add them. The new policy tokenizes by default, loads every bundled PII
   rulepack except `secrets`, uses the Davlan NER model, and turns Nym on.
2. **Re-ingest every `gaze index`** and pass it the NER model
   (`--ner-model-dir` or `GAZE_NER_MODEL_DIR`).
3. **Load `secrets`** if you relied on Gaze to tokenize API keys, tokens or
   passwords.
4. **Expect more, and differently shaped, tokens.** Containment precedence,
   per-character residual coverage, the strictest-member family action and the
   `core` floor under custom rulepack paths all protect bytes that used to
   pass through raw. Manifests written by v0.14.x still restore.

### Security fix: `gaze setup` policies tokenize every detected class

**Action required if you ran `gaze setup` on v0.11.2 through v0.14.0.** The
policy it wrote set `default` to `preserve`, so every detected class without
its own rule, including phone numbers, IBANs, payment card numbers and IP
addresses, left the process raw with a success exit.

- **Regenerate.** Back up any custom rules in the policy, run
  `gaze setup --force`, then re-add the custom rules.
- **Or repair by hand.** Set the `kind = "default"` rule's action to
  `"tokenize"`, delete the old per-class rules (the old
  `location = generalize` rule emits a one-way marker), and add the other
  bundled packs and their locales to `[policy.rulepacks]` and `[locale]`.
- **Watch stderr.** `gaze clean`, `gaze daemon` and `gaze proxy` now name any
  detected class a loaded policy still sends through raw, and any reachable
  one-way `generalize` rule. The warning does not change output or exit code.

### `gaze setup` turns on Nym; safety-net flags changed

**Action required if you script `gaze setup` or `--safety-net-backend`.**

- `gaze setup` installs the SHA-pinned Nym-small bundle and writes
  `[safety_net] backend = "nym"` with `[safety_net.nym] model_dir`. Pass
  `gaze setup --safety-net none` for a policy without a net.
  `--safety-net ner` is removed; `none` is its replacement.
- `--safety-net-backend nym` now needs one explicit `--safety-net nym`. The
  old backend-only form succeeded without activating any net.
- `--safety-net` is repeatable. Values given on the command line stack and
  replace the policy's selection for that run; `--safety-net none` disables
  every net for one run and prints a notice.
- A policy without `[safety_net]` still runs no net. `[safety_net.nym]` alone
  configures Nym but does not activate it.

### `gaze-mcp-rmcp`, `gaze-mcp-bridge` and `gaze-document` move to rmcp 2.x

**Action required if you name rmcp types next to these crates.** Upgrade your
own rmcp dependency to 2.x with them; rmcp's `ContentBlock` replaces `Content`
and `RawContent`. MSRV stays 1.89. Bridge ingress now refuses non-text content
variants and any text-block or `annotations` field it does not redact
(`unsupported_content_field`).

### Custom rulepack paths retain core detection

**Action required if your policy sets `[policy.rulepacks].paths` but omits
`bundled`, or you pass `gaze clean --rulepack-path` without
`--rulepack-bundled`.** These configurations now load `core` alongside the
custom pack. This closes a silent leak of core classes and can produce more
tokens than before. Policies without a rulepack table still load `core`.

To keep an intentional custom-only setup, set `bundled = []` in
`[policy.rulepacks]` or pass `--rulepack-bundled=none` with your CLI custom
path. Gaze emits one stderr notice after a successful build whenever the
resolved bundled selection omits `core` and its `core-extended` alias, including
selection of another bundled pack without a custom path.
The omitted-key behavior dates to the v0.4.0 rulepack policy loader.

### One entity, one token; protection beats preservation

**Action required if you count manifest entries per entity, pin token
shapes for nested identifiers, or rely on `preserve` shielding every byte
of a span.** Two resolver changes from solo todo #3740, both breaking in
0.x.

1. **A span that wholly contains a differently-classed span now wins the
   whole span as one token** (`ConflictTier::ContainmentPrecedence`), unless
   it is less certain than what it would swallow (validator passed > anchored
   or cue-structured match > plain regex or dictionary > learned NER; ties go
   to the container). `IBAN PL56 0942 … 4500 BIC` (a spaced Polish IBAN) is one
   `<iban_n>` where it used to be `<iban_n><phone_n><iban_m><iban_k>
   <postal_code_n>`; a card number whose tail is a phone shape is one card
   token; a cue-less IBAN over a phone shape is one family token. Fewer
   manifest entries, never more; leaked bytes are unchanged or lower on every
   measured corpus. Partial overlaps are unchanged. A learned NER span or a
   plain adopter regex still cannot relabel a validated phone, email or IBAN
   inside it. Audit: the swallowed candidate's loser row carries
   `containment_precedence`.
2. **`preserve` keeps a class's characters unless the same characters are
   also PII of a class you protect.** With `custom:url = preserve` and
   `email = tokenize` the email inside a URL now leaves as one `<Email_n>`
   fragment inside the otherwise raw URL; with `custom:postal_code =
   preserve` a postal code that is also part of a protected IBAN is
   protected. The fragment takes the protected class's own action (`redact`
   writes `[REDACTED:<class>]`, `generalize` the placeholder) and its audit
   row says `decided_by: protection_override`. The candidates a preserved
   span represents are not affected: an explicit
   `custom:family:<name> = preserve` rule still leaves the ambiguous span
   raw. If you need a preserved span left entirely raw although a protected
   class claims part of it, preserve that class too.
3. **Residual fragments merge per claimant.** One losing candidate yields one
   fragment per uncovered run; a fragment is no longer split where an inner
   candidate starts or ends.

Manifests written before this change still restore. `redact` and
`generalize` fragments are one-way, like whole spans under those actions.

### Policy regex collision families protect their family token

**Action required only if your policy declares
`[policy.custom_recognizers.collision]` on a `kind = "regex"` recognizer and
relies on the family token that a precedence tie or a missing anchor cue emits
being left raw.** A regex custom recognizer used to register under the constant
recognizer id `legacy-detector`, so the registry could not find it by the policy
`name` its membership is filed under. Precedence, ties and mandatory anchors
still decided (a candidate always carried the policy `name`), but the family
token they emit (`custom:family:<name>`) derived its action from no member at
all and took your `default` rule: under `default = "preserve"` the span left the
process raw, with zero detections and a success exit, although every member
rule said `tokenize`. Dictionary custom recognizers (`dict/<name>`) were never
affected. The id mismatch dates from v0.7.1, when the metadata was introduced;
until the strictest-member derivation (the entry below) it was invisible
because every family token took the default rule.

1. **Family tokens over regex members now derive the strictest member action**,
   exactly as bundled and dictionary families do
   ([How a family-level token picks its action](docs/reference/policy.md#how-a-family-level-token-picks-its-action)).
   To keep such a token raw on purpose, declare a rule for the family class
   **before** your `default` rule:

   ```toml
   [[rule]]
   kind = "class"
   class = "custom:family:tenant-orders"
   action = "preserve"
   ```

2. **Audit rows.** Loser rows of a regex-member family carry the member's own
   class (they carried the winner's family class), and the family token's
   `ambiguity_record.losing_candidates` lists every member (it was empty).
   `recognizer_id` is unchanged: it was already the policy `name`.
3. **Library API.** `Pipeline::registry()` exposes the built
   `RecognizerRegistry`; `FamilyPolicyTable::anchored_families()` lists the
   families with a `mandatory_anchor` member; `RegexDetector::with_base_score()`
   sets the emitted confidence; `gaze` re-exports `AmbiguityRecord`,
   `AmbiguityReason`, `LosingCandidate` and `DerivedFamilyAction`.
   `PipelineBuilder::detector` is unchanged: a `Detector` registered through it
   still reports the constant id and cannot join a collision family.

Manifests, tokens and restore are unchanged.

### The safety net redacts with a marker instead of deleting

**Action required if your clean output goes anywhere that assumed redaction
removed bytes.** This affects everyone on the shipped default policy, because
the default is `Resolve` + `Redact`: the fallback runs whenever the resolve
pass cannot honour a suspect reversibly.

Previously the redact path replaced a flagged span with the empty string. It
now writes a one-way `[REDACTED:<class>]` marker — `[REDACTED:name]`,
`[REDACTED:custom:phone]` — and records it in the manifest. Which spans get
redacted has not changed. What is written in their place has.

1. **Expect clean output to be longer, not shorter, for redacted spans.** Any
   assertion that clean text is no longer than the raw input, or that a
   redaction shrinks the document, no longer holds. Byte-count diffing between
   raw and clean needs to account for marker text.

2. **Do not pattern-match the marker yourself.** Call
   `gaze::is_redaction_marker` (also `redaction_marker_spans` and
   `redaction_marker_byte_len` for whole-document work). A local copy of the
   shape is a second spelling to keep in step with the emitter, and the
   predicate takes authority from the manifest where the runtime does.

3. **Restore is unchanged, deliberately.** A marker is not a token: restore
   passes it through verbatim and the strict restore scan does not flag it.
   Redacted bytes are still unrecoverable — that is what "one-way" means — but
   the restored document now shows *where* they were.

4. **If you consume the manifest, expect one more entry per redaction.** It
   carries `Action::Redact`, is not owned, and stands for the original bytes it
   covered. Code that inferred "a redaction happened" from the *absence* of a
   manifest entry must now look for the entry instead; that inference was never
   safe, because an absence cannot distinguish a redaction from a net that did
   nothing.

5. **Custom classes render lowercased, with every non-alphanumeric byte
   except `:` mapped to `-`** (`custom:address_2` →
   `[REDACTED:custom:address-2]`). Mapping `_` keeps the marker outside the
   token grammar, which requires a trailing `_<digits>`. Mapping the rest means
   `gaze::is_redaction_marker(gaze::redaction_marker(&class))` holds for every
   `PiiClass`, including one you built as `PiiClass::Custom(..)` yourself in a
   custom `SafetyNet` — so your redactions are recognised as protected output by
   every consumer, including the index. The exact class is unchanged in the
   audit row.

### The Kiji DistilBERT safety net is removed

**Action required if you ran `gaze setup`, use `gaze index`, or selected the
Kiji net.** The Kiji DistilBERT safety net is gone. On the 2,910-document
benchmark it recovered 1,831 leaked gold bytes (scored-label contract v2) for
+169,657 false-positive bytes, a 2.5% action precision. No safety net runs
without a policy that selects one; the policy `gaze setup` writes selects Nym. The full removed surface is in the
[0.15.0 CHANGELOG section](CHANGELOG.md).

1. **Re-run `gaze setup`.** Earlier `gaze setup` runs installed the Kiji
   distilbert-NER bundle as the primary `[ner]` model in the policy they wrote.
   `gaze setup` now installs the pinned Davlan mBERT bundle
   (`onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` at
   `cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8`) into
   `$XDG_DATA_HOME/gaze/models/davlan-mbert-ner-hrl` (else
   `~/.local/share/gaze/models/davlan-mbert-ner-hrl`), the model the benchmark
   scores. Re-run it, with `--force` if you want it to overwrite the old
   policy file, or point `[ner].model_dir` at the new directory yourself.
2. **Replace Kiji flags.** Drop `--safety-net kiji-distilbert`,
   `--safety-net-backend kiji-distilbert`, `--safety-net-add kiji-distilbert`,
   `--kiji-backend`, `--kiji-distilbert-precision`,
   `--kiji-distilbert-command`, `--kiji-distilbert-model-dir`,
   `--kiji-distilbert-locales`, and the `GAZE_KIJI_DISTILBERT_*` variables.
   If you want a second opinion after the deterministic passes, use
   `--safety-net openai-filter` (bring your own `opf` and checkpoint) or
   `--safety-net nym` (install with `gaze setup --safety-net nym`). With
   `--safety-net-registry`, `openai-filter` is the only registry-capable
   backend.
3. **Pass the NER model to `gaze index ingest`.** It now requires
   `--ner-model-dir <dir>` or `GAZE_NER_MODEL_DIR` pointing at the pinned
   Davlan bundle. Without it, or with an unpinned directory, ingest fails
   closed with `IndexNerModelMissing` (exit 2) and writes nothing.
   `gaze setup` prints the `export GAZE_NER_MODEL_DIR=<dir>` line. The net,
   `gaze index --safety-net {openai-filter|nym}`, is optional for ingest and
   required for `gaze index search`, which fails closed with `SafetyNetConfig`
   without one.
4. **Rebuild without removed features.** Remove `safety-net-kiji`,
   `runtime-tract` and `runtime-candle` from Cargo feature lists. There is no
   musl-static build path through `tract` any more. Rust callers of
   `gaze_recognizers::safety_net::kiji_distilbert` or the `gaze-model-setup`
   Kiji installers move to `OpenAiFilterSafetyNet`, `NymSafetyNet`, or
   `gaze_model_setup::install_ner_bundle` for the NER model.

Manifests written before this change still restore. Only which spans get
detected differs.

### Collision-family tokens take the strictest member action

**Action required only if you relied on a family token falling to a `preserve`
default, or run `redact` / `generalize` / `format_preserve` member rules behind
the MCP or proxy chokepoint.** A collision-family token
(`custom:family:<name>`, today `custom:family:payment-card-or-iban`, emitted
when no IBAN cue is in range or a Luhn-valid card run collides with the IBAN)
no longer takes the `default` rule when no reachable rule names the family
class. It takes the strictest action among its member classes' resolved rules
and its own default (`redact` > `tokenize` > `generalize` > `format_preserve`
> `preserve`); an explicit family rule declared before the default still wins
verbatim. Full contract:
[How a family-level token picks its action](docs/reference/policy.md#how-a-family-level-token-picks-its-action).

1. **To keep family tokens raw, say so explicitly.** A member-only policy
   (`custom:iban = tokenize`, `default = preserve`) now tokenizes the family
   token instead of shipping the ambiguous IBAN raw. If that raw output was
   intended, add, **before** your `default` rule:

   ```toml
   [[rule]]
   kind = "class"
   class = "custom:family:payment-card-or-iban"
   action = "preserve"
   ```

   A rule placed after the `default` rule is dead (`default` matches
   unconditionally); `gaze clean` prints a load-time `warning:` naming the
   family class whenever a member or family rule shows intent without a
   reachable family rule, dead post-default rules included.
2. **Behind a protection trace, a derived `redact` fails closed.** The MCP
   and proxy chokepoints accept only `tokenize` and `preserve`. A family token
   that derives `redact`, `generalize` or `format_preserve` from a member rule
   now fails the request with `UnsupportedActionVariant`, the same error an
   explicit rule with that action on a member class already produced there.
   Either tokenize the member, or add an explicit `tokenize` family rule
   before the default.
3. **Residual coverage is unchanged in output shape, wider in reach.** The
   cells that cover a losing candidate's bytes beside an overlapping winner
   are now planned whenever every action in the overlap protects its span,
   not only under `tokenize`; they still emit tokens.
4. **Audit rows.** A family token's `ambiguity_record` gains
   `derived_action = { action, member_class }`; `member_class` names the
   member whose explicit rule set the action, or is `null` when the default
   applied. Rows written before this change are unchanged.

Manifests written before this change still restore. Only which action an
ambiguous span takes differs.

### Security fix: `gaze clean` without `--policy` runs `core`

Through v0.14.0, `gaze clean` with no `--policy` and no rulepack flag ran an
email-only stub, so cards, IBANs, IPs and the other `core` classes passed
through raw. It now runs the bundled `core` rulepack, the same as
`--rulepack-bundled core`.

- **Expect more tokens.** Policy-less output now tokenizes every `core` class.
  Anything downstream that relied on those values arriving raw was relying on
  the leak; restore round-trips them as before.
- **Audit rows name real recognizers.** Emails log `source` and
  `recognizer_id` `email.global` instead of `regex`. Update `gaze audit
  query --source regex` filters.
- **Emails on `test.local` are no longer tokenized** without a policy: `core`
  excludes that fixture domain by design.
- **Older releases:** pass `--rulepack-bundled core`, or a policy, to get the
  protected default.

### Security fix: `gaze index ingest` runs `core`

From v0.11.0 through v0.14.0, `gaze index ingest` detected only emails,
`Label: value` fields and NER names and organizations. Cards, IBANs, IPs,
phones and the other `core` classes stayed raw in the stored snippets, and
`gaze index search` printed them. Ingest now runs the same `core` floor as a
policy-less `gaze clean`.

- **Re-ingest every index.** Run `gaze index ingest` again over the same
  directory and domain; it replaces the domain's documents. Until then search
  keeps printing the raw values stored by the old ingest.
- **Expect more tokens in search snippets** for every `core` class. They are
  protected, not searchable: `gaze index search` still looks up names, emails,
  organizations and field classes only.
- **Emails on `test.local` are no longer tokenized** at ingest: `core`
  excludes that fixture domain by design.
- **Older releases:** there is no workaround flag. Do not hand their search
  output to an agent for documents that contain structured identifiers.

### Security fix: prefix reuse disabled

`enable_prefix_cache()` and `PipelineOptimizationConfig::with_prefix_cache(true)`
remain source-compatible but no longer skip detection or retain raw prefixes.
Every input is fully rescanned under its current field, locale, dictionaries,
recognizers and rules. Both transactional prefix-cache modes use that same path.

Adopters that enabled prefix reuse should budget for full-scan latency on growing
inputs and update audit consumers to expect actual recognizer/rule rows instead
of `prefix_cache` provenance. Token mappings and manifest restoration retain their
normal behavior. See [the safety rationale](docs/explanation/pipeline/tier4-pipeline-gating.md).

### Credential recognizers move to the opt-in `secrets` rulepack

**Action required if you rely on Gaze to tokenize credentials.** Credentials
are not PII, so the `core` rulepack (0.6.0) no longer detects them:

- `security_token.anchored` (`custom:security_token`: AWS access keys, JWTs,
  cue-anchored API keys and tokens) and `password.field` (`custom:password`:
  `password:` / `passwort:` records) moved unchanged into the bundled `secrets`
  rulepack. It is opt-in and never loaded by default.
- `username.field` (`custom:username`) is removed. No bundled recognizer emits
  `custom:username` any more; keep a custom recognizer if you need it.

To keep the previous credential protection, load `secrets` next to `core`:

```toml
[policy.rulepacks]
bundled = ["core", "secrets"]
```

or, for one CLI run, `gaze clean --rulepack-bundled core,secrets`. Library
callers using `CorePipelineConfig` add
`.with_bundled_rulepack("secrets")`.

Manifests written before this change still restore: token spellings and the
restore contract are unchanged, only which spans get detected differs. Policy
rules that name `custom:security_token`, `custom:password` or
`custom:username` still parse; without `secrets` loaded the first two simply
never match.

### Policy `schema_version` needs a patch component

The loader now accepts `0.1.x` only. A bare `schema_version = "0.1"` fails
closed, as do `0.10.0` and `0.2.0`:

```text
{"error":"PolicySchemaUnsupported","exit":2,"found":"0.1","supported":"0.1."}
```

Write `schema_version = "0.1.0"`. Policies written by `gaze setup` already do.
If you match on the error's `supported` field, it now reads `"0.1."`.

---

## How this file is organized

- One H2 section per `MAJOR.MINOR` release in **reverse-chronological** order.
- Each section opens with **TL;DR** (the one or two actions an adopter
  cannot skip), then drills into details.
- "Additive" entries are no-action and noted for awareness only.
- "Action required" entries are the ones a human upgrade reviewer should
  read in full.

## Pre-1.0 promise

Gaze is pre-1.0. Per the [SemVer pre-1.0 contract][semver-pre1] minor bumps
*may* introduce breaking changes; we minimize them. Every breaking surface
in this file is also a breaking entry in CHANGELOG.md, gated by closed
non-exhaustive enums + typed errors so that downstream code only breaks
at compile time, never silently at runtime.

The five north-star axes — **reliability, reversibility, agentic-first,
trust, ergonomics** — bound every upgrade. Reversibility means: if an
upgrade ever changes a manifest's restore round-trip, that is a bug, not
a migration step. Manifests written by an older minor restore on the new
minor unless this file explicitly says otherwise. (No such exception
exists today.)

[semver-pre1]: https://semver.org/spec/v2.0.0.html#spec-item-4

---

## v0.9.x → v0.10.0

Status: **unreleased.**

### TL;DR

1. **Document bundles now split agent and owner outputs.** `gaze document clean`
   requires either `--agent-out` + `--owner-out` or the `--out` shorthand that
   creates `<PATH>/agent` + `<PATH>/owner`.

### gaze document clean — bundle layout split (axis 1)

Previous behavior: `gaze document clean --out <PATH>` wrote `clean.md`,
`manifest.json`, and `report.json` into a single directory. Uploading
that directory to an LLM workspace leaked restorable manifest material —
an axis-1 violation that depended on caller discipline rather than
runtime enforcement.

New behavior: `gaze document clean` requires `--agent-out` + `--owner-out`
or the `--out` shorthand that auto-creates `<PATH>/agent` + `<PATH>/owner`
subdirs. `clean.md` and `report.json` land in the agent path; `manifest.json`
lands in the owner path. The writer rejects equal or nested agent/owner
paths with a typed `DocumentError::BundleLayoutInvalid`.

Migration:

- If you used `--out <PATH>` and you intend `<PATH>` to remain agent-shippable,
  switch to `--agent-out <PATH> --owner-out <SOMEWHERE_ELSE>`.
- If you can accept the agent/ + owner/ subdir split, keep `--out <PATH>` —
  the shorthand now creates both subdirs for you.
- Downstream tooling that read files from `<PATH>` must move manifest reads
  to `<PATH>/owner/manifest.json` (or the explicit owner path).

---

## v0.7.x → v0.8.0

Status: **shipped.** v0.8.0 is published to crates.io; the workspace
now includes ten published crates (the new `gaze-proxy` joins
`gaze-types`, `gaze-recognizers`, `gaze-audit`, `gaze-pii`,
`gaze-assembly`, `gaze-mcp-core`, `gaze-mcp-rmcp`, `gaze-document`,
and `gaze-cli`).

### TL;DR

1. **Bundle unification.** If your CLI invocation or `policy.toml`
   references `core-extended`, switch to `core` and pass an explicit
   `--locale` (or `policy.locale`). `core-extended` is now a deprecation
   alias that warns at runtime. See "Tier 1.5".
2. **Audit-row schema.** If you persist `gaze-audit` SQLite rows, the
   `recognizer_id` and `recognizer_version_id` columns are now populated.
   Forward-compatible: pre-v0.8 rows stay readable, new rows carry
   `_vN`-suffixed lineage. See "Tier 1".
3. **Custom recognizers** in `[[policy.custom_recognizers]]` may now
   declare an optional `safety_tier`. When omitted, the loader defaults
   to `safe_default` — your existing policy files keep working without
   edits.

Everything else in v0.8.0 is additive (new entities, new locales, new
opt-in SafetyNet backend).

### Tier 1 — Versioned recognizer-IDs (additive)

PR [#203](https://github.com/CertaMesh/gaze/pull/203) (`3c95304`).

- `RedactionEntry` now carries both `recognizer_id` (semantic slug used
  for registry/collision lookup, unchanged shape) and
  `recognizer_version_id` (audit-facing, suffixed with `_vN`).
- `gaze-audit`'s SQLite schema gains nullable `recognizer_id` +
  `recognizer_version_id` columns. The schema migrates forward without
  rewriting existing rows; legacy rows carry a `legacy_unversioned`
  marker.
- The NER recognizer's bare `"ner"` slug is now extended with the loaded
  model id (e.g. `ner.distilbert.v1`).

**Action required:** none. If you query the audit table directly, your
existing SQL keeps working. If you want to consume the new columns, they
are nullable so a simple `SELECT recognizer_id, recognizer_version_id
FROM gaze_audit_log` is forward-safe.

### Tier 1.5 — Bundled rulepack unification (action required for some)

PR [#201](https://github.com/CertaMesh/gaze/pull/201) (`8ab9daf`).

The two embedded rulepacks (`core` with 6 recognizers, `core-extended`
with 10) have been collapsed into **one unified `core` bundle**. Each
recognizer now declares a closed-enum `safety_tier` that machine-encodes
its activation contract:

| Tier            | Activation rule                                                 |
| --------------- | --------------------------------------------------------------- |
| `safe_default`  | Active whenever the bundle is loaded.                           |
| `locale_gated`  | Active only when the resolved locale matches `recognizer.locales`. |
| `opt_in`        | Active only when explicitly named under `[[policy.custom_recognizers]]` or future opt-in surface. |

The pre-v0.8 PR #58 no-policy surprise activation (where
`--rulepack-bundled core-extended` silently turned on
`phone.national.{de,us}` + `postal.{de,us}`) is gone. Those recognizers
are now `locale_gated` and require an explicit `--locale=de-DE` or
`--locale=en-US`.

**Action required**

- **If your CLI scripts pass `--rulepack-bundled core-extended`**, they
  keep working in v0.8.x: the flag aliases to `--rulepack-bundled core`
  and emits a deprecation warning. The alias will be removed in a future
  major (target v0.10.0). Update at your convenience.
- **If your scripts rely on bare 5-digit postal or German/US national
  phone tokenization without passing a locale**, you will see those
  spans pass through untokenized. Add the matching locale flag (or
  `policy.locale` field) to restore behavior. The deprecation warning
  on `core-extended` calls this out at runtime.
- **If your `[[policy.custom_recognizers]]` blocks need explicit tier
  declarations**, set `safety_tier = "safe_default"` (or the tier you
  want) on each entry. When omitted, the loader defaults to
  `safe_default` so existing policies load unchanged.

**No action required**

- Manifest contracts are unchanged. Tokens emitted by v0.7.x deserialize
  + restore on v0.8.x.
- Adopters who already passed `--locale` were unaffected by PR #58
  surprise activation and are unaffected by this change.

### Tier 2 — Checksum-backed locale parity (additive)

In flight at tag time as `v0.8/tier2-validator-locales`. When merged, the
release notes for v0.8.0 will replace this paragraph with the merged PR
number(s) and the entity table below.

| Entity     | Locale | Validator        | `ValidatorKind`         |
| ---------- | ------ | ---------------- | ----------------------- |
| Aadhaar    | IN     | Verhoeff         | `AadhaarVerhoeff`       |
| NIR        | FR     | MOD-97 variant   | `FrNirMod97`            |
| Steuer-ID  | DE     | MOD 11,10        | `DeSteuerIdMod1110`     |
| BSN        | NL     | MOD-11           | `BsnMod11`              |
| CPF        | BR     | MOD-11           | `CpfMod11`              |
| CNPJ       | BR     | MOD-11           | `CnpjMod11`             |
| NHS number | UK     | MOD-11           | `UkNhsMod11`            |

All seven ship with `safety_tier = "safe_default"` (activated whenever
the `core` bundle is loaded). New locale packs ship at `locale-fr`,
`locale-nl`, `locale-br`, `locale-in`, `locale-uk`.

**Action required:** none — every entity is additive, gated by locale
unless your policy enables it globally. Adopters in BR / FR / NL / IN /
UK get out-of-box coverage; everyone else sees no behavior change.

### Tier 2.5 — Kiji DistilBERT SafetyNet backend (opt-in)

PR [#202](https://github.com/CertaMesh/gaze/pull/202) (`0cd9ccc`).

A second Pass-3 SafetyNet observer is available alongside the existing
OpenAI Privacy Filter. Subprocess contract is identical to
`OpenAiFilterSafetyNet` — read clean text on stdin, emit JSON spans on
stdout, never mutate the manifest. New CLI flags:

- `--safety-net-backend {openai-filter|kiji-distilbert}`
- `--kiji-distilbert-command <path>`
- `--kiji-distilbert-model-dir <dir>`

Fetcher: `scripts/fetch/fetch-kiji-safetynet-model.sh`. Pinned-artifact
contract: model dir must carry `SHA256SUMS`, `labels.json`,
`model.onnx`, `tokenizer.json` with `0o700` directory + `0o600` file
permissions on Unix. Missing artifacts fail closed with typed
`CliError::SafetyNetArtifactMissing` (exit `2`) before the subprocess
spawns.

Setup walkthrough: removed together with the backend; see the [removal section](#the-kiji-distilbert-safety-net-is-removed).

**Action required:** none. The backend is opt-in. If you do not select
it, your current SafetyNet configuration (OpenAI Privacy Filter or
none) is unchanged.

### Tier 3 — Regex-only locale recognizers (additive)

PR [#208](https://github.com/CertaMesh/gaze/pull/208).

Adds US SSN, UK NINO, and Indian PAN as `safety_tier = "locale_gated"`
recognizers — they fire only when the resolved locale matches. No
validator math; regex shape plus cue context only.

| Entity     | Locale | Cue examples                              | ValidatorKind |
| ---------- | ------ | ----------------------------------------- | ------------- |
| US SSN     | US     | `SSN`, `Social Security Number`, `SS#`    | None          |
| UK NINO    | UK     | `NINO`, `NI Number`, `National Insurance` | None          |
| Indian PAN | IN     | `PAN`, `Permanent Account Number`, `पैन`  | None          |

**Action required:** none — pure additive coverage when the relevant
locale is set.

### Depending on v0.8.0

The workspace is published to crates.io. Pin by version:

```toml
[dependencies]
gaze-pii = "0.8.0"
```

The exact crate name is `gaze-pii` (cargo package); the library imports
as `gaze` (e.g. `use gaze::Pipeline;`).

### Schema-version field on `policy.toml`

Shipped in v0.7.2 (PR #192) but worth re-stating because v0.8.0 is the
first minor where the field is *exercised by new content*:

```toml
schema_version = "0.1.0"
```

The loader checks the `major.minor.` prefix against the supported version
and fails closed with
`{"error":"PolicySchemaUnsupported","exit":2,"found":"...","supported":"0.1."}`.
Since v0.15.0 a bare `"0.1"` no longer loads (see the v0.15.0 section above).
Existing policies without the field continue to load via a soft default;
add it explicitly to lock yourself onto a known schema.

---

## v0.6.x → v0.7.0

Highlights only — backfill in detail if adopter friction surfaces.

- **New crate `gaze-document`** for OSS document → SafeBundle ingestion
  (PNG/JPG/PDF → Tesseract OCR → redact → `clean.md` + `manifest.json`
  + `report.json`). Opt-in via `gaze-cli`'s `document` feature.
- **MCP runtime split.** `gaze-mcp-core` (transport-free) +
  `gaze-mcp-rmcp` (rmcp transport sink) replace the prior in-tree MCP
  surface. Opt-in via `gaze-cli`'s `mcp` feature.
- **Validator-veto pre-resolver** rejects invalid candidates before
  conflict resolution, logs loser-only audit rows with
  `decided_by: ValidatorVeto`. See
  [`docs/explanation/detection/validator-veto.md`](docs/explanation/detection/validator-veto.md).
- **Collision-family metadata + `FamilyPolicyTable`** for cross-class
  recognizer rivalries (PAN-vs-IBAN, phone family). See
  [`docs/explanation/detection/collision-family.md`](docs/explanation/detection/collision-family.md).
- **Mandatory-anchor resolution** keeps structural candidates on their
  precise variant when a `[locale.cues.<key>]` cue is in scope, else
  emits a family-level fallback token. See
  [`docs/explanation/detection/anchor-resolution.md`](docs/explanation/detection/anchor-resolution.md).
- **`PiiClass::Custom("eth_address")`** for EIP-55 Ethereum addresses;
  new `Ipv4Parse`/`Ipv6Parse`/`EthEip55` validator kinds.
- **`gaze_pii::default_policy` falls back to `Tokenize`** (axis-1
  fail-closed). Adopters who relied on the previous default-allow path
  must declare per-class policy explicitly.

**Action required**

- The `Tokenize` default change may surface previously-allowed classes
  as tokens. Review your `[policy.classes]` block and set explicit
  policies for any class you want to allow through.
- The MCP runtime split changes the import path: replace any
  `gaze::mcp::*` imports with `gaze_mcp_core::*` or `gaze_mcp_rmcp::*`.

---

## v0.5.x → v0.6.0

- `KijiDistilbertSafetyNet`'s predecessor — the OpenAI Privacy Filter
  Pass-3 SafetyNet — landed as an observer-only backend. Manifests are
  not mutated by Pass-3; restore round-trip is unaffected.
- Cue-anchored Name detection (`anchored_match` recognizer kind +
  `forward_markers` / `agent_recipient_cues` / `footer_cues` locale
  buckets). Adopters using `locale-de` or `locale-en` get this for
  free.
- `gaze` no longer carries `rusqlite` in any feature graph. Adopters
  who want SQLite audit logging now depend on `gaze-audit` directly:

  ```rust
  use gaze_audit::SqliteLogger;
  ```

  The one-minor `audit` feature shim on `gaze` (introduced in v0.5
  Phase C) is gone. `gaze::SqliteLogger` no longer compiles.

---

## v0.4.x → v0.5.0

- New crate `gaze-types` for shared value contracts (serde-only, no
  ML/sql deps). Adopters who want the contract surface without
  `ort` / `tokenizers` / `ndarray` should depend on `gaze-types`
  directly.
- The `RedactionLogger` trait moved into `gaze-types`. `gaze`
  re-exports it for source compatibility.
- Audit-sink protected-path enforcement switched from the legacy
  syn-walker to a Dylint resolver-based gate
  (`xtask dylint-gate`).

---

## Reversibility statement (every upgrade)

If an upgrade ever causes a manifest written by an older minor to fail
restore on a newer minor, that is a bug. Open an issue tagged
`reversibility-regression` and we will treat it as a critical defect
against north-star axis 2. There is no migration step that asks you to
re-tokenize stored manifests.

# v0.9.0

## Perf wave

v0.9.0 is a performance and deployment release: in-process Kiji ORT
removes the Python subprocess boundary for adopters who select it, int8 dynamic
quantization adds a separately SHA-pinned smaller/faster model path, `gaze
daemon` keeps multi-session state behind a JSONL stdio process boundary,
pipeline skip-gating/capitals/prefix-cache/length-bucketing optimizations are
available behind explicit opt-in flags, and `tract`/`candle` feature gates give
static-binary deployments alternatives to the default `ort` runtime. Public
benchmark claims are documented in [`docs/reference/benchmarks/README.md`](docs/reference/benchmarks/README.md):
Kiji int8 ORT warm p50 is 1.849ms in the committed model leaderboard snapshot,
and the safety-net matrix records a 0.000 F1 delta versus fp32 Kiji.

Measured on: Apple M5 Max / macOS 26.5 hosts in the committed v0.9 snapshots
and final rc revalidation.

## New CLI flags (opt-in)

- `--kiji-backend {subprocess|ort}` (default `subprocess`): selects Kiji DistilBERT runtime.
- `--kiji-distilbert-precision {fp32|int8}` (default `fp32`): selects precision for ORT path.
- Pipeline-optimization flags wired through CLI: skip-class-gating, capitals-heuristic-gate, prefix-cache, length-bucketing (opt-in default-off).

## New subcommand

- `gaze daemon --policy <path> [--idle-timeout <secs>]` — long-lived JSONL stdio session manager. Protocol: `{session_id, text}` request, `{session_id, clean_text, manifest, tokens}` response. SIGTERM-graceful, multi-session-isolated.

## New opt-in features (Cargo)

- `gaze-recognizers` features: `runtime-tract`, `runtime-candle` — alternative ONNX runtimes for static-binary deployments.

## Reversibility

Manifest restore semantics + signed snapshot wire format unchanged from v0.8.1.

# v0.8.1

v0.8.1 made SafetyNet `resolve` the default mode, added Kiji DistilBERT bundle
SHA verification, and introduced the `LocaleAwareModel` registry groundwork in
`gaze-recognizers`. The public default `--safety-net-mode` flipped from
`strict` to `resolve`; adopters who require strict hard-fail semantics must opt
back in explicitly with `--safety-net-mode=strict`.
# v0.8.0

## gaze-proxy

The new off-by-default `proxy` feature adds `gaze-proxy` and `gaze proxy`
subcommands for multi-provider LLM SDK base-URL swaps. OpenAI, Anthropic, and
Gemini ship as separate provider adapters from day one. The proxy uses native
provider wire shapes and does not transcode between providers.

Daemon UX is available through:

```bash
gaze proxy serve
gaze proxy start
gaze proxy status
gaze proxy logs --follow
gaze proxy stop
gaze proxy restart
```

Pidfiles are stored in platform local-data directories and stale pidfiles are
cleaned after process liveness checks.
