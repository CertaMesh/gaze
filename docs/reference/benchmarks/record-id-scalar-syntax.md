# Record identifier scalar syntax

Generator v15 adds authored CC0 synthetic scalar syntax cases for person-linked
customer, employee and person/contact record identifiers. The cells follow JSON
value semantics and schema-writing traffic, independently of recognizer patterns.
They add 30 A positives, 115 D counterweights and 30 R repeats per partition.

A supplies explicitly labelled six-digit database keys as JSON numbers, alongside
the quoted-string values covered by v14. R repeats each numeric value three times,
including later references without a repeated label. Every occurrence is gold,
and each gold span selects the inserted number in UTF-8 bytes.

D supplies valid JSON fields whose values are bare `null`, `true` or `false`.
These keywords supply no identifier value. A public tool-turn field makes each
scalar document distinct without changing its identifier semantics. Other counterweights quote the schema
label while leaving an ordinary placeholder word unquoted. Quoting the label
alone does not establish an opaque string value. All predicted bytes in these
documents are false positives.

Numeric pools are independently seeded and disjoint between dev and test. Field
vocabulary and template IDs also differ between partitions. Values are invented
locally; no real customer or employee data is used. All earlier generator versions
3–14 remain byte-identical and retain their frozen agentic contracts. The new
syntax cells use the same three scored labels under primary contracts v1–v4;
the primary corpus, contracts and gate-credit exceptions are unchanged.

Known limitations: numeric identifiers still have six digits. Shorter unlabelled
repeats remain unmeasured and below the existing deterministic sweep floor.
Separate positive, repeat and benign coverage is required before changing it.
