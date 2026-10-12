# Atomic registry admission evidence

These value-free receipts compare a frozen native generator v19 on the base and measured candidate revisions recorded in `preservation-evidence.json`. They do not change the corpus, generator, scorer, or scored-label contracts. The final PR adds receipts after the measured candidate; its `crates` tree must equal `candidate_crates_tree` in the receipt.

The acceptance contract is valid-input preservation plus SDK construction admission. Default and the #800 family-precedence arm must have identical ordered registry censuses and identical document observations, leaked bytes, false-positive bytes, refusals, exact restores, and valid manifests under v1, v2, and v4. Invalid registrations fail before processing any document, so they have no corpus leak or refusal credit. A zero byte delta is expected; this is not a measured detection gain.

`base-*-census.json` and `candidate-*-census.json` record recognizer IDs, classes, locales, and optional collision membership in scan order. The separate admission tables cover valid, forward, orphan, and duplicate cases through both entry points. The base's absent `try_build` API is explicitly unavailable. Construction scan counts remain zero. The baseline-only patch exposes scan order and adapts the evidence example to the old API; it changes no detection logic.

## Reproduce preparation

Use Rust 1.96.0, the same pinned model files, and the corpus whose SHA-256 is recorded in the receipt. Work in dedicated base and candidate checkouts. Use one owned Cargo target directory sequentially: complete both captures on one side before rebuilding the other side. Before switching sides, clean release outputs for every workspace package and rebuild the CLI, example, inference binary, and validator probe from that side's source. Keep source and binaries immutable while either arm runs.

For the base checkout, copy `crates/gaze-assembly/examples/registry_contract.rs` from the measured candidate, then apply the candidate's `scripts/bench/registry-census-base.patch`. The patch is evidence-only and is not part of the production baseline.

With `CARGO_TARGET_DIR` set to the owned build directory, prepare each side:

```sh
cargo build --release --locked -p gaze-cli
cargo build --release --locked -p gaze-assembly --features safety-net-nym --example registry_contract
mkdir -p target/gate
"$CARGO_TARGET_DIR/release/gaze" setup --non-interactive --force --policy-out target/gate/policy.toml
uv run --project scripts/bench python scripts/bench/family_precedence_arm.py policy \
  --input target/gate/policy.toml --output target/gate/family-policy.toml
"$CARGO_TARGET_DIR/release/examples/registry_contract" target/gate/policy.toml > default-census.json
"$CARGO_TARGET_DIR/release/examples/registry_contract" target/gate/family-policy.toml > arm-census.json
"$CARGO_TARGET_DIR/release/examples/registry_contract" --admission > admission.json
```

Prepare inference and the validator probe with the canonical runner's `dataiku_en_de_gaze_bench.build_binary(root, ('policy-file',), release=True)` and `run_no_opf_benchmark.build_validator_probe(root, release=True)`. Capture actual source, CLI, example, inference, probe, and setup-policy receipts before launching. Prepared inference hashes must equal the post-capture hashes on both arms. Policy and census hashes must agree across sides.

## Capture and replay

Exactly four physical runs were performed: each side once for default and once for the family-precedence arm. Each command is recorded as `runs.<side>-<arm>.physical_command` in the receipt; substitute the corpus path without changing its SHA-256. The runner appends `/full` to each output directory. Physical v1 uses no `--scored-labels` argument; `scorecard-v4.json` denotes schema version four, not credit contract v4. All four runs use `--no-cache`.

Each retained record is replayed with the canonical `scripts/bench/scorecard_record.py`, its `--expected-sha256`, and `--jobs 2`. Omit `--scored-labels` for v1; use `docs/reference/benchmarks/scored-labels-v2.json` or `scored-labels-v4.json` for v2 or v4. All exact replay commands and output hashes are recorded under each run's `contracts`. Each v1 replay must match its physical scorecard's layer totals. Sorted observations and all C/A/D/R layer totals must match base and candidate separately for each arm. Observations and scorecards remain local because the corpus is not a public fixture.

Compare the committed census and admission files with:

```sh
python scripts/bench/registry_contract.py \
  --base-default scripts/bench/fixtures/registry-contract/base-default-census.json \
  --candidate-default scripts/bench/fixtures/registry-contract/candidate-default-census.json \
  --base-arm scripts/bench/fixtures/registry-contract/base-arm-census.json \
  --candidate-arm scripts/bench/fixtures/registry-contract/candidate-arm-census.json \
  --base-admission scripts/bench/fixtures/registry-contract/base-admission.json \
  --candidate-admission scripts/bench/fixtures/registry-contract/candidate-admission.json \
  --output registration-contract-report.json
```

The preservation receipt retains actual physical process exits and absolute release-readiness results. Equal failures are a preservation result, not release-readiness approval. Construction errors are reported only by the SDK admission table.
