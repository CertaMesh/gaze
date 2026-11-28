- **Breaking:** every class the shipped rulepacks emit is now a first-class
  `PiiClass` variant (`Phone`, `Iban`, `PostalCode`, `CreditCard` and 25 more),
  and collision-family classes are `PiiClass::Ambiguous(family)`. `PiiClass`
  is now `#[non_exhaustive]`: a `match` on it needs a wildcard arm, and minor
  releases may add a variant for each newly shipped class. Parsing a shipped
  name such as `custom:phone` yields its variant, and
  `PiiClass::Custom("phone")` compares equal to `PiiClass::Phone`. Token
  labels, audit and policy names, safety-net `raw_label` values, serde and
  stored manifests keep their `custom:` spelling unchanged. The new
  `shipped-class-coverage` xtask gate fails when a bundled recognizer emits a
  class without its own variant.
- **Breaking:** `Debug` output of `PiiClass` changed. `Custom("phone")` now
  prints `Phone`, which affects `{:?}` assertions and error messages that use
  `Debug`. Stored and wire spellings are unchanged; `legacy_debug_label()`
  returns the old spelling.
- Equal `PiiClass` values can differ on `as_custom_name()`:
  `Custom("family:x") == Ambiguous("x")`, but construction normalizes, so
  callers should use `as_family_name()` and `legacy_custom_name()` rather than
  matching on variants.
