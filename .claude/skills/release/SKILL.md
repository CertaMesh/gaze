---
name: release
description: "Use when orchestrating an CertaMesh/gaze release end to end, especially when the user says cut release, ship vX.Y.Z, tag vX.Y.Z, publish vX.Y.Z, or release vX.Y.Z. Covers pre-flight gates, explicit tag authorization, GitHub and crates.io workflow expectations, post-tag verification, escalation rules, and the counter-pattern to avoid: do not just push a tag and hope."
---

# Release Orchestration

Use this skill for Gaze release execution. It complements the `release-notes`
skill, which controls release-note voice only.

Do not cut a release, push a tag, or trigger publish workflows until the
pre-flight checklist is green and the user gives an explicit lock signal such
as "Do it yourself", "Tag + push", or an equivalent direct instruction.

## Pre-Flight Gates

Run from `main` after all release-blocker PRs are merged.

1. Confirm the working tree is clean and `main` is at the intended merge commit.
2. Confirm all open PRs marked as `v0.X.0-final` blockers are merged.
3. Confirm the workspace version pin matches the about-to-tag version in the
   root `Cargo.toml` and all per-crate `Cargo.toml` files.
4. Run `cargo run -p xtask -- ci-feature-matrix` and require green.
5. Run `cargo run -p xtask -- readme-version-check` and require green.
6. Run `git grep -E '/Users/[a-z]+'` and require zero tracked-file matches.
   Path-leak output is release-blocking until scrubbed to `~/` or `$HOME`.
7. Dogfood Gaze on its own release text: pipe the modified `CHANGELOG.md`
   section, plus any optional extra release text passed to the release
   preflight, through `gaze clean` and verify zero detections. GitHub Release
   bodies use generated notes; `dist/release-notes/` is not committed. This
   preserves the `feedback-dogfood-gaze-on-own-output` memory.
8. Verify benchmark claims in the changelog or release PR body link to the
   script and hardware specification that produced them. This preserves the
   `feedback-bench-claims-reproducible` memory.
9. Confirm every detection PR merged since the last tag carries passing
   evidence under the
   [benchmark gain gate](../../../AGENTS.md#benchmark-gain-gate), and that
   past-release rows were re-measured if the benchmark itself changed.
10. Re-run `scripts/bench/compare/compare.py` for this release's Gaze rows and
    every competitor on the same corpus, contracts, and variant packs. Retain
    aggregate results and model hashes for the later documentation update.
11. Resolve the substantive benchmark timing decision before tagging. The
    split sequence below is a proposal, not an approval or a waiver of the
    existing pretag public-row/chart and updated-document dogfood requirements.
    If explicitly adopted, freeze the exact merged release commit `R` and the
    harness, input, policy, model, and renderer identities; measure `R` freshly
    in a clean worktree and retain immutable private evidence `E`. Complete all
    required gain, history, competitor, native and private-preview checks;
    dogfood the actual preview. `E.measured_commit` must equal `R`, with complete
    fresh scorecards, comparisons, receipts and failed attempts retained.
    A branch name, predicted SHA or “same code” statement is never evidence.
    Unresolved timing or other original release gates mean HOLD.
12. Require reviewed external rollout evidence before a public release tag:
    authorized creation and no update/deletion for both release and readiness
    tag namespaces, and publisher/environment controls that admit only reviewed
    release refs using the guarded workflows. Older workflow-dispatch refs must
    not bypass the hold. The source gate cannot establish these settings from
    workflow YAML; missing or unknown proof means HOLD. See the
    [acceptance boundary and rollout prerequisites](../../../docs/how-to/maintainers/release-process.md#readiness-acceptance-boundary).

If any step fails, stop and fix the release branch. Do not tag around a red
checklist.

## Tag Procedure

Only after the explicit user lock signal, the substantive timing decision,
and **pretag steps 1–12** have passed:

```bash
git tag -s vX.Y.Z -m "vX.Y.Z" <R>
git push origin vX.Y.Z
```

Use a signed annotated tag on the merged measured commit: peeled target `T`
must equal `R`. Record both `T` and the annotated tag-object ID. Do not tag a
local-only commit, an unmerged branch head, or a dirty working tree. Never put
later documentation commit `D` in the release tag tree.

The tag push auto-fires two workflows, which build but intentionally hold
publication until the posttag documentation receipt passes:

- `release.yml`: builds binaries and creates the GitHub Release.
- `publish-crates.yml`: publishes every workspace member with `publish != false`
  via OIDC trusted publishing. The set and its topological order are derived from
  `cargo metadata` by `cargo run -p xtask -- publish-plan`, so new crates are
  included automatically.

Do not publish to crates.io manually. The workflow owns publication order and
idempotent retries after the exact same tag passes readiness.

## Post-Tag Readiness (conditional on timing adoption)

These are finite posttag tasks, not pretag self-preconditions. Their completion
cannot retroactively authorize the public tag or waive an original release gate.

1. Once the real tag exists, render and validate the public benchmark in an
   owned staging checkout. Commit version-owned scorecards, history, charts,
   pages, `comparison.json`, `competitors.md` and aggregate evidence bindings
   in later signed documentation commit
   `D`, merged through the protected default branch. Dogfood the actual public
   bytes. Require `T = E.measured_commit = scorecard.gaze.revision = history.commit = R`
   for the **current** release row; historical rows retain their own revisions.
   Preserve the frozen harness/renderer, corpus, policy and model identities.
2. Prepare the versioned manifests and PASS receipts described in the
   [readiness acceptance contract](../../../docs/how-to/maintainers/release-process.md#readiness-acceptance-boundary).
   A maintainer must accept the actual results and external rollout proof,
   not merely hash files. Create the signed readiness tag targeting `D`:

   ```bash
   git tag -s release-readiness/vX.Y.Z -F <receipt-file> <D>
   git push origin release-readiness/vX.Y.Z
   ```

   The receipt starts with `gaze-release-readiness-v1` and binds version, `R`,
   real release-tag object, `D`, frozen harness/renderer commits, evidence digest
   and documentation-manifest digest. The source gate verifies the signed
   payload, joins the current scorecards/history and trusted successful main
   docs check/run, and rejects missing, unknown, incomplete or failed evidence.
   It does not rerun the numerical/native gates or prove their assertions.
3. Both publication paths initially fail closed. After readiness is independently
   accepted, retry the original tag workflow runs using the unchanged `R` tag.
   The readiness tag does not trigger publication itself. Expired build assets
   may require rerunning the original full workflow. Never manually publish
   crates or create a release to bypass the hold; manual crate dispatch is
   dry-run only.

## Post-Tag Verification

After the workflows finish:

1. Confirm `gh release view vX.Y.Z` returns the release.
2. Confirm both workflow runs succeeded: `release.yml` and `publish-crates.yml`.
3. Confirm every published crate reports the new version. Derive the expected
   set with `cargo run -p xtask -- publish-plan` rather than a hard-coded list,
   then check `https://crates.io/api/v1/crates/<name>` and expect
   `max_version == X.Y.Z` for each. As of v0.14.0 the plan covers 15 crates:
   `gaze-pii`, `gaze-types`, `gaze-audit`, `gaze-inspection`, `gaze-recognizers`,
   `gaze-assembly`, `gaze-model-setup`, `gaze-mcp-core`, `gaze-mcp-rmcp`,
   `gaze-mcp-bridge`, `gaze-document`, `gaze-proxy`, `gaze-proxy-dashboard`,
   `gaze-token-bridge`, and `gaze-cli`. A count that does not match the plan is a
   partial publish, not a pass.
4. Update the orchestrator scratchpad with released URLs:
   GitHub Release URL plus one crates.io URL per crate in the publish plan.

## Escalation Rules

- Do not push tags without an explicit user lock signal.
- Do not publish to crates.io manually.
- Do not amend, delete, or force-push a tag after it has been pushed. If a
  released tag is wrong, make a new patch release.
- Do not put local absolute paths in release notes, PR bodies, or commit
  messages. Use `~/` or `$HOME`.
- If generated GitHub release notes need prose changes after publication, edit
  the GitHub Release body explicitly and keep `CHANGELOG.md` as the curated
  source.

## Counter-Pattern

Do not just push a tag and hope. A Gaze release is complete only when the
pre-flight gates are green, the tag was explicitly authorized, both workflows
succeeded, every crate in the publish plan reports the expected version, and the orchestrator
scratchpad records the shipped URLs.
