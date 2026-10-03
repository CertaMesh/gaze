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
9. Use the approved split release/documentation sequence. After all approved
   source, version, workflow, and release-note changes have merged through the
   protected process, freeze the exact release commit `R` and the harness,
   input, policy, model, and renderer identities. Measure `R` freshly in a
   clean worktree, retain an immutable private evidence bundle `E`, complete
   the gains, history, competitor, native, and private-preview checks, and
   dogfood the actual private preview. The pretag evidence must prove
   `measured_commit = R`, name every frozen input, and include the complete
   fresh scorecards, historical comparisons, competitor results, receipts, and
   failed attempts. A branch name, predicted SHA, or “same code” statement is
   never evidence.
10. Only then create and push the signed annotated release tag. Its peeled
    target `T` must equal `R`; record both `T` and the annotated tag-object ID.
    Do not put the later documentation commit in the release tag tree.
11. After the real tag exists, render and validate the public benchmark in an
    owned staging checkout. Commit the version-owned scorecards, history,
    charts, pages, and evidence bindings in a later signed, protected
    documentation commit `D`. Dogfood the actual public bytes and verify the
    docs checks, the public scorecard hashes, and this exact equality:
    `T = E.measured_commit = scorecard.gaze.revision = history.commit = R`.
    `D` must not silently replace the frozen harness or renderer identities.
12. Create a signed annotated `release-readiness/vX.Y.Z` tag targeting `D`.
    Its `gaze-release-readiness-v1` receipt binds `R`, the real release-tag
    object, `D`, the frozen harness and renderer commits, immutable `E` digest,
    and the SHA-256 of
    `docs/reference/benchmarks/release-readiness/vX.Y.Z.json`. That manifest
    repeats those identities, names the immutable evidence-manifest path and
    digest,
    hashes every published scorecard, and lists the successful documentation
    checks (including `docs`). Protect this readiness-tag namespace against
    update/deletion. The workflows independently fetch GitHub’s immutable tag,
    commit, content, and check-run objects; missing, stale, failed, unsigned,
    or mismatched receipts fail closed.
13. A tag push initially leaves both publication paths held. After `D` and its
    readiness tag are accepted, retry the original failed tag workflow runs;
    they publish from the unchanged `R` tag. Never manually publish crates or
    create a release to bypass the hold. Manual crate dispatch remains dry-run
    only.
14. Confirm every detection PR merged since the last tag carries passing
    evidence under the
    [benchmark gain gate](../../../AGENTS.md#benchmark-gain-gate), and that
    past-release rows were re-measured if the benchmark itself changed.
15. Re-run `scripts/bench/compare/compare.py` for this release's Gaze rows and
    every competitor on the same corpus, contracts, and variant packs. Update
    `comparison.json` and `competitors.md`; retain aggregate results and model
    hashes. The PR docs gate checks competitor inputs, while this release step
    refreshes the Gaze revision shown on the page.

If any step fails, stop and fix the release branch. Do not tag around a red
checklist.

## Tag Procedure

Only after explicit user lock signal and steps 1–10 above have passed:

```bash
git tag -a vX.Y.Z -m "vX.Y.Z" <merge-sha>
git push origin vX.Y.Z
```

Use an annotated tag on the merge commit. Do not tag a local-only commit, an
unmerged branch head, or a dirty working tree.

The tag push auto-fires two workflows, which build but intentionally hold
publication until the posttag documentation receipt passes:

- `release.yml`: builds binaries and creates the GitHub Release.
- `publish-crates.yml`: publishes every workspace member with `publish != false`
  via OIDC trusted publishing. The set and its topological order are derived from
  `cargo metadata` by `cargo run -p xtask -- publish-plan`, so new crates are
  included automatically.

Do not publish to crates.io manually. The workflow owns publication order and
idempotent retries after the exact same tag passes readiness.

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
