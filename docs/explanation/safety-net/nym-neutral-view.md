# Nym's neutral token view

Nym scans text after Gaze has already replaced some PII with manifest tokens.
Those tokens change the words around an uncovered value, so a model decision
can change when a nearby deterministic recognizer begins tokenizing. Gaze
therefore gives Nym two views of the same clean text and joins their findings.

The existing **stable view** replaces each verified token's session hex with a
deterministic digest of its shape. The **neutral view** replaces each verified
ASCII token with `[PII]` followed by spaces to the token's original byte
length. Both views preserve every UTF-8 byte boundary outside and inside the
replaced token, so a finding's byte span still refers to the observable clean
text. Unowned token-shaped input stays literal. Tokens with non-ASCII bytes are
left out of the neutral view because replacing them with ASCII would change
character boundaries.

Nym checks both views on the initial safety-net pass and every follow-up pass.
Findings with the same span and class are combined; the stable finding keeps
its score. A finding's audit label records `view=stable`, `view=neutral`, or
`view=stable+neutral`. The existing manifest correlation and placeholder
clipping then remove token-contained findings before any policy action. The
observable clean text, manifest, and restore path never use the neutral view.

Neutral-only findings also keep the class's meaning: a room, suite, unit,
apartment, office, floor, or desk number is not a building's street number,
and one bare character is not a complete date. These checks do not remove findings
from the stable view. They are separate from the pagination-key guard.

Only Nym opts into this second scan. Other safety nets still receive the stable
view once. Nym inference may run twice per pass when verified tokens are
present. Failure in either view fails the scan closed. The benchmark gain gate
and per-label review decide whether the added findings are safe to adopt;
direct model replays alone cannot establish that.
