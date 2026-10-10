# Collision-family policy

Families resolve cross-class overlaps before the generic class-priority chain.
For example, `card.structural` emits `custom:credit_card` and `iban.structural`
emits `custom:iban`.

## Contract

Collision metadata sits beside recognizer definitions, outside the `Recognizer`
trait. `FamilyPolicyTable` indexes it by stable recognizer ID. Validator veto
runs first; failed candidates do not reach family policy unless explicitly kept.

| Candidates | Result |
| --- | --- |
| Same family and variant | Cooperate; no family arbitration |
| Same family, different variants | Lower precedence wins with `ConflictTier::CollisionPolicy` |
| Equal variant precedence | One `PiiClass::Custom("family:<name>")` token; `AmbiguityReason::PrecedenceTie`; `collision_family = <name>`, `collision_variant = NULL` |
| No collision declarations | Existing class/rule priority, score, span-length, and recognizer-ID ordering |

A family win settles the span. Later unrelated overlaps may change `decided_by`
for audit, but cannot reopen the family or trigger missing-anchor fallback.

Family-level actions use the shared first-match policy walk. Without a reachable
family rule, choose the strictest resolved member action or family default
(`gaze_types::Action::strictness_rank`). Explicit family rules override; audit
records `AmbiguityRecord::derived_action`. See the
[policy reference](../../reference/policy.md#how-a-family-level-token-picks-its-action).

Custom policy members use the regex `name` or dictionary `dict/<name>` as their
ID. Registry lookup, member-action derivation, and loser-class attribution use
that same ID.

## TOML shape

```toml
[[recognizers]]
id = "iban.structural"
class = "custom:iban"

[recognizers.collision]
family = "payment-card-or-iban"
variant = "iban"
precedence = 10
mandatory_anchor = "iban" # optional
```

`family` and `variant` must be non-empty kebab-case IDs, at most 64 bytes.
Recognizers sharing a variant must share its precedence. Rulepack load rejects
equal precedence between different variants.

## Bundled families

| Family | Members |
| --- | --- |
| `payment-card-or-iban` | `iban.structural`: 10; `card.structural`: 20 |
| `phone-or-imei` | `phone.structural`, `phone.national.de`, `phone.national.us`: variant `phone`, precedence 10; reserved for a future IMEI variant |

Custom policy recognizers cannot claim reserved bundled family names.
