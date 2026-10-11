# Record identifier value ownership

Generator v17 adds authored CC0 CRM/HR field-ownership cells independently of
runtime detector expressions. Each partition adds 780 A positives, 780 D benign
counterweights and 660 R repeat documents. The complete test partition has 4,508
A, 2,924 D and 940 R documents. Layer C and its scoring contracts are unchanged.

An explicitly personal label owns the whole inserted identifier value. It does
not own the next field's label or value. Customer, employee and person/member
record values retain their separate classes, even when adjacent fields share a
class. Customer-number labels remain personal; public order, invoice, version,
equipment and inventory fields are benign.

| Failure class | Positive and repeat cells | Benign counterweights |
| --- | --- | --- |
| Grouped values | Alphabetic prefixes followed by six-digit alphanumeric groups, ordinary and nonbreaking spaces, English/German cues | Same grouped values behind inventory labels; schema placeholders |
| Quoted scalars | Whole ASCII-quoted word groups, Unicode, punctuation, tabs, quotes and backslashes in JSON, embedded JSON and English/German logs | Identically shaped public artifact scalars; quoting a schema label without quoting its placeholder |
| Adjacent fields | Two independently generated personal IDs separated by assignment syntax, same/different classes, key-case variation; later repetition of the second ID | Personal values followed by public assignments; placeholder followed by a public assignment; two public fields |
| Cue resumption | Rejected unquoted placeholder followed by an independently labelled ID and its later repeat | Placeholder-only schema text and quoted-label controls |

Every gold or decoy span selects the exact inserted UTF-8 bytes. JSON gold covers
the raw scalar spelling, including its escapes, with the surrounding string
delimiters outside gold. Embedded JSON preserves the outer string's additional
escape layer. Adjacent personal values have separate gold spans; labels and field
separators are never gold. Public labels and values are recorded decoys, so a
scanner that consumes them pays false-positive bytes even if restoration works.

Repeat variants use at least six digits and repeat the exact raw spelling.
Alphabetic-only quoted scalars are labelled positives with public scalar twins,
without a claim about unlabelled-repeat protection. Vocabulary, seeded values
and template identities are disjoint between dev and test; no real PII is used.
[Committed pre-fix probes](record-id-ownership-pre-fix-sensitivity.json) reproduce
these failures across all three classes using independently generated cells.
Canonical scorer tests detect a prefix-only guard veto, truncated quoted scalar,
combined adjacent-field span, omitted second-value repeat and overbroad public
scalar prediction. Exact gold predictions have zero leaked and false-positive
bytes in these tests. These sensitivity checks do not establish runtime coverage.

The v16 corpus and its frozen contract reconstruct byte-for-byte. The new cells
use the existing customer, employee and record labels under contracts v1/v2/v4;
no gate-credit exception changes. Historical release code is measured by the
current committed harness, not by substituting the current runtime for a tag.

Known limitations: short unlabelled repeats below the six-digit sweep floor,
typographic value quotes and CSV header-to-value linkage remain uncharacterized.
Malformed or unterminated quoted scalars are outside this generated slice. Quoted
scalar cells do not change structured-document leaf-key inference. The prior v15
and v16 results do not establish coverage for these new ownership boundaries;
runtime repairs require fresh measurements on v17 and independent real-text review.
