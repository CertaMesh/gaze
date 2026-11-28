- **Breaking:** every class the shipped rulepacks emit is now a first-class
  `PiiClass` variant (`Phone`, `Iban`, `PostalCode`, `CreditCard` and 25 more),
  and collision-family classes are `PiiClass::Ambiguous(family)`. Exhaustive
  `match` statements on `PiiClass` need the new arms. Parsing a shipped name
  such as `custom:phone` yields its variant, and `PiiClass::Custom("phone")`
  compares equal to `PiiClass::Phone`. Token labels, audit and policy names,
  serde and stored manifests keep their `custom:` spelling unchanged. The new
  `shipped-class-coverage` xtask gate fails when a bundled recognizer emits a
  class without its own variant.
