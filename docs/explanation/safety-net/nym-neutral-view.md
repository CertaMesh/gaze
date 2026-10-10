# Nym's neutral token view

Nym scans two views because nearby Gaze tokens can change a model's decision
about an uncovered value. Neither view changes observable output or restore.

| View | Verified-token replacement |
|---|---|
| Stable | Session hex becomes a deterministic digest of token shape |
| Neutral | ASCII token becomes `[PII]` plus spaces to its original byte length |

Both preserve UTF-8 boundaries and offsets. Unowned token-shaped input stays
literal. Non-ASCII tokens are excluded from neutral masking to avoid changing
character boundaries.

Every initial/follow-up pass checks both views. Stable findings keep scores
and spans. Neutral findings are split into exposed gaps, excluding manifest
tokens and bytes already covered by stable findings. This prevents overlapping
actions when views disagree. Exact same-class actions record
`view=stable+neutral`; others record `view=stable` or `view=neutral`.

Neutral-only checks reject room/suite/unit/apartment/office/floor/desk numbers
as street-building numbers and a bare character as a complete date. Stable
findings remain unchanged; the pagination-key guard is separate.

Only Nym uses the second view. With verified tokens, it may infer twice per
pass; either failure fails closed. Other nets receive one stable scan.
Adoption requires benchmark gain and per-label review, not only model replays.
