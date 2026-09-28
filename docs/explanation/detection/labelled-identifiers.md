# Labelled identifiers

The bundled tax-number, driver-licence, and ID-card fallbacks read a class-specific field name,
an explicit separator (`:`, `=`, `#`, or `|`), then one digit-bearing identifier on the same line.
They cover form fields, JSON keys, logs, and table cells whose values do not fit a country's
fixed identifier shape. English, German, French, Dutch, and Portuguese labels are included.

Only the value becomes a token. The label and punctuation remain visible so an agent can still
understand the field. The original value bytes go into the manifest, so strict restore returns
the input exactly. A terminal boundary stops the fallback from tokenizing a short prefix of a
longer identifier.

The rules do not infer a class from a bare value or a generic `id` or `number` field. A field
name supplies the class, and the value must contain a digit. The tax fallback excludes bare
digits so an invalid German Steuer-ID stays subject to its checksum validator. These are
precision boundaries, not claims that every real identifier will fit the fallback. Benchmark
evidence and hand-written shape probes are required before adding a new label or value shape.
