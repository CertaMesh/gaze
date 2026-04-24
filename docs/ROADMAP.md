---
title: Gaze roadmap — v0.4, v0.4.1, v0.5
status: live (updated 2026-04-24)
audience: humans + AI agents
---

# Gaze roadmap

Strategic overview of Gaze's next-major directions. This doc is **not** a
spec — specs live under `docs/roadmap/v0.<N>/`. Locked decisions are cited
by MemPalace drawer ID rather than duplicated inline so the memory graph
stays the source of truth.

## North star (quick reference)

> **Gaze is the most reliable, reversible PII pseudonymization runtime for
> agentic workflows. Zero PII leaks between the agent and the data owner —
> ever. Any byte of PII that reaches an LLM outside the manifest contract is
> a critical defect.**

Every roadmap decision is evaluated against five axes:
**reliability · reversibility · agentic-first · trust · adopter-ergonomics**.

Correctness axes 1–4 always beat performance. Full rationale, word-choice
justification ("pseudonymization" over redaction/obscuring/tokenization),
and drift-measurement policy are in MemPalace drawer
`drawer_gaze_decisions_ba559e1cf1fbca5c1098b12f` (wing=gaze, room=decisions).

See also `docs/research/gaze-first-principles-vision.md`.

---

## Current state — v0.3.0 shipped 2026-04-24

v0.3.0 delivers the first adopter-ready Gaze runtime: `gaze clean` and
`gaze restore` CLI with two-pass restore (Pass 1 manifest lookup plus Pass 2
hallucination trap), angle-bracket counter-family tokens (`<Email_1>`,
`<Custom:order_id_1>`) with format-preserving bare emails, `policy.toml`
configuration surface, SQLite redaction log, and the Laravel adapter
(`gaze-laravel`) that pipes requests through the binary. The homebrew
formula `Naoray/gaze/gaze` resolves to the real release artifact, and the
drift-gate fixture compile-errors if `PiiClass` grows without a matching
grammar update.

Known spec-drift scheduled for the v0.3.1 patch (already landed on main as
of 2026-04-24 — see `b70947d`):

- `[session]` policy.toml key now authoritative over `--session-ttl`
- Broken `[ner] model_dir` exits `PolicyConfig` (exit code 2) instead of
  silently degrading
- `kind = "column"` policy rules rejected by `gaze clean` CLI load

Live binary + homebrew formula + adopter deployment (gaze-laravel) are
production. Next focus is v0.4 engine/corpus separation.

---

## v0.4 — engine/corpus split + tenant PII (target: weeks)

### Why

- **Adopter signal** from `gaze-laravel` #5: tenant-specific PII (Dashboard
  order IDs, song titles, artist names) slips past regex + Davlan NER.
  Generic detectors never close this class.
- **Engine/corpus separation** lets the rulepack evolve independently of
  the engine, so adopters can ship their own TOML rulepack without forking.
- **Session-scoped tokens** close the #44 cross-session scope-isolation bug
  at the grammar layer, replacing the earlier text-provenance fingerprint
  idea with something deterministic and auditable.

### Phases

Each phase is a solo todo epic. Follow the epic for acceptance criteria.

**Phase 1 — Foundation (solo #34).** F1 crate split
(`gaze` / `gaze-recognizers` / `gaze-cli`). F2 `RecognizerRegistry` trait.
F3 TOML rulepack schema. F4 locale infra (DACH + EN). F5 `.invalid`
domain handling. F6 Dictionary detector. Typed `Context` envelope. Token
grammar becomes session-scoped: `<{8-hex}:{Class}_{N}>`. Pass 2 regex
splits into a two-branch form (prefixed manifest-lookup + unprefixed trap
for fail-closed). `SnapshotPayload` envelope byte bumps 1 → 2.
**Status:** multi-review loop; Layer A impl auto-fires on review clear.

**Phase 2 — Engine (solo #35).** Overlap resolver, context-aware enhancer,
validator trait, in-house validator implementations (Luhn, IBAN MOD-97,
IPv4/IPv6 parser, VIN). Depends on Phase 1 F2 traits.

**Phase 3 — Validators (solo #36).** External phone-parser crate audit +
integration. Isolated from Phase 2 because dep-audit is its own
workstream.

**Phase 4 — Recognizers (deferred → v0.4.1).** Tier 1 structured set
(email, credit-card, IBAN, phone, IP). Does **not** block v0.4.0 ship.

**Phase 5 — Quality + docs (solo #38).** FP-budget CI gates, gold
fixtures, rulepack authoring guide, migration guide v0.3 → v0.4. A
dual-audience documentation subtask (solo #71) and a date-shift trade-off
doc (solo #67) land in this phase.

### Locked decisions (do not re-litigate)

Source drawer: `drawer_gaze_decisions_0dde54dfb4fb679bd0150ca4` (Q7.1
re-lock) plus the session-lock drawers under wing=gaze room=decisions.

- **Locale:** DACH (`de-DE`, `de-AT`, `de-CH`) + EN
  (`en-US`, `en-GB`, `en-IE`, `en-AU`, `en-CA`).
- **SemVer:** clean major break. No v0.3 → v0.4 shims.
- **Restore mode:** `--restore-mode={strict|tolerant}`, default strict.
- **FakeGaze O3/O4:** deferred to the v0.4 Laravel adapter pass.
- **Tier 1 structured recognizers:** deferred to v0.4.1.
- **F7 text-provenance fingerprint:** REMOVED. Scope-isolation closes at
  the grammar layer via session-scoped tokens; no `SnapshotPayload` v=2
  fingerprint field, no `ProvenanceMismatch` stderr variant.
- **Session-hex width:** 32 bits (8 hex chars). Collision probability
  1/4.3B per pair — acceptable for single-worker scope isolation.
- **Pass 2 regex:** two-branch — prefixed manifest-lookup + unprefixed
  trap. LLM hallucinations without a session prefix fail closed.
- **Q8.3:** rebase-often. No main-freeze during the long-lived
  `v0.4-phase-1` branch.
- **Q-Dispatch:** Layer A auto-fires post-review-clear per orchestrator
  compass.

---

## v0.4.1 — Tier 1 corpus + safety-net composition

Follow-up minor after v0.4.0 ships. Closes the baseline structured-PII
surface and adds neural safety-net composition.

### Why

- Tier 1 structured (email, credit-card, IBAN, phone, IP) is the
  research-recommended baseline every PII runtime should cover. Research
  source: solo scratchpad 9 (`research/gaze-vnext`).
- OpenAI released an open Privacy Filter on 2026-04-22. Gaze treats it as
  a composable Pass-3 safety net — not a dependency, not a primary
  detector. This strengthens the reliability axis without compromising
  the reversible moat.

### Workstreams

- **Phase 4 landing (solo #37).** Ship all 5 Tier 1 recognizers against
  the v0.4 `RecognizerRegistry` trait.
- **F8 OpenAI-filter Pass-3 safety net recognizer (solo #65).** Optional
  last-line recognizer; outputs detect-only tokens for free-text classes
  the rule layer missed.
- **Phase 5 Q2 CI sanity gate (solo #66).** Runs OpenAI Privacy Filter
  over the Gaze gold fixtures in CI as a second opinion; failures surface
  as advisory, not blocking (reversibility > detection recall).

---

## v0.5 — Tenant reverse-index + grammar evolution (longer horizon)

Post-v0.4.1 direction. Adopter feedback from v0.4 informs what lands
first. Research anchor: solo scratchpad 9, §"Engine architecture proposal".

### Why

- The proper solve for adopter-specific PII (Markus's repeating request
  across v0.1/v0.2/v0.3) is a tenant-aware reverse index — not another
  rulepack expansion. The v0.4 Dictionary detector is a step; v0.5
  generalizes it.
- Token-grammar edges (session-hex collisions, edited-text restores)
  surface only under real adopter load. v0.4 ships the grammar; v0.5
  polishes based on incidents.

### Candidate workstreams

- **Tenant reverse-index engine.** Dashboard adopter's full-catalog PII
  lookup. Superset of v0.4 F6 Dictionary. Aho-Corasick over 100k+
  entries, memory-mapped, rebuildable per-tenant.
- **Token-grammar evolution.** Review `session_hex` width against real
  incident data; possible move to 64-bit hex if >65k-token-per-session
  workloads emerge. Or hash-chained tokens if collision patterns are
  observed.
- **OpenAI-filter as primary NER backend.** If v0.4.1 safety-net usage
  shows OpenAI's model outperforms Davlan/GLiNER on DACH + EN
  person/location, consider swap as default NER. Keep rule layer
  authoritative.
- **Edited-text restore semantics.** `--restore-mode=tolerant-edited`
  for LLM paraphrase-heavy flows where tokens get rewritten but
  manifest entries still match by session-hex prefix.
- **OWASP ASVS class mapping.** Compliance docs — only if adopter demand
  surfaces.
- **Windows support.** v0.3 deferred because of `libc::mlock` /
  `madvise`; revisit with conditional compilation once adopter signal
  justifies the maintenance cost.

### Non-goals for v0.5

- **Operations-proxy** (separate project — see MemPalace drawer
  `project_operations_proxy_idea`).
- **General-purpose data scrubbing.** Gaze stays agentic-first. Generic
  data masking is a different product; the north star rejects that
  dilution.

---

## Guidance for AI agents working on this roadmap

When you pick up a roadmap item:

1. `mempalace_search wing=gaze` for the relevant locked-decision drawers
   cited above. Read the drawer verbatim before proposing changes.
2. Read the solo todo (#34–#38, #65, #66, #67, #71) for acceptance
   criteria and current status comments.
3. Follow the orchestrator-mode spec-formalization flow:
   brainstorm → plan → multi-review → dispatch impl.
4. **Never re-open locked decisions.** File a new decision drawer if
   evidence requires change, and surface the change to the user before
   acting.

Current plan revision lives in solo scratchpad 51
(`plan/v0.4-phase-1`). Architectural source of truth is solo scratchpad
9 (`research/gaze-vnext`).

---

## Revision policy

This doc is living. Update when:

- A phase ships → move details from the active section to "Current state".
- A locked decision changes → cite the new drawer ID and note what
  superseded the old lock.
- An adopter signal shifts priority → cite the signal (GitHub issue,
  drawer, scratchpad) in the edit.

Do **not** update for minor status changes — that is what solo todos are
for. The roadmap is strategic overview; the todos are execution state.

---

## References

- **North star:** MemPalace drawer
  `drawer_gaze_decisions_ba559e1cf1fbca5c1098b12f`
- **Q7.1 session-scoped tokens re-lock:** MemPalace drawer
  `drawer_gaze_decisions_0dde54dfb4fb679bd0150ca4`
- **Research report:** solo scratchpad 9 (`research/gaze-vnext`)
- **Current phase-1 plan:** solo scratchpad 51 (`plan/v0.4-phase-1`)
- **Phase-1 brainstorm state:** solo scratchpad 46
  (`brainstorm/v0.4-phase-1`)
- **First-principles vision:** `docs/research/gaze-first-principles-vision.md`
- **v0.3 spec archive:** `docs/roadmap/v0.3/cli.md`,
  `docs/roadmap/v0.3/laravel.md`
- **Project rules:** `AGENTS.md`, `CLAUDE.md`
- **Solo todo epics:** #34 Phase 1 · #35 Phase 2 · #36 Phase 3 · #37
  Phase 4 (v0.4.1) · #38 Phase 5 · #65 F8 OpenAI-filter · #66 CI sanity
  gate · #67 date-shift trade-off · #71 dual-audience docs
