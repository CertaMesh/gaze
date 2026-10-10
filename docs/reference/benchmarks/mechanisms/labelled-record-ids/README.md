# Labelled record ID mechanism

This ablation measures the customer, employee and person-record ID rules on
primary and generated corpora. Both arms use one binary and the same rules-only
policy: model sections are absent, and address blocks and locale rulepacks are
unchanged. Full setup-policy base/candidate gates separately measure interaction
with NER and the Nym safety net.

`base-core.toml` contains every core recognizer except the three ID rules.
`mechanism.toml` contains those three rules unchanged. The exporter receipt pins
the original embedded core and both exact subsets. The candidate policy appends
only `mechanism.toml`; `delta.toml` declares that append for the normal comparison
checker. Relative file paths resolve from the repository root.

Both policies retain the same locale order and default tokenize action. Neither
arm runs neural inference. This evidence supports byte comparisons only; it does
not establish full-pipeline latency or model interaction.
