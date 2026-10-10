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

Known limitations: generator v15 does not characterize short unlabelled numeric
repeats, typographic value quotes or rejected placeholders followed by valid ID
cues. Independent positive, repeat and benign counterweight cells are deferred.

Both measured source comparisons fail v1, v2 and v4. The `ref_number_9`
benign-counterweight guard rises from 0 to 180 false-positive bytes. Its JSON and
log keys are `customerNumber`; the protection trace attributes these spans to
`customer_id.labelled`, not to the BSN recognizer. The current gate failure is
retained without a scoring exception or a detection change.

Scorecards use deterministic gzip archives. Regenerate an ordinary JSON
scorecard from its bound observations with `scripts/bench/rescore.py`, or unpack
the archive before passing it to `scripts/bench/agentic_layers.py gate`. The
receipts pin the source revisions, policies and observation records.
