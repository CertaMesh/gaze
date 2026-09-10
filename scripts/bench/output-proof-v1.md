# Actual-output proof v1

This opt-in observer extends the existing `pipeline_text/clean_for_bench` route.
Schema-v4 scorecards retain their existing trace-derived meaning. Quality claims
must use the separate `output-proof-v1/repetition-N/<config>.json` sidecars and
paired comparator. Kiji is a benchmark profile, not the literal CLI default.

## Replay contract

The observer checks the closed wire protocol, token trace/manifest 1:1 identity,
actual original/clean UTF-8 bounds, and disjoint monotonic mappings. It requires
all manifest integrity errors to be zero, including token restore and raw-value
mismatch counters. In raw order it copies untouched original byte segments,
inserts each final manifest token from its exact expected clean offset, and
omits each final trace deletion. The whole reconstructed output must equal the
supplied `clean_text`. Identical token strings elsewhere cannot satisfy a wrong
clean offset.

Source semantics: `pipeline.rs::clean_text_internal` normalizes detection input,
translates candidates back to original offsets, and copies the original text.
The observer therefore does not normalize gold, raw copy segments, or clean
output. Safety-net deletion expands over intersecting tokens, merges overlapping
plans, drops their manifests, and replaces their trace entries. Only the final
disjoint manifest/trace participates in replay. Primary non-Tokenize/Preserve
policies are unsupported by this producer route.

This proves positional passthrough/replacement, conditional on the trusted
producer's per-token restore integrity checks. It is not independent token-store
verification, arbitrary serialized-egress analysis, or a substring/no-shared-byte
oracle. No additional runtime offsets or retained source/clean/token values are
needed. A forged deletion that leaves its source bytes fails replay.

Only a valid replay receives verified counts. `gold_bytes` is the union of gold
UTF-8 intervals; `verified_covered_bytes` intersects that union with proved
replacements/deletions; `surviving_bytes` counts gold bytes in proved passthrough
segments. `false_positive_bytes` counts replaced/deleted nongold bytes.
`full_span_escapes` retains the existing uncovered-entity meaning: gold spans
not fully covered, including partial escapes. `fully_surviving_spans` separately
counts gold spans with no coverage. Gold overlap is merged only for byte counts.

## One outcome per planned row

Precedence is explicit:

1. Invalid protocol or replay/integrity: `unmeasured(reason)`, verified counts
   are JSON null, never zero. No supplied exception text is exported.
2. Valid pipeline refusal: `fail_closed(stage, reason)` from the existing closed
   vocabularies; verified counts are null.
3. Valid replay with non-success restore decision, even if exact=true, or
   nonexact restore without deletion: `restore_failure`.
4. Otherwise any final deletion: `completed_nonreversible`.
5. Otherwise: `completed_reversible` with exact restore.

Rows contain only IDs, closed outcome metadata, negative indicator and counts.
The source contract hashes exact source text, gold spans/labels, language,
region, source dataset, category, and IDs with length-delimited records. The
planned population includes its canonical sorted-ID digest. Each sidecar must
contain every planned ID exactly once; duplicates, missing, unknown IDs, null
measured counts and inconsistent counts are rejected.

All planned arms/repetitions receive an initial unmeasured sidecar before the
first producer starts. Backend, cancellation or protocol errors abort the legacy
run and invalidate that arm's observations, including previously received rows,
so trailing protocol errors cannot leave a successful quality artifact. Future
arms remain explicitly unmeasured. The original exception boundary is preserved.
A host crash can leave the initial unmeasured artifact; there is no promise of
an export if the filesystem itself is unavailable.

## Frozen IDs and named arms

Add `--output-proof --evaluate-ids-file <ids.json>` to the existing runner.
`ids.json` is a nonempty JSON array of unique IDs in frozen evaluation order;
missing IDs are rejected. It overrides quick/full sampling without changing the
source documents or gold. Sampling metadata retains the ordered ID digest while
the sidecar binds the canonical set. Freeze the confirmation complement before
looking at its result; this is same-generator confirmation, not an independent
holdout. The existing quick256 dev selection is owned by the baseline receipt
(ordered digest `3993b14aeded90b056854d29172de148424b6e7b0eb967395e6b78de2fbbce36`).

`--config` may repeat. Defaults are unchanged. Additional arms are
`pass2-ner-redact` and `rule-floor-redact`; their build enables `redact-live`.
After integrating the matching producer, supply its private absolute executable
and verified model directory through `GAZE_REDACT_BRIDGE` and
`GAZE_REDACT_MODEL_DIR`. The runner preserves these environment settings.
Threshold 0.6 and ORG-on belong to the frozen producer; no new tuning flags exist.
The runner's existing model/provenance checks remain in place. Sidecars live
beside that run's scorecard/provenance; they do not assert independently verified
model or executable identity. No model execution is needed for observer tests.

## Paired comparison

```sh
uv run --project scripts/bench --locked python scripts/bench/gaze_bench_score.py \
  --baseline <baseline-sidecar.json> --candidate <candidate-sidecar.json> \
  --output <paired-result.json>
```

Exit 0 means accepted; 1 means quality gates failed; 2 means invalid comparison
contract. The comparator requires identical planned IDs and source/gold contract.
It reports full planned outcome tables, transitions, common completed and common
reversible populations with digests and counts, gained availability, and the
baseline-completed population with candidate unavailable IDs and gold bytes.
Candidate measured counts on that population are explicitly partial when any
rows are unavailable; unavailable bytes are never treated as zero survival.

Acceptance requires fewer verified surviving bytes on common completion, no
increase in full-span escapes or total false-positive bytes, no negative row
with increased false positives, no new refused/error/unmeasured count or lost
baseline-completed row, and full restoration of candidate completions. No
allowance, fabricated ceiling, or recovered unrelated row can offset losing a
baseline-completed row. Irreversible deletion gains remain visible but fail
acceptance. An identical result is not an improvement. The common-population
byte delta is descriptive even when an availability gate fails.
