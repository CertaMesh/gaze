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

This is the same fixture the CLI integration suite uses
(`crates/gaze/tests/cli_pipe.rs::t16_clean_with_policy_tokenizes_email`).

## Schema reference

All TOML tables use `deny_unknown_fields` — any key the parser does not
recognise is a hard error. v0.4 adds `[policy.rulepacks]` and
`[[policy.custom_recognizers]]`; legacy top-level `[[detector]]` is rejected.

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
recognizers; no built-in recognizer consumes them in v0.4.0. `class_map` is
runtime metadata for dictionary recognizer construction, not a general class
override mechanism. Broader `fields` consumers are deferred to v0.4.1.

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

## Classes

`class` accepts a fixed vocabulary of built-in names plus a `custom:<name>`
prefix for user-defined classes.

### Built-ins

| `class` value     | `PiiClass` variant       | Default token shape (`tokenize`)     | Format-preserve shape    | Generalize shape   |
|-------------------|--------------------------|--------------------------------------|--------------------------|--------------------|
| `"email"`         | `PiiClass::Email`        | `<{session_hex}:Email_1>`, `<{session_hex}:Email_2>`, …          | `email1.{session_hex}@gaze-fake.invalid`    | `[EMAIL]`          |
| `"name"`          | `PiiClass::Name`         | `<{session_hex}:Name_1>`, `<{session_hex}:Name_2>`, …            | `{session_hex}:name_1`, `{session_hex}:name_2`, …    | `[NAME]`           |
| `"location"`      | `PiiClass::Location`     | `<{session_hex}:Location_1>`, …                    | `{session_hex}:location_1`, …          | `[LOCATION]`       |
| `"organization"`  | `PiiClass::Organization` | `<{session_hex}:Organization_1>`, …                | `{session_hex}:organization_1`, …      | `[ORGANIZATION]`   |

Counter-family `tokenize` shapes wrap in angle brackets as of v0.3.0 so the LLM cannot dissolve them into adjacent words. Format-preserve shapes stay bare — the whole point of format-preserve is to look like a real value of that type.

Counters are per-class and per-session; the same raw value is interned so it
always maps to the same token within one session.

### Custom classes

Use `custom:<name>` to declare a project-specific class.

```toml
[[policy.custom_recognizers]]
kind = "regex"
name = "order_ids"
pattern = '\bORD-\d{6}\b'
class = "custom:order_id"

[[rule]]
kind = "class"
class = "custom:order_id"
action = "tokenize"
```

The `<name>` after `custom:` is normalised before use:

- Lowercased.
- Non-alphanumeric runs collapse to a single `_` separator.
- Leading and trailing separators are dropped.

So `"custom:Order ID"`, `"custom:order-id"`, and `"custom:order_id"` all
produce the same internal class.

Custom tokens carry a `Custom:` namespace prefix so they cannot collide with
built-ins or with each other:

| Policy `class`           | Normalised name | `tokenize` token         | `format_preserve` token | `generalize` token |
|--------------------------|-----------------|--------------------------|-------------------------|--------------------|
| `"custom:order_id"`      | `order_id`      | `<{session_hex}:Custom:order_id_1>`    | `{session_hex}:custom:order_id_1`     | `[ORDER_ID]`       |
| `"custom:tenant_slug"`   | `tenant_slug`   | `<{session_hex}:Custom:tenant_slug_1>` | `{session_hex}:custom:tenant_slug_1`  | `[TENANT_SLUG]`    |
| `"custom:song"`          | `song`          | `<{session_hex}:Custom:song_1>`        | `{session_hex}:custom:song_1`         | `[SONG]`           |

A class string of `"custom:"` (empty name) is rejected with `PolicyConfig`.

`custom:email` and other names that mirror built-ins are safe to use — the
`Custom:` prefix keeps them in their own counter family, so `custom:email`
emits `<{session_hex}:Custom:email_1>` while built-in email detections continue to emit
`<{session_hex}:Email_1>`.

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
4. **v0.4.0-rc.1 gated rulepack fields (runtime consumers pending v0.4.1).**
   The rulepack schema parses `token.family`, `token.format`,
   `context.hotwords`, `context.boost`, and `context.window` for
   forward-compatible authoring, but the loader rejects any non-default value
   with `RulepackError::UnsupportedFieldInB1`. Runtime consumers ship in
   v0.4.1; until then, leave these fields unset or explicitly default.
5. **v0.4.0-rc.1 dictionary audit granularity.** The redaction log carries
   `dictionary:{name}` for dictionary hits; per-term `[#term_index]`
   traceability is scheduled for v0.4.1.
6. **v0.4.0-rc.1 NER context-sensitivity gap.** Default Davlan-HRL may
   pass names embedded in prompt boilerplate or RFC822 email headers.
   Workarounds (wrap with a dictionary recognizer, tighten locale gating
   via `[ner] locale`) and roadmap in GitHub issue #24.
