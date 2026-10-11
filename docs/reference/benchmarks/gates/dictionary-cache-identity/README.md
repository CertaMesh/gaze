# Dictionary cache identity gain gate

## Benchmark evidence

Fresh base: `65f3d6aa599c235231b6c0c21ea46f77c38773c4` (origin/main with generator v16 and primary-contract attachment).
Candidate runtime: `9f87f1996c4f1b1aa15b0918b036e2bc5b0a1638`.
Both measured trees were clean. Signed DCO main integration retains all CHANGELOG entries.

Each side generated its own `gaze setup` policy and ran fresh `full --seed 20260710 --no-download --no-cache --release` observations separately under contracts v1 and v2 on the same corpus and machine. The independently captured ordinary v1/v2 observations and switch/fresh/literal dictionary records were replayed under v1 and v2; v4 replays the fresh v2 observation set. C uses the primary contract; A/D/R use the committed agentic contract. The dictionary extension uses the primary contract through the merged `dictionary_sequences.attach` path, then the committed `agentic_layers.py gate` command evaluates the combined scorecards. Ordinary C/A/D/R uses each side's setup policy; dictionary arms exercise the public library sequence.

Machine: MacBook Pro, Apple M5 Max, 18 cores, 64 GB, macOS 26.5 (25F71). Separate checkout-owned targets, locked Python 3.13 and full Rust 1.96.0 toolchain. Correctness only; no timing claim.

Evidence: `docs/reference/benchmarks/gates/dictionary-cache-identity/` retains combined base/candidate v1/v2/v4 scorecards, fresh ordinary observations, six-arm dictionary captures, canonical gate reports, reproduction script, collision proof and harness identity. This replaces the superseded neutral evidence at `f6712fee2ea3becdea6424b781a74190889ee253` and the blocked attachment evidence at `91edd160d1197d568e1b96b9a17240c132d64b89`; both remain in Git history. No corpus, scorer, generator, adapter or release-headline changes in this PR.

Every cell is base → candidate. The gate uses raw false-positive bytes; the adjacent column reports the gold-gap-adjusted diagnostic. Refusals, exact restores and valid manifests are reported for every layer and arm.

| Contract | Layer/arm | Refused | All-gold leaked bytes | Gated leaked bytes | Raw FP bytes | FP after gold-gap credit | Exact restores | Valid manifests |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4 | C | 0 → 0 | 9,813 → 9,813 | 9,813 → 9,813 | 26,890 → 26,890 | 15,112 → 15,112 | 2,910 → 2,910 | 2,910 → 2,910 |
| 4 | A | 0 → 0 | 13,825 → 13,825 | 13,433 → 13,433 | 1,915 → 1,915 | 1,915 → 1,915 | 3,728 → 3,728 | 3,728 → 3,728 |
| 4 | D | 0 → 0 | 0 → 0 | 0 → 0 | 6,892 → 6,892 | 6,892 → 6,892 | 2,144 → 2,144 | 2,144 → 2,144 |
| 4 | R | 0 → 0 | 9,806 → 9,806 | 9,806 → 9,806 | 479 → 479 | 479 → 479 | 280 → 280 | 280 → 280 |
| 2 | C | 0 → 0 | 6,748 → 6,748 | 6,748 → 6,748 | 26,873 → 26,873 | 26,873 → 26,873 | 2,910 → 2,910 | 2,910 → 2,910 |
| 2 | A | 0 → 0 | 13,825 → 13,825 | 13,433 → 13,433 | 1,915 → 1,915 | 1,915 → 1,915 | 3,728 → 3,728 | 3,728 → 3,728 |
| 2 | D | 0 → 0 | 0 → 0 | 0 → 0 | 6,892 → 6,892 | 6,892 → 6,892 | 2,144 → 2,144 | 2,144 → 2,144 |
| 2 | R | 0 → 0 | 9,806 → 9,806 | 9,806 → 9,806 | 479 → 479 | 479 → 479 | 280 → 280 | 280 → 280 |
| 1 | C | 0 → 0 | 9,813 → 9,813 | 9,813 → 9,813 | 26,890 → 26,890 | 26,890 → 26,890 | 2,910 → 2,910 | 2,910 → 2,910 |
| 1 | A | 0 → 0 | 13,825 → 13,825 | 13,433 → 13,433 | 1,915 → 1,915 | 1,915 → 1,915 | 3,728 → 3,728 | 3,728 → 3,728 |
| 1 | D | 0 → 0 | 0 → 0 | 0 → 0 | 6,892 → 6,892 | 6,892 → 6,892 | 2,144 → 2,144 | 2,144 → 2,144 |
| 1 | R | 0 → 0 | 9,806 → 9,806 | 9,806 → 9,806 | 479 → 479 | 479 → 479 | 280 → 280 | 280 → 280 |

Dictionary extension (K positives, B benign counterweights):

| Contract | Layer/arm | Refused | All-gold leaked bytes | Gated leaked bytes | Raw FP bytes | FP after gold-gap credit | Exact restores | Valid manifests |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4 | switch/K | 0 → 0 | 35,856 → 0 | 35,856 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 4 | switch/B | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 4 | fresh/K | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 4 | fresh/B | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 4 | literal/K | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 4 | literal/B | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 2 | switch/K | 0 → 0 | 35,856 → 0 | 35,856 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 2 | switch/B | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 2 | fresh/K | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 2 | fresh/B | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 2 | literal/K | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 2 | literal/B | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 1 | switch/K | 0 → 0 | 35,856 → 0 | 35,856 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 1 | switch/B | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 1 | fresh/K | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 1 | fresh/B | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 1 | literal/K | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |
| 1 | literal/B | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | 324 → 324 | 324 → 324 |

**Combined gain gate PASS under v1, v2 and v4.** Switch/K leaked bytes fall 35,856 → 0 under each contract; ordinary C/A/D/R and all five remaining dictionary arms are unchanged. No layer's gated or all-gold leak, false-positive bytes or refusals rise. Exact restore and valid-manifest document counts are unchanged; no restore failures or invalid manifests. All extension FP and refusal counts are zero. The separate ordinary release-readiness status is failed on both sides with complete observations; this change-specific gain pass does not claim release readiness.

Reproduce with `uv run --project scripts/bench python docs/reference/benchmarks/gates/dictionary-cache-identity/replay.py --output target/dictionary-cache-replay`. The script rescores both retained observation sets, attaches each primary-contract dictionary extension and invokes the committed gate command, requiring exit 0 and PASS under all three contracts. `sha256.json` pins retained inputs and results. Capture manifests pin generator, adapter, binary, compiler and runtime identity. The collision proof is retained from the prior runtime-identical head.
