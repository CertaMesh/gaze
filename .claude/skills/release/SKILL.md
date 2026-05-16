---
name: release
description: Use when orchestrating a Gaze release end to end, including trigger phrases like "cut release", "ship vX.Y.Z", "tag vX.Y.Z", "publish vX.Y.Z", or "release vX.Y.Z". Covers pre-flight gates, explicit tag authorization, GitHub/crates.io workflow expectations, post-tag verification, and escalation rules. Counter-pattern: do not just push a tag and hope.
---

# Release Orchestration

This skill governs release execution for `EmpireTwo/gaze`. It complements the
`release-notes` skill, which controls release-note voice only.

Do not cut a release, push a tag, or trigger publish workflows until the
checklist below is green and the user gives an explicit lock signal such as
"Do it yourself", "Tag + push", or an equivalent direct instruction.

## Pre-Flight

Run from `main` after all release-blocker PRs are merged.

1. Confirm the working tree is clean and `main` is at the intended merge commit.
2. Confirm all open PRs marked as `v0.X.0-final` blockers are merged.
3. Confirm the workspace version pin matches the about-to-tag version in the
   root `Cargo.toml` and all per-crate `Cargo.toml` files.
4. Run `cargo run -p xtask -- ci-feature-matrix` and require green.
5. Run `cargo run -p xtask -- readme-version-check` and require green.
6. Run `git grep krishankoenig` and require zero tracked-file matches.
7. Run `git grep -E '/Users/[a-z]+'` and require zero tracked-file matches.
   Path-leak output is release-blocking until scrubbed to `~/` or `$HOME`.
8. Dogfood Gaze on its own release text: pipe `dist/release-notes/vX.Y.Z.md`
   plus the modified `CHANGELOG.md` section through `gaze clean` and verify
   zero detections. This preserves the `feedback-dogfood-gaze-on-own-output`
   memory.
9. Verify benchmark claims in release notes link to the script and hardware
   specification that produced them. This preserves the
   `feedback-bench-claims-reproducible` memory.

If any step fails, stop and fix the release branch. Do not tag around a red
checklist.

## Tag Procedure

Only after explicit user lock signal:

```bash
git tag -a vX.Y.Z -m "vX.Y.Z" <merge-sha>
git push origin vX.Y.Z
```

Use an annotated tag on the merge commit. Do not tag a local-only commit, an
unmerged branch head, or a dirty working tree.

The tag push auto-fires two workflows:

- `release.yml`: builds binaries and creates the GitHub Release.
- `publish-crates.yml`: publishes the 10 crates via OIDC trusted publishing.

Do not publish to crates.io manually. The workflow owns publication order and
idempotent retries.

## Post-Tag Verification

After the workflows finish:

1. Confirm `gh release view vX.Y.Z` returns the release.
2. Confirm both workflow runs succeeded: `release.yml` and `publish-crates.yml`.
3. Confirm all 10 crates are published by checking
   `https://crates.io/api/v1/crates/<name>` and expecting
   `max_version == X.Y.Z` for:
   `gaze-types`, `gaze-audit`, `gaze-recognizers`, `gaze-pii`,
   `gaze-assembly`, `gaze-mcp-core`, `gaze-mcp-rmcp`, `gaze-document`,
   `gaze-proxy`, and `gaze-cli`.
4. Update the orchestrator scratchpad with released URLs:
   GitHub Release URL plus the 10 crates.io URLs.

## Escalation Rules

- Do not push tags without an explicit user lock signal.
- Do not publish to crates.io manually.
- Do not amend, delete, or force-push a tag after it has been pushed.
  If a released tag is wrong, make a new patch release.
- Do not put local absolute paths in release notes, PR bodies, or commit
  messages. Use `~/` or `$HOME`.
- If release notes need prose changes, use the `release-notes` skill before
  publishing the GitHub Release body.

## Counter-Pattern

Do not just push a tag and hope. A Gaze release is complete only when the
pre-flight gates are green, the tag was explicitly authorized, both workflows
succeeded, all 10 crates report the expected version, and the orchestrator
scratchpad records the shipped URLs.
