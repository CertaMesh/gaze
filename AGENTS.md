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
2. Never leak PII in examples, tests, or fixtures. Use reserved `example.invalid` email addresses and
   synthetic names such as `Dr. Schmidt`, never real PII, even in docs. Phone numbers MUST come from documented fictional
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

## Working in worktrees (Anvil)

Use Anvil 1.13 or newer for implementation, review copies, and local gates.
Run these commands from a linked Gaze checkout:

```sh
anvil ensure <branch> --json --lease-owner <agent> --task-id <id>
anvil ensure --review <sha> --json
```

Save the returned lease token privately as `ANVIL_LEASE_TOKEN`. Use the returned
worktree path for edits. Review copies are detached at the exact SHA. Review
commands omit registry and publish tokens, while retaining GitHub authentication.
Preparation runs the locked CI Python install with the `comparison-test` group.
It does not build Rust or copy model bundles, environment files, or `target/`.

Inside the prepared worktree, run:

```sh
anvil context --json
anvil run -- rustc -V
anvil check fast --json
anvil check --list --json
anvil check --reuse fast --json
anvil check test --allow-expensive --json
anvil check feature-matrix --allow-expensive --json
anvil check bench-py --allow-expensive --json
```

`fast` runs fmt and clippy. Individual `fmt` and `clippy` checks also exist.
Accept a result only when `assessment.passing_and_current` is true. Reuse is
per worktree in Anvil 1.13 and requires an identical environment. A different
Solo process or shell environment can invalidate reuse. Delegates may inspect
the original commit and provenance, but a review copy cannot import that pass.
Tests and feature checks are not reusable because optional shared models can
change outside Git. The Python suite also consults external release builds.

The environment binds Cargo, trybuild, and rustdoc to Rust 1.96.0 through rustup
proxies, with four build jobs. Install rustup under `~/.cargo` and the pinned
rustfmt and clippy components. Never export `CARGO_TARGET_DIR`, including to the
worktree's own `target/`. Nested mutation gates require cwd-relative targets.

Full workspace builds share the exclusive `workspace-build` Anvil lock. Use
`--lock-wait 30m` when another check owns it. This conservatively allows one
Anvil workspace build at a time on the shared machine. The model-free Python
suite also holds `bench-machine` because it reports durations. Before any real
benchmark capture, acquire the Solo `bench-machine` lease as well. Solo remains
the cross-tool authority; an Anvil lock alone does not reserve the machine.

Shared bundles use the standard `~/.local/share/gaze/models/` directories and
`~/.opf/privacy_filter`. They are optional and used read-only in place. Missing
bundles do not block preparation. Anvil reports their existence in `context`;
model-dependent commands still fail closed on missing or invalid bundles.
GLiNER has no model-directory environment variable in Gaze; use
`ANVIL_SHARED_GLINER_DOB` when explicitly configuring that opt-in bundle.
Anvil 1.13 exports optional resource paths even when absent and does not expand
shell default-value expressions. For a custom XDG model location, use explicit
model paths in the policy or command.

After the work is harvested, release your lease and preview cleanup:

```sh
anvil lease release <worktree> --json
anvil remove <worktree> --dry-run
anvil remove <worktree>
```

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

1. Capture each side once. Base is the `main` commit the branch starts from. Candidate is the
   exact integrated branch head. Each side uses its own worktree and the policy its own
   `gaze setup` writes, with the same corpus, seed and machine. Re-score that one observation
   record for v2, v1 and every later contract. Never launch a capture per contract. Verified
   cache reuse requires an exact match of every effective execution input and record integrity.
   Skip the release-only copy and render steps:

   ```bash
   cargo run -p gaze-cli -- setup --non-interactive --force --policy-out target/gate/policy.toml
   uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py full \
     --seed 20260710 --no-download --release --policy target/gate/policy.toml \
     --output-dir target/bench-data/gate-<side>
   uv run --project scripts/bench python scripts/bench/rescore.py \
     target/bench-data/gate-<side>/full/observations-v1.jsonl.gz \
     --scored-labels docs/reference/benchmarks/scored-labels-v2.json \
     --output target/bench-data/gate-<side>/full/scorecard-v2.json
   # The original scorecard is v1; rescore the same record for any other contract.
   ```

   Compare the two sides per contract with `agentic_layers.py gate`, as shown in [Agentic layers and the rule gate](docs/reference/benchmarks/README.md#agentic-layers-and-the-rule-gate).
2. Report both gated scored-label contracts. Report v2 and v1 separately. The benchmark document
   leads with the latest headline contract; switching the gated contracts requires a separate
   decision.
3. Merge rule. Checked separately under each scored-label contract. No layer's leaked bytes rise, on
   gated gold or on headline (all-gold) bytes. Under refusal contract v2, typed untransformable refusals qualify as blocked
   operations only with producer-proven `evidence_complete: true` covering every refused path,
   and every refused span exactly matching ownership-labelled confirmed-ID gold. Single-path
   first-error evidence and unsupported encoded-carrier evidence are ineligible.
   Benign or non-gold refusals still fail. The gate removes blocked operations from both sides'
   byte, exact-restore and valid-manifest denominators, reports blocked-operation and false-refusal
   rates with denominators, and requires an independent gain. Ordinary refusals may not rise.
   On the common comparison population, no layer's exact-restore or valid-manifest count falls,
   and no layer's restore-failure or invalid-manifest count rises. Refusal contract v1 remains
   available for historical reproduction with its original unconditional guards. At least one layer's leaked bytes fall, and the false-positive bytes added across all
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

Main stays certified. No detection change reaches main without full-corpus-equivalent evidence
for the exact integrated candidate. Certification requires a full evaluation, a verified
incremental evaluation with a sound affected-unit selector validated in shadow first, or a
full-gated combined merge train. A targeted subset can inform a PR, but never certifies main.
Both gated contracts, mechanism policy deltas and the full release benchmark remain required.

Benchmark specifications have immutable version IDs. Never compare results across specifications.
Integrity checks remain mandatory. Historical remeasurement is off the PR critical path:
backfills are automated and non-blocking. Before release, measure the current and previous
release under the same specification. Displayed rows retain their specification and provenance;
a row unavailable under the current specification says why. Local-only harness evidence does
not count; commit the harness and its inputs or pin them by hash in the repository.

The headline uses the latest scored-label contract and benchmark specification. Historical rows
from another specification cannot support a gain comparison.

## Source of truth

Shared rules live here; `CLAUDE.md` defers to them.
