# Detection Coverage Feedback Loop

The coverage feedback loop is a synthetic, deterministic regression harness for
Gaze recognizer coverage. It exists for axes 1 and 4: reliability and trust.

It does not train models, does not call an LLM, and does not expand production
rulepacks by itself. It measures the current rule floor against a committed
oracle so rule gaps can be fixed deliberately.

## Contract

The loop is:

```text
xtask templates + generators
    -> committed corpus fixtures
    -> core + core-extended pipeline
    -> emitted manifest
    -> labels.json span diff
    -> coverage-report.md/json
    -> baseline trend gate
```

The oracle is the committed `labels.json` span set under
`crates/gaze-recognizers/testdata/coverage-loop/corpus/`. Each span records byte
boundaries, audit-form class id, generator id, generator seed, and
`license_origin`.

The test classifies each labeled span:

- `Covered`: manifest has same-class coverage for the full label span.
- `Uncovered`: no manifest span overlaps the label span.
- `PartialBleed`: same-class overlap exists but does not cover the full label.
- `ClassMismatch`: overlap exists, but with a different class.

Trend gating only compares `Uncovered`: current must be less than or equal to
baseline for each `(class_id, locale)` bucket. `PartialBleed` and
`ClassMismatch` remain reported but not gated because they usually require
separate class-priority or resolver analysis.

## Data Rules

All fixtures are synthetic. Current accepted origin:

- `synthetic-rust-generator`

Deferred origin:

- `synthetic-vendored-kiji`

Kiji snippets are intentionally out of this PR. Any future vendored snippet work
must extend the origin enum, document provenance, and keep fixture bytes out of
production `src/` paths.

## Gate Mode

The ignored integration test is:

```bash
cargo test -p gaze-recognizers --test coverage_loop -- --ignored --nocapture
```

Default mode is informational. `GAZE_COVERAGE_LOOP_INFO_ONLY` unset or set to
`1` prints and writes the report without failing on baseline regressions.

Blocking mode:

```bash
GAZE_COVERAGE_LOOP_INFO_ONLY=0 cargo test -p gaze-recognizers --test coverage_loop -- --ignored --nocapture
```

Blocking mode loads
`crates/gaze-recognizers/testdata/coverage-loop/baseline.json` and fails if any
current `Uncovered` count exceeds baseline for the same class and locale.

## Corpus Realism

The v0.7.x corpus extends the original short-snippet set with deterministic
longer-form synthetic templates. The committed corpus now covers:

- Conversational support/email prose with greetings, filler, and signoffs.
- Code-mixed inputs with stack traces, fenced blocks, tool-call JSON, and PII
  embedded in args.
- High-density blocks with 5+ labeled spans in a compact paragraph.
- Distractor-heavy prose where example-domain, reserved-address, role-name, and
  placeholder-number bait appears beside generated labels.
- Threaded conversations with `>` quoted replies, `On ... wrote:` markers, and
  `Am ... schrieb ...:` markers.

Template metadata carries `length_tier`:

- `snippet`: short one-line or paragraph fixtures; existing templates default to
  this tier.
- `page`: longer support-ticket style fixtures with more surrounding context.
- `multi-page`: threaded fixtures that repeat context through quoted prior
  replies.

The builder keeps old snippet fixture counts byte-compatible and uses larger
variant counts for `page` and `multi-page` tiers. All spans still use
`synthetic-rust-generator`; no production traffic or LLM-generated prose is
allowed in the oracle.

## Adding Coverage

1. Add a generator under `crates/xtask/src/coverage_corpus/generators/`.
2. Register it in `GeneratorRegistry::default_phase_1()`.
3. Add a cheap unit test over at least 100 seeds.
4. Add templates under `crates/xtask/src/coverage_corpus/templates/<context>/`.
5. Include the templates in `templates/mod.rs`.
6. Regenerate with `cargo run -p xtask -- coverage-corpus --regenerate --seed 0`.
7. Run the ignored coverage-loop test and inspect `target/coverage-report.md`.
8. Commit the corpus and update `baseline.json` only after deciding the current
   leak set is the accepted baseline.

## Sibling Gates

This loop complements existing gates rather than replacing them:

- `fixture-citation-lint` protects production source from uncited fixture-like
  literals.
- `bundle-tokenization-drift` guards recognizer bundle activation drift.
- Safety-net tests guard leak-report correlation and fail-closed behavior.

The coverage loop is narrower: committed synthetic labels versus emitted
manifest spans.
