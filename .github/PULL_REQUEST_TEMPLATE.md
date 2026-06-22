<!--
Thanks for contributing to Gaze. This template turns the PR-checks ritual
(CONTRIBUTING.md + .github/workflows/test.yml) into a guided checklist.
Run the gates locally before pushing — CI runs the same set and will fail otherwise.
-->

## What & why

<!-- One or two sentences. Link the issue this resolves (e.g. "Resolves #123"). -->

Resolves #

## Five-axes note (required if any axis is affected)

<!--
Gaze decisions are evaluated against five axes: Reliability (never leak), Reversibility,
Agentic-first, Trust (auditable + deterministic), Adopter ergonomics. Correctness axes 1-4
always beat performance. If this PR weakens any axis, say so here and justify the tradeoff.
-->

- [ ] This change does not weaken any axis, **or** the tradeoff is justified above.

## Local gates — run before pushing (CONTRIBUTING.md → "PR-checks ritual")

```sh
cargo fmt --all
cargo run -p xtask -- readme-version-check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- symmetric-potemkin
cargo run -p xtask -- class-map-override-safety
cargo run -p xtask -- recognizer-composition-validator
cargo run -p xtask -- no-tenant-knowledge
cargo run -p xtask -- bundle-tokenization-drift
cargo run -p xtask -- fixture-citation-lint
cargo run -p xtask -- ci-feature-matrix
cargo run -p xtask -- cargo-metadata-audit-isolation
```

- [ ] `cargo fmt --all` — formatted
- [ ] `cargo run -p xtask -- readme-version-check` — crate README version pins match `Cargo.toml`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` — no warnings
- [ ] `cargo test --workspace --all-features` — green
- [ ] Behavioral `xtask` gates above — all pass
- [ ] (If touching audit-sink boundaries) ran `dylint` manually: `gh workflow run dylint.yml`

## Fixtures & PII hygiene

- [ ] No real PII anywhere (code, tests, fixtures, docs). Synthetic only (`alice@example.invalid`, `<Email_1>`).
- [ ] Phone numbers use reserved synthetic ranges (NANPA `555-01xx`, Ofcom `7700-900xxx`, DE `1555 …`) — CONTRIBUTING.md#phone-number-fixtures.
- [ ] Any production PII-shaped literal carries a `// fixture-cited(path::test)` marker pointing at a real, listed test.
- [ ] Test/bench class names are neutral (`class_alpha`), not tenant-specific (`order_id`, `Song_42`).

## Docs & changelog

- [ ] Public-facing behavior change is documented (relevant `docs/` page and/or crate README).
- [ ] `CHANGELOG.md` updated under `[Unreleased]` if user-visible.

## DCO

- [ ] All commits are signed off (`git commit -s`) — the DCO check (`.github/workflows/dco.yml`) requires it. No CLA.

## Commit discipline (per AGENTS.md)

- [ ] `[agent]` prefix on commits if this PR was produced by an AI agent; files staged by name; no `--no-verify`, no force-push.
