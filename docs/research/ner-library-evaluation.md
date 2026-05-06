# NER Runtime Evaluation — Gaze v0.2 Phase 0

**Date:** 2026-04-15
**Purpose:** Resolve Phase 0 decision gate in `2026-04-14-gaze-v02-core-engine-design.md` — pick the NER runtime backing for `NerDetector`, replacing the `branch="main"`-pinned `worka-ai/pii` dependency.

---

## Selection Criteria (derived from Gaze constraints)

1. **Byte-span accuracy** under UTF-8 — `Detector::detect` contract returns `Range<usize>` byte spans. Char-offset libraries need a conversion layer and are a source of off-by-one bugs on German text (umlauts, ß, emoji).
2. **German NER quality** — primary market. GermEval 2014 highlights German-specific challenges: noun capitalization, compounding, derived adjectives, nested entities.
3. **On-prem reproducibility** — model artifacts must be locally available and versioned. Runtime downloads (e.g., `hf-hub`) are a compliance hazard unless locked to pinned local artifacts.
4. **Embeddable** — single-binary preference; mlock-friendly memory hygiene; no Python runtime in the hot path.

## Shortlist Matrix

| Candidate | Mode | German NER | Entities | Health | License | Verdict |
|---|---|---|---|---|---|---|
| **`ort` + `tokenizers` + pinned ONNX** | Rust crates | Yes (model-dependent) | Transformer NER with direct span reconstruction | Mature runtimes, active releases | Apache-2.0 / MIT | **Chosen v0.2 backing** |
| worka-ai/pii | Rust crate | Partial (Candle NER feature) | Regex/validator/dict + NER | Young (4 commits, 6 stars, no releases) | Apache-2.0 / MIT | Keep only if German eval improves fast |
| arclabs561/anno | Rust crate | Model-dependent | NER + coref + patterns | Active (v0.4, Mar 2026) | MIT/Apache-2.0 | **Disqualified**: emits char offsets (Unicode scalars), not bytes |
| DataDog/dd-sensitive-data-scanner | Rust crate | No | Regex + keywords + validators/checksums | Vendor-maintained | Apache-2.0 | Excellent structured-PII layer; NOT NER |
| rust-bert | Rust crate (libtorch) | Yes (model-dependent) | Transformer pipelines | Mature, 3k stars, widely used | Apache-2.0 | Good quality but libtorch distribution kills "single binary" |
| HF Candle + candle-transformers | Rust crate | Yes (model-dependent) | Inference framework | Very active, 19k stars | Apache-2.0/MIT | Full control, but you build the NER head + spans yourself |
| pykeio/ort (ONNX Runtime bindings) | Rust crate | Yes (model-dependent) | Inference substrate | Mature, 1.9k stars, active releases | Apache-2.0/MIT | Strong deterministic deployment |
| mozilla-ai/encoderfile | Rust tool / sidecar | Yes (model-dependent) | Single-binary encoder packaging | Rapid release cadence | Apache-2.0 | **Best on-prem single-binary model distribution** |
| Microsoft Presidio | Python (subprocess/PyO3) | Partial (backend-dependent) | Rule-based + NLP recognizers | 6.5k stars, mature | MIT | Industry baseline; heavy ops |
| spaCy | Python | Yes (de_core_news_lg) | Full NLP | 33k stars | MIT | Strong German; Python tax |
| Flair | Python | Yes (German models) | NER+more | 14k stars | MIT-ish | Benchmark baseline only |
| Apache OpenNLP | Java | Partial | Classic NER | ASF long-lived | Apache-2.0 | Baseline only; transformer-era obsolete |

## Top 3 Recommendations

### (a) Chosen backing
**Direct `ort` + `tokenizers` integration with a pinned ONNX export.** This keeps the runtime narrow, avoids taking a larger framework dependency just to reach ONNX inference, and lets Gaze own byte-span reconstruction directly. The chosen default model is **`Davlan/bert-base-multilingual-cased-ner-hrl`** exported to ONNX and mounted as a pinned local artifact.

### (b) Default model — bilingual German + English

Gaze serves both German (primary market) and English (broad relevance). Three model-loading strategies on top of `ort` + `tokenizers`:

1. **Multilingual single model** — one artifact, one inference, covers both.
   - **`Davlan/bert-base-multilingual-cased-ner-hrl`** — mBERT fine-tuned on 10 high-resource languages incl. German + English, CoNLL schema (PER/LOC/ORG/MISC). **Recommended v0.2 default.**
   - Alternative: an XLM-RoBERTa-based multilingual NER fine-tune.
2. **Stacked language-specific detectors** — two `NerDetector` instances in the pipeline:
   - `dslim/bert-base-NER` (English)
   - `FacebookAI/xlm-roberta-large-finetuned-conll03-german` (German)
   - Span-conflict resolution picks longest / first-on-tie; losers logged for QA.
   - Cost: 2× inference. Benefit: best-in-class per language.
3. **Per-request language routing** — `whatlang` (cheap) upfront, dispatch to one model.
   - Single inference cost, best per-language quality. Adds a language-detection pre-pass.

**v0.2 decision:** start with **(1)** — `Davlan/bert-base-multilingual-cased-ner-hrl`. Simplest ops, covers both markets. Upgrade to **(2)** or **(3)** when eval shows recall dropping below threshold on either language. Both upgrade paths are additive — no pipeline architecture change.

⚠️ "High F1 on GermEval 2014 / CoNLL-2003" ≠ "good PII detection on customer-support logs." Public benchmarks are Wikipedia/news; deployment text is messier. Evaluate on our own bilingual corpus.

### (c) Long-term roadmap
**Mozilla encoderfile for single-binary ONNX + tokenizer + config packaging, run as local sidecar.** Apache-2.0, zero-runtime-dependency goal, explicit stdin/stdout/MCP sidecar shape. Keeps Gaze core pure-Rust with strict memory rules; isolates the model/runtime packaging problem into a separately sandboxable artifact.

## Migration Effort (vs current worka-based NerDetector shape)

- **Direct `ort` + `tokenizers`**: Medium. Own token→span reconstruction (subword merging, overlap handling). Export + pin the multilingual model. No runtime downloads.
- **Encoderfile sidecar**: Medium-high but *isolated*. Define stable IPC (stdin/stdout JSON lines). Sidecar stateless + deterministic. Two artifacts shipped, but both "single file".

## Gaps No Library Covers

General NER focuses on PER/ORG/LOC (CoNLL 4-class). **Tenant-specific identifiers** (custom entity types specific to your application) are not NER output. Handle with dedicated `IndexDetector` / pattern-dictionary detectors per Gaze's existing design.

**Recommended hybrid layering:**
1. Pre-filter — Aho-Corasick / RegexSet for likely PII triggers.
2. Exact match — checksum/validator confirm (Luhn, IBAN mod-97, UUID format) — near-100% precision.
3. Contextual confirm — transformer NER *only* on suspicious regions or trigger-bearing text. Keeps throughput high without losing recall.

## Anti-patterns

- **Framework ≠ detector** — `candle`/`ort` don't give taxonomy, overlap resolution, or byte-span correctness out of the box. Budget engineering time.
- **Char-offset libraries masquerading as PII detectors** — `anno` emits Unicode-scalar offsets. Incompatible with our `Range<usize>` byte-span contract without conversion.
- **Runtime model downloads without pinning** — worka-ai/pii's `PII_CANDLE_MODEL_ID + hf-hub` path is a compliance hazard. Lock to local, versioned artifacts.
- **License ambiguity** — enforce Apache-2.0 / MIT / dual in critical deps. Verify repo-root `LICENSE`, not just crate metadata.
- **Dependency audit debt** — modern Rust pulls large transitive graphs; institutionalize `cargo-vet` / `cargo-deny` for GDPR-critical infra.

## Benchmarking Plan

**Public baselines (reference only, not primary eval target):**
- GermEval 2014 — German-specific phenomena (compounding, derivation, nesting); 4 main classes + subclasses. Most relevant public baseline.
- CoNLL-2003 German — classic PER/ORG/LOC/MISC schema.
- `ai4privacy/open-pii-masking-500k-ai4privacy` — augmentation corpus for LLM-assistant masking scenarios.

**Real-data methodology:**
1. Build a gold dataset from actual German customer-support + log payloads (with permission, minimized exposure). Annotate the categories we care about, *including business identifiers*.
2. Measure **at the span level, not token level** — off-by-one kills restore.
3. Stress-test UTF-8: umlauts, ß, emoji, mixed-language inserts, zero-width chars (Unicode normalization should be active per pipeline pre-pass).
4. Separate structured PII (validator-based, should be ~100% precision) from NER PII (will never be 100%) so error budgets don't conflate.

**Synthetic red-team generation:**
- Target categories where real positives are rare: IBAN, VIN, IMEI, UUID.
- For NER: generate German sentences exercising compounding + derived forms (GermEval-style).

## Pinned artifacts

> Status: Phase 2 scaffolding — placeholder hashes. Replace every `TODO`
> value below before the first real production run and flip this status
> to "signed off".

The runtime `NerDetector` consumes a pinned local artifact set. No network
at runtime. Artifacts are produced by `scripts/fetch-ner-model.sh` against
the pinned HF commit and installed at the runtime model directory.
See `scripts/fetch-ner-model.sh` for the current pinned checksums.

| Field | Value |
|---|---|
| HF repo | `Davlan/bert-base-multilingual-cased-ner-hrl` |
| Pinned HF commit SHA | `__PINNED_COMMIT_SHA_TODO__` (fill before first sign-off run) |
| Default runtime model dir | `${XDG_DATA_HOME:-~/.local/share}/gaze/models/davlan-mbert-ner-hrl/` |
| Operator override | `[ner] model_dir = "..."` in `policy.toml` |

### Expected files

Every file is required; absence is fail-closed:

| File | Purpose |
|---|---|
| `model.onnx` | Token-classification head export. |
| `tokenizer.json` | HuggingFace `tokenizers` serialized form; used for byte-offset reconstruction. |
| `config.json` | Must carry `id2label` (stringified indices → CoNLL labels). May also carry `backend` so the same `NerDetector` load path can switch driver without pipeline code changes; omitted defaults to `ort`. |
| `labels.json` | Gaze-authored CoNLL-label → `PiiClass` map. `"drop"` is the skip sentinel. |
| `SHA256SUMS` | `shasum -a 256` output covering all four files above. |

### Expected SHA-256 hashes (placeholders)

These values are **placeholders**. Replace with the hashes produced by the
first sign-off run of `scripts/fetch-ner-model.sh`.

```
0000000000000000000000000000000000000000000000000000000000000000  model.onnx
1111111111111111111111111111111111111111111111111111111111111111  tokenizer.json
2222222222222222222222222222222222222222222222222222222222222222  config.json
3333333333333333333333333333333333333333333333333333333333333333  labels.json
```

### Follow-ups

- TODO: run `scripts/fetch-ner-model.sh` once against the pinned commit,
  copy the resulting `SHA256SUMS` into this section and into
  `crates/gaze/testdata/ner/SHA256SUMS.example` (shape only; placeholder
  hashes remain there for unit-test fixtures).
- TODO: open a follow-up issue tracking the pinned-artifact data protection
  review (model-license + weight provenance).
- TODO: decide whether `MISC` stays as `drop` by default or is opted into
  a new `PiiClass` for deployments where Wikipedia-entity MISC recall is
  useful. Currently the label map ships `MISC → drop`.

### Running ignored / real-model tests

```
scripts/fetch-ner-model.sh
export GAZE_NER_MODEL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl"
cargo test -p gaze -- --ignored ner_span_correctness
```

CI does NOT run these tests; operator invokes them when pinning a new
artifact.

## Decision Ask

Spec Phase 0 gate: **adopt direct `ort` + `tokenizers` integration as the v0.2 `NerDetector` backing, with `Davlan/bert-base-multilingual-cased-ner-hrl` (mBERT, 10 langs incl. DE+EN) exported to ONNX and mounted as a pinned local artifact.** Upgrade path to stacked DE+EN models or language-routed dispatch is additive. Defer encoderfile-sidecar packaging to v0.3+.

Drop `worka-ai/pii`. If keeping it short-term as a fallback, fork + pin SHA.
