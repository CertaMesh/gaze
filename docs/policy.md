# Policy — authoring `policy.toml`

A `policy.toml` is the configuration file `gaze clean --policy=<path>` loads to
build its detection-and-redaction pipeline. It declares which detectors run,
which PII classes they emit, and what action the pipeline takes when each class
is found.

This document describes the schema as shipped in v0.4.0-rc.1. The canonical
parser lives at [`crates/gaze/src/policy.rs`](../crates/gaze/src/policy.rs);
the CLI wiring (argument parsing, context envelope assembly, policy-error
mapping) is in [`crates/gaze-cli/src/main.rs`](../crates/gaze-cli/src/main.rs).
Recognizer backends (regex, dictionary, NER) live in
[`crates/gaze-recognizers`](../crates/gaze-recognizers). For the full CLI
contract — exit codes, stderr discipline, blob format — see
[`docs/roadmap/v0.3/cli.md`](roadmap/v0.3/cli.md).

## What `policy.toml` is for

`gaze clean` accepts `--policy=<path>`. The path is opened, parsed as TOML,
and turned into a [`Pipeline`](../crates/gaze/src/pipeline.rs) via
`Pipeline::from_policy`. Two failure modes:

- **File cannot be opened** (missing path, permission denied) → exit `4`,
  stderr `{"error":"PolicyOpen","exit":4}`.
- **File parses but is invalid** (unknown key, bad regex, unknown class,
  unknown action, missing required field, no recognizers/rulepacks, no rules) → exit `2`,
  stderr `{"error":"PolicyConfig","exit":2}`.

If `--policy` is omitted, `gaze clean` falls back to a hard-coded stub pipeline
(email regex + tokenize). The stub exists only so the CLI surface can be
exercised before a policy is written; **production use requires `--policy`**.

## CLI overrides for runtime knobs

`policy.toml` is the durable source of truth. `gaze clean` also exposes
runtime-only overrides for knobs operators commonly vary between invocations.
Resolution is always:

```text
CLI flag > policy.toml > Gaze default
```

| Policy field | CLI flag | Notes |
|--------------|----------|-------|
| `[session].scope` | `--session-scope <ephemeral|conversation|persistent>` | Overrides session lifetime for the current clean run. `ephemeral` keeps export-forbidden semantics, so pipe-mode clean exits `Pipeline` if a session blob would be required. |
| `[session].ttl_secs` | `--session-ttl <SECONDS>` | Existing override for persistent session TTL. |
| `[ner].model_dir` | `--ner-model-dir <PATH>` | Overrides the NER model directory. If neither CLI nor TOML sets a model directory, no NER detector is registered. |
| `[ner].locale` | `--ner-locale <BCP47>` | Overrides the NER locale hint. Invalid tags fail closed with `PolicyConfig`. |
| `[ner].threshold` | `--ner-threshold <FLOAT>` | Existing override for NER confidence threshold; must be `0.0..=1.0`. |
| `[locale].active` | `--locale <BCP47,...>` | Existing override for the active locale fallback chain. |
| `[policy.rulepacks].bundled` | `--rulepack-bundled <ID,...>` | Comma-separated and repeatable. Replaces TOML bundled rulepack IDs for the current run. |
| `[policy.rulepacks].paths` | `--rulepack-path <PATH>` | Repeatable. Replaces TOML rulepack paths for the current run. |

Example:

```sh
gaze clean \
  --policy=policy.toml \
  --session-scope=conversation \
  --ner-model-dir="$HOME/.local/share/gaze/models/davlan-mbert-ner-hrl" \
  --ner-locale=de \
  --rulepack-bundled=core,locale-de \
  --rulepack-path=./workspace-rulepack.toml
```

If `policy.toml` sets `[session].scope = "persistent"` and the command passes
`--session-scope=conversation`, the exported `session_blob` records a
conversation-scoped session. If neither source mentions `[ner].model_dir`, Gaze
keeps NER disabled rather than registering a placeholder detector.

Policy-document fields have no CLI override by design. Examples include
recognizer definitions (`[[policy.custom_recognizers]]`), rule definitions
(`[[rule]]`), rulepack internals, and policy-document metadata. Those fields
define the auditable contract; changing them requires changing the policy or
rulepack document itself.

## Configuration surfaces - three-surfaces parity table

This table audits every current `policy.toml` field accepted by
[`Policy::load`](../crates/gaze/src/policy.rs). Runtime knobs are scalar,
enum, or path values that can reasonably vary for one `gaze clean` execution;
they must have a CLI flag, TOML field, and documented default or required
state. Policy-document fields define recognizers, rules, dictionaries, or
rulepacks and intentionally stay in TOML only, per the three-surfaces boundary.

| Policy field | Type | CLI flag | TOML | Default | Class | Rationale |
|---|---|---|---|---|---|---|
| `Policy.session.scope` | enum | `--session-scope` | `[session].scope` | Required in TOML; policy-less CLI uses `persistent` | runtime knob | CLI/TOML/default parity required for per-run session behavior. |
| `Policy.session.ttl_secs` | `u64` | `--session-ttl` | `[session].ttl_secs` | Required for `persistent`; policy-less CLI uses `86400` | runtime knob | CLI/TOML/default parity required for per-run session lifetime. |
| `Policy.ner.model_dir` | path | `--ner-model-dir` | `[ner].model_dir` | Absent; NER disabled unless configured | runtime knob | CLI/TOML/default parity required for per-run NER backend selection. |
| `Policy.ner.locale` | BCP47 string | `--ner-locale` | `[ner].locale` | Absent; NER backend default | runtime knob | CLI/TOML/default parity required for per-run NER locale selection. |
| `Policy.ner.threshold` | `f32` | `--ner-threshold` | `[ner].threshold` | `0.3` | runtime knob | CLI/TOML/default parity required for per-run NER sensitivity. |
| `Policy.locale` | BCP47 list | `--locale` | `[locale].active` | Rulepack defaults, then system default chain | runtime knob | CLI/TOML/default parity required for per-run locale gating. |
| `Policy.rulepacks.bundled` | string list | `--rulepack-bundled` | `[policy.rulepacks].bundled` | `["core"]` when `[policy.rulepacks]` is omitted | runtime knob | CLI/TOML/default parity required for per-run bundled rulepack selection. |
| `Policy.rulepacks.paths` | path list | `--rulepack-path` | `[policy.rulepacks].paths` | Empty | runtime knob | CLI/TOML/default parity required for per-run external rulepack selection. |
| `Policy.detectors` | recognizer list | none | `[[policy.custom_recognizers]]` | Empty when custom recognizers are omitted | policy document | Recognizer definitions are TOML-only structural policy; drawer `e8b5c041` boundary; bulk authoring is better in TOML. |
| `Policy.detectors[].kind` | enum | none | `[[policy.custom_recognizers]].kind` | Required | policy document | Recognizer type is part of TOML-only recognizer definition; drawer `e8b5c041` boundary, not a per-run CLI knob. |
| `Policy.detectors[].name` | string | none | `[[policy.custom_recognizers]].name` | Required | policy document | Recognizer identity is audit-relevant structural policy; drawer `e8b5c041` boundary keeps it in TOML. |
| `Policy.detectors[].pattern` | regex string | none | `[[policy.custom_recognizers]].pattern` | Required for regex recognizers | policy document | Regex authoring needs reviewable TOML structure; drawer `e8b5c041` boundary, not shell-flag input. |
| `Policy.detectors[].class` | class string | none | `[[policy.custom_recognizers]].class` | Required | policy document | Class mapping is recognizer policy data; drawer `e8b5c041` boundary keeps auditable mappings in TOML. |
| `Policy.detectors[].dictionary_name` | string | none | `[[policy.custom_recognizers]].dictionary` or `.terms_from_context` | Recognizer name | policy document | Dictionary binding is adopter-defined recognizer policy; drawers `e8b5c041` and `eac549ae`, TOML-only. |
| `Policy.detectors[].case_sensitive` | bool | none | `[[policy.custom_recognizers]].case_sensitive` | `false` | policy document | Per-recognizer dictionary behavior belongs with the recognizer definition; drawer `e8b5c041`, not a runtime knob. |
| `Policy.detectors[].token_family` | string | none | `[[policy.custom_recognizers]].token_family` | `"counter"` | policy document | Token-family choice is part of restorable recognizer policy; drawer `e8b5c041`, TOML-only for auditability. |
| `Policy.dictionaries` | dictionary list | none | `[[policy.custom_recognizers]].terms`, `.terms_file`, `.terms_from_context` | Empty unless dictionary recognizers define terms | policy document | Term-list authoring is adopter-defined policy data; drawer `eac549ae`; bulk authoring belongs in TOML or files. |
| `Policy.dictionaries[].terms` | string list | none | `[[policy.custom_recognizers]].terms` | Required for inline dictionary recognizers without `terms_file` or `terms_from_context` | policy document | Inline terms are adopter-defined dictionary data; drawer `eac549ae`; TOML is safer than CLI list entry. |
| `Policy.dictionaries[].terms_file` | path | none | `[[policy.custom_recognizers]].terms_file` | Absent | policy document | Dictionary file references are policy data; drawer `eac549ae`; TOML keeps reviewable data-source provenance. |
| `Policy.dictionaries[].terms_from_context` | string | none | `[[policy.custom_recognizers]].terms_from_context` | Absent | policy document | Context dictionary binding is adopter-defined policy; drawer `eac549ae`, not a global runtime flag. |
| `Policy.rules` | rule list | none | `[[rule]]` | At least one rule required | policy document | Class and column action mapping is TOML-only structural policy; bulk authoring is better in TOML. |
| `Policy.rules[].kind` | enum | none | `[[rule]].kind` | Required | policy document | Rule kind selects structural policy shape (`class` or `column`); TOML-only to preserve auditability. |
| `Policy.rules[].action` | enum | none | `[[rule]].action` | Required | policy document | Rule action is policy contract data, not a per-run override; TOML keeps restore behavior auditable. |
| `Policy.rules[].class` | class string | none | `[[rule]].class` | Required for `kind = "class"` | policy document | Class rule mapping (`[[rule]] class = "...", action = "..."`) is TOML-only structural data. |
| `Policy.rules[].column` | string | none | `[[rule]].column` | Required for `kind = "column"`; rejected by CLI mode | policy document | Column rules require file-shaped policy context and are rejected by CLI mode, so no CLI flag is exposed. |
| `Policy.detectors` legacy surface | recognizer list | none | `[[detector]]` | Unsupported in v0.4; migrate to `[[policy.custom_recognizers]]` | explicitly deferred: retired compatibility surface | Retired compatibility surface remains documented only to explain migration; no CLI flag should revive it. |

Runtime-knob verification is covered by the CLI integration suite:
`s1_three_surfaces_flags_are_exposed_and_bundled_ids_unchanged` checks the
complete flag set, while focused tests cover symmetric failure and observed
behavior for session scope, session TTL, NER threshold/model/locale, active
locale, bundled rulepacks, and rulepack paths. The audit found no runtime
policy field missing a CLI flag.

## Classes

A `class` identifies what kind of PII a recognizer detects. Every recognizer
(regex, dictionary, NER) emits one class per match. Rules then act on classes.

### Built-in classes

Gaze ships four built-in classes:

| Class          | Description                              | Example token                    |
|----------------|------------------------------------------|----------------------------------|
| `Email`        | Email addresses                          | `<{session_hex}:Email_1>`        |
| `Name`         | Personal names                           | `<{session_hex}:Name_1>`         |
| `Location`     | Geographic locations (cities, addresses) | `<{session_hex}:Location_1>`     |
| `Organization` | Company / org names                      | `<{session_hex}:Organization_1>` |

Policy files spell built-ins case-insensitively as `"email"`, `"name"`,
`"location"`, and `"organization"`. Their token grammar is
`<{session_hex}:{Class}_{n}>` for the default `tokenize` action.

### Adding your own classes (no code changes required)

Adopters can define new classes purely via `policy.toml` — Gaze does not need
to be rebuilt or modified. Use `custom:<name>` to declare a project-specific
class.

#### Pattern 1 — domain-specific regex class

```toml
[[policy.custom_recognizers]]
kind = "regex"
name = "phone_us"
pattern = '\b\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b'
class = "custom:phone"

[[rule]]
kind = "class"
class = "custom:phone"
action = "tokenize"
```

Output tokens carry the `Custom:` namespace prefix to disambiguate from
built-ins:

```text
Input:  "Call 555-010-0100 to confirm."
Output: "Call <{session_hex}:Custom:phone_1> to confirm."
```

#### Pattern 2 — tenant-specific dictionary class

```toml
[[policy.custom_recognizers]]
kind = "dictionary"
name = "tenant_orders"
terms_from_context = "orders"
class = "custom:order_id"

[[rule]]
kind = "class"
class = "custom:order_id"
action = "tokenize"
```

Then pass tenant data via `--context-json`:

```json
{
  "dictionaries": {
    "orders": { "terms": ["ORD-12345", "ORD-99999"], "case_sensitive": true }
  }
}
```

```text
Input:  "Reference ORD-12345 is shipped."
Output: "Reference <{session_hex}:Custom:order_id_1> is shipped."
```

### Class naming rules

- Built-in class names (`Email`, `Name`, `Location`, `Organization`) live in the
  top-level token grammar (`<Email_1>`). Custom classes always render with a
  `Custom:` prefix (`<Custom:my_class_1>`), so a custom class named `"email"` is
  unambiguous from the built-in class because it has a different token shape.
- Custom classes use the `custom:<name>` policy spelling.
- Custom class names are normalized: characters outside `[a-z0-9_]` collapse to
  `_` one run at a time. Adopters should pass non-empty alphanumeric names;
  passing all-punctuation strings like `"!!!"` currently normalizes to an empty
  stem and emits `<Custom:_1>`. Validate adopter input before passing it to
  `PiiClass::custom` if this matters for your integration.
- Two recognizers may share a class, provided they follow the rulepack composition contract (`cooperates_with` in rulepacks as of v0.4.1+).

### Choosing class names

For tenant-specific classes (orders, songs, users, customer IDs), pick a stable
lowercase identifier. The class name appears in redaction-log entries as
`custom:<name>`, so audit-friendly names help debugging.

For universal categories (phone numbers, IBAN, IP addresses), check whether a
current or planned core rulepack already covers the class before defining your
own.

## Minimal working example

`minimal.toml`:

```toml
[session]
scope = "persistent"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = '(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b'
class = "email"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
```

Run it:

```console
$ echo "Email alice@example.invalid now" | gaze clean --policy=minimal.toml
{"clean_text":"Email <{session_hex}:Email_1> now","session_blob":"<base64>","stats":{"detections":1}}
```

Add `--audit-db=redaction.sqlite` to persist the metadata-only SQLite
redaction log for the invocation. Dictionary rows use
`dictionary:{name}[#term_index]` source labels so an operator can trace which
configured term fired without storing raw PII.

This is the same fixture the CLI integration suite uses
(`crates/gaze/tests/cli_pipe.rs::t16_clean_with_policy_tokenizes_email`).

## Schema reference

TOML tables use closed schemas unless explicitly documented otherwise — any key
the parser does not recognise is a hard error. v0.4 adds
`[policy.rulepacks]` and `[[policy.custom_recognizers]]`; legacy top-level
`[[detector]]` is rejected.

### `[session]`

```toml
[session]
scope = "persistent"   # required
ttl_secs = 86400       # required when scope = "persistent"; optional otherwise
```

| Field      | Type     | Required                     | Notes                                           |
|------------|----------|------------------------------|-------------------------------------------------|
| `scope`    | string   | yes                          | One of `"ephemeral"`, `"conversation"`, `"persistent"`. |
| `ttl_secs` | integer  | yes if `scope = "persistent"`| Must be `> 0`. Zero is rejected.                |

> Resolved in v0.3.1: `gaze clean` now constructs its session from
> `[session]`. `--session-ttl` is an explicit CLI override for persistent
> session TTL; when the flag is omitted, `ttl_secs` from policy is used.

### `[policy.rulepacks]`

Rulepacks declare reusable recognizers outside the host policy file. The CLI
loads bundled rulepacks by name and custom rulepack TOML files by path.

```toml
[policy.rulepacks]
bundled = ["core"]
paths = ["./tenant-rulepack.toml"]
```

Bundled rulepacks:

| Bundle | Recognizers | Classes | Notes |
|--------|-------------|---------|-------|
| `core` | `email.global`, `email.header.name` | `email`, `name` | Default bundle when `[policy.rulepacks]` is omitted. |
| `core-extended` | `phone.structural`, `iban.structural`, `card.structural`, `ip.v4`, `ip.v6`, `postal.de`, `postal.us` | `custom:phone`, `custom:iban`, `custom:credit_card`, `custom:ip_address`, `custom:postal_code` | Opt-in bundle. Validator-backed E.164 phone, IBAN, and credit-card recognizers plus structural IP/postal recognizers. |

Opt into `core-extended` alongside `core`:

```toml
[policy.rulepacks]
bundled = ["core", "core-extended"]
```

Or override the bundle list for one CLI run:

```bash
gaze clean --rulepack-bundled core,core-extended --policy ./policy.toml
```

`core-extended` recognizers are intentionally conservative:

- `phone.structural` matches E.164-only `+\d{6,15}` numbers and emits
  `custom:phone` only when the match passes `e164_phone`. National phone
  patterns are not included. Regex-passing but unassigned values such as
  `+99999999` do not emit detections.
- `iban.structural` emits `custom:iban` only for IBAN-shaped candidates that
  pass `iban_mod97`; the canonical form is normalized with `iban_canonical`.
- `card.structural` emits `custom:credit_card` only for 13- to 19-digit
  candidates that pass `luhn`.
- `ip.v4` and `ip.v6` emit `custom:ip_address`.
- `postal.de` emits `custom:postal_code` only under active locale `de-DE`.
- `postal.us` emits `custom:postal_code` only under active locale `en-US`.
  Plain `en` does not activate `postal.us`.

Phone, IBAN, and credit-card recognizers are universal (`global`) because their
validation rules are format-level checks, not locale gates in Gaze policy. They
are also solo recognizers in their classes, so they do not need
`cooperates_with` rows. Tenant numeric IDs such as `Subscriber_0001234567` and
`Order_0815` are explicit negative fixtures for those recognizers; broad
numeric shapes must not become phone or credit-card detections without a
passing validator.

Within a rulepack, every `[[recognizers]]` block has an `id`, `class`, and
`[recognizers.match]` table. If two recognizers in the same rulepack emit the
same `class`, at least one must explicitly list the other recognizer id in
`cooperates_with`.

```toml
[[recognizers]]
id = "email.header.name"
class = "Name"
cooperates_with = ["salutation.name"]

[recognizers.match]
kind = "regex"
pattern = '''(?m)^From:\s+([A-Z][a-z]+)\s+<[^>]+>'''
capture_groups = [1]
```

Missing cooperation fails rulepack load with
`RulepackError::SameClassWithoutCooperation`. The check is strict by design:
there is no line-anchor heuristic or implicit overlap analysis.

#### Built-in validators

Regex rulepack recognizers may include an optional `[recognizers.validator]`
table. Validators are deterministic, closed-registry names. Unknown validator
strings fail policy load with `RulepackError::UnsupportedValidator`.

```toml
[recognizers.validator]
kind = "luhn"
```

| Kind | Applies to | Behavior |
|------|------------|----------|
| `email_rfc` | Email-like regex candidates | Basic email shape validation used by the bundled core email recognizer. |
| `e164_phone` | E.164-like phone candidates | Parser-backed phone validation. `core-extended` uses it with `phone.structural` so assigned international numbers such as `+4915550112233` emit `custom:phone`, while unassigned regex-only values such as `+99999999` are dropped. |
| `luhn` | Credit-card-like numeric candidates | Mod 10 checksum. ASCII whitespace is ignored; any other non-digit fails validation. |
| `iban_mod97` | IBAN-like alphanumeric candidates | ISO 7064 mod-97 check. Input is canonicalized as uppercase with ASCII whitespace removed before validation. |

Validator-backed regex candidates fail closed: a regex match whose validator
returns false emits no detection. This is intentionally stricter than emitting
an unvalidated candidate because shape-only false positives are a PII-leak risk
for agent workflows.

Validator names live in rulepack TOML and are compile-time/library behavior,
not per-invocation CLI policy. `e164_phone` is backed by the
`gaze-recognizers` `phone-parser` feature, which gates the optional
`phonenumber` dependency and is enabled in default builds. There is no CLI flag
or policy runtime knob for swapping validator semantics per invocation.

#### Built-in normalizers

Regex rulepack recognizers may include an optional `[recognizers.normalizer]`
table. Normalizers affect canonical form used for validated candidate identity;
restore still uses the original matched bytes from the session manifest.
Unknown normalizer strings fail policy load with
`RulepackError::UnsupportedNormalizer`.

```toml
[recognizers.normalizer]
kind = "iban_canonical"
```

| Kind | Behavior |
|------|----------|
| `email_canonical` | Lowercase ASCII email candidates. |
| `iban_canonical` | Remove ASCII whitespace and uppercase letters. |

Rulepack locale metadata can define adopter-specific vocabulary buckets under
`[locale.<bucket>]`. Bucket tables are intentionally open by name; each bucket
contains `names = [...]`. Regex `pattern_template` values may reference those
buckets with `{locale.<bucket>}`. Assembly lowers the placeholder after the
active locale chain is known.

```toml
[locale.salutations]
names = ["Dr", "Mx"]

[[recognizers]]
id = "salutation.name"
class = "Name"

[recognizers.match]
kind = "regex"
pattern_template = '''(?m)^(?:{locale.salutations}):\s+([A-Z][a-z]+)$'''
capture_groups = [1]
```

If a template references an unknown locale bucket, assembly fails closed with
`PolicyError::UnknownLocaleBucket`. The legacy `{locale_email_headers}`
placeholder remains a v0.4.2 compatibility alias for
`{locale.email_headers}`; prefer the generic syntax in new rulepacks. The alias
is deprecated for removal in the v0.5 cycle.

### `[[policy.custom_recognizers]]`

Optional custom recognizer blocks. If omitted, `[policy.rulepacks]` defaults to
the bundled `core` rulepack. To disable bundled rulepacks, set
`[policy.rulepacks] bundled = []`; a policy with neither bundled/path rulepacks
nor custom recognizers is rejected with `PolicyConfig`.

```toml
[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = '(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b'
class = "email"
```

| Field                | Type      | Required | Notes                                                     |
|----------------------|-----------|----------|-----------------------------------------------------------|
| `kind`               | string    | yes      | `"regex"` or `"dictionary"`. Other values parse but fail at pipeline build with `PolicyConfig`. |
| `name`               | string    | yes      | Used as the recognizer id/source label for debugging and conflict-loser logs. |
| `pattern`            | string    | regex    | Compiled with the [`regex`](https://docs.rs/regex) crate at policy load. Bad patterns → `PolicyConfig`. |
| `class`              | string    | yes      | A class name (see [Classes](#classes)). Unknown classes → `PolicyConfig`. |
| `terms`              | array     | dictionary | Inline dictionary terms. Use only with `kind = "dictionary"`. |
| `terms_file`         | string    | dictionary | Newline-delimited dictionary terms. Blank lines and `#` comments are ignored. |
| `terms_from_context` | string    | dictionary | Reads the named dictionary from `--context-json`; cannot be combined with `terms` or `terms_file`. |
| `case_sensitive`     | boolean   | no       | Dictionary only. Defaults to `false`; non-ASCII insensitive dictionaries fail closed in v0.4.0. |
| `token_family`       | string    | no       | Defaults to `"counter"`. |

Dictionary recognizers are registered through the same recognizer registry as
rulepack recognizers and are gated by the active locale chain when they come
from a rulepack. The CLI passes the merged dictionary bundle from rulepacks,
policy inline terms, and `--context-json` into the runtime `DetectContext`.

```toml
[[policy.custom_recognizers]]
kind = "dictionary"
name = "songs"
class = "custom:song"
terms = ["Song A", "Song B"]

[[policy.custom_recognizers]]
kind = "dictionary"
name = "tenant_order_ids"
class = "custom:order_id"
terms_from_context = "order_ids"
case_sensitive = true
```

`--context-json` can also supply dictionaries without a matching policy
recognizer. In that mode, Gaze registers one dictionary recognizer per context
dictionary and uses `class_map` for the class, falling back to `custom:<name>`
when a mapping is absent. `fields` are threaded into `DetectContext` for
recognizers and are available to library users through the borrowed
`Context::fields_typed() -> ContextFieldsRef<'_>` accessor. `class_map` is
runtime metadata for dictionary recognizer construction, not a general class
override mechanism.

NER is **not** a detector kind. NER is configured via the top-level `[ner]`
block (below) — when set, the pipeline appends a transformer NER detector
alongside the regex detectors declared here.

#### Migrating `[[detector]]`

The legacy top-level `[[detector]]` table is no longer accepted in v0.4.
Move each block to `[[policy.custom_recognizers]]` with the same fields. Gaze
fails loudly with `PolicyConfig` instead of silently accepting both surfaces.

### `[[rule]]`

One block per rule. **At least one rule is required** — an empty list is
rejected with `PolicyConfig`. Rules are evaluated in declaration order;
the first rule whose match condition fires decides the action. If no rule
matches, the pipeline falls back to `Action::Preserve`.

```toml
[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
```

| Field    | Type   | Required           | Notes                                            |
|----------|--------|--------------------|--------------------------------------------------|
| `kind`   | string | yes                | One of `"class"`, `"column"`, `"default"`.       |
| `action` | string | yes                | One of `"tokenize"`, `"redact"`, `"format_preserve"`, `"generalize"`, `"preserve"`. |
| `class`  | string | yes if `kind="class"` | Class name; same vocabulary as detector `class`. |
| `column` | string | yes if `kind="column"` | Field name to match against the document context. |

#### Rule kinds

- **`kind = "class"`** — fires when a detection's class equals `class`. The
  most common rule shape.
- **`kind = "column"`** — fires when the document being redacted is a
  structured value and the current field name equals `column`. Resolved in
  v0.3.1: `gaze clean` rejects policies containing `column` rules with
  `PolicyConfig`, because the CLI only accepts text on stdin and has no field
  name. `column` rules are useful only when driving the library directly with
  `RawDocument::Structured`.
- **`kind = "default"`** — always fires. Place last as a catch-all. If
  omitted, unmatched detections fall through to `Preserve` automatically,
  but an explicit `default` makes the policy intent visible.

### `[ner]` (optional)

```toml
[ner]
model_dir = "~/.local/share/gaze/models/davlan-mbert-ner-hrl"
locale = "de"
threshold = 0.3
```

| Field       | Type   | Required | Notes                                                          |
|-------------|--------|----------|----------------------------------------------------------------|
| `model_dir` | string | no       | Directory containing the ONNX model bundle. `~/` is expanded from `$HOME`. If absent, NER is silently disabled and the pipeline runs with regex detectors only (a `tracing::warn!` is logged). |
| `locale`    | string | no       | Locale hint passed to the NER detector (e.g. `"de"`).          |
| `threshold` | float  | no       | Confidence floor in the inclusive range `0.0..=1.0`. Defaults to `0.3`. `gaze clean --ner-threshold=<float>` overrides this value for one invocation. |

If `model_dir` is set but the model fails to load (missing files, bad
manifest), the CLI maps the failure to **exit `2` `PolicyConfig`**. Treat
NER load errors as policy configuration failures: verify the install path
against [README §"NER Model Runtime"](../README.md#ner-model-runtime).

## Detectors

### Regex (`kind = "regex"`)

Pattern syntax follows the [Rust `regex` crate](https://docs.rs/regex). The
crate intentionally **does not support look-ahead, look-behind, or
back-references**, so patterns ported from PCRE / Python `re` may need
rewriting.

Common idioms:

- Use `\b` word boundaries to avoid matching inside identifiers
  (`\bORD-\d{6}\b`, not `ORD-\d{6}`).
- Use `(?i)` at the start of a pattern for case-insensitive matching.
- TOML literal strings (`'...'`) avoid double-escaping backslashes —
  prefer them over basic strings (`"..."`) for regex patterns.

The pattern is compiled at `Policy::load` time, so a malformed regex fails
fast with `PolicyConfig` and never reaches `gaze clean`'s stdin read.

Detection order matters when spans overlap: longer spans win first, then
declaration order, then earlier start position (see
[`pipeline.rs::select_winners`](../crates/gaze/src/pipeline.rs)). This is
why the document above stresses "rules are evaluated in declaration order"
— the same applies to detectors when they fight over the same bytes.

### NER (`[ner]` block)

NER is opt-in and stacks on top of regex detectors. The runtime expects a
local ONNX model directory; no models are downloaded at runtime. See the
[README NER Model Runtime](../README.md#ner-model-runtime) section for the
required files and the canonical install path.

When loaded, the NER detector emits `PiiClass::Name`, `PiiClass::Location`,
and `PiiClass::Organization` for entities the model recognises. Map them to
actions via `kind = "class"` rules — declare detector-side once via `[ner]`,
then act on the classes the model produces.

## Rule actions

| `action` value      | What it does                                                                                                       |
|---------------------|--------------------------------------------------------------------------------------------------------------------|
| `"tokenize"`        | Replace the matched span with an angle-bracketed counter-family token (`<{session_hex}:Email_1>`, `<{session_hex}:Name_2>`, `<{session_hex}:Custom:order_id_3>`, …). Restorable via the session blob. |
| `"redact"`          | Replace the matched span with the literal string `[REDACTED]`. Not restorable — the original value is dropped from the session map. |
| `"format_preserve"` | Replace with a fake value that preserves the surface shape (`email1.{session_hex}@gaze-fake.invalid` for emails; `{session_hex}:name_1`, `{session_hex}:location_1`, `{session_hex}:custom:order_id_1` for everything else). Restorable. |
| `"generalize"`      | Replace with a bracketed class label: `[EMAIL]`, `[NAME]`, `[LOCATION]`, `[ORGANIZATION]`, or `[CUSTOM_NAME]` (uppercased custom name with underscores preserved). Restoration returns the label, not the original value. |
| `"preserve"`        | Leave the matched span unchanged. The detection is still logged, but no replacement happens. |

`Tokenize`, `FormatPreserve`, `Redact`, and `Generalize` all increment the
`stats.detections` counter in `gaze clean`'s stdout. `Preserve` does not.

There is no `"passthrough"` action — the closest equivalent is `"preserve"`.

## Session scope and TTL

`[session]` declares the session contract the policy expects. `gaze clean`
exports a `SensitiveSnapshot` (the `session_blob` field of stdout) so that
`gaze restore` can rebuild the token↔value map later.

- `scope = "ephemeral"` — *not usable from the CLI*. The library refuses to
  export ephemeral sessions (`Error::ExportForbidden`); a CLI invocation
  with this scope would be unable to emit `session_blob`.
- `scope = "conversation"` — reserved for library callers that scope
  sessions to a specific conversation id; the CLI does not surface this
  today.
- `scope = "persistent"` — the only scope `gaze clean` produces. Requires
  `ttl_secs > 0`.

The `--session-ttl=<secs>` CLI flag overrides the policy TTL for persistent
sessions. If the flag is omitted, `gaze clean` uses `[session].ttl_secs`;
policy-less stub mode falls back to `86400`.

The `--ner-threshold=<float>` CLI flag overrides `[ner].threshold` for one
`gaze clean` invocation. Precedence is CLI flag, then policy TOML, then the
default `0.3`. Values outside `0.0..=1.0` fail closed as `PolicyConfig`.

TTL enforcement on `gaze restore`: when the imported snapshot's `issued_at +
ttl_secs` has passed, restore fails with **exit `3` `BlobExpired`**. (The
`issued_at` field landed in v0.3.0-rc.2 — older blobs predating the field
treat the TTL as bypassed for forward-compatibility.)

## Full worked examples

### Example A — Tokenize emails, redact phone numbers

```toml
[session]
scope = "persistent"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = '(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b'
class = "email"

[[policy.custom_recognizers]]
kind = "regex"
name = "phones_de"
pattern = '\+49[ \-]?\d{2,4}[ \-]?\d{3,8}'
class = "custom:phone_de"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:phone_de"
action = "redact"

[[rule]]
kind = "default"
action = "preserve"
```

Input `Reach Alice at alice@example.invalid or +49 30 1234567` produces
`Reach Alice at <{session_hex}:Email_1> or [REDACTED]`.

### Example B — Custom class for tenant order IDs

```toml
[session]
scope = "persistent"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "order_ids"
pattern = '\bORD-\d{6}\b'
class = "custom:order_id"

[[rule]]
kind = "class"
class = "custom:order_id"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
```

`Order ORD-123456 is queued.` → `Order <{session_hex}:Custom:order_id_1> is queued.`

### Example C — Format-preserving emails for downstream parsers

When a downstream LLM or parser expects emails to look like emails, use
`format_preserve` so the surface shape survives redaction.

```toml
[session]
scope = "persistent"
ttl_secs = 86400

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = '(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b'
class = "email"

[[rule]]
kind = "class"
class = "email"
action = "format_preserve"

[[rule]]
kind = "default"
action = "preserve"
```

`Mail alice@example.invalid` → `Mail email1.{session_hex}@gaze-fake.invalid`. Restoration returns
the real address.

### Example D — Mixed regex + NER + custom class

```toml
[session]
scope = "persistent"
ttl_secs = 86400

[ner]
model_dir = "~/.local/share/gaze/models/davlan-mbert-ner-hrl"
locale = "de"

[[policy.custom_recognizers]]
kind = "regex"
name = "emails"
pattern = '(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b'
class = "email"

[[policy.custom_recognizers]]
kind = "regex"
name = "order_ids"
pattern = '\bORD-\d{6}\b'
class = "custom:order_id"

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "class"
class = "name"
action = "tokenize"

[[rule]]
kind = "class"
class = "location"
action = "generalize"

[[rule]]
kind = "class"
class = "organization"
action = "preserve"

[[rule]]
kind = "class"
class = "custom:order_id"
action = "redact"

[[rule]]
kind = "default"
action = "preserve"
```

NER provides `name`, `location`, `organization` detections; regex
detectors provide `email` and `custom:order_id`. Each class maps to a
different action. Note that `organization = preserve` lets brand names
through while `name = tokenize` swaps person names for restorable tokens.

## Troubleshooting

Each `PolicyError` variant maps to one exit code via `gaze clean`. The
mapping lives at [`gaze-cli/src/main.rs::map_policy_error`](../crates/gaze-cli/src/main.rs)
and is summarised here.

| Symptom (stderr variant)                | `PolicyError`              | Exit | Common cause                                                                 |
|-----------------------------------------|----------------------------|------|------------------------------------------------------------------------------|
| `{"error":"PolicyOpen","exit":4}`       | `Io`                       | 4    | `--policy` path does not exist, is unreadable, or points at a directory.     |
| `{"error":"PolicyConfig","exit":2}`     | `TomlParse`                | 2    | TOML syntax error, or an unknown key (`deny_unknown_fields` is on everywhere). Re-check field spelling. |
| `{"error":"PolicyConfig","exit":2}`     | `UnknownClass(s)`          | 2    | A `class` value not in `{email, name, location, organization}` and not prefixed `custom:`. Or `"custom:"` with an empty name. |
| `{"error":"PolicyConfig","exit":2}`     | `BadRegex { name, … }`     | 2    | `pattern` failed to compile. Watch for unsupported PCRE features (lookaround, backrefs). |
| `{"error":"PolicyConfig","exit":2}`     | `MissingTtl`               | 2    | `scope = "persistent"` but `ttl_secs` was omitted.                           |
| `{"error":"PolicyConfig","exit":2}`     | `BadTtl(s)`                | 2    | `ttl_secs = 0`, an unknown `session.scope`, an unknown `rule.kind`, an unknown `rule.action`, a missing `column`, or `~/` expansion failed because `$HOME` is unset. (The variant name is historical — it covers more than just TTL.) |
| `{"error":"PolicyConfig","exit":2}`     | `NoDetectors`              | 2    | No bundled rulepacks, no rulepack paths, and zero `[[policy.custom_recognizers]]` blocks. |
| `{"error":"PolicyConfig","exit":2}`     | `NoRules`                  | 2    | Zero `[[rule]]` blocks. At least one is required.                            |
| `{"error":"PolicyConfig","exit":2}`     | `BadTtl("unknown detector.kind …")` | 2 | `kind` was not `"regex"`. Surfaced from `Pipeline::from_policy`, not the parser. |
| `{"error":"PolicyConfig","exit":2}`     | `NerLoad`                  | 2    | `[ner] model_dir` resolves but the model bundle is missing or corrupt. Verify the install path against the README. |
| `{"error":"PolicyConfig","exit":2,"detail":"column rules not supported in CLI mode"}` | `UnsupportedRuleKind` | 2 | `gaze clean` received a policy containing `kind = "column"`. Use library structured input for column rules. |

## See also

- [`docs/roadmap/v0.3/cli.md`](roadmap/v0.3/cli.md) — full CLI contract
  (subcommands, exit codes, stderr discipline, blob format).
- [`docs/roadmap/v0.3/laravel.md`](roadmap/v0.3/laravel.md) — host-side
  integration shape.
- [`README.md`](../README.md) — install, build, NER model runtime.
- [`crates/gaze/src/policy.rs`](../crates/gaze/src/policy.rs) — canonical
  parser; the source of truth for every field on this page.

## Known spec drift

Documented here so users get the truth while the gaps land on the
engineering board:

1. **Resolved in v0.3.1: `policy.session` is honoured by `gaze clean`.**
   The CLI constructs sessions from `[session]`; `--session-ttl` is now only
   an explicit persistent-TTL override.
2. **Resolved in v0.3.1: `[ner]` load failures exit `2` `PolicyConfig`.**
   `PolicyError::NerLoad` keeps missing or corrupt model bundles in the
   policy/configuration failure class.
3. **Resolved in v0.3.1: `kind = "column"` rules are rejected in CLI mode.**
   `gaze clean` now fails policy load with exit `2` `PolicyConfig` and a
   detail string, avoiding silent no-op column rules for text stdin.
4. **v0.4.1 still gates `token.format` and context scoring hints.**
   The rulepack schema parses `token.format`, `context.hotwords`,
   `context.boost`, and `context.window` for forward-compatible authoring, but
   the loader rejects any non-default value with
   `RulepackError::UnsupportedFieldInB1`.
5. **v0.4.1 dictionary audit granularity is per term.** Dictionary redaction
   sources use `dictionary:{name}[#term_index]`, where `term_index` is the
   term's position in the loaded dictionary.
6. **v0.4.0-rc.1 NER context-sensitivity gap.** Default Davlan-HRL may
   pass names embedded in prompt boilerplate or RFC822 email headers.
   Workarounds (wrap with a dictionary recognizer, tighten locale gating
   via `[ner] locale`) and roadmap in GitHub issue #24.
