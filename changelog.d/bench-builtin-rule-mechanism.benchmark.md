- Benchmark certification accepts the mechanism kind `builtin-rules`: a change
  that touches only embedded rule definitions and `gaze-recognizers` tests certifies its
  mechanism on the gain pair itself, after an independent reviewer confirms the
  scope. The certifier recomputes the base-to-candidate build diff and refuses
  any other build change; every other gate guard still applies.
