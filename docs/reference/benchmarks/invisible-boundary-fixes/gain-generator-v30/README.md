# Invisible-separator gain on generator v30

Contract v4 is the headline. Both gated contracts, v2 and v1, pass on the same retained observation pair.

Base main `9fb578e421a9f91d3868c7eeb585156e2a07eae8` and candidate `5df6f47ad4d6a54de44db150e345b699ff722aee` each produced one clean release capture of 44,040 observations.
Both use specification `gaze-benchmark-v1:1370fde4fa845fc4ba68d632265b8e878588142ca0312e552eef0617167a05f9` and generator v30. No historical specification is used for a gain comparison.

## Headline contract v4

| Layer | Leaked bytes, base to candidate | False-positive bytes after gold-gap credit, base to candidate |
| --- | --- | --- |
| C | 9,717 → 9,717 | 15,224 → 15,224 |
| A | 147,944 → 78,442 | 7,577 → 15,197 |
| D | 0 → 0 | 16,216 → 16,212 |
| R | 246,290 → 120,944 | 2,045 → 9,665 |

## Gated contracts v2 and v1

| Contract | Layer | Gated leaked bytes | Headline leaked bytes | False-positive bytes | Refusals | Exact restores and valid manifests |
| --- | --- | --- | --- | --- | --- | --- |
| v2 | C | 6,652 → 6,652 | 6,652 → 6,652 | 27,004 → 27,004 | 0 → 0 | 2,910 = 2,910 |
| v2 | A | 146,742 → 77,240 | 147,944 → 78,442 | 7,577 → 15,197 | 0 → 0 | 19,687 = 19,687 |
| v2 | D | 0 → 0 | 0 → 0 | 16,216 → 16,212 | 0 → 0 | 8,832 = 8,832 |
| v2 | R | 246,290 → 120,944 | 246,290 → 120,944 | 2,045 → 9,665 | 0 → 0 | 12,611 = 12,611 |
| v1 | C | 9,717 → 9,717 | 9,717 → 9,717 | 27,021 → 27,021 | 0 → 0 | 2,910 = 2,910 |
| v1 | A | 146,742 → 77,240 | 147,944 → 78,442 | 7,577 → 15,197 | 0 → 0 | 19,687 = 19,687 |
| v1 | D | 0 → 0 | 0 → 0 | 16,216 → 16,212 | 0 → 0 | 8,832 = 8,832 |
| v1 | R | 246,290 → 120,944 | 246,290 → 120,944 | 2,045 → 9,665 | 0 → 0 | 12,611 = 12,611 |

Both contracts remove 194,848 leaked bytes and add 15,236 false-positive bytes, for a net improvement of 179,612 bytes.
No layer has more gated or headline leaked bytes. No document is refused. Every manifest is valid and every document restores exactly.
The `invisible_card_twin` guard stays at zero in both contracts. Other credit guards do not rise.

## Method and retained evidence

Each checkout writes its own `gaze setup` policy. The policy bytes match. The executed policy digest and model-bundle identities remain in the capture provenance.
The portable policy file substitutes `~` for the home directory only for presentation. Its bytes were not the executed policy.
The committed canonical `rescore_past_release.py --build` producer uses the release profile and `policy-file` configuration with the pinned Nym bundle.
All four canonical producer and validator build preambles finish without recompilation. The same observation records are replayed under contracts v1, v2, v3, and v4.
The v2 gate freezes gold verdicts to the base and also requires its candidate-verdict sensitivity view to pass. The v1 gate uses the ordinary comparison.
This pair supports byte and integrity claims. Recorded timing fields are retained as execution evidence and support no speed or memory claim.
Normalization always runs and has no policy switch. No separate policy-delta mechanism arm applies.

## Verification and limits

The measured candidate passes formatting, workspace Clippy with all features and targets and warnings denied, and all 3,616 executed workspace tests.
All 51 separator mutations fail their regression tests and restore the original source. Both-parent normalization parity and the Python CI suite pass.
The complete benchmark Python suite passes before the final main merge. Its measured source and the incomplete post-merge setup-policy diagnostic are identified in `verification.json`.
Post-merge no-policy CLI parity completes all 1,072 inputs with zero differences. The setup-policy diagnostic completes at least 28 inputs with no observed difference before interruption.
The full release pair above covers the default policy on the integrated candidate. The interrupted diagnostic is not counted as a complete parity pass.
Email-like values joined through invisible characters can absorb adjacent words. This fail-closed choice accounts for the false-positive rise in layers A and R.
Phone fragments remain protected. A failing enclosing card or identifier does not suppress a valid hard-boundary subspan.
The PEM integration regression fails before the bounded source-regex compilation fix and passes after it. Original patterns retain their 10 MiB admission limit. Transformed patterns have an 80 MiB compilation limit.

## Reproduce the evidence checks

With the canonical Dataiku holdout available at the documented path, run:

```sh
uv run --project scripts/bench --python 3.13 python scripts/ci/verify_invisible_separator_gain.py --dataset target/bench-data/dataiku-en-de/test.parquet
```

The checker verifies stored-byte pins, clean capture sources, execution artifact bindings, the current immutable specification, every contract replay, and both gate results. It performs no detection or compilation.
An evidence-only descendant may publish these files. The checker rejects changes outside this evidence directory, its checker, and the CI input inventory.
