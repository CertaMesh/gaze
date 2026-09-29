# Labelled identifiers

The bundled tax-number, driver-licence, and national-ID or ID-card fallbacks read a class-specific field name,
an explicit separator (`:`, `=`, `#`, `|`, or a comma) or a short connector such as `is`, `lautet`,
`est`, or `Nr.`. A value can follow on the next line when the label ends the current line.
They cover form fields, JSON keys, logs, and table cells whose values do not fit a country's
fixed identifier shape. English, German, French, Dutch, and Portuguese labels are included.

Only the value becomes a token. The label and punctuation remain visible so an agent can still
understand the field. The original value bytes go into the manifest, so strict restore returns
the input exactly. A terminal boundary stops the fallback from tokenizing a short prefix of a
longer identifier. Bounded letter-led and digit-led groups may use spaces, dots, slashes, or hyphens.
Grouped numbers need at least three chunks, keeping ordinary decimal amounts out of this fallback.

The rules do not infer a class from a bare value or a generic `id` or `number` field. A field
name supplies the class, and the value must contain a digit. The tax fallback excludes bare
digits so an invalid German Steuer-ID stays subject to its checksum validator. All three
fallbacks reject calendar dates that can resemble grouped identifiers. These are
precision boundaries, not claims that every real identifier will fit the fallback. Benchmark
evidence and hand-written shape probes are required before adding a new label or value shape.
An unpunctuated `license number` remains outside the driver-licence fallback because it can
refer to a non-personal licence.
