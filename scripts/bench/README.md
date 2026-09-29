# Gaze benchmark scripts

The one canonical local regression entry point is:

```bash
scripts/bench/run_no_opf_benchmark.py
```

The other files in this directory are scorer, producer, or specialized research
components. They are not alternate release-regression entry points.

## Setup and profiles

Create the locked Python environment once:

```bash
uv sync --project scripts/bench --locked
```

Use `quick` while developing. It takes a seeded, deterministic stratified
sample through `gaze_bench_score.stratified_sample`:

```bash
uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py \
  quick --seed 20260710 --no-download
```

Use `full` for a local release candidate. It evaluates every English/German
document selected from the pinned Dataiku test split and all 1,024 committed A4
negative fixtures:

```bash
uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py \
  full --seed 20260710 --no-download \
  --compare-baseline target/bench-data/no-opf/baseline.json
```

Omit `--no-download` on the first run to fetch and SHA-256 verify the pinned
Dataiku Parquet file. Both profiles use only `rule-floor-extended` and
`pass2-ner`; `pass2-ner` (rules plus Davlan NER, no safety net) is the production
arm. The opt-in `full-stack-nym-resolve` and `full-stack-opf-resolve` arms run
through `openpii_gaze_bench.py` or `dataiku_en_de_gaze_bench.py --config`, not
this runner. `full-stack-nym-redact` runs the same Nym model under
`SafetyNetMode::Redact`; it is not a shipped default, and exists because the
shipped Nym arm resolves every suspect on this corpus, so a change to what
redaction writes is only measurable here. `marker_ab.py` is the document-level
A/B built on it. The runner removes OPF environment variables and
passes no OPF command, checkpoint, or daemon socket, even if the invoking shell
defines them.

The `clean_for_bench` producer derives a repeatable session prefix from each
fixture ID by default. Set `GAZE_BENCH_RANDOM_SESSION=1` when measuring the
effect of fresh random prefixes; record that setting with the scorecard.

When the generated adjacency layers change, measure the displayed past releases
with `agentic_layers.py measure`, then pass their scorecard files to
`render_agentic_adjacency_doc.py --record <file>` (repeat `--record` for each
release). That command writes the committed measurement ledger and its table
in `docs/reference/benchmarks/README.md`; `--check` verifies both in CI.

Planning estimates on a modern laptop are roughly 2–10 minutes for the default
256-document quick profile and 30–120 minutes per measured full repetition.
Thermals, CPU runtime, and filesystem cache state can move those estimates
substantially; A6 records the authoritative observed runtime.

## Scored-label contracts

A scored-label contract says which corpus labels count as gold PII. Without
`--scored-labels` the runner uses contract **v1**, which scores every corpus
label, so every row measured before contracts existed stays reproducible.

```bash
uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py \
  full --seed 20260710 --no-download \
  --scored-labels docs/reference/benchmarks/scored-labels-v2.json
```

[`scored-labels-v2.json`](../../docs/reference/benchmarks/scored-labels-v2.json)
rules on every corpus label with a reason and a `settled`/`pending` ruling. v2
puts the credential labels `PASSWORD` and `SECURITYTOKEN` out of contract:
credentials authenticate a system, they are not personal data (user ruling
2026-09-16). An
out-of-contract span is removed from gold, and the bytes only it covers are
ignored, so they count as neither leaked nor false positive. Whether the
pipeline still protected them is reported per run under
`excluded_label_coverage`. A corpus label the contract does not list fails
closed.

[`scored-labels-v3.json`](../../docs/reference/benchmarks/scored-labels-v3.json)
keeps v2's labels and adds a `gold_gap` block, a diagnostic that reports
unlabelled byte-identical repeats of a gold value in the same document as
`metrics.gold_gap.gold_gap_protected_bytes` beside the unchanged v2 numbers.
Every `gold_gap` setting has one supported value and every scored label must be
ruled on once (in `compatible_labels` or `not_creditable`); anything else fails
closed, and v1/v2 never run the step. To compare contracts on the same saved
predictions without re-running a model, to draw or render the audit sample,
and to score the audit once every entry has a verdict:

```bash
python scripts/bench/gold_gap_evidence.py replay --trace <trace.jsonl>
python scripts/bench/gold_gap_evidence.py sample --trace <trace.jsonl>
python scripts/bench/gold_gap_evidence.py sheet  --trace <trace.jsonl>  # local only
python scripts/bench/gold_gap_evidence.py accept  # exits 1 if the audit fails
```

`accept` first checks every verdict against the judges recorded in the sample
and the tiebreak results in `fixtures/gold-gap-tiebreak-v3.json`, written by
`gold_gap_tiebreak.py`. The tiebreak's inputs quote document text and live in
the private audit archive, not in this repo:

```bash
python scripts/bench/gold_gap_tiebreak.py --archive <audit archive> --payloads-only <out.json>
```

v2 also lists `neutral_prediction_classes` (`custom:password`,
`custom:security_token`, `custom:secret`). A prediction of one of those classes
still counts as protection where it covers scored gold; its other bytes are
ignored instead of counted as false positive, and are reported per run under
`neutral_prediction_utf8_bytes_outside_scored_gold`.

Every run writes `observations-v1.jsonl.gz` beside `scorecard-v4.json` and pins
its SHA-256 in `scorecard.observation_record`. The gzip JSONL header contains
one canonical descriptor per document: layer, gold byte offsets and class,
validator offsets, and corpus and generator identity. A digest binds the A/D/R
document IDs and gold spans to the generator version and corpus SHA-256.
Observation rows contain only the layer, config, document ID, and response:
protected byte offsets, class, source IDs, action, refusal and restore facts.
For v3, the response records which gold span matches a trimmed prediction and
whether the word boundary passes. Neither header nor rows contain document or
protected values. Rows omit run-dependent timing; the header keeps the original
scorecard timing. A re-score needs no corpus, binary or model:

```bash
uv run --project scripts/bench python scripts/bench/rescore.py \
  target/bench-data/no-opf/observations-v1.jsonl.gz \
  --scored-labels docs/reference/benchmarks/scored-labels-v3.json \
  --expected-sha256 HASH_FROM_SCORECARD \
  --output target/bench-data/no-opf/scorecard-v3.json
```

Omit `--scored-labels` for v1. Use `--agentic-scored-labels` to change the
generated layer contract too; otherwise replay uses the layer contract pinned
inside the record. The same scoring accumulators compute the new scorecard.
The v0.15.1 records remain committed as replay proof. Starting with v0.16.0,
upload release records as assets on the matching GitHub release, using
`observations-vX.Y.Z.jsonl.gz` and, when generated layers are measured,
`observations-vX.Y.Z-agentic.jsonl.gz`. Pin each asset's filename, SHA-256,
byte size, and release URL in `docs/reference/benchmarks/release-history.json`.
Download from that release and verify byte size and SHA-256 before re-scoring.

The committed v0.15.1 C record proves exact v1/v2 replay except timing and
valid v3 scoring. `observations-v0.15.1-agentic.jsonl.gz` also replays A/D/R:

```bash
uv run --project scripts/bench python scripts/bench/verify_record_scorecards.py \
  docs/reference/benchmarks/observations-v0.15.1.jsonl.gz \
  --v1 docs/reference/benchmarks/scorecard-v0.15.1.json \
  --v2 docs/reference/benchmarks/scorecard-v0.15.1-scored-labels-v2.json \
  --v2-contract docs/reference/benchmarks/scored-labels-v2.json \
  --v3-contract docs/reference/benchmarks/scored-labels-v3.json
```

A record captured later with today's harness (v0.15.0 and v0.14.0, captured
2026-09-27 with `rescore_past_release.py`) is checked with `--capture`, which
compares dataset identity and every run field except timing against the
release's committed scorecards. A field the harness computes may be skipped
by name, and the result reports whether it differed:

```bash
uv run --project scripts/bench python scripts/bench/verify_record_scorecards.py \
  docs/reference/benchmarks/observations-v0.15.0.jsonl.gz \
  --v1 docs/reference/benchmarks/scorecard-v0.15.0.json \
  --v2 docs/reference/benchmarks/scorecard-v0.15.0-scored-labels-v2.json \
  --v2-contract docs/reference/benchmarks/scored-labels-v2.json \
  --v3-contract docs/reference/benchmarks/scored-labels-v3.json \
  --capture --ignore-run-field validator_recall_by_label
```

The scorecard records the contract under `scoring.scored_label_contract`: its
id, version, file SHA-256, excluded labels, scored and excluded gold counts, and
`scored_gold_digest` over every scored `(document, start, end, label)`. The
comparator refuses to compare scorecards measured under different contracts,
and the benchmark document labels every non-v1 row with its contract version.
Its release trend line joins only rows measured under the latest row's
contract (same version and file SHA-256) and says how many rows it left out.

Each appended history row records its `shipped_default_arm`, since the default
changes between releases. Rows committed before that field existed resolve
through a version-keyed legacy map in `render_benchmark_doc.py` (`v0.14.0`
shipped `full-stack-kiji-resolve`); an unmapped row without the field is
refused rather than re-labelled with today's default.

## Required local models

The runner validates the model bundle before it builds or starts a benchmark
cell. Missing or mismatched bytes are typed, actionable failures; no cell is
silently skipped.

| Model | Default location | Pin source | Validation |
| --- | --- | --- | --- |
| Davlan multilingual BERT NER (production pass2 ONNX) | `~/.local/share/gaze/models/davlan-mbert-ner-hrl` | `[pass2_ner]` in `scripts/bench/no_opf_models.toml` | pinned `SHA256SUMS` digest, exact seven-artifact manifest and eight-file bundle surface, then every artifact digest |

Override the location with `--model-dir`. The runner rejects
symlinks in validated bundle material. Davlan's canonical bundle contains exactly
`model.onnx`, `tokenizer.json`, `config.json`, `tokenizer_config.json`,
`special_tokens_map.json`, `vocab.txt`, `labels.json`, and `SHA256SUMS`. Its
production provenance is repository
`onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX`, commit
`cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8`, and canonical manifest digest
`7b0b9d0d200bf7f3a39654257f8723998316600852edff8404834eb7edfc5c16`.
The manifest must list exactly the seven runtime artifacts and match that digest;
the directory may contain no other files, directories, or symlinks. Missing
artifacts, Transformers or safetensors weights, cache metadata, and all other
extras fail closed.

This production ONNX pass2 pin is intentionally separate from
`crates/gaze-recognizers/benches/ner_models.toml`. That file remains the research
Transformers model matrix and is not the Davlan source of truth for the canonical
no-OPF runner.

At scorer initialization, stable provenance IDs are loaded from every committed
`crates/gaze-recognizers/embedded/*.toml` recognizer and from the model IDs in
`scripts/bench/no_opf_models.toml`. That file's `[builtin_source_ids]` declaration
adds built-in producer IDs only from committed ground truth; tests require every
declared ID to appear as a string literal in the read-only `gaze-recognizers`
sources. Runtime producer assertions never extend this vocabulary. An exact,
case-sensitive vocabulary match may skip only the protected-content reproduction
check; source-ID grammar, ordering, non-empty, and uniqueness validation still
apply. Missing, unreadable, malformed, or empty committed vocabulary inputs abort
scoring with a typed error.

## Outputs and verdicts

Generated artifacts are under ignored `target/bench-data/no-opf/<profile>/`:

| Artifact | Purpose |
| --- | --- |
| `scorecard-v4.json` | full schema-v4 scorecard, runner provenance, and identified scored/failed-closed populations per cell |
| `summary.md` | concise human-readable result, including side-by-side validator-backed and shape-only recall |
| `diagnostics.json` | per-language, per-label, validator-split, and per-negative-category diagnostics |
| `regression-status.json` | baseline-relative integer-count ratchet verdict |
| `release-readiness-status.json` | candidate-only absolute correctness verdict |
| `performance-status.json` | separately configured p95 `clean_ms` comparison |

The scorecard's `layers` block holds the generated agentic layers A
(identifiers in agentic surfaces), D (benign lookalikes) and R (the
repeat-value slice with its decoys), scored as separate
cells from `agentic_layers.py` under
`docs/reference/benchmarks/scored-labels-agentic.json`; `summary.md` appends
their totals and coverage grid. `--no-agentic-layers` skips them. The layers,
the held-out protocol and the rule gate (`agentic_layers.py gate`) are described
in [Agentic layers and the rule gate](../../docs/reference/benchmarks/README.md#agentic-layers-and-the-rule-gate).
Regression and release readiness below read layer C (`runs[]`) only.

`known_record_cells.py` generates layer K for the separate known-record oracle
arm only, never the main scorecard. Each probe pair gives a record that differs
from the text only in the way one match kind is for (`whitespace_flexible`,
`whitespace_case_folded`, `corroborated_single`), next to a benign twin with the
same record and shape. Kinds are derived after Gaze's own folding, which turns
NBSP-class separators into spaces before detection, so an NBSP copy is an exact
match. Such pairs, and the other pairs that exercise no probe kind, are typed
controls in their own buckets. `python3 scripts/bench/known_record_cells.py`
prints the pinned manifest. `... known_record_cells.py prove --binary <clean_for_bench>
--policy <policy> --model-dir <ner> --contract v2 --output <json>` scores layer K
three ways (no record, record with the default kinds, record with the probe kinds
on) and reports bytes per bucket. Its `kind_switch_effective` is false when the
binary ignores the per-request `record_match_kinds`.

The PR #724 proof used the Known-Record producer at #718 commit
`2be21a54ec925e13d5e8fec64182bfbe21cd703c`, whose `clean_for_bench` had SHA-256
`3f33b3528c705b97a3fa489fde12b98b96d253a30d3d311d73921ec8c7dd16cc`. It was built
on aarch64-apple-darwin with Homebrew cargo and rustc 1.95.0 (`59807616e`), which
ignore the repository's `rust-toolchain.toml` pin. Two separate builds from the
same checkout path produced that hash. From a detached checkout of that commit, run:

```bash
CARGO_TARGET_DIR="$PWD/target" /opt/homebrew/bin/cargo build -q -p gaze-recognizers --example clean_for_bench --features safety-net-nym --release
```

The hash depends on the checkout path and toolchain; a rustup 1.96 build
differs. Compare the proof's `binary_sha256` field rather than expecting a
byte-identical build elsewhere.

Regression and release readiness are deliberately independent. Regression uses
integer counts with zero tolerance and fails closed on missing, empty, invalid,
or population-mismatched candidates. Release readiness requires every candidate
cell to have no pipeline, restore, manifest, telemetry-agreement, or strict
rejection failures. The production candidate cell must additionally have no
leaked labeled bytes, uncovered entities, unscanned documents, residual suspects,
or redact actions. A full command exits nonzero for either correctness failure.

Every cell records sorted `scored_population.document_ids` and
`failed_closed_population.document_ids` plus a SHA-256 digest over each list.
Failed-closed entries also name the synthetic document ID, closed failure reason,
and closed stage; their reason/stage counts must reconcile exactly to the
failed-closed total. Regression comparison requires scored-set identity before
emitting any metric or per-label delta. A mismatch names the IDs added to and
removed from the scored set even when cardinality and gold counts are unchanged.

Schema v4 also carries additive validator diagnostics. The project-owned
`scripts/bench/validator_recall_probe` links the production `ValidatorKind`
implementations, runs the same rule floor with validators intact and with only
the validator fields removed in memory, and emits offsets/classes rather than
matched values. Each scored cell reports `validator_recall_by_label` with
side-by-side `validator_backed_recall` and `shape_only_recall`. The dataset block
reports a full available-population `validator_gold_census` with validator pass
and fail counts. Labels without an applicable validator say `not_applicable` and
use null pass/fail and recall fields, so not-applicable cannot be confused with a
measured zero.

These fields define schema v4. Older schema-v3 artifacts remain readable for
diagnostic replay and their aggregate evidence is not rewritten. Because v3
does not identify each cell's scored set, however, it cannot pass the current
like-for-like population or per-label evaluability gates and cannot serve as a
v4 comparison baseline. Generate a v4 baseline with the current harness before
using those gates.

A quick result is always marked not release-ready because it samples the
population. Quick exits successfully when that incompleteness is its only
readiness failure, but any observed correctness failure still exits nonzero.

Performance compares `timing.clean_ms.p95` with
`--performance-tolerance-percent` and is informational by default. Add
`--performance-gating` only for an explicitly reviewed performance gate.
Warmup count, measured-repetition count, every discarded warmup sample and its
outcome, and external process/model cold-start-to-first-validated-response are
recorded in `runner_provenance`. Warmups are timing-only: their pipeline and
correctness outcomes never abort or contribute to the scorecard. Each document is
counted exactly once in the scored pass. Cold start is separate from Rust response
timing.

## Baseline acceptance

Baseline replacement has three guards: a full profile, an exact review
confirmation, and a release-ready candidate. Replacing an existing file also
requires a regression-clean comparison against that same file.

```bash
uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py \
  full --no-download \
  --compare-baseline target/bench-data/no-opf/baseline.json \
  --accept-baseline target/bench-data/no-opf/baseline.json \
  --accept-baseline-confirm I_HAVE_REVIEWED_FULL_RESULTS
```

Review `scorecard-v4.json`, all three status files, and `diagnostics.json`
before using that command. Quick results and failed candidates can never
replace a baseline.

To initialize a baseline only when the target does not yet exist, omit
`--compare-baseline` but keep the full profile and exact confirmation. The
runner refuses to overwrite an existing file through this initialization path.

## Publishing a release scorecard

Every release measures its own tree and updates the single benchmark document.
After a full run on the release commit:

```bash
cp target/bench-data/no-opf/full/scorecard-v4.json \
   docs/reference/benchmarks/scorecard-vX.Y.Z.json
uv run --project scripts/bench python scripts/bench/render_benchmark_doc.py \
  --scorecard docs/reference/benchmarks/scorecard-vX.Y.Z.json \
  --version vX.Y.Z \
  --machine "<CPU, cores, RAM, OS and build>" \
  --append-history
```

`render_benchmark_doc.py` appends one release-keyed row to
`docs/reference/benchmarks/release-history.json` and re-renders the generated
sections of `docs/reference/benchmarks/README.md` (headline table, charts,
history). `--machine` is required: the scorecard schema does not capture the
host, so it is the one hand-carried reproducibility field. A scorecard produced
from a dirty tree is refused.

Provenance a released row cannot omit — the corpus `sha256` and its per-component
digests, the evaluated population, and every pinned model bundle — is validated
on the way in and again when the history file is loaded. A missing one is a hard
error, never an `n/a` cell: a published number that names no evidence is worse
than no row at all. The renderer contract tests bind that shape to a real
harness scorecard committed at `scripts/bench/fixtures/real-scorecard-v4.json`
(see `fixtures/make_real_scorecard_fixture.py` for its provenance and how to
re-derive it after a schema change).

`--check` re-renders from the history file and fails if the committed document
has drifted. It is stdlib-only and needs no corpus, model, or network, so it
runs on every pull request in `.github/workflows/docs.yml`:

```bash
python3 scripts/bench/render_benchmark_doc.py --check
```

The same run writes the benchmark panels: `benchmark_charts.py` turns the
release history, `comparison.json` and `their-benchmarks.json` into a light and
a dark SVG under `docs/assets/benchmarks/` (README `<picture>`) and the
model-card tables in the benchmark reference. `--check` fails when either SVG
drifts. Only tagged Gaze releases are drawn: the comparison report's main-branch
run and the third-party files' `gaze-full` row are never read, `GazeRow`
refuses a version that is not `vX.Y.Z`, and a tagged third-party run enters
through `gaze_releases` in `their-benchmarks.json` (until then its slot reads
"pending"). The panels' headline is character-level F2; `compare/release_char_level.py
record` scores each tagged release from its committed observation record (needs the
corpus) into `release-char-level.json`, and `check` (in `docs.yml`) verifies it against
the release history and the metrics implementation without a corpus.

## Policy-matrix enumerations (base binary vs head binary)

Three model-free scripts drive two `gaze daemon` binaries over the #3708 IBAN
document set (`iban_trailing_word_enumeration.documents()`, 55,536 synthetic
documents) and compare the two per document. Each records both binaries'
SHA-256 and refuses identical builds; run them against immutable copies.

- `iban_trailing_word_enumeration.py` — the IBAN-plus-trailing-word contract:
  per-document IBAN view and raw residue under the bundled locale packs.
- `family_action_policy_matrix.py` — ten arms (five payment-family policies
  x two locales); every changed document must be `family-derived` or
  `family-residual`, `lost_bytes` must be 0.
- `family_action_full_matrix.py` — every `Action` value on every axis the
  family derivation reads: `custom:iban` x `custom:credit_card` in
  {tokenize, redact, generalize, format_preserve, preserve, unset}, default in
  {tokenize, redact, preserve}, family rule in {none, preserve, tokenize},
  two locales: 648 arms on the family-capable subset plus a seeded 1%
  control. Scores the bytes that leave the process (IBAN 4-grams surviving
  after every replacement shape is blanked), never manifest arithmetic, and
  fails on any document whose head leak exceeds its base leak. Every changed
  document is written per arm with its transition to a gzip JSONL next to the
  report. This is the acceptance bar for any change to how a policy action is
  chosen: a rank-monotone derivation is not a byte-monotone one (review of
  PR #624 found 872 regressed documents behind a `redact` arm the ten-arm
  matrix did not have).
- `policy_regex_collision_matrix.py` — the policy REGEX custom-recognizer
  family contract (todo #3757): four regex rules (an equal-precedence tie
  family, a precedence family, a `mandatory_anchor` family, an `email`-class
  rule beside the bundled `email.global`) under collision {on, off} x member
  action {tokenize, redact, preserve} x default {tokenize, preserve} x
  bundles {none, core+locale-de, core+locale-en}: 36 arms over a synthetic
  set plus the pinned Dataiku en/de holdout (`--holdout`). Scores per expected
  span the manifest bytes AND whether the raw value survives in the clean
  text; `lost_bytes` and `lost_values` must be 0, every collision-off and
  preserve-member arm must be byte-identical (pins that the fix moved no
  conflict winner), and every changed document must be `family-derived` or
  `family-strictest-member`.

## Model-free verification

Normal CI runs only the locked Python tests:

```bash
uv run --project scripts/bench python -m unittest discover -s scripts/bench
```

CI does not download models or run either benchmark profile. The authoritative
full model run and baseline/error-bucket analysis belong to A6.
