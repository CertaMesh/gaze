# Write a policy: worked examples

Copy these complete policies; A–C include sample results. `[at]` means `@`
in the displayed input. See the [policy reference](../../reference/policy.md)
for fields and actions.

None of these policies declares `[policy.rulepacks]`, so the bundled `core`
rulepack supplies email detection. Do not add a generic email regex as a
custom recognizer: it also matches Gaze's own email-shaped tokens, and the
loader rejects it with `TokenShapeShadow`.

## Example A — Tokenize emails, redact phone numbers

<!-- policy -->
```toml
[session]
scope = "persistent"
ttl_secs = 86400

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
action = "tokenize"
```

Input `Reach Alice at alice [at] example.invalid or +49 171 3920010` produces
`Reach Alice at <{session_hex}:Email_1> or [REDACTED]`.

## Example B — Custom class for tenant order IDs

<!-- policy -->
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
action = "tokenize"
```

`Order ORD-123456 is queued.` → `Order <{session_hex}:Custom:order_id_1> is queued.`

## Example C — Format-preserving emails for downstream parsers

`format_preserve` keeps email syntax for downstream parsers.

<!-- policy -->
```toml
[session]
scope = "persistent"
ttl_secs = 86400

[[rule]]
kind = "class"
class = "email"
action = "format_preserve"

[[rule]]
kind = "default"
action = "tokenize"
```

`Mail alice [at] example.invalid` → `Mail email1.{session_hex}@gaze-fake.invalid`. Restoration returns
the real address.

## Example D — Mixed regex + NER + custom class

The canonical NER subset of this example lives in
[`crates/gaze-recognizers/assets/ner/policy-snippet.davlan-mbert.toml`](../../../crates/gaze-recognizers/assets/ner/policy-snippet.davlan-mbert.toml).

<!-- policy -->
```toml
[session]
scope = "persistent"
ttl_secs = 86400

[ner]
model_dir = "~/.local/share/gaze/models/davlan-mbert-ner-hrl"
locale = "de"

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

NER detects names, locations, and organizations; the bundled `core` rulepack
detects emails, and the custom regex detects order IDs. `tokenize` is reversible; `redact` and `generalize` are one-way.
`preserve` sends raw values, including organizations and any detected class
without its own rule. Review this exposure before production use.
