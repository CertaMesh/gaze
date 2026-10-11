# Labelled record ID measurements

The full-policy comparison uses fresh main and candidate binaries with the policy
each revision writes through `gaze setup`. It measures interaction with NER and
the Nym safety net. Bound observations and v1/v2/v4 scorecards live in
`full-policy/`.

The rules-only source comparison uses those two revisions with one identical
policy: NER and Nym sections are absent, while bundled core recognizers, address
blocks, locale order and the default tokenize action remain enabled. Each arm
records its binary SHA-256; identical binary hashes or different policy hashes
invalidate the comparison. Bound observations, scorecards and the source-pair
receipt live in `rules-only-source/`.

The ordinary layer gate checks both source comparisons. `mechanism_arms.py`
requires one shared source revision and therefore does not admit this source
pair; its checker and ledger remain unchanged. These measurements establish
byte comparisons, not a latency claim.

The exact exports `base-core.toml` and `mechanism.toml`, their exporter receipt,
and the append-only policy pair are retained as reproducible inputs for a
separate policy ablation. They contain every core recognizer except the three ID
rules and those three rules unchanged, respectively. That policy ablation is not
measured here and receives no gain credit.

Known limitations: generator v16 does not characterize short unlabelled numeric
repeats, typographic value quotes or rejected placeholders followed by valid ID
cues. Independent positive, repeat and benign counterweight cells are deferred.

Both measured source comparisons pass v1, v2 and v4 on generator v16. The
full-policy pair removes 19,030 gated leaked bytes and adds 14 net false-positive
bytes; the rules-only pair removes 19,120 and adds 16, respectively. No layer's
leaked bytes or refusals rise, and exact restores and valid manifests remain
complete. Restore failures and invalid manifests remain zero. Layer D
false-positive bytes are unchanged in both pairs.

The measured base is `7464d8158845bed59755176928c475434270f68d`; the candidate is
`d9e705152d87277c6ff5298e3bb992d964a1f445`. Generator v16 corrects the earlier
BSN benign twins to use non-person labels. The customer-number cue remains
unchanged: a bare customer number is personal data. The failed v15 measurements
remain available in this directory's Git history.

Scorecards use deterministic gzip archives. Regenerate an ordinary JSON
scorecard from its bound observations with `scripts/bench/rescore.py`, or unpack
the archive before passing it to `scripts/bench/agentic_layers.py gate`. The
receipts pin the source revisions, policies and observation records.
