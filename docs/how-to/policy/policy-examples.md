# Write a policy: worked examples

Copy these complete policies; A–C include sample results. See the
[policy reference](../../reference/policy.md) for fields and actions.

## Example A — Tokenize emails, redact phone numbers

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
action = "tokenize"
```

Input `Reach Alice at alice@example.invalid or +49 171 3920010` produces
`Reach Alice at <{session_hex}:Email_1> or [REDACTED]`.

## Example B — Custom class for tenant order IDs

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
action = "tokenize"
```

`Mail alice@example.invalid` → `Mail email1.{session_hex}@gaze-fake.invalid`. Restoration returns
the real address.

## Example D — Mixed regex + NER + custom class

The canonical NER subset of this example lives in
[`crates/gaze-recognizers/assets/ner/policy-snippet.davlan-mbert.toml`](../../../crates/gaze-recognizers/assets/ner/policy-snippet.davlan-mbert.toml).

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

NER detects names, locations, and organizations; regex detects emails and
order IDs. `tokenize` is reversible; `redact` and `generalize` are one-way.
`preserve` sends raw values, including organizations and any detected class
without its own rule. Review this exposure before production use.
