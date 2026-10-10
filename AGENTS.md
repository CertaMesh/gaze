# AGENTS.md — Gaze project agent context

Read this file at session start. It is the canonical project context for all
coding agents; [CLAUDE.md](CLAUDE.md) adds agent guidance.

## Project north star

> Gaze is the most reliable, reversible PII pseudonymization runtime for agentic workflows. Zero PII
leaks between the agent and the data owner — ever. Any byte of PII that reaches an LLM outside the
manifest contract is a critical defect.

Pseudonymization is reversible substitution with tokens (GDPR Art. 4(5)).

## The five axes

Evaluate every design, implementation and review against all five axes.

1. Reliability (never leak). Fail-closed always. Defense in depth (regex + NER + dictionary +
   optional neural safety net). Every known detection gap is a todo; every leak incident is a
   postmortem + fix pattern baked into skill/memory.
2. Reversibility. Manifest-first restore. Format-preserving tokens stay restorable. No one-way
   primitives in the core contract. Anything that breaks restore round-trip is a design regression.
3. Agentic-first. Decisions prioritize agent workflow needs over generic text handling — tool-call
   JSON embedding, streaming LLM, multi-turn sessions with evolving context, tenant-specific PII
   (songs, order IDs, artist names).
4. Trust (auditable + deterministic). Rule-based detectors preferred over neural for precise
   classes. Neural is an addon (safety net, free-text NER), not the floor. Every token emission
   traceable to a rule or recognizer. Typed exceptions + closed error-variant set. No silent
   mismatches.
5. Adopter ergonomics. Low-friction integration (Laravel adapter pattern, clear TOML policy, sane
   defaults). Framework adapters pave the 80% case; library API serves the 20% power case. Adopter
   can pick Gaze up in under a day without deep PII domain expertise.

Correctness axes 1–4 beat performance. Justify any weakened axis in the PR body.

## Workspace shape (v0.9)

The workspace now has 15 published crates plus `xtask`; see the canonical
[workspace table](CONTRIBUTING.md#workspace-shape).

- Core `gaze` re-exports `gaze_types::RedactionLogger`; it has no `rusqlite`
  dependency in any feature graph. `gaze-types` is serde-only, without ML/SQL.
- `rusqlite` lives in `gaze-audit`. Only `gaze-cli` may consume it outside
  compatibility tests.
- No safety net runs without a policy. `gaze setup` enables Nym
  (`safety-net-nym`); OPF and GLiNER DOB (`--dob-judge`) stay opt-in.
- MCP uses a sealed `ToolCtx` and `PiiEnvelope::dispatch`; rmcp defaults to
  stdio, with opt-in HTTP and adopter-supplied `PrincipalResolver`.
- Document ingestion uses Tesseract and produces `clean.md`, `manifest.json`
  and `report.json` in a `SafeBundle`.
- The dashboard is a killable, memory-only child behind default-off
  `gaze-cli/dashboard`. Its only Gaze dependencies are `gaze-types` and
  `gaze-inspection`, never proxy/provider/private transports.
  `dashboard-isolation` enforces this behaviorally.
- `gaze_module_isolation` Dylint lives in detached `lint/dylint/`, pinned to
  `nightly-2025-09-18`; it replaces the removed `audit-metadata-only` syn walker.

## Universal rules (ALL agents)

1. Justify any weakened axis in the PR body. Correctness axes 1–4 beat performance.
2. Never leak PII in examples, tests, or fixtures. Use `alice@example.invalid` / `Dr. Schmidt` /
   `<Email_1>` — never real PII, even in docs. Phone numbers MUST come from documented fictional
   ranges: NANPA `555-0100` to `555-0199` behind any area code for US, Ofcom `+44-7700-900xxx` for
   UK, and the BNetzA drama numbers for DE (for example `+49 171 39200xx`; `+49 1555` is an
   allocated block, not a fictional one). See
   [`CONTRIBUTING.md`](CONTRIBUTING.md#phone-number-fixtures) for the canonical rule + rationale.
3. Commit discipline: every commit must be signed (SSH commit signing is configured on this machine;
   delegates in unsigned sandboxes must have their commits re-created signed before merge). No
   commit-message prefix is required. Stage specific files by name. No `git add -A` or `git add .`.
   No amend, no force-push, no `--no-verify`. Commit after each logical phase, not only at the end.
4. Branch per task. Work on a dedicated branch; keep `main` clean.
5. Completion signaling: every agent brief includes a sentinel line (e.g. `IMPL DONE:`, `REVIEW DONE:`, `DOCS DONE:`). Print it on the final stdout line.
6. Never put private tracker ids in public text. Exclude private todo, scratchpad, process and audit
   ids from PR titles/bodies, commits, CHANGELOG, docs, code/test comments and scripts. Describe
   behavior ("tracked as a follow-up"). Public GitHub numbers (`#723`) stay.

## Benchmark gain gate

Every detection-output change must be measured and improve the benchmark:
rules, cues, locales, mechanisms, models, safety nets and resolver behavior.
Behavior-preserving refactors require identical base/candidate scorecards.
The reviewer enforces this gate; CI lacks the local corpus.

Layers are holdout C and generated A, D and R
([contract](docs/reference/benchmarks/README.md#agentic-layers-and-the-rule-gate)). Gate credit
contract v2 counts checksum-invalid gold for IBAN and payment cards on every surface. In layer A it
counts Steuer-ID, CPF, BSN, and NHS number classes only when cued; their `prose_nocue` twins stay
excluded. Phone has no layer A family or layer D counterweight yet. Layer C lacks per-gold cue
metadata and currently credits all validator-failed gold for these classes, including phone, a
documented limit pending a cue-aware record contract. Other checksum-invalid gold is reported but
not credited; a rise in its leaked bytes still fails. Any false-positive rise on a credited label's
layer D benign counterweight fails outright, whatever the net bytes. IBAN is counterweight-exempt.
Within the caller-known-record oracle only, exact matches to declared phone or payment-card values
are also exempt; report their layer D false-positive bytes and require a positive net under both
contracts. This exception does not apply to ordinary phone or card recognizer rules.

1. Measure both sides fresh. Base is the `main` commit the branch starts from, scored now, not an
   old scorecard. Candidate is the branch head. Each side runs in its own worktree on the policy its
   own `gaze setup` writes, with the same corpus, seed and machine. Score it twice, once per
   contract, and skip the release-only copy and render steps:

   ```bash
   cargo run -p gaze-cli -- setup --non-interactive --force --policy-out target/gate/policy.toml
   uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py full \
     --seed 20260710 --no-download --release --policy target/gate/policy.toml \
     --output-dir target/bench-data/gate-<side>-v1
   # repeat with --scored-labels docs/reference/benchmarks/scored-labels-v2.json \
   #   --output-dir target/bench-data/gate-<side>-v2
   ```

   Compare the two sides per contract with `agentic_layers.py gate`, as shown in [Agentic layers and the rule gate](docs/reference/benchmarks/README.md#agentic-layers-and-the-rule-gate).
2. Report both gated scored-label contracts. Report v2 and v1 separately. The benchmark document
   leads with contract v3, whose leaked bytes equal v2's and whose false positives exclude the
   audited gold-gap credit; switching the gate to v3 is a separate decision.
3. Merge rule. Checked separately under each scored-label contract. No layer's leaked bytes rise, on
   gated gold or on headline (all-gold) bytes. No layer's refusals rise, no layer's exact-restore or
   valid-manifest document count falls, and no layer's restore-failure or invalid-manifest count
   rises. At least one layer's leaked bytes fall, and the false-positive bytes added across all
   layers are fewer than the leaked bytes removed across all layers. Refused documents leave the
   leak count, so increased refusals hide leaks. State false-positive bytes per layer. A
   false-positive-only fix passes when false-positive bytes fall and no layer's leaked bytes or
   refusals rise. A rise in exact restores or valid manifests also counts as an improvement when no
   byte count regresses.
4. A blind benchmark is not a pass. If the benchmark cannot see the change, extend the generated
   layers first, with positives and false-positive counterweights, then measure. Never merge on "the
   corpus is blind".
5. Passing the gate is necessary, not sufficient: reviewers still judge real-text recall and
   precision outside these corpora.
6. Never tune a rule to the corpus. A rule that only matches corpus-specific shapes will miss real
   text; see [A scorecard measures the corpus, not the
   recognizer](docs/reference/benchmarks/README.md#a-scorecard-measures-the-corpus-not-the-recognizer).
7. Evidence goes in the PR body through the template's "Benchmark evidence" block: base and
   candidate sha, scorecard paths, v2 and v1 leaked bytes per layer, false-positive bytes per layer,
   and refusals. A PR skips the block only by ticking "not a detection change and not a benchmark
   change".

The headline is always the latest. The benchmark headline uses the latest scored-label contract and
benchmark data, and every past version the document displays is re-measured under it. Contract v3
becomes the headline once its gold-gap audit passes.

Benchmark changes re-measure past releases. Any change to the benchmark itself (a layer, a
scored-label contract, corpus or generated data, the scorer, or the benchmark document) re-measures
every past release the document displays: the current harness drives each release tag's own
detection code. A row that cannot be re-measured says why in the document. Local-only harness
evidence does not count; the harness and its inputs must be committed to the repo or pinned there by
hash.

## Source of truth

Shared rules live here; `CLAUDE.md` defers to them.
