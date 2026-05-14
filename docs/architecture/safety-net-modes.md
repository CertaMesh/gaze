# SafetyNet modes — design

Status: **design-only**, no implementation in this PR. Target ship train: v0.8.x point release, additive.

This document scopes two new `--safety-net-mode` variants — **`redact`** and **`resolve`** — alongside the existing `strict` and `tolerant` modes. It also tightens the production posture of the existing modes. It does not propose any new safety-net backends, manifest contract changes, or new restore semantics for the deterministic path.

Companion doc: a follow-up [`feedback-loop.md`](feedback-loop.md) (placeholder, owned in coordination with the pid-82 README-rewrite track) captures the *mechanics* of how `resolve`-mode promotes safety-net suspects into candidates and runs them through conflict resolution. This doc is the *catalog* and *contract*; that doc is the *plumbing*. The split is intentional — adopters reading "which mode do I pick?" should not have to wade through resolver re-entry to find the answer.

Existing cross-reference: [`docs/architecture/safety-nets.md`](safety-nets.md) is the canonical safety-net trait and observer-only chokepoint contract; this doc layers a *post-detection action policy* on top of that contract without renegotiating any of its invariants.

## TL;DR

- Today's CLI only has two outcomes when a safety net flags a suspect: **fail closed** (`strict`, exit 3, empty stdout) or **ship the leak with a warning** (`tolerant`). Both are blunt instruments and `tolerant` is **explicitly not a production mode** (§3).
- `redact` adds a third outcome: *one-way redact the suspect span and continue*. The cost is reversibility (axis 2) — the redacted bytes are gone for that suspect. The win is axis 1: no leak ships, no exit code, no human-in-the-loop.
- `resolve` adds a fourth outcome: *promote each suspect into a synthetic custom-recognizer match and let the existing conflict resolver decide*. Manifest stays intact, restore round-trips for every emitted token, no new pipeline re-entry point. Naming choice and impl-alt comparison in §6 and §7.
- The existing `SafetyNetMode` enum (`crates/gaze-cli/src/commands/mod.rs:399`) gains two additive variants. The current strict/tolerant semantics at `crates/gaze-cli/src/pipeline/run.rs:774` are unchanged.
- **Recommended ship order: `redact` first, `resolve` second**, both within v0.8.x. Reasoning in §8.
- Concurrent recommendation that ships in the same train: **stderr warning on every `--safety-net-mode tolerant` invocation**, and an open question on tolerant-mode deprecation in v0.9 / removal in v0.10 (§3.3, §10 Q5).
- The biggest open design question: should the `redact` sentinel be adopter-configurable per `policy.toml`, or hard-coded to a single `[REDACTED-by-safety-net]` literal? Lean: configurable with sane default. See §10 Q1.

## 1. Status quo (strict + tolerant)

`enforce_safety_net_mode` at `crates/gaze-cli/src/pipeline/run.rs:774` is the entire policy surface today. It runs after the pipeline emits tokens and after the safety net has produced a `LeakReport`:

```rust
fn enforce_safety_net_mode(
    report: &LeakReport,
    mode: SafetyNetMode,
) -> std::result::Result<(), CliError> {
    let suspected_leaks = report.stats.uncovered_count + report.stats.partial_bleed_count;
    if suspected_leaks > 0 {
        match mode {
            SafetyNetMode::Strict => return Err(CliError::SafetyNetFailure { variant: "SuspectedLeak" }),
            SafetyNetMode::Tolerant => emit_safety_net_warning("SuspectedLeak", suspected_leaks),
        }
    }
    if report.stats.class_mismatch_count > 0 {
        emit_safety_net_warning("ClassMismatch", report.stats.class_mismatch_count);
    }
    Ok(())
}
```

Two facts shape this design:

1. The safety net already produces a `Vec<LeakSuspect>` with a byte span, a mapped `PiiClass`, and a `LeakKind` (`crates/gaze-types/src/lib.rs:894` — `Uncovered | PartialBleed { uncovered } | ClassMismatch { pipeline_class, safety_net_class }`). Both new modes can act on that structured data without any new safety-net API. The trait surface in [`safety-nets.md`](safety-nets.md) is sufficient.
2. The audit row schema (`RedactionEntry` at `crates/gaze-types/src/lib.rs:1465`) already carries a `decided_by: ConflictTier` field (`ConflictTier` at lib.rs:1419) and is `#[non_exhaustive]`. Redact mode adds one new variant. Resolve mode (under the recommended impl path, §7) adds **none**.

## 2. Mode catalog: five-axis matrix

Axes follow [`AGENTS.md`](../../AGENTS.md): **A1** reliability (never leak), **A2** reversibility, **A3** agentic-first, **A4** trust/auditability, **A5** adopter ergonomics.

| Mode       | A1 (no leak) | A2 (reversible) | A3 (agentic)        | A4 (trust)          | A5 (ergonomics) | Latency    | Default eligible |
|------------|--------------|-----------------|---------------------|---------------------|-----------------|------------|------------------|
| `strict`   | full         | full            | bad (exit 3 stalls) | full (typed error)  | low (need retry) | none extra | yes (today)      |
| `tolerant` | **broken**   | full            | great (no stall)    | warning JSON only   | high            | none extra | **no — dev only** |
| `redact`   | full         | partial loss†   | great               | full (audit row)    | high            | tiny       | candidate        |
| `resolve`  | bounded‡     | full            | good (extra pass)   | full (audit row)    | medium          | +1 pass    | candidate        |

† Reversibility is preserved for every token gaze emitted itself. The break is scoped to safety-net suspect spans; restore returns the sentinel unchanged for those spans. Adopters must treat redacted bytes as lost.

‡ `resolve` makes no axis-1 promise unless the second pass succeeds. If the second pass also flags a suspect above threshold, the design (§7.4) falls back to `strict`. So the bound is: *axis 1 holds, but at the cost of an axis-1-style exit 3 on second-pass failure.*

Where each fits in adopter posture:

- **High-throughput agent loop** (multi-turn conversation, blocking on `strict` kills UX): `redact` is the right production default. The agent never sees a hard fail; redacted spans surface to the agent as `[REDACTED-by-safety-net]` which it can self-correct around.
- **Batch document pseudonymization** (the `gaze clean` one-shot CLI use case): `resolve` is the right production default. Latency budget is loose, every byte should round-trip, an extra pipeline pass is cheap.
- **Interactive shell** (developer running `gaze clean | jq` on a single file): `strict` stays the right default. Hard fail surfaces the gap to the human, who fixes the recognizer.
- **Dev / exploratory**: `tolerant` exists for measuring safety-net false-positive rates against known-clean corpora and debugging recognizers — never production. See §3.

## 3. Production posture per mode

| Mode      | Production posture                                  | Adopter signal                                          |
|-----------|------------------------------------------------------|---------------------------------------------------------|
| `strict`  | ✓ **PRODUCTION DEFAULT**                              | Fail-closed on suspect leak. Axis-1 safe.               |
| `redact`  | ✓ Production (with audit-row monitoring)              | One-way redaction; Axis 2 broken but Axis 1 safe.       |
| `resolve` | ✓ Production (with latency budget headroom)           | Manifest-restorable second pass; Axis 1 + Axis 2 safe.  |
| `tolerant`| ✗ **DEV / LOCAL ONLY — never production**             | Ships the leak. Axis-1 violation by design.             |

### 3.1 Why `tolerant` is not a production mode

`tolerant` is the only mode in the table that violates the north-star reliability axis by design. When the safety net flags a suspect leak, the deterministic pipeline already had a chance to detect it and didn't; `tolerant` then proceeds to **ship that suspect to the LLM anyway**. Every byte of PII that reaches an LLM outside the manifest contract is a critical defect ([`AGENTS.md`](../../AGENTS.md)); `tolerant` makes that defect routine.

The mode is preserved in the surface — and not deleted — because it has a legitimate development use case: measuring safety-net false-positive rates against a known-clean corpus, or debugging a recognizer that's missing coverage and wanting to see *what* the safety net catches before deciding how to act on it. In those contexts the operator owns both the input and the output and is not feeding either to an LLM.

> **`tolerant` is not a production mode.** It is provided for development and exploratory use (e.g. measuring safety-net false-positive rates against a known-clean corpus, debugging a recognizer that's missing coverage). Selecting `tolerant` in production violates the north-star reliability axis — the safety net flagged a leak and you shipped it anyway. Adopters MUST surface this restriction in their own deployment runbooks. Future Gaze versions may emit a stderr warning on every `--safety-net-mode tolerant` invocation; the warning is not a bug.

### 3.2 Recommended stderr warning (ships with this work)

The CLI should emit, on **every** `--safety-net-mode tolerant` invocation, a stderr warning. Not gated on TTY. Not silenceable except by switching modes. Goes to stderr so the stdout JSON / clean-text contract is undisturbed.

Suggested literal:

```
WARNING: --safety-net-mode tolerant ships PII leaks the safety net flagged.
         This mode is for development only. See docs/architecture/safety-net-modes.md
         for production-safe alternatives (--safety-net-mode {strict|redact|resolve}).
```

This warning is part of the redact/resolve work, not a separate ticket. Reason: the warning's information density depends on the three production-safe alternatives existing. Shipping the warning without those alternatives would be uselessly nagging; shipping the alternatives without the warning would miss the chance to nudge existing `tolerant` users toward the right mode.

### 3.3 Deprecation trajectory (proposed)

If the warning is acted on broadly, the natural next step is:
- v0.8.x: warning lands alongside `redact` + `resolve`.
- v0.9: `tolerant` is marked **deprecated** in `--help`, `policy.toml` parse warnings, and CHANGELOG. CLI exit on `tolerant` switches from `0` to `0` with a louder stderr block. No behavior break.
- v0.10: `tolerant` is **removed** from `SafetyNetMode`. Adopters who still need the dev affordance can pin v0.9 or use `--safety-net-mode redact` and ignore the manifest delta during their corpus measurement.

This is an open question (§10 Q5), not a committed plan. The deprecation trajectory should be confirmed with adopter signal first.

## 4. `redact` mode contract

### 4.1 Sentinel choice

Three options were considered:

1. **Hard-coded literal**: `[REDACTED-by-safety-net]`. Simple, no policy surface, no token vocabulary contamination.
2. **Manifest-shaped sentinel**: `<{session_hex}:UnknownPii_N>`. Looks like a normal gaze token, lives in the same vocabulary downstream tooling already lexes.
3. **Adopter-configurable per policy**: a string field in `policy.toml`, e.g. `[policy.safety_net] redact_sentinel = "<<REDACTED>>"`.

**Recommendation: option 3, with option 1 as the default literal.** Rationale:

- Option 2 lies. A `<session:UnknownPii_N>` token implies a manifest entry exists — it does not for redact mode. Restore tooling would try to look it up, miss, and fail confusingly. The whole point of redact mode is that the byte is *gone*; the wire form should announce that.
- Option 3 with a sane default (`[REDACTED-by-safety-net]`) lets adopters who pipe gaze output into LLMs choose a sentinel the model is unlikely to repeat verbatim. The Laravel adapter, for example, may want `<<safety-net-redacted>>`.
- The string is validated at policy load — must be non-empty, must not collide with the existing manifest token grammar (`<hex:Class_N>`). Validation is fail-closed at load time (axis 4).

### 4.2 Manifest: explicit non-entry policy

A `redact`-mode action **MUST NOT** emit a manifest entry. The pipeline's deterministic emission is unchanged; only the suspect span is mutated post-emission, after the manifest is sealed. This keeps `Manifest::diff_against` correct and keeps the restore-round-trip property of every gaze-emitted token intact.

Concretely:
1. Pipeline runs, emits manifest M and clean text C.
2. Safety net checks C against M, returns `Vec<LeakSuspect>`.
3. For each suspect with `LeakKind::Uncovered` or `LeakKind::PartialBleed`, the redact path overwrites the suspect byte range in C with the sentinel string. M is not touched.
4. A new audit row is appended (see §4.3).
5. The mutated C is what ships to stdout.

If a suspect's span straddles an existing token boundary — i.e. `PartialBleed { uncovered }` — only the `uncovered` sub-range is overwritten. The gaze-emitted token bytes inside the suspect span survive untouched. This preserves manifest validity.

### 4.3 Audit-row contract

Add one variant to `ConflictTier` (`crates/gaze-types/src/lib.rs:1419`):

```rust
pub enum ConflictTier {
    // ... existing variants unchanged ...
    /// Safety net redacted a suspect span; no manifest entry exists.
    SafetyNetRedacted,
}
```

Emit one `RedactionEntry` per redacted suspect with:

- `source` = `"safety_net.<backend>.v<N>"` (e.g. `"safety_net.kiji.v1"`, `"safety_net.openai_filter.v1"`).
- `recognizer_id` = backend id from `SafetyNet::id()`.
- `class` = `SafetyNetPiiClass::to_pii_class()` of the safety-net label (`crates/gaze-types/src/lib.rs:1309`, `:1330`).
- `action` = `Action::Redact` (existing, `crates/gaze-types/src/lib.rs:1407`).
- `conflict_loser` = `false`. The suspect was not in conflict with another candidate; it was an action *on* the post-emission stream.
- `decided_by` = `ConflictTier::SafetyNetRedacted`.

The audit row carries the action and the byte range. It does **not** carry the original suspect bytes — that would re-introduce the leak into the audit DB. This is consistent with the existing safety-nets contract (no raw bytes cross the adapter boundary, see [`safety-nets.md`](safety-nets.md)).

Update the string mapping in `redaction_conflict_tier_as_str` at `crates/gaze-types/src/lib.rs:1559` to cover the new arm with `"safety_net_redacted"`.

### 4.4 Restore behavior

`gaze restore` reads the manifest and detokenizes manifest tokens back to their original bytes. Sentinel strings are not manifest tokens. They will pass through unchanged. **This is the intended behavior** and must be documented in `UPGRADE.md` and the `gaze restore` CLI help:

> Spans redacted by a safety net in `--safety-net-mode redact` are one-way. `gaze restore` returns the sentinel string as-is. Adopters who need full reversibility should use `--safety-net-mode resolve` or `strict`.

This is the single axis-2 exception in the entire gaze design, and it is *explicit*. No silent byte-loss. Restore tooling can detect the sentinel and surface a clear "redacted by safety net at $bytes" diagnostic if desired.

### 4.5 Failure paths

Three failure conditions and the recommended fallback:

1. **Safety net initialization fails** (subprocess not found, model not loadable). Fall back to `strict`. Reason: axis 1 wins. An adopter who asked for redact-mode was explicitly trading reversibility for liveness; if the safety net itself is down, we cannot honor that trade.
2. **Safety net returns `LeakKind::ClassMismatch`**. ClassMismatch means the deterministic pipeline already tokenized the span, just with the "wrong" class. **Redact mode does NOT act on class mismatches** — the manifest is intact, restore round-trips, and overwriting the gaze-emitted token would corrupt the manifest. Warn on stderr (existing path) and continue.
3. **Sentinel write changes byte length of stream** (it always does, since suspect span and sentinel are not generally byte-equal). Acceptable. Downstream tooling that depends on byte offsets relative to the original input must use the manifest, not the clean stream.

### 4.6 `leak_report` JSON exposure

The `LeakReport` JSON emitted on stderr in tolerant mode (today) and in the audit DB (always) **should** gain a per-suspect `action_taken` field: `"redacted" | "resolve_recovered" | "resolve_failed" | "none"`. This lets agent-loop adopters introspect: "the safety net dropped these N bytes; I should ask the user to confirm."

## 5. `resolve` mode contract

The mechanism here has two viable implementations (§7). The *contract* is the same regardless of mechanism:

- Every suspect above `--safety-net-resolve-threshold` becomes a candidate for tokenization.
- Existing conflict resolution decides whether the safety-net candidate wins, ties, or loses.
- Manifest grows by the number of winning safety-net candidates. Every winning candidate is fully restorable.
- The safety net is invoked **once more** against the new clean text. If suspects above threshold remain, fall back to `strict` (`CliError::SafetyNetFailure { variant: "ResolveExhausted" }`, exit 3).

### 5.1 Threshold semantics

Add `--safety-net-resolve-threshold <float>` (default `0.7`). Suspects below threshold are dropped *before* candidate construction. If, after dropping, no suspects remain, resolve is a no-op and the pipeline ships the original clean text. If suspects remain but all are below threshold, fall back to `strict`.

Threshold of `0.0` disables filtering (every suspect is reused). Threshold of `1.0` disables resolve entirely (functionally equivalent to `strict`).

The CLI flag is `policy.toml`-overridable under `[policy.safety_net] resolve_threshold = 0.7`.

### 5.2 Loop termination

**Cap at one resolve pass.** Axis-4 (trust): bounded behavior is auditable. A user can reason about "at most one extra pass." Unbounded retries risk pathological backoff under adversarial input. If a third pass would have helped, the *recognizer* needs to be improved, not the loop count. Open question for v0.9: see §10 Q2.

### 5.3 Class taxonomy mapping

Reuses the existing per-backend `class_map.rs` modules (`crates/gaze-recognizers/src/safety_net/kiji_distilbert/class_map.rs`, `crates/gaze-recognizers/src/safety_net/openai_filter/class_map.rs`). No new mapping surface. New backends inherit this pattern.

## 6. Naming choice: `resolve` vs `rerun`

**Locked: `resolve`.** We chose `resolve` over `rerun` because the verb conveys "fixes the gap" from the adopter's perspective, where `rerun` is mechanism-shaped. (User confirmation, 2026-05-14.)

Both verbs describe the same observable behavior. They differ in what they *emphasize* to an adopter reading the CLI help:

- **`rerun`** — mechanism-honest. Conveys "we run the pipeline a second time with the safety net's suspects added in." The adopter reads `--safety-net-mode rerun` and knows the cost model is "one extra pass." Downside: the verb fixates on the *how*. An adopter who reads only the help text might think the second pass is the point, when in fact the point is *closing the gap the deterministic recognizers missed*.
- **`resolve`** — outcome-honest. Conveys "we fix the gap the safety net flagged." Adopter reads `--safety-net-mode resolve` and knows the *intent* is resolution, not retry. Downside: the verb hides the latency cost. An adopter benchmarking gaze in CI might be surprised by the second pass.

**Recommendation: `resolve`.** Reason: per axis-5 (adopter ergonomics), the CLI help text is the right place to explain *intent* — that's the criterion the adopter is shopping on when they pick a mode. The latency cost belongs in this doc, in `--help`'s detailed description, and in the `policy.toml` comments — not in the mode name itself. The pid-82 README-rewrite track is also gravitating toward `resolve` for adopter-facing copy; aligning the CLI flag with the marketing copy reduces friction.

The original draft of this doc used `rerun`. References to `rerun` elsewhere in the gaze codebase (if any) are pre-implementation and can be renamed without breakage.

## 7. Implementation alternative: synthetic Candidate injection vs. Custom-recognizer promotion

Two viable mechanisms reach the same contract from §5. The choice is an axis-4 (trust / cleanliness) and axis-5 (adopter ergonomics) call; both produce equivalent observable behavior.

### 7.1 Option A: Synthetic Candidate injection (original draft)

For each suspect above threshold, construct a synthetic `Candidate` and inject it into the resolver at a new entry point. Adds:

- A new `Source::SafetyNet` variant (or equivalent).
- A new resolver re-entry point that accepts a `Vec<Candidate>` and runs only the merge + resolution stages.
- A new `ConflictTier::SafetyNetFeedback` variant on the audit row to label the resolver's verdict.
- A new `SAFETY_NET_BASE_RULE_PRIORITY` constant tuned so deterministic recognizers beat safety-net candidates on tied spans.

Cost: two new closed-enum variants (one on `Source`, one on `ConflictTier`), one new code path through the resolver.

### 7.2 Option B: Custom-recognizer promotion (pid-82's proposal)

For each suspect above threshold, register a synthetic entry in the existing `[[policy.custom_recognizers]]` table at runtime — same surface adopters use today to declare custom regex / dictionary recognizers — with `source = "safety_net.<backend>"`, an exact-span anchor pattern, and the safety-net-mapped class. Then re-run the pipeline (or the resolver alone, as an impl detail). The resolver sees these synthetic entries as ordinary custom recognizers.

Cost: zero new closed-enum variants. The audit row's `source` string carries the safety-net identity; `decided_by` is whichever existing `ConflictTier` broke the tie (`RulePriority`, `Score`, etc.). The custom-recognizers code path is well-trodden, well-tested, and already participates in collision-family policy + validator-veto correctly.

### 7.3 Comparison

| Dimension                    | A: synthetic Candidate                  | B: custom-recognizer promotion          |
|------------------------------|------------------------------------------|------------------------------------------|
| New `ConflictTier` variant   | Yes (`SafetyNetFeedback`)                | No                                       |
| New resolver entry point     | Yes                                      | No (reuses existing pipeline path)       |
| Interaction with collision-family policy | Needs explicit tier wiring   | Free — custom recognizers already covered |
| Interaction with validator-veto | Needs explicit tier wiring            | Free — validator-veto runs before resolver |
| Adversarial fixture surface  | Large — new code path                    | Small — exercises existing custom-recognizer fixtures |
| Audit-row delta              | Two new strings (`safety_net_redacted` + `safety_net_feedback`) | One new string (`safety_net_redacted` only, from redact mode) |

### 7.4 Recommendation: Option B (custom-recognizer promotion)

Three reasons:

1. **Axis-4 cleanliness.** Smaller surface delta. One new `ConflictTier` variant (for redact, not resolve) instead of two. The audit-row schema barely moves.
2. **Reuses validated semantics.** Custom recognizers already correctly interact with collision-family policy ([collision-family.md](collision-family.md)), validator-veto ([validator-veto.md](validator-veto.md)), and the locale chain. Synthetic candidates would have to re-prove all of that.
3. **Naming alignment.** `resolve` reads more naturally if the mechanism is "the suspect *becomes a recognizer rule for this run*" than if it's "the suspect bypasses recognizers and joins the resolver directly." The verb and the mechanism converge.

Implementation note (for the impl PR, not this design): the synthetic custom-recognizer entries must be **scoped to the current pipeline invocation only** — they are not persisted to policy, not written to disk, and not visible to subsequent invocations. The `SafetyNet`-promoted recognizers carry an in-memory rule-priority that places them below deterministic recognizers but above the no-rival default, matching the priority rationale in §7.1.

## 8. Closed-enum surface delta

The full additive surface, in one place. Note this is smaller than the original draft due to the §7 recommendation:

```rust
// crates/gaze-cli/src/commands/mod.rs (line 399 today)
// Recommend adding #[non_exhaustive] as part of this work.
pub(crate) enum SafetyNetMode {
    Strict,
    Tolerant,
    Redact,      // NEW (this work)
    Resolve,     // NEW (this work; replaces "rerun" naming from earlier draft)
}

// crates/gaze-types/src/lib.rs:1419 (already #[non_exhaustive])
pub enum ConflictTier {
    None, ClassPriority, RulePriority, Score, SpanLength,
    Validator, ValidatorVeto, CollisionPolicy, AnchoredContext,
    RecognizerId, Merged,
    SafetyNetRedacted,   // NEW — only for redact mode
    // (Resolve mode uses existing ConflictTier variants via custom-recognizer promotion.)
}
```

Also update the string mapping in `redaction_conflict_tier_as_str` at `crates/gaze-types/src/lib.rs:1559` to cover `SafetyNetRedacted` with `"safety_net_redacted"`.

Today's `SafetyNetMode` enum at `crates/gaze-cli/src/commands/mod.rs:399` is **not** annotated `#[non_exhaustive]`. The impl PR for redact mode should add that attribute as part of the same change. This is one-time hygiene, not a contract change.

**`LeakKind` is NOT extended.** `LeakKind` describes *what was found* — uncovered, partially bled, or class-mismatched. The action taken (`redact`, `resolve`, `nothing`) is encoded in the audit row and in the new `LeakReport.action_taken` field (§4.6). Mixing find-and-act into one enum is a category error.

**`Action` is NOT extended.** Redact mode reuses `Action::Redact` (existing variant at lib.rs:1407). Resolve mode reuses `Action::Tokenize`.

## 9. Adopter migration

No breaking changes in v0.8.x. Release notes call out:

- `--safety-net-mode redact` and `--safety-net-mode resolve` are new opt-in modes; default stays `strict`.
- Adopters currently running `--safety-net-mode tolerant` to dodge exit 3 should switch to `redact` (preserves axis 1 at small axis-2 cost) or `resolve` (preserves both axes at one-extra-pass cost) rather than continuing to ship leaks.
- The new stderr warning will fire on every `tolerant` invocation. Adopters with `tolerant` baked into CI workflows should update those workflows to one of the production-safe modes before upgrading.
- The `RedactionEntry.decided_by` column may now contain `"safety_net_redacted"`. Downstream BI queries that string-match this column must add this string to their allow-list.
- Cross-link in [`safety-nets.md`](safety-nets.md) §1 to this doc once the impl PRs land.
- README Quickstart (post pid 82's marketing rewrite) gains a mode comparison table aligned with §2 + §3.
- A short `UPGRADE.md` note for v0.8.x captures (a) the four-mode summary, (b) the redact-sentinel-is-one-way restore caveat, (c) the new audit-row string, (d) the tolerant deprecation trajectory (§3.3) as a heads-up.

## 10. Ordering recommendation

**Ship `redact` first, then `resolve`.** Both in v0.8.x. The tolerant-mode stderr warning ships with whichever PR lands first.

`redact` first because:
- Smaller scope. One new `ConflictTier` variant, one post-emission stream rewrite, one new audit row source. No resolver interaction.
- Manifest contract is untouched. The change cannot regress restore-round-trip for any existing token.
- Adopter demand pattern (multi-turn agent loops on `tolerant`) maps directly to redact mode without any new CLI flag beyond the mode enum.
- Estimated 1–1.5 days of impl + tests + audit-row xtask gate update + tolerant stderr warning.

`resolve` second because:
- Larger scope. Synthetic custom-recognizer entry registration, one-shot loop guard, threshold flag and policy field, adversarial fixtures around collision-family + validator-veto interaction.
- Lower behavioral risk than the original synthetic-Candidate approach (because it reuses existing code paths), but still meaningful.
- Estimated 2–3 days of impl + tests, partly absorbed by the impl-alt choice in §7.4.

If maintainer review prefers a single bundled PR, that is fine — the two contracts are independent and can be reviewed as two commits in one PR. Two separate PRs is preferred for blast-radius reasons but not required.

## 11. Five-axis alignment for this design

- **A1 (never leak)**: redact closes the leak by deletion; resolve closes by re-tokenization; both have explicit strict-mode fallbacks on internal failure. The `tolerant`-mode "ship the leak" path is preserved for adopters who explicitly chose it but is documented as non-production (§3) and gets a stderr warning on every invocation.
- **A2 (reversible)**: redact's reversibility break is named, scoped, documented, and surfaced in `gaze restore`. Resolve is fully reversible — every safety-net-promoted custom-recognizer match has a manifest entry.
- **A3 (agentic-first)**: both new modes are designed for agent loops. Redact eliminates the strict-mode stall; resolve adds at most one pass of latency.
- **A4 (trust)**: every action emits a typed `ConflictTier` audit row. Sentinel string is validated at policy load. Loop count is bounded. No silent fallbacks — every failure path is named in §4.5 and §5.2. Closed-enum surface delta is minimized (one new variant) by the §7.4 impl choice.
- **A5 (ergonomics)**: the matrix in §2 and the production-posture table in §3 give adopters a posture-based decision guide. Defaults are unchanged (`strict` stays the default) so no existing deployment shifts behavior on upgrade. The `resolve` naming aligns CLI and marketing copy.

## 12. Scope coordination with pid-82's `feedback-loop.md`

The pid-82 README-rewrite track proposed a companion `docs/architecture/feedback-loop.md` to capture the *mechanics* of how safety-net suspects are promoted into the resolver. This doc — `safety-net-modes.md` — is the *catalog and contract*. The split is intentional:

- This doc answers "which mode do I pick, and what does each one promise?" Audience: adopters making a deployment decision.
- `feedback-loop.md` answers "how does `resolve` actually work, what's the candidate lifecycle, what's the failure-mode fixture surface?" Audience: contributors implementing or reviewing the resolver re-entry / custom-recognizer-promotion code.

Recommend: this doc lands first (design-only PR, no impl). `feedback-loop.md` lands with the `resolve`-mode impl PR. Both cross-link.

## 13. Open questions

**Q1 — adopter-configurable redact sentinel?** Lean: yes, per `policy.toml`, default `[REDACTED-by-safety-net]`. The literal is the right default but Laravel / agent adopters will want to pick a string the LLM is unlikely to echo. Validated at load.

**Q2 — multi-iteration resolve?** Lean: no, cap at one. Bounded behavior is auditable. If a recognizer needs the safety net to teach it twice, the recognizer should be fixed. Defer to v0.9 with adopter signal.

**Q3 — expose redacted ranges in `LeakReport` JSON?** Lean: yes, add `action_taken` per suspect (see §4.6). Agent loops can use this to self-correct.

**Q4 — chained mode `--safety-net-redact-fallback resolve` (try resolve, fall back to redact)?** Lean: defer to v0.9. The two modes are designed to be independently chosen for a reason — chaining them re-introduces the multi-iteration loop's auditability cost. If adopter signal demands a chain, the natural API is a `Vec<SafetyNetMode>` list rather than a new mode variant.

**Q5 — should `redact` act on `LeakKind::ClassMismatch`?** Lean: no, as written in §4.5. The manifest is already correct for class mismatches; overwriting a gaze-emitted token would corrupt restore.

**Q6 — `tolerant`-mode deprecation trajectory?** Lean: warn in v0.8.x, deprecate in v0.9, remove in v0.10 (§3.3). Confirm with adopter signal before committing the v0.10 removal.

**Q7 — `resolve` impl: custom-recognizer promotion vs. synthetic Candidate?** Lean: custom-recognizer promotion (§7.4). Smaller surface delta, reuses validated semantics. Confirm before impl PR.

**Q8 — naming: `resolve` vs `rerun`?** **RESOLVED:** `resolve` (§6, locked by user 2026-05-14).
