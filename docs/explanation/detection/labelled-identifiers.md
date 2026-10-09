# Labelled identifiers

The bundled tax-number, driver-licence, and national-ID or ID-card fallbacks read a class-specific field name,
an explicit separator (`:`, `=`, `#`, `|`, tab, comma, semicolon, or hyphen) or a short connector such as `is`, `lautet`,
`est`, or `Nr.`. A value can follow on the next line when the label ends the current line.
They cover form fields, JSON keys, logs, and table cells whose values do not fit a country's
fixed identifier shape. English, German, French, Dutch, and Portuguese labels are included.

Only the recognized field's value becomes a token. Its label and punctuation remain visible so an agent can still
understand the field. The original value bytes go into the manifest, so strict restore returns
the input exactly. A shared complete-value scan extends digit-bearing or uppercase groups after
a bounded regex capture. It protects adjacent grouped runs even when they exceed four groups or
40 bytes; those limits produce an audit reason, not a cutoff that exposes a suffix. The scan
stops before a following date or field cue. For an uppercase next-field label, it waits until a
second recognizer claims the following value before exposing that label. If no rule claims the
value, the prior token keeps the label and value covered. The shared connector set is `:`, `=`,
`|`, tab, comma, semicolon, and hyphen, with optional surrounding spaces. The
audit reason records the boundary without recording value bytes. Letter-led and digit-led groups
may use spaces, dots, slashes, or hyphens. A single ungrouped value still needs a matching regex
shape; digit-bearing identity-card values have no upper length cutoff. The three older `*.cue_anchored` rules also use this scan; their original regex captures
remain protected when a date-like group occurs inside them.
Grouped numbers need at least three chunks, except fixed EIN (2–7 digits) and German slash tax-number layouts. A directly labelled nine-digit tax value also qualifies. These shapes keep ordinary decimal amounts out of the fallback.

The rules do not infer a class from a bare value or a generic `id` or `number` field. A field
name supplies the class, and the value must contain a digit. The tax fallback admits compact numeric values only in the nine-digit form under an explicit
tax label. Eleven-digit German Steuer-ID values remain owned by the specific Steuer-ID rule
and its auditable checksum-failure contract. The new compact nine-digit arm requires at least
one nonzero digit; zero runs inside letter-bearing values remain protected. All three
fallbacks reject calendar dates that can resemble grouped identifiers. These are
precision boundaries, not claims that every real identifier will fit the fallback. Benchmark
evidence and hand-written shape probes are required before adding a new label or value shape.
An unpunctuated `license number` remains outside the driver-licence fallback because it can
refer to a non-personal licence.

The three fallback rows and their activation stay in the
[redaction-class inventory](../../reference/redaction-classes.md#embedded-recognizers). The
rulepack's `complete_labelled_value` field is documented in the
[policy reference](../../reference/policy.md); it is available to external rulepacks but not to
`[[policy.custom_recognizers]]`.
