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

## DCO

- [ ] All commits are signed off (`git commit -s`) — the DCO check (`.github/workflows/dco.yml`) requires it. No CLA.

## Commit discipline (per AGENTS.md)

- [ ] `[agent]` prefix on commits if this PR was produced by an AI agent; files staged by name; no `--no-verify`, no force-push.
