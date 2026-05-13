---
name: release-notes
description: Use when drafting GitHub release notes for piinuts/gaze, gaze-lens, or gaze-laravel — including new releases, RC pre-releases, and re-writes of auto-generated notes. Enforces the curated-narrative voice destilled from v0.4.5 (TL;DR → Highlights → Known limitations → Adopter notes → Download → CHANGELOG link), past-tense PR-anchored prose, and a North-Star tie. Counter-pattern: auto-generated Keep-a-Changelog dumps with Added/Changed bullets are NOT acceptable as release-notes voice (they are fine in CHANGELOG.md only). Trigger phrases: "draft release notes", "release notes for vX.Y.Z", "rewrite the release body", "publish v…".
---

# Release Notes — Voice & Structure for piinuts/gaze

Canonical voice, locked 2026-04-29 by Naoray. Reference release: [v0.4.5](https://github.com/piinuts/gaze/releases/tag/v0.4.5). Counter-example (do NOT imitate): [v0.5.2](https://github.com/piinuts/gaze/releases/tag/v0.5.2) — auto-generated Keep-a-Changelog dump with Added/Changed bullets and backslash-escaped backticks.

This skill applies to release notes published on GitHub Releases. It does NOT replace `CHANGELOG.md` — that file stays in Keep-a-Changelog format. Release notes are the curated narrative on top of the changelog.

## When to use

- Drafting GitHub release notes for any `piinuts/*` repo (gaze, gaze-lens, gaze-laravel, future siblings).
- Rewriting auto-generated release bodies that arrived as raw CHANGELOG dumps.
- Reviewing a release-notes PR before publish — gate against this skill's checklist.

## Required structure (in this order, do not reorder)

```
[Opening paragraph]
## TL;DR
## Highlights
## Known limitations
## Adopter notes
## Download
## Full CHANGELOG
```

Skip-allowed sections: `## Known limitations` may be omitted if the release truly has none. Every other section is required.

### 1. Opening paragraph (1–3 sentences)

- Release identifier + bundle context. Examples: `"v0.4.5 is the S1-S6 bundle release for the v0.4 line."`, `"v1.2.0 ships the WebSocket replay rewrite alongside two follow-up bug fixes."`
- High-level statement of what the release adds, in one breath.
- North-Star tie: at least once anchor the release against `"fail closed, preserve reversibility, keep PII out of agent-visible surfaces"` or a faithful paraphrase. The tie shows up here OR in the TL;DR — at least one of the two must carry it.

### 2. ## TL;DR

One paragraph. Dense narrative. No bullets. Covers all major deltas in one flow so a busy adopter can read the section and stop. Should answer: what landed, what flag/feature gates it, what the most important behavior shift is.

### 3. ## Highlights

Prose paragraphs (NOT bullets), one per major item. The pattern:

> [Feature/Subsystem] landed in PR #N. [WHAT it does] using [HOW, in one sentence — crate, feature, validator, gate]. [WHY / implication for adopters or for the trust contract.]

Hard rules:

- **Past-tense, third-person.** "DE and US national phone recognizers landed", not "We added DE/US national phone recognizers" and not "Add DE/US recognizers".
- **PR anchor inline.** Always `"…landed in PR #58"` inside the prose, never as a footer reference.
- **Subjects are artifacts, not authors.** "the gate enforces", "the recognizer validates", "`gaze audit purge` deletes". Do not introduce humans.
- **Exact crate / flag / feature names.** `core-extended`, `phone-parser`, `--rulepack-bundled`, `RulepackError::UnsupportedValidator` — verbatim. No paraphrase.
- **No marketing words.** Forbidden vocabulary: blazing, powerful, seamless, now you can, magic, effortless. If a sentence reads like a tweet from a startup CEO, rewrite it.
- **No emojis.**

### 4. ## Known limitations

Accepted-risk paragraph. Names intentional gaps the release ships with. Always references:

- The `docs/` page that documents the limitation in detail.
- The follow-up todo number or planned-fix release (`"see todo #181"`, `"planned for the v0.5 dylint pivot"`).

This section is a trust-signal. It is the strongest competitive differentiator vs. Presidio and Tonic — they don't publish their gaps, we do. Phrase as `"intentionally syntax-based"`, `"documented accepted-risk for this release"`, never as an apology.

### 5. ## Adopter notes

Behavior shifts that adopters need to know about, with concrete migration recipes. Includes:

- Default-behavior changes (e.g. new locale activation, new fail-closed paths).
- Repository-URL or distribution moves.
- Platform / dependency constraint changes (versioned exact: `"glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer)"`, never `"modern Linux"`).
- Migration recipes as exact commands or flag changes (`"To restore prior behavior, pass --locale=global or use a policy with narrower locale gating."`).

### 6. ## Download

Bullet list. Per platform: binary URL, then SHA256 sidecar URL. Order by adopter mass: Apple Silicon macOS, Linux x86_64, then any others. Future Linux aarch64 and Apple Silicon SHA-2 alternatives go in the same pattern.

### 7. ## Full CHANGELOG

Single line, single link. Anchor to the release section in `CHANGELOG.md`:

```
https://github.com/piinuts/gaze/blob/main/CHANGELOG.md#045
```

## Voice rules (quick reference)

| Rule | Example | Counter-example |
| ---- | ------- | --------------- |
| Past-tense narrative | "Audit retention manual purge landed in PR #59." | "Add `gaze audit purge` command." |
| Third person, no we/you | "Adopters using the bundle without a policy may see…" | "You'll see new tokens for…" |
| PR anchor inline | "…landed in PR #58." | "PR: #58" in footer |
| Subjects = artifacts | "the gate enforces", "the recognizer validates" | "We added a gate that…" |
| Exact constraints | "glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10)" | "modern Linux distros" |
| North-Star tie | "fail closed, preserve reversibility, keep PII out of agent-visible surfaces" | (omitted) |
| Limits before adopter notes | `## Known limitations` precedes `## Adopter notes` | adopter notes first |
| No hype words | "uses the `phonenumber` crate for E.164 region-aware validation" | "blazing-fast E.164 validation" |
| No emojis | (plain text) | 🚀 ✨ 🎉 |

## Worked skeleton

Copy the block below into the GitHub release body and fill in. Keep the section headers verbatim.

```markdown
[OPENING — 1-3 sentences. Release id + bundle context + high-level new + North-Star tie if not in TL;DR.]

## TL;DR

[ONE paragraph dense narrative. No bullets. All major deltas in one flow. North-Star tie if not in opening.]

## Highlights

[FEATURE A] landed in PR #N. [WHAT it does] using [HOW]. [WHY / implication.]

[FEATURE B] landed in PR #N. [WHAT it does] using [HOW]. [WHY / implication.]

[Optional: third-and-beyond highlights paragraph. Stop at 4 unless release truly has more standalone items.]

## Known limitations

[ACCEPTED-RISK paragraph. Document the gap, link to `docs/` page, reference follow-up todo / planned release. Phrase honestly, never apologetically.]

## Adopter notes

[BEHAVIOR SHIFT 1] — [migration recipe with exact flag/command].

[BEHAVIOR SHIFT 2] — [migration recipe].

[Repository / distribution / platform constraint changes, exact versions.]

## Download

- Apple Silicon macOS: https://github.com/piinuts/gaze/releases/download/vX.Y.Z/gaze-aarch64-apple-darwin
- Apple Silicon macOS SHA256: https://github.com/piinuts/gaze/releases/download/vX.Y.Z/gaze-aarch64-apple-darwin.sha256
- Linux x86_64: https://github.com/piinuts/gaze/releases/download/vX.Y.Z/gaze-x86_64-unknown-linux-gnu
- Linux x86_64 SHA256: https://github.com/piinuts/gaze/releases/download/vX.Y.Z/gaze-x86_64-unknown-linux-gnu.sha256

## Full CHANGELOG

https://github.com/piinuts/gaze/blob/main/CHANGELOG.md#XYZ
```

## Annotated reference — v0.4.5 (the canonical example)

The opening of v0.4.5 carries every voice rule in one paragraph:

> v0.4.5 is the S1-S6 bundle release for the v0.4 line. It adds parser-backed national phone recognition, ships manual audit retention purge, and hardens restore-path audit boundaries while keeping the release focused on Gaze's core contract: fail closed, preserve reversibility, and keep PII out of agent-visible surfaces.

Notice: release-id + bundle context + three deltas in one sentence + explicit North-Star tie at the end. No emojis, no marketing words, no first person.

The Highlights paragraphs in v0.4.5 follow the pattern strictly:

> DE and US national phone recognizers landed in PR #58. The recognizers use the `phonenumber` crate for E.164 region-aware validation, cooperate with the broader structural phone recognizer, and are available through the bundled `core-extended` rulepack.

Subject = "the recognizers", verbs = past-tense + present descriptive, PR anchor inline, exact crate name (`phonenumber`), exact bundle name (`core-extended`).

The Known-limitations paragraph in v0.4.5 demonstrates the trust-signal frame:

> The v0.4.5 `audit_metadata_only` gate is intentionally syntax-based. It covers natural-code escape routes that have caused drift risk in this codebase, but fully-qualified path references, `include!`, let-else diverge cases, and macro-emitted imports remain documented accepted-risk limitations for this release. See `docs/architecture/xtask.md` for the current coverage table and `PIInuts/business:research/v0.5-dylint-audit-gate.md` for the planned v0.5 pivot to a dylint-based name-resolution lint (todo #181).

Pattern: `"intentionally"` framing + named exception classes + two `docs/` links + one todo number for the planned-fix release.

## Counter-example — what NOT to do

The v0.5.2 release body is a Keep-a-Changelog dump:

```
### Added
- **NER adopter assets (todo #301, GH issue #90 items 1+4):** promoted...

### Changed
- **Pinned default NER artifact source (todo #292, GH issue #90 item 2):** \`scripts/...\`
```

Why this fails the voice:

- `### Added` / `### Changed` headers are CHANGELOG schema, not release-notes structure.
- Backslash-escaped backticks (`\`scripts/...\``) reveal an auto-generation step that did not get human-curated before publish.
- No TL;DR, no Highlights prose, no Known-limitations section, no Adopter-notes structure.
- "PR: #94 (squash-merge `4776ff49`)" sits in a footer block instead of inline anchors.

If you find yourself starting from this shape, treat it as raw input and rewrite into the curated structure above before publishing.

## Pre-publish checklist

Run this checklist before clicking Publish on GitHub Releases. Every item must pass.

- [ ] Opening paragraph present, 1–3 sentences, names the release id and what it ships.
- [ ] North-Star tie present in opening OR TL;DR ("fail closed, preserve reversibility, keep PII out…" or faithful paraphrase).
- [ ] `## TL;DR` is one prose paragraph, no bullets.
- [ ] `## Highlights` paragraphs follow `[Feature] landed in PR #N. [WHAT] using [HOW]. [WHY].` pattern.
- [ ] Every Highlight references at least one PR inline.
- [ ] Every flag / crate / feature / error type named verbatim (no paraphrase).
- [ ] No first person (`we`, `you`, `our`, `your`).
- [ ] No marketing words (`blazing`, `powerful`, `seamless`, `now you can`, `magic`, `effortless`).
- [ ] No emojis anywhere in the body.
- [ ] No `### Added` / `### Changed` Keep-a-Changelog headers.
- [ ] `## Known limitations` present whenever any intentional gap ships, with `docs/` link + todo number.
- [ ] `## Adopter notes` lists every behavior shift with a concrete migration recipe.
- [ ] Versioned constraints exact (e.g. glibc, MSRV) — no "modern" / "recent" / "latest".
- [ ] `## Download` lists binary + SHA256 sidecar per platform.
- [ ] `## Full CHANGELOG` is a single link to the right anchor in `CHANGELOG.md`.

## Failure mode: starting from auto-generated notes

When `gh release create --generate-notes` or a CI workflow drops a Keep-a-Changelog dump into the body, the correct workflow is:

1. Copy the auto-generated body into a scratch buffer.
2. Open the curated skeleton from this skill.
3. Walk each Added/Changed/Fixed bullet and decide which Highlight paragraph it belongs to. Some bullets merge into a single Highlight; some are too small to mention at all (those stay in CHANGELOG.md).
4. Write the prose. Do not paste bullets back in.
5. Run the pre-publish checklist.
6. Replace the GitHub release body with the curated narrative.

The CHANGELOG.md anchor at the bottom links readers to the raw bullets if they want them.

## Out of scope

This skill does NOT govern:

- `CHANGELOG.md` — that file stays in Keep-a-Changelog format.
- Internal commit messages — those follow the `[agent] …` convention from project CLAUDE.md.
- Pull-request descriptions — those follow the standard PR-template guidelines.
- Marketing copy on `gaze-website` — that has its own voice owner.
