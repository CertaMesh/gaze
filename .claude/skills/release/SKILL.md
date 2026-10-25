---
name: release
description: "Execute a Gaze release with verified evidence, signed tags, workflow publishing and post-tag checks."
---

# Release orchestration

Use this skill for Gaze release execution. It complements the `release-notes`
skill, which controls release-note voice only.

Require green preflight checks and explicit user authority before pushing a tag
or publishing. An existing instruction to finish the full publication supplies
that authority; do not ask again. A worker assigned only evidence or source
preparation must stop at its assigned boundary.

## Preflight gates

Use an isolated Anvil checkout of the actual remote `main` merge after all
release blockers are merged. Read the local-development workflow before
isolation, service startup or full verification. Do not reset a divergent local
`main` or change remote refs to satisfy a source check.

Before these gates, run the competitor metadata check and retain its aggregate:

```sh
uv run --project scripts/bench --locked python scripts/bench/compare/freshness.py \
  --report docs/reference/benchmarks/comparison.json \
  --output target/competitor-freshness.json
```

Review every finding, including unknown freshness after source/network failures.
Record exact pins, release dates and the UTC check date with the comparison.
Use `compare/render.py --freshness target/competitor-freshness.json` when
rendering the comparison page. An incompatible latest driver keeps an explicitly
old measured row. Qualify new pins in a separate reviewed PR with new subject
identities; never upgrade a package or model inside a measurement. Untagged
models remain visibly unversioned. A partial `--source` check does not cover the
aggregate inventory. See the comparison README for optional scheduled checks.

1. Confirm the working tree is clean and `HEAD` is the intended remote-main merge.
2. Confirm all open PRs marked as `v0.X.0-final` blockers are merged.
3. Confirm the workspace version pin matches the about-to-tag version in the
   root `Cargo.toml` and all per-crate `Cargo.toml` files.
4. Require `cargo run -p xtask -- ci-feature-matrix` and applicable hosted
   preflights to pass on the exact source. Retain completed native qualification
   with its original source and input provenance. Reuse it only when the
   qualified code, features, toolchain and inputs are unchanged; record that
   binding rather than claiming a new run. Rerun affected checks after changes.
5. Run `cargo run -p xtask -- readme-version-check` and require green.
6. Run `git grep -E '/Users/[a-z]+'` and require zero tracked-file matches.
   Path-leak output is release-blocking until scrubbed to `~/` or `$HOME`.
7. Run `cargo run -p xtask -- scrub-public-text --published` with the proposed
   release notes and PR text as additional files. Require the actual checker
   to accept all publication text, including the version's changelog section.
   Use supported public-doc citations; do not widen URL exceptions to silence
   a rejected CI or commit link. GitHub initially generates release notes;
   keep curated notes in `CHANGELOG.md`, not `dist/release-notes/`.
8. Verify benchmark claims in the changelog or release PR body link to the
   script and hardware specification that produced them.
9. Pair version X with verified benchmark evidence for its actual source,
   policy, models and inputs. Follow the evidence rules below. Store scorecards,
   portable observation archives and receipts under `docs/reference/benchmarks`.
   Before tagging, use the history renderer's `--provisional` option. Charts
   admit only real tagged releases; plan their promotion after the tag exists.
10. Confirm every detection PR merged since the last tag carries passing
    evidence under the
    [benchmark gain gate](../../../AGENTS.md#benchmark-gain-gate). Before release,
    measure the current and previous release on the same immutable specification.
    Retain unchanged historical subjects and inputs under their original identities.
    Additional history backfills are non-blocking.
11. Update comparisons through supported collectors, composition and renderers.
    Reuse unchanged competitors with full original provenance. Collect affected
    rows only; do not rerun an unaffected roster because Gaze's source changed.
    Require exact source guards, complete populations and reviewed native hash
    receipts. Preserve all inherited Gaze, threshold and OPF identities.
12. Bind the measured source, actual merge and intended tag target. Require
    exact raw `crates` tree equality, including crate READMEs and manifests.
    Record and review any changes outside that tree. Inspect the publish plan
    and package file lists; package the coordinated workspace as described in
    [Release process](../../../docs/how-to/maintainers/release-process.md).
    Retain package digests and source bindings. A Git tree hash alone does not
    establish identical `.crate` payloads.

If any step fails, stop and fix the release branch. Do not tag around a red
checklist.

## Benchmark evidence and reuse

The latest headline is scored-label contract v4. Capture each side once, then
use `rescore.py` on that record for every contract. Run the gain gates separately
under v1 and v2. Never run inference per contract. The runner accepts a
contract-specific capture only with `--cross-check` for equivalence verification.
Export publication archives with `export_portable_record.py`;
retain original/private and portable hashes, unchanged-row proof and source,
binary, policy, model, corpus, seed and hardware provenance. Never restamp a
retained capture with the current source or a fresh replay time.

Reuse evidence only while its measured subject and benchmark inputs remain
unchanged and their full provenance is verified. A full cache-key match may
justify reuse; a changed revision alone is not an exact cache hit. Past-release
`--build --release-tag` captures support exact reuse for qualified `policy-file`
producers. Retain the original scorecard, observation record,
`release-capture-key.json` and `run-evidence.json` together. Render and history
admission verify this frozen proof and reconstruct the metrics. Preserve the
measurement revision and time. Unsupported configurations and legacy records
remain uncacheable. Never fabricate a receipt or key to bypass admission.
See [past-release reuse](../../../scripts/bench/README.md#reuse-a-past-release-capture)
for eligibility and the separately scheduled native qualification command.
When direct final-source evidence is required, collect only the changed
candidate sides and retain valid baselines. Benchmark specifications have immutable
version IDs; never compare across specifications. Integrity checks remain mandatory.
History backfills are automated, non-blocking and off the PR critical path. Before
release, measure the current and previous release on the same specification.
Main requires full-corpus-equivalent evidence for the exact integrated candidate:
full evaluation, shadow-validated sound incremental evaluation, or a full-gated
combined merge train. A targeted subset never certifies main.

Resolve the immutable manifest with `benchmark_spec.py` before measurement.
Use `benchmark_history.py <plan> --output <spec-directory>` to inspect the
current/previous selection, then add `--execute` to collect missing subjects.
Run `--older` separately for optional backfills. Resume verifies frozen subject
pins, artifact hashes and full record populations. Keep the resulting manifest,
`history.json` and `history.md` with the release evidence. Unproven legacy rows
remain unavailable under the current specification; preserve their old ledgers.
See [benchmark specification and history commands](../../../scripts/bench/README.md#freeze-the-benchmark-specification).

For an integrated detection candidate, run `benchmark_certification.py` with
`--candidate <exact-head>` and every reviewed `--required-mechanism` before
accepting its gain evidence. Pin both v1 and v2 scorecards and their shared
observation record per arm in the certification manifest. Required mechanism
pairs must use that same candidate binary. A pass does not replace the full
release benchmark, hosted CI, or publication authority.

Strict identity `not_comparable` stays distinct from an explicitly accepted
comparison of each release's own setup policy. Such a numeric comparison must
retain policy differences, layer-by-layer leaked and false-positive bytes,
populations, refusals, exact restores, restore failures, valid/invalid manifests
and every credited-label counterweight guard. Apply the same discipline to an
explicitly accepted external byte-micro F2 comparison: report F2 and every
per-set guard, with the original net-byte verdict separately, including FAIL.
An override of the comparison rule does not waive correctness or identity proof.

For mixed-source comparisons, retain immutable original/supplement snapshots
and the fixed sibling native receipt produced by `finalize_report.py`.
Validate their hashes and reconstruction with the existing row-source checks.
Tuned Presidio remains anchored to the original snapshot. Its byte-equivalence
verdict does not establish typed-label equality. Retain disclosed label
differences, failed mechanism arms and unavailable-tool attempts. No partial
OPF metrics or extra attempts are implied by a documentation refresh.

Run relevant renderer, receipt, public-text and source-binding checks on the
final branch. No contended timing result supports a speed claim. A renderer's
offline check proves consistency; a corpus replay establishes new character
counts. Do not substitute one for the other.

## Tag procedure

After the checks pass and publication authority is established:

```bash
git tag -s vX.Y.Z -m "vX.Y.Z" <merge-sha>
git verify-tag vX.Y.Z
git rev-parse vX.Y.Z^{commit} vX.Y.Z:crates
git push origin vX.Y.Z
```

Use a signed annotated tag on the verified remote-main merge commit. Verify
the actual tag target and measured crates tree before pushing. All created
commits must also be signed, cryptographically verified and DCO-signed by their
authors. Do not tag a local-only commit, an unmerged head or a dirty tree.

The tag push auto-fires two workflows:

- `release.yml`: builds binaries and creates the GitHub Release.
- `publish-crates.yml`: publishes every workspace member with `publish != false`
  via OIDC trusted publishing. The set and its topological order are derived from
  `cargo metadata` by `cargo run -p xtask -- publish-plan`, so new crates are
  included automatically.

Do not publish to crates.io manually. The workflow owns publication order and
idempotent retries.

## Post-tag verification

After the workflows finish:

1. Confirm `gh release view vX.Y.Z` returns the release.
2. Confirm both workflow runs succeeded: `release.yml` and `publish-crates.yml`.
3. Confirm every planned crate reports the exact new version on crates.io.
   Derive the set with `cargo run -p xtask -- publish-plan`; v0.17 expects 15
   crates. Check each version's source/package metadata and published checksum
   against retained package evidence. A missing crate or count mismatch is a
   partial publication, even if one workflow job succeeded.
4. Verify each workflow-declared `gaze` binary and its checksum: macOS ARM64
   and Linux x86_64 for v0.17. Check version output and artifact provenance
   against the actual tag/source. Verify the NER checksum manifest separately.
   An uploaded asset alone does not prove the right binary was published.
5. Promote provisional history only after actual tag/source/crates binding.
   Use `compare/release_char_level.py record` with the pinned corpus, retain its
   output, then run `check` and regenerate history, comparator pages and charts.
   Preserve historical evidence; do not invent a tag to render a candidate.
6. Record the GitHub Release URL, one crates.io URL per planned crate, binary
   verification and any unresolved publication failure in the release report.

## Escalation rules

- Do not push tags without an explicit user lock signal.
- Do not publish to crates.io manually.
- Do not amend, delete, or force-push a tag after it has been pushed. If a
  released tag is wrong, make a new patch release.
- Do not put local absolute paths in release notes, PR bodies, or commit
  messages. Use `~/` or `$HOME`.
- If generated GitHub release notes need prose changes after publication, edit
  the GitHub Release body explicitly and keep `CHANGELOG.md` as the curated
  source.
