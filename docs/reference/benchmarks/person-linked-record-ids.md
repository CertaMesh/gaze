# Person-linked record identifier cells

Generator v14 adds authored CC0 synthetic customer, employee and person/contact
record identifiers. These cells follow the approved class meanings, independently
of production recognizer patterns. They represent CRM and HR tool traffic:
semantic JSON fields and same-line English or German labels explicitly link the
value to a person. No third-party benchmark values or runtime regexes are used.

Each partition has 360 A documents, 265 D documents and 120 R documents. A uses
numeric database keys, UUIDs, grouped application keys and long opaque keys for
all three labels. R repeats each value three times, including follow-up references
without a repeated label; every occurrence is gold. Gold spans are taken from the
inserted values in UTF-8 bytes.

D reuses exactly the positive shapes and values behind public order, invoice,
ticket, issue, build, commit and tracking fields, and generic `id`, `record_id` or
`unique_id` fields without person linkage. Prose twins put a personal-ID schema
hint beside a public resource identifier to price accidental cue ownership.
Placeholder twins exercise ordinary words following personal-ID labels, including
the token-bridge schema placeholder. These documents contain no gold: every
predicted byte is a false positive.

Dev and test have distinct field vocabulary and seeded value pools. Values are
invented locally and are never customer or employee records from a real system.
Numeric keys have no checksum convention; application prefixes carry no issuer
claim. UUIDs have the version/variant structure of random version-4 UUIDs, but
are deterministic synthetic fixtures. Previous generator versions 3–13 rebuild
byte-identically and retain their frozen agentic contracts.

The new labels are scored in the agentic contract for every primary scoring
contract (v1–v4). The primary corpus and its contracts are unchanged: it contains
none of these new labels. No checksum-credit or false-positive exception is added.
