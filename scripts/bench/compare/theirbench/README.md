# Gaze on competitors' own benchmarks

These scripts score Gaze and every compared tool on the benchmarks that competitors publish their own numbers on. The results are a generated block in [the benchmark README](../../../../docs/reference/benchmarks/README.md#competitors-own-benchmarks).

**Report-only.** Nobody reads these sets to design or tune a Gaze rule. The repository holds only aggregate numbers; document text, spans and per-document predictions never enter it.

## Sets

| Set | Source | Licence | Used as |
| --- | --- | --- | --- |
| Presidio Research | `microsoft/presidio-research` at `6db3769a`, `data/synth_dataset_v2.json`, 1,500 documents | MIT code; Fake Name Generator identities CC-BY-SA-3.0-US | whole set |
| PIIBench-commercial | `pritesh-2711/pii-bench` at `f96e1364`, its own consolidation and split code, run on four sources only | Apache-2.0 code; sources are Apache-2.0 (Gretel finance), CC-BY-4.0 (Nemotron-PII) and CC-BY-SA-4.0 (Few-NERD, FiNER-139) | the 5,000-record `test_5k` subset |
| PII-TRACE public subset | `perplexity-ai/PII-TRACE` at `1c3eb67b`, `data/train.parquet`, 500 English conversations | MIT | whole set, one document per message (4,500) |

PII-TRACE is Perplexity's benchmark for PII-Tracer. Only this 500-conversation subset is public; the paper's 13,148-conversation, 13-language set and its 1,922-document test split are not, so no figure from the paper can be reproduced. Perplexity publishes no scorer either: `pii_trace_repro.py` implements the dataset card's exact typed and untyped span P/R/F1 and the paper's label-agnostic character P/R/F1 from their definitions. All 2,653 gold spans are in user messages; assistant messages have none, so a detection there is a false positive. The paper says PII-Tracer's training data shares production traffic with PII-TRACE and the subset carries no split label, so PII-Tracer's row is an upper bound rather than a clean holdout. On this set the vendor bar is PII-Tracer itself. Its native labels are the dataset's labels, so `label-maps.json` `native_gold` maps them by identity; every other tool goes through the canonical labels, which cannot reach `other_pii`.

Not run, because of their licences:

- The other six PIIBench sources. ai4privacy 400k and 300k carry a custom licence under which commercial users must license. MultiNERD is CC-BY-NC-SA-4.0, CoNLL-2003 is non-commercial research only, and Isotonic 200k is CC-BY-NC-4.0 on its dataset card. WikiANN's card lists its licence as unknown.
- ai4privacy PII-Masking-300k, the set OpenAI Privacy Filter publishes on. Its custom licence requires commercial users to license it.

PIIBench's published Presidio F1 of 0.1385 comes from all ten sources. It is quoted here, not reproduced.

## Disclosed differences

- PIIBench's current code keeps 71 label types, where its paper reports 48. Its normaliser maps only ai4privacy-style names, so labels from Nemotron-PII and Gretel pass through unchanged. `MISC` (Few-NERD's art, event and product mentions) and `FINANCIAL_ENTITY` (FiNER-139's XBRL tags) are gold labels. No compared tool claims either, so both drop out of the common-intersection view.
- Gretel finance is also one of the sealed report-only sets in the business repo. Neither use informs rule design.
- Every gold label counts; no scored-label contract applies. Leaked and false-positive bytes do not depend on labels. [`label-maps.json`](label-maps.json) drives only the typed metrics and the common-intersection view.

## Reproduce

The vendors' own evaluators run in one environment, pinned by [`requirements-theirbench.lock`](requirements-theirbench.lock). The Gaze and competitor rows run in the comparison environment from [`../requirements.lock`](../requirements.lock).

```bash
# 1. Vendor evaluators: one environment, one checkout of each benchmark repo.
uv venv -p 3.12 "$VENDOR_ENV" && VIRTUAL_ENV="$VENDOR_ENV" uv pip sync scripts/bench/compare/theirbench/requirements-theirbench.lock
git clone https://github.com/microsoft/presidio-research "$PR_CHECKOUT" && git -C "$PR_CHECKOUT" checkout 6db3769a3388b4075b93ab2229c5e0b9c30137f7
git clone https://github.com/pritesh-2711/pii-bench "$PB_CHECKOUT" && git -C "$PB_CHECKOUT" checkout f96e13645f217c507d0cafa12340b99a26c2d32c

# 2. Build PIIBench-commercial (downloads the four sources at pinned revisions).
"$VENDOR_ENV/bin/python" scripts/bench/compare/theirbench/piibench_commercial.py \
  --checkout "$PB_CHECKOUT" --output-dir "$PB_DATA"

# 3. Reproduce the vendors' numbers first, with their own tools. The published
#    Presidio Research numbers predate later evaluator fixes, so run-theirbench.sh also
#    reproduces each with the evaluator commit that produced it
#    (REPRODUCTION_COMMITS in presidio_research_repro.py).
"$VENDOR_ENV/bin/python" scripts/bench/compare/theirbench/presidio_research_repro.py \
  --checkout "$PR_CHECKOUT" --openmed-model "$OPENMED_SNAPSHOT" --output "$OUT/presidio-research-repro.json"
"$VENDOR_ENV/bin/python" scripts/bench/compare/theirbench/piibench_repro.py \
  --checkout "$PB_CHECKOUT" --data-dir "$PB_DATA" --output "$OUT/piibench-repro.json"

# 4. Gaze and every compared tool, same metric code as the main comparison
#    (takes the same tool flags as compare.py).
"$COMPARE_PYTHON" scripts/bench/compare/theirbench/theirbench.py --benchmark presidio-research \
  --presidio-research-checkout "$PR_CHECKOUT" --predictions-dir "$OUT/pred-pr" --output "$OUT/presidio-research.json" ...
"$COMPARE_PYTHON" scripts/bench/compare/theirbench/theirbench.py --benchmark piibench-commercial \
  --piibench-data "$PB_DATA" --predictions-dir "$OUT/pred-pb" --output "$OUT/piibench-commercial.json" ...

# 5. Score each row's spans with the benchmark's own evaluator, then assemble and render.
#    Run once per row (--system) and benchmark; see run-theirbench.sh.
uv run --project scripts/bench python scripts/bench/compare/theirbench/render_theirbench.py assemble \
  --report "$OUT/presidio-research.json" --report "$OUT/piibench-commercial.json" \
  --own presidio-research=... --own piibench-commercial=... \
  --reproduction presidio-research="$OUT/presidio-research-repro.json" \
  --reproduction piibench-commercial="$OUT/piibench-repro.json"
uv run --project scripts/bench python scripts/bench/compare/theirbench/render_theirbench.py render
```

## A tagged Gaze release as its own row

The panels show released Gaze versions only, so a release is measured from its tag, never from `main`:

```bash
# 1. A clean checkout of the tag in its own worktree (no build there: the harness builds).
git worktree add ../gaze-v0.15.1 v0.15.1     # or: anvil work bench-v0151 -b v0.15.1
# 2. The release's own `gaze setup` policy (built from that checkout), unedited.
(cd ../gaze-v0.15.1 && cargo run -q -p gaze-cli -- setup --non-interactive --force --policy-out /tmp/policy-v0.15.1.toml)
# 3. Measure, twice, into FRESH reports (a tagged row is never resumed into an existing report).
"$COMPARE_PYTHON" scripts/bench/compare/theirbench/theirbench.py --benchmark presidio-research \
  --presidio-research-checkout "$PR_CHECKOUT" --predictions-dir "$OUT/pred-pr-1" --output "$OUT/pr-run1.json" \
  --gaze-release-tag v0.15.1 --gaze-release-root ../gaze-v0.15.1 \
  --gaze-model-dir "$NER_MODEL_DIR" --gaze-policy /tmp/policy-v0.15.1.toml
"$COMPARE_PYTHON" scripts/bench/compare/theirbench/theirbench.py ... --output "$OUT/pr-run2.json" \
  --gaze-release-tag v0.15.1 ... --reproduces "$OUT/pr-run1.json"
# 4. Vendor evaluator on the same spans, then merge (repeat for PIIBench-commercial).
"$VENDOR_PYTHON" scripts/bench/compare/theirbench/presidio_research_repro.py --checkout "$PR_CHECKOUT" \
  --predictions "$OUT/pred-pr-2/gaze-v0.15.1.test.jsonl" --labels "$OUT/pred-pr-2/labels.gaze.json" \
  --system gaze-v0.15.1 --output "$OUT/own-pr.json"
python3 scripts/bench/compare/theirbench/render_theirbench.py add-tagged --report "$OUT/pr-run2.json" --own "$OUT/own-pr.json"
python3 scripts/bench/compare/theirbench/render_theirbench.py render
```

What the harness enforces in this mode: the tag is resolved as `refs/tags/<tag>`, the checkout is exactly that commit with no local changes, the harness itself is clean and its state is re-read every run; the benchmark binary is built by the harness from that checkout into a fresh `CARGO_TARGET_DIR` (an existing directory or `--gaze-binary` is refused, so a stale executable cannot be attributed to the tag); the policy and both model bundles must equal the digests pinned in `tagged_gaze.RELEASE_PINS`. The row's provenance records the exact build command, toolchain, `Cargo.lock` hash and binary SHA-256, the pins, the prediction file's SHA-256 and, from the second run, the first run's prediction and binary hashes; `add-tagged` refuses a row whose second run did not reproduce the predictions byte for byte. Predictions themselves are never committed.

## A vendor's own tuned setup as its row

On a vendor's own benchmark the chart compares Gaze with that vendor's best published setup, not its defaults (user ruling 2026-09-29). [`vendor-tuned.json`](vendor-tuned.json) declares it per benchmark: the setup, its source and pinned commit, and the panel caption. Presidio Research publishes one, notebook 5 (custom analyzer, OpenMed NER, extra recognizers, context enhancement; F2 0.91 with the evaluator that produced it, reproduced here). PIIBench-commercial has none, so Presidio keeps its declared best configuration there and the panel caption says so. The declaration lives beside the harness, not in `../chart-configs.json`, whose bytes `comparison.json` pins.

```bash
OPENMED_SNAPSHOT=...   # OpenMed/OpenMed-PII-SuperClinical-Large-434M-v1 at df7af994 (see presidio_research_repro.py)
# 1. The vendor environment runs the analyzer once and writes raw findings (offsets, entities, scores; no text).
"$VENDOR_PYTHON" scripts/bench/compare/theirbench/tuned_presidio.py --checkout "$PR_CHECKOUT" \
  --openmed-model "$OPENMED_SNAPSHOT" --output "$OUT/tuned-raw.jsonl" --meta "$OUT/tuned-meta.json"
# 2. Our harness resolves overlaps with the comparison's resolver and scores the spans (fresh report).
"$COMPARE_PYTHON" scripts/bench/compare/theirbench/theirbench.py --benchmark presidio-research \
  --presidio-research-checkout "$PR_CHECKOUT" --predictions-dir "$OUT/pred-tuned" --output "$OUT/tuned.json" \
  --vendor-tuned --tuned-raw "$OUT/tuned-raw.jsonl" --tuned-meta "$OUT/tuned-meta.json"
# 3. The vendor evaluator replays the same spans with notebook 5's mapper rules (no label map).
"$VENDOR_PYTHON" scripts/bench/compare/theirbench/presidio_research_repro.py --checkout "$PR_CHECKOUT" --tuned \
  --predictions "$OUT/pred-tuned/presidio-tuned-presidio-research.test.jsonl" \
  --system presidio-tuned-presidio-research --output "$OUT/own-tuned.json"
# 4. Merge: the Presidio bar on that panel becomes this row.
python3 scripts/bench/compare/theirbench/render_theirbench.py add-tuned --report "$OUT/tuned.json" --own "$OUT/own-tuned.json"
```

The OpenMed model is pinned in `vendor-tuned.json` by repository, revision and tree digest. `tuned_presidio.py` hashes the directory it is given and refuses a mismatch before any inference; `theirbench.py --vendor-tuned` and `add-tuned` compare the producer's receipt with the pin again, so no step trusts the one before it. Every vendor-evaluator result (`presidio_research_repro.py --predictions`, `piibench_repro.py --predictions`, `pii_trace_repro.py`) records an `input` receipt, the SHA-256 of the prediction file it read and of the dataset it scored; `add-tagged`, `add-tuned`, `add-tool` and `add-benchmark` refuse a result whose receipt differs from the measured row's `prediction_sha256` or the committed dataset digest, and keep the receipt as `own_scorer_input` in the row's provenance. `theirbench.py` records that digest for every competitor row. The receipt also carries the raw counts, and every published score must be exactly what they give; `pii_trace_repro.py` keeps the character, exact-span and per-label counts, so its whole result is rebuilt and compared, including the message count.

The tuned row's typed metrics and common-intersection view use the comparison's Presidio label table plus the extra OpenMed entities in `vendor-tuned.json`; the common-intersection label set is still computed over the standard roster. Leaked and false-positive bytes do not depend on labels. `add-tuned` refuses a row whose provenance does not match the declaration (setup, source, commit), whose producer record is a smoke run, or whose own-scorer result is not the notebook-5 replay of the same spans, and it needs the vendor's tuned number reproduced first.

Measurements take the bench machine lock. `render --check` fails when the README block drifts from `their-benchmarks.json`.
