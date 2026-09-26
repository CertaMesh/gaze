<!--
Thanks for contributing to Gaze. This template turns the PR-checks ritual
(CONTRIBUTING.md + .github/workflows/test.yml) into a guided checklist.
Run the gates locally before pushing — CI runs the same set and will fail otherwise.
-->

## What & why

<!-- One or two sentences. Link the issue this resolves (e.g. "Resolves #123"). -->

Resolves #

## Local gates — run before pushing (CONTRIBUTING.md → "PR-checks ritual")

CI runs the same set and will fail otherwise.

- [ ] `cargo fmt --all` — formatted
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` — no warnings
- [ ] `cargo test --workspace --all-features` — green
- [ ] `xtask` gates pass — run each `cargo run -p xtask -- <gate>` listed in CONTRIBUTING.md's "PR-checks ritual" (the feature-matrix + correctness-invariant gates: audit-sink isolation, no-tenant-knowledge, bundle-drift, symmetric-potemkin, …). CONTRIBUTING explains what each guards.
- [ ] (If touching audit-sink boundaries) ran `dylint`: `gh workflow run dylint.yml`

## Fixtures & PII hygiene

- [ ] No real PII anywhere (code, tests, fixtures, docs) — synthetic only (`alice@example.invalid`, `<Email_1>`).
- [ ] Any PII-shaped literal is covered by a cited test (`fixture-citation-lint` enforces this).

## Benchmark evidence (AGENTS.md → "Benchmark gain gate")

- [ ] Not a detection change and not a benchmark change. Tick this only to skip the rest of this block.
- [ ] Changes the benchmark (layer, contract, corpus or generated data, scorer, or benchmark doc): past-release rows re-measured with the current harness on each tag's code; rows that could not be re-measured give the reason in the doc.

Detection changes (adds, widens, narrows or removes rules, cues, locale buckets, mechanisms, models, or safety-net/resolver behaviour) fill in the table. Layers are C, A, D and R.

Base sha (`main` the branch starts from, scored fresh):
Candidate sha:
Scorecard paths:

| Layer | Leaked bytes v2 (base → candidate) | Leaked bytes v1 (base → candidate) | False-positive bytes v2 (base → candidate) | False-positive bytes v1 (base → candidate) | Refusals (base → candidate) |
| --- | --- | --- | --- | --- | --- |
|  |  |  |  |  |  |

Net bytes over all layers (leaked bytes removed − false-positive bytes added; must be above zero unless false-positive-only), v2:  v1:

## DCO

- [ ] All commits are signed off (`git commit -s`) — the DCO check (`.github/workflows/dco.yml`) requires it. No CLA.

## Commit discipline (per AGENTS.md)

- [ ] `[agent]` prefix on commits if this PR was produced by an AI agent; files staged by name; no `--no-verify`, no force-push.
