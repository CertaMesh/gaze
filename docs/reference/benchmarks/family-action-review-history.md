# Historical family-action review annotation

The [PR #624 review table](https://github.com/CertaMesh/gaze/pull/624)
published 16,553 leaked characters for each of these two review-head arms.
That published total is superseded and unreproducible on the pinned inputs and binaries.
Both the scripts before the checked transport change and the checked transport
replay 17,220. The cause of the 667-character difference is unresolved.

| Reviewed arm | Published head total (superseded, unreproducible) | Replayed head total | Base total | Documents | Regressed documents |
| --- | ---: | ---: | ---: | ---: | ---: |
| `iban=tokenize/card=redact/default=tokenize/family=none/de-de` | 16,553 | 17,220 | 1,619 | 18,556 | 872 |
| `iban=unset/card=redact/default=tokenize/family=none/de-de` | 16,553 | 17,220 | 1,619 | 18,556 | 872 |

The [committed replay receipts](../../../scripts/bench/fixtures/checked-daemon-batch-evidence.json)
pin base `a40dba4fff0cf8636f3a75d5dc9bb41197561770` and review head
`98519327e75f6f4f494faefadf8297975e57648e`, including binary SHA-256 values.
The unchanged replay scripts are at `d9db072e8b801ce310d6423fac0bb965833b4691`.
Their source hashes and both arm counts are recorded in
`independent_published_discrepancy`.

The base total, document count and 872 regressions reproduce. The discrepancy
predates the checked transport. This annotation changes neither the detection
code nor the original public review record. The original table contains two
displays of 16,553; the replay fixture is the other numeric display in the repository.
The benchmark script reference links here so its historical claim has the same qualification.
