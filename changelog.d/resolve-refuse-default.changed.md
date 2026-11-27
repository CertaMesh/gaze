- **Breaking:** the safety-net default is now `resolve` with the new `refuse`
  fallback (`SafetyNetFallback::Refuse`, `--safety-net-fallback refuse`) in
  `SafetyNetPolicy::default()`, `gaze clean`, `gaze daemon`, `gaze eval`, and
  `gaze index ingest --on-residual`. `refuse` runs every reversible step the
  `redact` fallback runs and rejects the document with
  `Error::SafetyNetFallback` (CLI exit 3) where `redact` wrote a one-way
  `[REDACTED:<class>]` marker, and where a residual the nets flag could not be
  tokenized (including a sub-word or a finding only the final re-scan sees)
  would otherwise ship. One-way markers now require an explicit `redact`.
  Other surfaces on the default: `CorpusIngestor::with_safety_net_resolution`
  and the `gaze setup` doctor's Nym and DOB checks. The proxy's routing probe
  keeps `redact`, so a residual there is still reported as unsurfaced PII
  (HTTP 422).
