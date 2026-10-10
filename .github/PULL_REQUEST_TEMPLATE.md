<!-- Run local gates before pushing. -->

## What & why

<!-- One or two sentences. Link the issue this resolves (e.g. "Resolves #123"). -->

Resolves #

## Local gates: run before pushing (CONTRIBUTING.md → "PR-checks ritual")

- [ ] `cargo fmt --all`: formatted
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`: no warnings
- [ ] `cargo test --workspace --all-features`: green
- [ ] Run every `cargo run -p xtask -- <gate>` in CONTRIBUTING.md’s "PR-checks ritual".
- [ ] (If touching audit-sink boundaries) ran `dylint`: `gh workflow run dylint.yml`

## Fixtures & PII hygiene

- [ ] No real PII anywhere (code, tests, fixtures, docs): synthetic only (reserved email domains and generated tokens).
- [ ] Any PII-shaped literal is covered by a cited test (`fixture-citation-lint` enforces this).

## Benchmark evidence (AGENTS.md → "Benchmark gain gate")

- [ ] Not a detection change and not a benchmark change. Tick this only to skip the rest of this block.
- [ ] Changes the benchmark (layer, contract, corpus or generated data, scorer, or benchmark doc): past-release rows re-measured with the current runner on each tag's code; rows that could not be re-measured give the reason in the doc.

Detection changes (adds, widens, narrows or removes rules, cues, locale buckets, mechanisms, models, or safety-net/resolver behaviour) fill in the table. Layers are C, A, D and R.

Base sha (`main` the branch starts from, scored fresh):
Candidate sha:
Scorecard paths:

| Layer | Leaked bytes v2 (base → candidate) | Leaked bytes v1 (base → candidate) | False-positive bytes v2 (base → candidate) | False-positive bytes v1 (base → candidate) | Refusals (base → candidate) |
| --- | --- | --- | --- | --- | --- |
|  |  |  |  |  |  |

Net bytes over all layers (leaked bytes removed − false-positive bytes added; must be above zero unless false-positive-only), v2:  v1:

## DCO

- [ ] All commits are signed off (`git commit -s`): the DCO check (`.github/workflows/dco.yml`) requires it. No CLA.

## Commit discipline (per AGENTS.md)

- [ ] Signed commits (`git commit -S -s`), files staged by name; no amend, `--no-verify`, or force-push. No commit prefix required.
