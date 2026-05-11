# Collision-Family Policy

Collision-family metadata handles cross-class recognizer rivalries that cannot
be represented as one PII class. PAN vs IBAN is the first bundled example:
`card.structural` emits `custom:credit_card`, `iban.structural` emits
`custom:iban`, and family policy decides overlap before the generic
class-priority chain.

## Contract

- Collision metadata lives beside recognizer definitions, not on the
  `Recognizer` trait. The runtime compiles it into `FamilyPolicyTable` and
  queries by stable recognizer id.
- `ValidatorVeto` still runs first. Validator-failed candidates never reach
  family policy.
- Same `(family, variant)` recognizers cooperate but do not arbitrate each
  other. Different variants in the same family compare by precedence; lower
  precedence wins and emits `ConflictTier::CollisionPolicy`.
- Equal precedence between variants is ambiguous. The resolver emits a
  family-level token using `PiiClass::Custom("family:<name>")`, attaches an
  `AmbiguityRecord` with `AmbiguityReason::PrecedenceTie`, and writes
  `collision_family = <name>` with `collision_variant = NULL`.
- Normal class-priority, rule-priority, score, span-length, and recognizer-id
  ordering stays unchanged for recognizers without collision declarations.

## TOML Shape

```toml
[[recognizers]]
id = "iban.structural"
class = "custom:iban"

[recognizers.collision]
family = "payment-card-or-iban"
variant = "iban"
precedence = 10
mandatory_anchor = "iban" # optional; consumed by later ambiguity handling
```

`family` and `variant` are non-empty kebab-case identifiers up to 64 bytes.
Two recognizers may share a `(family, variant)` only when their precedence
matches. Two different variants in one family cannot share precedence in
rulepacks; that fails rulepack load.

## Bundled Families

Current bundled declarations:

- `payment-card-or-iban`: `iban.structural` precedence 10,
  `card.structural` precedence 20.
- `phone-or-imei`: `phone.structural`, `phone.national.de`, and
  `phone.national.us` all use variant `phone` precedence 10. IMEI can join as a
  later variant without changing phone-only behavior.

Adopter policy custom recognizers cannot claim reserved bundled family names.
That guard prevents local policy from silently changing core collision
semantics.
