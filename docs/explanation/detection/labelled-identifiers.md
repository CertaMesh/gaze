# Labelled identifiers

Bundled tax-number, driver-licence, and national-ID/ID-card fallbacks use
class-specific EN/DE/FR/NL/PT labels for values outside fixed country shapes.
A separator (`:`, `=`, `#`, `|`, tab, comma, semicolon, hyphen) or short connector
(`is`, `lautet`, `est`, `Nr.`) introduces the value. It may start on the next
line when the label ends the current line.

Only the value becomes a token; labels and punctuation stay readable. Exact
source bytes enter the manifest for strict restore.

A shared complete-value scan extends digit-bearing or uppercase groups beyond
the bounded regex capture. Four groups/40 bytes trigger an audit reason, never
a cutoff that leaks a suffix. It stops before dates or following field cues.
An uppercase next-field label is exposed only after another recognizer claims
its value; otherwise the prior token covers both. Scan boundary reasons contain
no value bytes.

Scan connectors are `:`, `=`, `|`, tab, comma, semicolon, and hyphen, with optional
spaces. Groups may use spaces, dots, slashes, or hyphens and start with letters
or digits. Ungrouped values still need a regex shape. The older `*.cue_anchored`
rules share this scan and keep their original capture protected when it includes
a date-like group.

| Boundary | Rule |
| --- | --- |
| Class | Requires a class-specific label; bare values and generic `id`/`number` do not qualify |
| Content | At least one digit |
| Grouped numbers | At least three chunks, except German slash tax layouts |
| Compact tax numbers | Nine digits under an explicit tax label; eleven under a full English tax-identification-number label |
| Compact numeric arms | At least one nonzero digit; zero runs in letter-bearing values stay protected |
| Calendar dates | Rejected by all three fallbacks |
| Unpunctuated `license number` | Excluded because it can describe a non-personal licence |

Compact eleven-digit English values remain generic tax numbers, so trusted
manifest copies can sweep uncued repeats. German Steuer-ID stays with its
specific rule and typed checksum-failure contract. These boundaries do not
cover every real identifier; new labels/shapes need benchmark evidence and
hand-written probes.

See the [fallback inventory](../../reference/redaction-classes.md#embedded-recognizers)
and [policy reference](../../reference/policy.md). `complete_labelled_value` is
available to external rulepacks, not `[[policy.custom_recognizers]]`.
