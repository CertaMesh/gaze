---
name: release-notes
description: Draft or review GitHub release notes for gaze, gaze-lens, or gaze-laravel. Use curated prose with inline PR links, limits, migration steps, downloads, and a CHANGELOG link.
---

# Release notes

Use for GitHub Releases. Keep `CHANGELOG.md` in Keep a Changelog format. The [v0.4.5 release](https://github.com/CertaMesh/gaze/releases/tag/v0.4.5) is the voice reference; auto-generated Added/Changed dumps need rewriting.

## Required structure

Keep this order. Omit Known limitations only when none ship.

| Section | Content |
| --- | --- |
| Opening | 1–3 sentences: release id, bundle context, and main changes. |
| `## TL;DR` | One prose paragraph covering major changes, feature gates, and behavior shifts. |
| `## Highlights` | One paragraph per major item, usually at most four: `[Feature] landed in PR #N. [What] using [how]. [Adopter implication].` |
| `## Known limitations` | Name each accepted gap, link its `docs/` page, and state the planned fix release or public GitHub follow-up. Never include private tracker ids. |
| `## Adopter notes` | Defaults, fail-closed paths, repository/distribution moves, and exact migration commands. State platform/dependency versions precisely, such as `glibc 2.39+`. |
| `## Download` | Binary and SHA256 sidecar links per platform: Apple Silicon macOS, Linux x86_64, then others. |
| `## Full CHANGELOG` | One link to the release anchor, for example `CHANGELOG.md#045`. |

Tie the opening or TL;DR to Gaze’s contract: fail closed, preserve reversibility, keep PII out of agent-visible surfaces.

## Voice rules

Use past-tense, third-person prose with artifacts as subjects. Put PR anchors inline. Preserve exact crate, flag, feature, and error names. Use prose for Highlights and bullets for downloads. No first person, emojis, marketing claims, backslash-escaped backticks, or Added/Changed sections.

Example: “DE and US national phone recognizers landed in PR #58. They use `phonenumber` for E.164 region-aware validation through `core-extended`.”

## Worked skeleton

```markdown
[Release id, bundle context, main changes, and Gaze contract.]

## TL;DR

[One paragraph: major changes, gates, behavior shifts.]

## Highlights

[Feature] landed in PR #N. [What] using [how]. [Adopter implication].

## Known limitations

[Gap, docs link, planned fix or public follow-up. Omit only if none.]

## Adopter notes

[Exact migration commands, defaults, distribution and platform constraints.]

## Download

- Apple Silicon macOS: [URL for gaze-aarch64-apple-darwin]
- SHA256: [URL for gaze-aarch64-apple-darwin.sha256]
- Linux x86_64: [URL for gaze-x86_64-unknown-linux-gnu]
- SHA256: [URL for gaze-x86_64-unknown-linux-gnu.sha256]

## Full CHANGELOG

[Absolute repository URL for CHANGELOG.md#XYZ]
```

## Pre-publish checklist

Check section order, opening length, contract tie, inline PR anchors, exact names/constraints, documented gaps, migration steps, binary/checksum pairs, and the correct changelog anchor. Check voice rules above.

## Failure mode: starting from auto-generated notes

Copy generated notes to a scratch buffer. Group major changes into Highlights prose; leave minor changes in CHANGELOG. Fill the skeleton, run the checklist, and replace the release body.

## Out of scope

Commits follow AGENTS.md. PRs follow the PR template. CHANGELOG retains its own format; website marketing has its own voice.
