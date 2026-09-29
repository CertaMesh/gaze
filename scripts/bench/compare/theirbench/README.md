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

Measurements take the bench machine lock. `render --check` fails when the README block drifts from `their-benchmarks.json`.
