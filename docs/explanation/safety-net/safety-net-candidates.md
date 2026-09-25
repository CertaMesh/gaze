# Safety-net candidates beyond OPF

**Status: research record, 2026-09-25. Nothing here is measured in this
repository and nothing here changes shipped behaviour.** It records which
models are worth testing as a lighter replacement for the OpenAI Privacy Filter
(OPF) safety net, the order to test them in, and the bars they must clear. Model
facts below come from upstream model cards and papers (research cut-off
2026-09-24); a fact marked *unverified* could not be established from a primary
source and must be measured, not assumed.

## The problem

Figures from the v2-contract runs on the 2,910-document population (see
[Safety nets → Measured](safety-nets.md#measured)):

| Stack | Leaked PII bytes | Cost |
|---|---:|---|
| rules + NER + Nym-small (setup default) | ~11.3 % | p50 89.8 ms, peak 1,027 MB |
| rules + NER + OPF | ~3.3 % | ~3.0 s model time per document, ~4 GB process |
| rules + NER + Nym-small + OPF | ~2.4 % | both of the above |

OPF closes most of the gap but costs 30× the latency and 4× the memory. The
bytes Nym-small misses are mostly structured values: tax IDs, national IDs,
cards, IBANs, driver licences, DOBs, postcodes, phones, ages, building numbers.
Names are already handled well by rules plus NER.

## Decision

**Replace OPF; do not optimize it as the main line of work.** Distil it only
if every replacement below misses the bars.

- A persistent OPF worker removes process start-up but not the ~3.0 s warm
  model time. OPF is 1.5B total parameters with ~50M active per token (sparse
  MoE, 128 experts, top-4); sparse activation cuts arithmetic, not the stored
  weights (2.8 GB BF16 safetensors).
- OpenAI already publishes ONNX, including q4 (~917 MB) and q4f16 (~809 MB).
  The smaller file alone uses most of the 1 GB process budget, and its PII
  accuracy after quantization, its peak RSS, and whether Gaze's `ort` version
  runs it are all *unverified*. Run it once as a control, nothing more.
- Distilling OPF into a 150–300M multilingual encoder is the fallback. The
  teacher pass alone is roughly 670–1,700 serial CPU-hours at 3.0 s per
  document for 0.8–2.0M documents (an engineering estimate, not a published
  figure), and whether OPF-generated labels may be redistributed is
  *unverified*.

## Experiment queue

Run in this order. Each step can end the queue.

| # | Candidate | Why | First measurement | Kill condition |
|---|---|---|---|---|
| 1 | [`ai4privacy/llama-ai4privacy-multilingual-categorical-anonymiser-openpii`](https://huggingface.co/ai4privacy/llama-ai4privacy-multilingual-categorical-anonymiser-openpii), official `model_int8.onnx` (151 MB) | ModernBERT-base BIO classifier; its 20 classes include `AGE`, `BUILDINGNUM`, `CREDITCARDNUMBER`, `DATE`, `DRIVERLICENSENUM`, `IDCARDNUM`, `SOCIALNUM`, `TAXNUM`, `TELEPHONENUM`, `ZIPCODE`; de/en/fr/nl among 8 languages; 8,192 positions | contamination check (below), then total RSS with Davlan loaded, then leaked bytes per residual class | licence conflict unresolved (below); IBAN and national-format variants still leak; its self-reported 95.76 % recall is in-distribution |
| 2 | [`Wismut/nym-pii-multilingual`](https://huggingface.co/Wismut/nym-pii-multilingual) (base, v3, `int8/` 359 MB) | same 40-type / 81-label family as the shipped Nym-small; reuses the existing decoder, class table and 512/64 windowing | peak RSS **before** accuracy | over 1 GB, or larger but not materially better: published F1 gap to small is only 76.4 → 79.1 real-text, 67.7 → 69.8 Ai4Privacy OOD |
| 3 | [`fastino/gliner2-privacy-filter-PII-multi`](https://huggingface.co/fastino/gliner2-privacy-filter-PII-multi) | span model queried with only the residual labels (`tax_id`, `national_id_number`, `iban`, `card_number`, `date_of_birth`, `drivers_license_number`, `postal_code`, `phone_number`, `account_number`, …); beats OPF on SPY exact-span recall (legal 0.722 vs 0.640, medical 0.681 vs 0.671, paper Table 2) | exact-span accuracy on the Gaze population in its reference runtime, **before** any export work | no portable ONNX graph (no official export exists); SPY precision ~0.35 blows the false-positive bar; no age or building-number label |
| — | OPF q4f16 ONNX | control for "cheap OPF" | `ort` load, byte-level parity with the reference checkpoint, p50, RSS | anything over the bars ends the OPF-optimization line |

Rejected for the first pass: `urchade/gliner_multi_pii-v1` (1.16 GB checkpoint,
SPY recall ~0.31), `nvidia/gliner-PII` (570M parameters, GPU-only published
evaluation, licence *unverified*), Piiranha (`cc-by-nc-nd-4.0` weights, 256-token
context), `OpenMed/privacy-filter-multilingual-v2` (a 1B OPF fine-tune: the same
cost problem), Presidio (a framework, not a model).

## Constraints the upstream numbers do not capture

These come from how Gaze runs a net, and every candidate inherits them.

- **The net reads tokenized text.** Pass 3 scans output that already contains
  Gaze tokens. Nym-small reads token text such as `Custom:building_number` as
  a building number; masking tokens before inference cost 18 % of bought bytes
  (todo 3681). Upstream F1 on raw text says nothing about this; measure every
  candidate through `clean_for_bench`, not on raw corpus text.
- **The memory bar is already spent.** rules + NER + Nym-small peaks at
  1,027 MB. A candidate replaces Nym-small in the default stack or it does not
  fit; `rules + NER + Nym-small + candidate` is a diagnostic arm only.
- **"int8" in a file name is not int8 compute.** The shipped Nym-small artifact
  is int8 embeddings, fp16 body, fp32 compute, with no int8-kernel speedup.
  Check each candidate's actual quantization before extrapolating latency from
  file size, and check fp32-versus-int8 span parity before trusting the int8
  file's accuracy.
- **Identifier labels have failed on precision before.** Nym-small's `TAX_ID`
  stayed off at 0.23–0.26 precision at every threshold, and `ZIP_CODE` stayed
  off because it flagged the invalid-identifier decoys in the A4 negative
  corpus. Candidate `TAXNUM` / `ZIPCODE` / `IDCARDNUM` labels get the same
  per-label precision check, and a class that fails stays off.
- **Thresholds and allowlists are never tuned on the holdout.** The Kiji EN/DE
  split is evaluation-only ([benchmarks](../../reference/benchmarks/README.md#primary-corpus--englishgerman-synthetic-holdout)).
  Per-label thresholds come from a separate development set and are frozen
  before the 2,910-document run.
- **`DATE` is not `DATE_OF_BIRTH`.** A general date label maps to
  `custom:date` only under the same contextual discipline Nym's
  `DATE_OF_BIRTH >= 0.9` gets today; it is never a DOB claim. IBAN stays with
  the deterministic recognizer for any candidate without an IBAN label.
- **Every label needs an explicit Gaze class or never fires**, as for Nym: no
  folding into a generic class, and the audit row names label and threshold.

## Contamination

Scores are reported twice: operational (all 2,910 documents) and
contamination-clean per candidate. Only the clean score supports a
generalization claim.

- Kiji's upstream lineage is *unverified*. Before step 1 counts, check the Kiji
  EN/DE rows against Ai4Privacy Open-PII-Masking-500k (the candidate's training
  set) for exact and near-duplicate text.
- Ai4Privacy's own models, Piiranha and OpenMed v2 trained on Ai4Privacy
  families; no Ai4Privacy set is out-of-distribution for them. NVIDIA used
  Ai4Privacy as an evaluation set, so it is exposed, not trained on.
- GLiNER2-PII declares its own 4,910-example synthetic training set. OPF's
  public training sources are not fully enumerated; its overlap is *unverified*.

## Pass bars

Frozen before the run; not moved on the test set.

| Metric | Bar |
|---|---:|
| Leaked PII bytes, same population and contract | ≤ Nym-small (~11.3 %) |
| False-positive bytes | ≤ Nym-small baseline + 10 % |
| Warm end-to-end p50, ~1 KB documents, quiet host | ≤ 150 ms |
| Whole-process peak RSS | ≤ 1.0 GB |
| Exact restore, manifest validity | unchanged from the Nym arm |
| Stretch | ≤ 3.3 % leaked (OPF territory) |

Report leaked bytes per residual class and per locale (DE-DE/AT/CH,
EN-US/GB/IE/CA/AU/NZ), not only the aggregate. Latency comes from a separate
quiet-host run (`nym-warm-latency.py` pattern: pre-load, fixed warm-up, fixed
`ort` thread count, model-only and end-to-end p50/p95, cold load recorded
separately), never from timings collected during the accuracy pass.

## Licence gates

A candidate may be benchmarked before these are resolved; it may not become a
`gaze setup` option until they are.

- **Ai4Privacy:** weights say MIT; the training dataset's metadata says
  `cc-by-4.0` while its card says use and derivatives are subject to the Llama
  Community License. The contradiction must be resolved with the publisher.
- **Nym base:** the same open review as Nym-small ([Licence review](safety-nets.md#licence-review-open)):
  MIT weights, CC-BY-SA Wikipedia text auto-labelled by an LLM.
- **GLiNER2-PII:** Apache-2.0 weights; the synthetic training set's licence is
  *unverified*.
- **An OPF-distilled student:** OPF code and weights are Apache-2.0; no grant
  covering inference-generated labels was found, and the source text's own
  licence still applies.

## Cheapest first screen

`scripts/bench/onnx-token-classification-runner.py` already loads an ONNX
token classifier with `tokenizer.json` and `config.json` `id2label`, so the
Ai4Privacy INT8 file can be screened as a pass-2 leaderboard entry in
`crates/gaze-recognizers/benches/ner_models.toml` without Rust changes. That
screens raw detection quality only; it is not a safety-net measurement, because
pass 2 sees raw text and pass 3 sees tokenized text. A candidate that passes
the screen still needs a Pass-3 backend and the `full-stack-*-resolve` arm.
