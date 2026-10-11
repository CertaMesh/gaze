# Dictionary cache identity gain gate

## Benchmark evidence

Fresh base: `816f4be632c459ee9eb51281554b297681752c01` (main with the merged #790 harness).
Candidate runtime: `ddbf050ca7779d472c5443c93380033cf53bd290`.
Both measured trees were clean. Signed DCO merges retain all CHANGELOG entries.

Each side generated its own setup policy and ran fresh `full --seed 20260710 --no-download --no-cache --release` observations on the same corpus and machine. The same frozen ordinary and switch/fresh/literal dictionary K/B records were replayed under v1, v2 and v4. As in the canonical full runner, C uses the selected primary contract and A/D/R retain the committed agentic contract. Standalone dictionary replay uses each primary contract and is retained as `base-dictionary-v*.json.gz` / `candidate-dictionary-v*.json.gz`; these are not valid attached combined scorecards. Ordinary C/A/D/R uses each side's setup policy; dictionary arms isolate the public library sequence and do not claim setup-policy recall gains.

Machine: MacBook Pro, Apple M5 Max, 18 cores, 64 GB, macOS 26.5 (25F71). Separate checkout-owned targets, locked Python 3.13 and full Rust 1.96.0 toolchain. Correctness only; no timing claim.

Evidence: `docs/reference/benchmarks/gates/dictionary-cache-identity/` contains `base-v1.json.gz`, `candidate-v1.json.gz`, the v2/v4 counterparts, both ordinary observation records, both six-arm dictionary captures, ordinary gate reports and attachment errors, collision proof and harness identity. This replaces the superseded neutral evidence at `f6712fee2ea3becdea6424b781a74190889ee253`, which remains available in Git history. No corpus, scorer, generator, adapter or release-headline changes.

Every cell is base → candidate. All-gold and gated leaked bytes use the canonical gate fields; The gate uses raw FP; the adjacent column reports the gold-gap-adjusted diagnostic (equal to raw where no credit applies). Refusals, restores and manifest counts are included for every layer and arm.

| Contract | Layer/arm | Refused | All-gold leaked bytes | Gated leaked bytes | Raw FP bytes | FP after gold-gap credit | Exact restores | Valid manifests |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4 | C | 0 → 0 | 9,813 → 9,813 | 9,813 → 9,813 | 26,890 → 26,890 | 15,112 → 15,112 | 2,910 → 2,910 | 2,910 → 2,910 |
| 4 | A | 0 → 0 | 13,825 → 13,825 | 13,433 → 13,433 | 1,915 → 1,915 | 1,915 → 1,915 | 3,728 → 3,728 | 3,728 → 3,728 |
| 4 | D | 0 → 0 | 0 → 0 | 0 → 0 | 6,874 → 6,874 | 6,874 → 6,874 | 2,144 → 2,144 | 2,144 → 2,144 |
| 4 | R | 0 → 0 | 9,806 → 9,806 | 9,806 → 9,806 | 479 → 479 | 479 → 479 | 280 → 280 | 280 → 280 |
| 2 | C | 0 → 0 | 6,748 → 6,748 | 6,748 → 6,748 | 26,873 → 26,873 | 26,873 → 26,873 | 2,910 → 2,910 | 2,910 → 2,910 |
| 2 | A | 0 → 0 | 13,825 → 13,825 | 13,433 → 13,433 | 1,915 → 1,915 | 1,915 → 1,915 | 3,728 → 3,728 | 3,728 → 3,728 |
| 2 | D | 0 → 0 | 0 → 0 | 0 → 0 | 6,874 → 6,874 | 6,874 → 6,874 | 2,144 → 2,144 | 2,144 → 2,144 |
| 2 | R | 0 → 0 | 9,806 → 9,806 | 9,806 → 9,806 | 479 → 479 | 479 → 479 | 280 → 280 | 280 → 280 |
| 1 | C | 0 → 0 | 9,813 → 9,813 | 9,813 → 9,813 | 26,890 → 26,890 | 26,890 → 26,890 | 2,910 → 2,910 | 2,910 → 2,910 |
| 1 | A | 0 → 0 | 13,825 → 13,825 | 13,433 → 13,433 | 1,915 → 1,915 | 1,915 → 1,915 | 3,728 → 3,728 | 3,728 → 3,728 |
| 1 | D | 0 → 0 | 0 → 0 | 0 → 0 | 6,874 → 6,874 | 6,874 → 6,874 | 2,144 → 2,144 | 2,144 → 2,144 |
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

**Combined gain gate BLOCKED under v1, v2 and v4.** Standalone switch/K leaked bytes fall 35,856 → 0 under each contract. The merged #790 harness checks the extension contract against `layers.scored_label_contract`, although its gold uses `FIRSTNAME` from the primary contracts. Primary replay fails attachment with `extension contract differs from ordinary layers`; the agentic contract instead fails closed because it does not rule on `FIRSTNAME`. No harness, contract, metadata or scorecard binding was changed to bypass this. The ordinary-only canonical gate remains neutral/fail because it cannot see the API transition. A scoped harness repair is required before a combined gain pass can be claimed. No ordinary layer leak, FP, refusal, restore or manifest count regresses. All four controls remain unchanged; all extension FP and refusal counts are zero. Across each side, all 9,062 ordinary and 1,944 extension documents restore exactly and have valid manifests; zero restore failures or invalid manifests. The separate ordinary release-readiness status is failed on both sides, with complete observations; each extension capture exits 0.


Reproduction: use the committed `scorecard_record.py` to replay each ordinary record, `dictionary_sequences.py replay --capture` with the same contract and ordinary scorecard to attach each extension, then `agentic_layers.py gate --base --candidate` for the ordinary pair. The merged harness currently rejects attaching the standalone dictionary replay; retained wrapper reports record that blocker and the actual ordinary gate result. `sha256.json` pins all retained inputs and results. Capture manifests pin the generator, adapter, binary, toolchain and runtime identity.

Exact replay from the repository root:

```bash
PYTHONDONTWRITEBYTECODE=1 uv run --project scripts/bench python \
  docs/reference/benchmarks/gates/dictionary-cache-identity/replay.py \
  --output target/dictionary-cache-gate-replay
```

The replay succeeds when it reproduces the blocker and writes the measured ordinary gate and standalone dictionary counts. It never bypasses the contract check or declares a combined gain pass.
