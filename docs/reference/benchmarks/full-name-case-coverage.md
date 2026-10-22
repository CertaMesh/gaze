# Full person names across letter case

Generator v20 adds independently authored full-name inputs, so the benchmark
can measure all-lowercase and UPPERCASE names before a detection repair is
proposed. All complete names are invented, with separate EN, DE and FR name
pools and text templates assigned to dev and test. No detection code changes.

| Layer | Documents added per partition | Coverage |
| --- | ---: | --- |
| A | 72 | Six invented names (two or three parts in each locale), three case forms, four surfaces; fresh first occurrence |
| R | 144 | The same matrix after either a bounded title-case recipient cue seed or an uncued title-case name seed |
| D | 261 | Matched service/team phrases on every surface and seed form; generic contact/reply/greeting phrases; common nouns with surname readings |

The surfaces are chat turns, tool-call JSON with a serialized arguments string,
form fields, and email greetings. Lowercase and uppercase forms are generated
from the same title-case producer value. Gold labels each given-name component
as `GIVENNAME` and the final component as `SURNAME`, including both seed and
repeat. Spaces remain non-gold, consistent with the existing name cells.
Offsets refer to original UTF-8 bytes, with accents retained.

`rule_seed` and `ner_seed` describe inputs, rather than credit a detector.
The rule seed is `reply to <Title Name>, please.` with a punctuation boundary;
the uncued seed is the title-case name on a line of its own. Runtime attribution
still has to establish whether a seed was actually protected, and by which
mechanism. Nym can protect some lowercase text as usernames; label-agnostic
byte scoring counts that coverage without claiming reliable Name recognition.

Counterweights keep the same cues and fields with values that mean services
or roles rather than people, plus `contact technical support`, `reply to the
team`, `dear customer`, and explicit botanical/colour uses of words with
surname readings. Every predicted byte on D is a false positive. Generic
recipient cues and name-field contracts must pay for such ambiguous values.

Tests verify the full locale/case/part/surface/seed matrix, JSON decoding,
Unicode offsets, disjoint name pools, the frozen v19 corpus and contract, and
canonical scorer sensitivity to omitted surnames, missing repeats and benign
phrase protection. Earlier generators and contracts reconstruct byte-for-byte.

Limitations: these are a finite authored matrix, not proof of general person
recognition. They do not measure streaming boundaries, OCR, cross-turn session
state, owner-supplied records, arbitrary scripts, or every name punctuation
shape. Repeat documents keep seed and repeat in one clean call.
