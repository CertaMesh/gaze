- When the whole-record observation cache misses, the benchmark runner reuses
  single detector responses keyed on the detection identity and the exact
  request (fixture ID, locale chain and text). A benchmark-only change, such as
  a new generator cell, captures only its new or changed documents. A detector
  change still captures every candidate document live; only its base side
  gains. `--unit-shadow` checks that a fully live capture and the record
  assembled from stored responses are byte-identical, and `--no-unit-reuse`
  turns reuse off.
