- Daemon v2 keeps one NER and Nym inference cache across the stabilization
  passes of a request, so a later pass reuses the model output for a block
  whose input is unchanged instead of running the models again. A warm turn
  runs each model window once instead of twice. Responses are byte-identical.
  `perf-timers` builds now also report per-request timings for v2 requests.
