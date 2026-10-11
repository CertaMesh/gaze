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

Known limitations: generator v17 does not characterize short unlabelled numeric repeats, typographic value quotes, malformed quoted scalars or CSV header linkage. Unquoted alphabetic-only values are deliberately rejected as placeholders. Unterminated ASCII quotes conservatively protect the remainder, possibly including following public text; that malformed-input behavior is covered by runtime fixtures, not a measured claim.

All six fresh source gates pass v1, v2 and v4 on generator v17. Full policy removes 70,650 gated leaked bytes with +204 net false-positive bytes. Rules only removes 74,783 gated leaked bytes with +272 net false-positive bytes.
No layer's gated or headline leaks or refusals rise. Restore failures and invalid manifests remain zero; every attempted document restores exactly and has a valid manifest. Layer D strict counterweights pass.

The measured base is `0fcfef14f9846ab1e85c723217fd67a4cae4fc86`; candidate source is `ebce7063b2667119e923c5969aae6ee6b4210567`. Generator v17 supplies independent ownership positives, repeats and benign controls, including grouped Unicode values, raw JSON Unicode escapes and newline-adjacent fields. Each source tree was clean at measurement. Earlier failed v15 and passing v16 results remain in Git history and receive no credit for this repair.

Scorecards use deterministic gzip archives. Regenerate an ordinary JSON
scorecard from its bound observations with `scripts/bench/rescore.py`, or unpack
the archive before passing it to `scripts/bench/agentic_layers.py gate`. The
receipts pin the source revisions, policies and observation records.
