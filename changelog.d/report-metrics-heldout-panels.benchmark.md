- `scripts/bench/report_metrics.py` reports, beside leaked bytes and outside
  the gain gate, exact-span precision, recall and F1, gold outcomes split into
  fully hidden, partially exposed and completely missed (counted on letters and
  digits, with the visible residue), type agreement on fully hidden spans, and
  harmless-lookalike masking.
- `scripts/bench/heldout_panels.py` scores two external held-out panels with
  those metrics: PrivacyBench+ (the two held-out companies Tracer recommends)
  and GraSCCo_PII v2, both CC BY 4.0, downloaded at run time and verified by
  SHA-256. Each panel has a frozen label-scope map and a disputed-label ledger
  that reports GraSCCo ward and room codes, and the standalone places and brand
  uses PrivacyBench+ leaves unlabelled, beside the as-annotated scores.
