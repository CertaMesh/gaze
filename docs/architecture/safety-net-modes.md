# SafetyNet modes — design

Status: **design-only**, no implementation in this PR. Target ship train: v0.8.x point release, additive.

This document scopes two new `--safety-net-mode` variants — **`redact`** and **`resolve`** — alongside the existing `strict` and `tolerant` modes, **flips the production default to `resolve` with a `redact` fallback**, and introduces a composable `--safety-net-fallback {strict|tolerant|redact}` flag for primary modes whose action may not be honorable on a given suspect. It does not propose any new safety-net backends, manifest contract changes, or new restore semantics for the deterministic path.

Companion doc: a follow-up [`feedback-loop.md`](feedback-loop.md) (placeholder, owned in coordination with the pid-82 README-rewrite track) captures the *mechanics* of how `resolve`-mode promotes safety-net suspects into candidates and runs them through conflict resolution. This doc is the *catalog* and *contract*; that doc is the *plumbing*. The split is intentional — adopters reading "which mode do I pick?" should not have to wade through resolver re-entry to find the answer.

Existing cross-reference: [`docs/architecture/safety-nets.md`](safety-nets.md) is the canonical safety-net trait and observer-only chokepoint contract; this doc layers a *post-detection action policy* on top of that contract without renegotiating any of its invariants.

## TL;DR

- Today's CLI only has two outcomes when a safety net flags a suspect: **fail closed** (`strict`, exit 3, empty stdout) or **ship the leak with a warning** (`tolerant`). Both are blunt instruments and `tolerant` is **explicitly not a production mode** (§3).
- `redact` adds a third outcome: *one-way redact the suspect span and continue*. The cost is reversibility (axis 2) — the redacted bytes are gone for that suspect. The win is axis 1: no leak ships, no exit code, no human-in-the-loop. Available as an explicit opt-in for adopters who want to skip the resolve attempt and strip suspects directly.
- `resolve` adds a fourth outcome: *promote each suspect into a synthetic custom-recognizer match and let the existing conflict resolver decide*. Manifest stays intact, restore round-trips for every emitted token, no new pipeline re-entry point. **This is the new production default** (§3, §14 Q9). Naming choice and impl-alt comparison in §7 and §8.
- **New composable flag `--safety-net-fallback {strict|tolerant|redact}`** (§6). Applies when the primary mode is `redact` or `resolve` and the primary action cannot be honored for a specific suspect. Default is `redact`. One-hop cascade only.
- **Defaults flip:** `--safety-net-mode` default is now `resolve`; `--safety-net-fallback` default is `redact`. The pairing attempts the reversibility-preserving path first and only strips suspect bytes when resolve cannot honor them (validator-veto, missing anchor, residual suspect after re-run). `strict` stays available as an opt-in for "must fail loud" deployments. Existing `strict` users must pass `--safety-net-mode strict` explicitly to retain that behavior on upgrade (§10).
- The existing `SafetyNetMode` enum (`crates/gaze-cli/src/commands/mod.rs:399`) gains two additive variants. The current strict/tolerant semantics at `crates/gaze-cli/src/pipeline/run.rs:774` are unchanged for adopters who opt back in.
- **Recommended ship order: `redact` first (with the new default flip and the fallback flag), `resolve` second**, both within v0.8.x. Reasoning in §11.
- Concurrent recommendation that ships in the same train: **stderr warning on every `--safety-net-mode tolerant` invocation**, and an open question on tolerant-mode deprecation in v0.9 / removal in v0.10 (§3.3, §14 Q6).
- The biggest open design question: should the `redact` sentinel be adopter-configurable per `policy.toml`, or hard-coded to a single `[REDACTED-by-safety-net]` literal? Lean: configurable with sane default. See §14 Q1.

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
2. The audit row schema (`RedactionEntry` at `crates/gaze-types/src/lib.rs:1465`) already carries a `decided_by: ConflictTier` field (`ConflictTier` at lib.rs:1419) and is `#[non_exhaustive]`. Redact mode adds one new variant (`SafetyNetRedacted`); the fallback flag adds one more (`Fallback`). Resolve mode (under the recommended impl path, §8) adds **none**.

## 2. Mode catalog: five-axis matrix

Axes follow [`AGENTS.md`](../../AGENTS.md): **A1** reliability (never leak), **A2** reversibility, **A3** agentic-first, **A4** trust/auditability, **A5** adopter ergonomics.

| Mode       | A1 (no leak) | A2 (reversible) | A3 (agentic)        | A4 (trust)          | A5 (ergonomics) | Latency    | Default eligible |
|------------|--------------|-----------------|---------------------|---------------------|-----------------|------------|------------------|
| `strict`   | full         | full            | bad (exit 3 stalls) | full (typed error)  | low (need retry) | none extra | opt-in           |
| `tolerant` | **broken**   | full            | great (no stall)    | warning JSON only   | high            | none extra | **no — dev only** |
| `redact`   | full†        | partial loss‡   | great               | full (audit row)    | high            | tiny       | opt-in           |
| `resolve`  | bounded§     | full            | good (extra pass)   | full (audit row)    | medium          | +1 pass    | **default**      |

† Reliability is preserved by deletion. The suspect span is overwritten with a sentinel before the clean text reaches the LLM; the audit log records the redaction with a typed `decided_by: SafetyNetRedacted` row. From the LLM's perspective the leak never occurred.

‡ Reversibility is preserved for every token gaze emitted itself. The break is scoped to safety-net suspect spans; restore returns the sentinel unchanged for those spans. Adopters must treat redacted bytes as lost.

§ `resolve` makes no axis-1 promise unless the second pass succeeds. If the second pass also flags a suspect above threshold, the design falls back via `--safety-net-fallback` (§6). Default cascade is `redact`, which preserves axis-1 at axis-2's expense for the residual suspect only; adopters may opt into `strict` for hard-fail.

Where each fits in adopter posture:

- **High-throughput agent loop and batch pseudonymization** (the dominant `gaze clean` use case): `resolve` is the right production default and is now the runtime default. The reversibility-preserving path is attempted first; only suspects that resolve cannot honor (validator-veto, missing anchor, residual suspect) cascade to the default `redact` fallback. The agent never sees a hard fail.
- **Latency-sensitive agent loop** (an adopter who cannot afford the +1 pipeline pass): `redact` is the right production opt-in. Skips the resolve attempt and strips suspect spans directly with the sentinel. The agent never sees a hard fail; redacted spans surface to the agent as `[REDACTED-by-safety-net]` which it can self-correct around.
- **Hard-fail / batch-validate**: `strict` is the right opt-in for adopters who want any suspect to halt the pipeline so a human (or CI gate) can investigate. Pass `--safety-net-mode strict`.
- **Dev / exploratory**: `tolerant` exists for measuring safety-net false-positive rates against known-clean corpora and debugging recognizers — never production. See §3.

## 3. Production posture per mode

| Mode      | Production posture                                  | Adopter signal                                          |
|-----------|------------------------------------------------------|---------------------------------------------------------|
| `resolve` | ✓ **PRODUCTION DEFAULT**                              | Manifest-restorable second pass attempted first; cascades to `--safety-net-fallback` (default `redact`) only when resolve cannot honor a suspect. Axis 1 + Axis 2 safe by construction. |
| `redact`  | ✓ Production opt-in (latency-sensitive deployments)   | One-way redaction; Axis 2 broken (per suspect) but Axis 1 safe via deletion + audit trail. Skips the resolve attempt. |
| `strict`  | ✓ Production opt-in (hard-fail deployments)           | Fail-closed on suspect leak. Axis-1 safe. Surfaces gaps loudly to a human or CI gate. |
| `tolerant`| ✗ **DEV / LOCAL ONLY — never production**             | Ships the leak. Axis-1 violation by design.             |

### 3.0 Why `resolve` is the production default

The chosen pairing — `--safety-net-mode resolve --safety-net-fallback redact` — is designed to attempt the reversibility-preserving path first and only strip suspect bytes when reversibility cannot be honored. This is a strict improvement over the v0.7.x strict-default along axes 2, 3, and 5 with no weakening of axis 1.

Axis-1 (never leak) is preserved: when `resolve` promotes a suspect to a synthetic custom-recognizer match, the existing conflict resolver tokenizes it through the manifest — the suspect bytes never reach the LLM. When `resolve` cannot honor a suspect (validator-veto, missing anchor, residual suspect after one-shot re-run), the default `redact` fallback strips the suspect span with a sentinel *before* the clean text leaves the chokepoint, and a typed `RedactionEntry` with `decided_by: Fallback` and `fallback_triggered: Some(...)` is appended to the audit DB. From the perspective of "did PII reach the LLM?", resolve-with-redact-fallback is identical to strict — the suspect bytes never crossed the boundary.

Axis-2 (reversibility) is **strengthened** versus the redact-only path: every suspect that resolve can honor produces a manifest entry, fully restorable through `gaze restore`. Only the residual suspects that resolve cannot honor are subject to the redact fallback's irreversibility. The axis-2 trade-off is scoped to a smaller suspect population than under a redact-default.

Axis-3 (agentic-first) is preserved: like a redact default, the resolve default eliminates the strict-mode stall in agent loops. The agent does not encounter exit-3 on every safety-net suspect.

Axis-4 (trust / determinism) is preserved: every action — successful resolve, fallback redaction, fallback strict-exit — emits a typed `ConflictTier` audit row. Loop count is bounded at one resolve pass plus at most one fallback hop. The fallback flag is closed-enum and one-hop only.

Axis-5 (adopter ergonomics) is preserved: defaults reduce friction for the most common deployment shape (agent loop, batch pseudonymization) without compromising axis-1. Adopters with stricter posture requirements opt into `--safety-net-mode strict` and get the v0.7.x behavior unchanged. Adopters who cannot afford the resolve pass opt into `--safety-net-mode redact` and skip directly to the strip-and-continue behavior.

The latency cost — one additional pipeline pass per `gaze clean` invocation when any suspect is flagged — is the design's main concession. For batch document pseudonymization and most agent-loop deployments the +1 pass is well within budget. Latency-sensitive callers (sub-100ms targets) should opt into `--safety-net-mode redact`.

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

The same opt-in warning fires when an adopter passes `--safety-net-fallback tolerant`; that path additionally requires `GAZE_ALLOW_TOLERANT=1` to be set (§6.5).

### 3.3 Deprecation trajectory (proposed)

If the warning is acted on broadly, the natural next step is:
- v0.8.x: warning lands alongside `redact` + `resolve` + the default flip.
- v0.9: `tolerant` is marked **deprecated** in `--help`, `policy.toml` parse warnings, and CHANGELOG. CLI exit on `tolerant` switches from `0` to `0` with a louder stderr block. No behavior break.
- v0.10: `tolerant` is **removed** from `SafetyNetMode` (and from `--safety-net-fallback`). Adopters who still need the dev affordance can pin v0.9 or use `--safety-net-mode redact` and ignore the manifest delta during their corpus measurement.

This is an open question (§14 Q6), not a committed plan. The deprecation trajectory should be confirmed with adopter signal first.

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

If the suspect span *fully overlaps* a committed manifest token, or if writing the sentinel at the requested byte boundary would break a Unicode grapheme cluster, the redact action cannot be honored cleanly. The runtime hands off to the fallback decided by `--safety-net-fallback` (§6). The default fallback is `redact` itself, which means *expand the redaction* to consume the overlapping token (sacrificing one manifest entry to preserve axis-1) or to the nearest grapheme boundary. Strict and tolerant fallbacks are also available; see §6.3.

### 4.3 Audit-row contract

Add one variant to `ConflictTier` (`crates/gaze-types/src/lib.rs:1419`):

```rust
pub enum ConflictTier {
    // ... existing variants unchanged ...
    /// Safety net redacted a suspect span; no manifest entry exists.
    SafetyNetRedacted,
    /// Primary safety-net action could not be honored for this suspect; fallback decided.
    Fallback,
}
```

Emit one `RedactionEntry` per redacted suspect with:

- `source` = `"safety_net.<backend>.v<N>"` (e.g. `"safety_net.kiji.v1"`, `"safety_net.openai_filter.v1"`).
- `recognizer_id` = backend id from `SafetyNet::id()`.
- `class` = `SafetyNetPiiClass::to_pii_class()` of the safety-net label (`crates/gaze-types/src/lib.rs:1309`, `:1330`).
- `action` = `Action::Redact` (existing, `crates/gaze-types/src/lib.rs:1407`).
- `conflict_loser` = `false`. The suspect was not in conflict with another candidate; it was an action *on* the post-emission stream.
- `decided_by` = `ConflictTier::SafetyNetRedacted`.
- `fallback_triggered` = `None`. Set to `Some(FallbackReason)` only when the primary mode's action could not be honored; see §6.6.

The audit row carries the action and the byte range. It does **not** carry the original suspect bytes — that would re-introduce the leak into the audit DB. This is consistent with the existing safety-nets contract (no raw bytes cross the adapter boundary, see [`safety-nets.md`](safety-nets.md)).

Update the string mapping in `redaction_conflict_tier_as_str` at `crates/gaze-types/src/lib.rs:1559` to cover `SafetyNetRedacted` with `"safety_net_redacted"` and `Fallback` with `"fallback"`.

### 4.4 Restore behavior

`gaze restore` reads the manifest and detokenizes manifest tokens back to their original bytes. Sentinel strings are not manifest tokens. They will pass through unchanged. **This is the intended behavior** and must be documented in `UPGRADE.md` and the `gaze restore` CLI help:

> Spans redacted by a safety net in `--safety-net-mode redact` are one-way. `gaze restore` returns the sentinel string as-is. Adopters who need full reversibility should use `--safety-net-mode resolve` or `strict`.

This is the single axis-2 exception in the entire gaze design, and it is *explicit*. No silent byte-loss. Restore tooling can detect the sentinel and surface a clear "redacted by safety net at $bytes" diagnostic if desired.

### 4.5 Failure paths

Three failure conditions and the recommended handling. The first two now compose with `--safety-net-fallback` (§6); the third remains a stream-formatting note.

1. **Safety net initialization fails** (subprocess not found, model not loadable). Fall back to `strict`, regardless of `--safety-net-fallback`. Reason: axis 1 wins. An adopter who asked for redact-mode was explicitly trading reversibility for liveness; if the safety net itself is down, we cannot honor that trade. Initialization failures are *infrastructure* failures, not per-suspect failures, and the fallback flag is scoped to per-suspect handling.
2. **Safety net returns `LeakKind::ClassMismatch`**. ClassMismatch means the deterministic pipeline already tokenized the span, just with the "wrong" class. **Redact mode does NOT act on class mismatches** — the manifest is intact, restore round-trips, and overwriting the gaze-emitted token would corrupt the manifest. Warn on stderr (existing path) and continue.
3. **Sentinel write changes byte length of stream** (it always does, since suspect span and sentinel are not generally byte-equal). Acceptable. Downstream tooling that depends on byte offsets relative to the original input must use the manifest, not the clean stream.

Per-suspect failure modes specific to the redact action (`OverlapConflict`, grapheme-cluster break) are handled by the fallback flag; see §6.3.

### 4.6 `leak_report` JSON exposure

The `LeakReport` JSON emitted on stderr in tolerant mode (today) and in the audit DB (always) **should** gain a per-suspect `action_taken` field: `"redacted" | "resolve_recovered" | "resolve_failed" | "fallback_redacted" | "fallback_strict" | "fallback_tolerant" | "none"`. This lets agent-loop adopters introspect: "the safety net dropped these N bytes; I should ask the user to confirm."

## 5. `resolve` mode contract

The mechanism here has two viable implementations (§8). The *contract* is the same regardless of mechanism:

- Every suspect above `--safety-net-resolve-threshold` becomes a candidate for tokenization.
- Existing conflict resolution decides whether the safety-net candidate wins, ties, or loses.
- Manifest grows by the number of winning safety-net candidates. Every winning candidate is fully restorable.
- The safety net is invoked **once more** against the new clean text. If suspects above threshold remain, hand off to `--safety-net-fallback` (§6). The default fallback is `redact`, which preserves axis-1 at the cost of axis-2 for the residual suspect spans.

### 5.1 Threshold semantics

Add `--safety-net-resolve-threshold <float>` (default `0.7`). Suspects below threshold are dropped *before* candidate construction. If, after dropping, no suspects remain, resolve is a no-op and the pipeline ships the original clean text. If suspects remain but all are below threshold, hand off to `--safety-net-fallback`.

Threshold of `0.0` disables filtering (every suspect is reused). Threshold of `1.0` disables resolve entirely (functionally equivalent to the fallback mode alone).

The CLI flag is `policy.toml`-overridable under `[policy.safety_net] resolve_threshold = 0.7`.

### 5.2 Loop termination

**Cap at one resolve pass.** Axis-4 (trust): bounded behavior is auditable. A user can reason about "at most one extra pass." Unbounded retries risk pathological backoff under adversarial input. If a third pass would have helped, the *recognizer* needs to be improved, not the loop count. Open question for v0.9: see §14 Q2.

### 5.3 Class taxonomy mapping

Reuses the existing per-backend `class_map.rs` modules (`crates/gaze-recognizers/src/safety_net/kiji_distilbert/class_map.rs`, `crates/gaze-recognizers/src/safety_net/openai_filter/class_map.rs`). No new mapping surface. New backends inherit this pattern.

### 5.4 Per-suspect resolve failure modes

Three per-suspect failure modes route to `--safety-net-fallback` (§6.3):

- **ValidatorVeto** — the promoted span fails its validator (e.g., `E164Phone` rejects a malformed phone number, `EmailRfc` rejects a malformed email).
- **AnchorMissing** — the promoted recognizer requires a `mandatory_anchor` (collision-family contract, [`anchor-resolution.md`](anchor-resolution.md)) but the locale's cue table contains no matching anchor in the surrounding context.
- **ResidualSuspect** — after the one-shot resolve pass, the safety net still reports a suspect at or above threshold (the "non-converging safety net" case from §5).

Each emits a `decided_by: Fallback` audit row with the corresponding `FallbackReason` (§6.6).

## 6. Fallback flag

The fallback flag is a per-suspect cascade decision: when the primary `--safety-net-mode` is `redact` or `resolve` and the primary action cannot be honored for a specific suspect, the fallback decides what happens to that suspect. Terminal modes (`strict`, `tolerant`) ignore the flag because their per-suspect action cannot fail in a per-suspect sense — `strict` exits at the boundary regardless and `tolerant` ships the leak regardless.

### 6.1 CLI surface

```
--safety-net-fallback <strict|tolerant|redact>
```

- Type: closed enum mirroring three variants of `SafetyNetMode`. No `resolve` value — chaining `resolve → resolve` would re-introduce the multi-iteration loop's auditability cost (§5.2).
- Default: `redact`.
- Ignored (with stderr warning if explicitly set) when `--safety-net-mode` is `strict` or `tolerant`.
- `policy.toml` overridable under `[policy.safety_net] fallback = "redact"`.

### 6.2 Composition matrix

Six cells. Rows = primary mode; columns = fallback. The **default** column for each row is marked.

| primary \ fallback | `strict`                                       | `tolerant`                                       | `redact`                                                    |
|--------------------|------------------------------------------------|--------------------------------------------------|-------------------------------------------------------------|
| `redact`           | Exit 3 with `decided_by: Fallback`, reason in row. | Warn on stderr, ship the original suspect bytes. Requires `GAZE_ALLOW_TOLERANT=1` (§6.5). | **Default.** Expand the redaction to swallow the overlapping manifest token or the nearest grapheme boundary. Sacrifices one manifest entry; preserves axis-1. |
| `resolve`          | Exit 3 with `decided_by: Fallback`, reason in row. | Warn on stderr, ship the residual suspect bytes. Requires `GAZE_ALLOW_TOLERANT=1` (§6.5). | **Default.** Redact the suspect (per §4 contract). Axis-1 preserved; axis-2 lost for that span. |

Both defaults preserve axis-1. Both `tolerant` cells violate axis-1 by design and are dev-only.

### 6.3 Failure conditions per primary mode

The fallback flag is invoked only when the primary action's per-suspect failure conditions trigger. The conditions are closed, enumerated, and audited.

**`redact` primary, per-suspect failures (route to fallback):**
- **`OverlapConflict`** — the suspect span fully overlaps a committed manifest token. Cleanly overwriting it would corrupt manifest validity.
- (Grapheme-cluster break case is also classified as `OverlapConflict`-family for the purposes of audit; treat as `OverlapConflict` until a separate variant proves useful.)

**`resolve` primary, per-suspect failures (route to fallback):**
- **`ValidatorVeto`** — the promoted span fails its validator (§5.4).
- **`AnchorMissing`** — the promoted recognizer's `mandatory_anchor` is absent in the surrounding context (§5.4, [`anchor-resolution.md`](anchor-resolution.md)).
- **`ResidualSuspect`** — the safety net still reports a suspect at or above threshold after the one-shot resolve pass (§5.4).

All four reasons live in a closed `FallbackReason` enum (§6.6).

### 6.4 One-hop cascade only

The flag composes **one hop only**. There is no `resolve → redact → strict` chain, no `redact → resolve` re-entry, no `tolerant → redact` rescue path. Rationale:

- **Auditability (axis 4).** A single hop produces at most two audit rows per suspect (the loser-only fallback row plus, when applicable, a winning manifest row). Multi-hop chains would multiply audit rows and obscure causation in BI joins.
- **Mental model (axis 5).** Adopters reading `--safety-net-mode redact --safety-net-fallback strict` can predict exactly what happens for each per-suspect failure mode in §6.3. Adopters reading a three-hop chain cannot.
- **Determinism (axis 4).** Cascade depth is bounded and constant. No pathological adversarial input can produce unbounded fallback churn.

If an adopter wants a longer chain in v0.9+, the natural API is a `Vec<SafetyNetMode>` (§14 Q4) rather than a new mode variant. Defer until adopter signal demands it.

### 6.5 `tolerant` fallback opt-in

Passing `--safety-net-fallback tolerant` requires the environment variable `GAZE_ALLOW_TOLERANT=1` to be set. Without it, the CLI exits at policy-load time with `CliError::PolicyConfig` and a typed message naming the env var. The env-var gate mirrors the existing `--safety-net-mode tolerant` posture (§3.1) — both opt-in violate axis-1, and both require an explicit operational signal that the operator understands the trade-off.

The same stderr warning that fires on `--safety-net-mode tolerant` (§3.2) also fires on `--safety-net-fallback tolerant`.

### 6.6 Audit-row delta

The fallback flag adds two pieces to the audit-row schema:

**`ConflictTier::Fallback`** — a new variant on the existing closed enum, mapped to string `"fallback"`. Set as `decided_by` on the audit row whenever the fallback flag was the deciding factor for a per-suspect outcome.

**`fallback_triggered: Option<FallbackReason>`** — a new optional field on `RedactionEntry` carrying the typed reason that the fallback was invoked.

```rust
#[non_exhaustive]
pub enum FallbackReason {
    OverlapConflict,
    ValidatorVeto,
    AnchorMissing,
    ResidualSuspect,
}
```

When the fallback triggers, gaze emits a **loser-only audit row** for the suspect with:

- `source` = `"safety_net.<backend>.v<N>"`.
- `class` = the safety-net-mapped class.
- `action` = the action that the fallback ultimately performed (`Action::Redact` for the redact-default cascade; `Action::Preserve` for tolerant — the leak shipped untokenized).
- `conflict_loser` = `true`. The primary action lost to the fallback.
- `decided_by` = `ConflictTier::Fallback`.
- `fallback_triggered` = `Some(FallbackReason::...)`.

The strict-fallback path emits the row *and* exits 3; the audit row persists for forensic replay even though stdout was empty. This is consistent with the existing strict-mode audit shape — the redaction log is sealed before the exit code is returned.

The migration pattern mirrors the v0.7.x ambiguity side-channel: a non-breaking additive column on `RedactionEntry`, a bundled SQLite migration in `SqliteLogger`, and a CLI audit-query filter (`--fallback-reason <reason>`). Contract details and migration shape: [`ambiguity-side-channel.md`](ambiguity-side-channel.md).

## 7. Naming choice: `resolve` vs `rerun`

**Locked: `resolve`.** We chose `resolve` over `rerun` because the verb conveys "fixes the gap" from the adopter's perspective, where `rerun` is mechanism-shaped. (User confirmation, 2026-05-14.)

Both verbs describe the same observable behavior. They differ in what they *emphasize* to an adopter reading the CLI help:

- **`rerun`** — mechanism-honest. Conveys "we run the pipeline a second time with the safety net's suspects added in." The adopter reads `--safety-net-mode rerun` and knows the cost model is "one extra pass." Downside: the verb fixates on the *how*. An adopter who reads only the help text might think the second pass is the point, when in fact the point is *closing the gap the deterministic recognizers missed*.
- **`resolve`** — outcome-honest. Conveys "we fix the gap the safety net flagged." Adopter reads `--safety-net-mode resolve` and knows the *intent* is resolution, not retry. Downside: the verb hides the latency cost. An adopter benchmarking gaze in CI might be surprised by the second pass.

**Recommendation: `resolve`.** Reason: per axis-5 (adopter ergonomics), the CLI help text is the right place to explain *intent* — that's the criterion the adopter is shopping on when they pick a mode. The latency cost belongs in this doc, in `--help`'s detailed description, and in the `policy.toml` comments — not in the mode name itself. The pid-82 README-rewrite track is also gravitating toward `resolve` for adopter-facing copy; aligning the CLI flag with the marketing copy reduces friction.

The original draft of this doc used `rerun`. References to `rerun` elsewhere in the gaze codebase (if any) are pre-implementation and can be renamed without breakage.

## 8. Implementation alternative: synthetic Candidate injection vs. Custom-recognizer promotion

Two viable mechanisms reach the same contract from §5. The choice is an axis-4 (trust / cleanliness) and axis-5 (adopter ergonomics) call; both produce equivalent observable behavior.

### 8.1 Option A: Synthetic Candidate injection (original draft)

For each suspect above threshold, construct a synthetic `Candidate` and inject it into the resolver at a new entry point. Adds:

- A new `Source::SafetyNet` variant (or equivalent).
- A new resolver re-entry point that accepts a `Vec<Candidate>` and runs only the merge + resolution stages.
- A new `ConflictTier::SafetyNetFeedback` variant on the audit row to label the resolver's verdict.
- A new `SAFETY_NET_BASE_RULE_PRIORITY` constant tuned so deterministic recognizers beat safety-net candidates on tied spans.

Cost: two new closed-enum variants (one on `Source`, one on `ConflictTier`), one new code path through the resolver.

### 8.2 Option B: Custom-recognizer promotion (pid-82's proposal)

For each suspect above threshold, register a synthetic entry in the existing `[[policy.custom_recognizers]]` table at runtime — same surface adopters use today to declare custom regex / dictionary recognizers — with `source = "safety_net.<backend>"`, an exact-span anchor pattern, and the safety-net-mapped class. Then re-run the pipeline (or the resolver alone, as an impl detail). The resolver sees these synthetic entries as ordinary custom recognizers.

Cost: zero new closed-enum variants. The audit row's `source` string carries the safety-net identity; `decided_by` is whichever existing `ConflictTier` broke the tie (`RulePriority`, `Score`, etc.). The custom-recognizers code path is well-trodden, well-tested, and already participates in collision-family policy + validator-veto correctly.

### 8.3 Comparison

| Dimension                    | A: synthetic Candidate                  | B: custom-recognizer promotion          |
|------------------------------|------------------------------------------|------------------------------------------|
| New `ConflictTier` variant   | Yes (`SafetyNetFeedback`)                | No                                       |
| New resolver entry point     | Yes                                      | No (reuses existing pipeline path)       |
| Interaction with collision-family policy | Needs explicit tier wiring   | Free — custom recognizers already covered |
| Interaction with validator-veto | Needs explicit tier wiring            | Free — validator-veto runs before resolver |
| Adversarial fixture surface  | Large — new code path                    | Small — exercises existing custom-recognizer fixtures |
| Audit-row delta              | Three new strings (`safety_net_redacted` + `safety_net_feedback` + `fallback`) | Two new strings (`safety_net_redacted` + `fallback`, both from redact + fallback machinery) |

### 8.4 Recommendation: Option B (custom-recognizer promotion)

Three reasons:

1. **Axis-4 cleanliness.** Smaller surface delta. Two new `ConflictTier` variants (for redact and fallback) instead of three. The audit-row schema barely moves.
2. **Reuses validated semantics.** Custom recognizers already correctly interact with collision-family policy ([collision-family.md](collision-family.md)), validator-veto ([validator-veto.md](validator-veto.md)), and the locale chain. Synthetic candidates would have to re-prove all of that. Validator-veto in particular is what surfaces the `ValidatorVeto` fallback reason (§6.3) cleanly through the existing path.
3. **Naming alignment.** `resolve` reads more naturally if the mechanism is "the suspect *becomes a recognizer rule for this run*" than if it's "the suspect bypasses recognizers and joins the resolver directly." The verb and the mechanism converge.

Implementation note (for the impl PR, not this design): the synthetic custom-recognizer entries must be **scoped to the current pipeline invocation only** — they are not persisted to policy, not written to disk, and not visible to subsequent invocations. The `SafetyNet`-promoted recognizers carry an in-memory rule-priority that places them below deterministic recognizers but above the no-rival default, matching the priority rationale in §8.1.

## 9. Closed-enum surface delta

The full additive surface, in one place. Note this is smaller than the original draft due to the §8 recommendation; the fallback flag adds one more `ConflictTier` variant and one new `FallbackReason` enum, both closed and `#[non_exhaustive]`.

```rust
// crates/gaze-cli/src/commands/mod.rs (line 399 today)
// Recommend adding #[non_exhaustive] as part of this work.
pub(crate) enum SafetyNetMode {
    Strict,
    Tolerant,
    Redact,      // NEW (this work; new production default)
    Resolve,     // NEW (this work; replaces "rerun" naming from earlier draft)
}

// crates/gaze-cli/src/commands/mod.rs — new sibling enum
#[non_exhaustive]
pub(crate) enum SafetyNetFallback {
    Strict,
    Tolerant,
    Redact,      // default
}

// crates/gaze-types/src/lib.rs:1419 (already #[non_exhaustive])
pub enum ConflictTier {
    None, ClassPriority, RulePriority, Score, SpanLength,
    Validator, ValidatorVeto, CollisionPolicy, AnchoredContext,
    RecognizerId, Merged,
    SafetyNetRedacted,   // NEW — only for redact-mode actions
    Fallback,            // NEW — primary action could not be honored; fallback decided
    // (Resolve mode uses existing ConflictTier variants via custom-recognizer promotion.)
}

// crates/gaze-types/src/lib.rs — new sibling enum
#[non_exhaustive]
pub enum FallbackReason {
    OverlapConflict,
    ValidatorVeto,
    AnchorMissing,
    ResidualSuspect,
}

// crates/gaze-types/src/lib.rs:1465 — RedactionEntry gains one optional column
pub struct RedactionEntry {
    // ... existing fields ...
    pub fallback_triggered: Option<FallbackReason>,  // NEW
}
```

Also update the string mapping in `redaction_conflict_tier_as_str` at `crates/gaze-types/src/lib.rs:1559` to cover `SafetyNetRedacted` with `"safety_net_redacted"` and `Fallback` with `"fallback"`.

Today's `SafetyNetMode` enum at `crates/gaze-cli/src/commands/mod.rs:399` is **not** annotated `#[non_exhaustive]`. The impl PR for redact mode should add that attribute as part of the same change. This is one-time hygiene, not a contract change.

**`LeakKind` is NOT extended.** `LeakKind` describes *what was found* — uncovered, partially bled, or class-mismatched. The action taken (`redact`, `resolve`, fallback) is encoded in the audit row and in the new `LeakReport.action_taken` field (§4.6). Mixing find-and-act into one enum is a category error.

**`Action` is NOT extended.** Redact mode reuses `Action::Redact` (existing variant at lib.rs:1407). Resolve mode reuses `Action::Tokenize`. Fallback paths reuse the same `Action` variants depending on which fallback was selected.

## 10. Adopter migration

**Default-flip is the breaking change.** Adopters upgrading from v0.7.x to v0.8.x who rely on strict-as-default will see behavior change at upgrade time: clean runs that previously exited 3 on a suspect now attempt to resolve (promote the suspect into a synthetic custom-recognizer match and re-run the resolver) and, on resolve-fail, fall back to redacting the suspect span. The full rollout sequence is:

- `--safety-net-mode resolve` (new default) and `--safety-net-mode redact` join the existing `strict` and `tolerant` modes.
- `--safety-net-fallback {strict|tolerant|redact}` is new; defaults to `redact`. Applies when primary = `redact` or `resolve`.
- `--safety-net-resolve-threshold <float>` is new; defaults to `0.7`. Applies when primary = `resolve`.
- Adopters who relied on strict-as-default must pass `--safety-net-mode strict` explicitly (or set `[policy.safety_net] mode = "strict"` in `policy.toml`).
- Adopters currently running `--safety-net-mode tolerant` to dodge exit 3 should switch to the new default (`redact`) or to `resolve` rather than continuing to ship leaks.
- The new stderr warning will fire on every `tolerant` invocation. Adopters with `tolerant` baked into CI workflows should update those workflows to one of the production-safe modes before upgrading.
- The `RedactionEntry.decided_by` column may now contain `"safety_net_redacted"` or `"fallback"`. Downstream BI queries that string-match this column must add these strings to their allow-list.
- The `RedactionEntry.fallback_triggered` column is new and optional. Downstream consumers should treat absence as the v0.7.x case and presence as v0.8.x+.
- Cross-link in [`safety-nets.md`](safety-nets.md) §1 to this doc once the impl PRs land.
- README Quickstart (post pid 82's marketing rewrite) gains a mode comparison table aligned with §2 + §3, and a one-paragraph note on the default-flip.
- A short `UPGRADE.md` note for v0.8.x captures (a) the four-mode + fallback summary, (b) the redact-as-default flip and how to opt back into strict, (c) the redact-sentinel-is-one-way restore caveat, (d) the new audit-row strings + column, (e) the tolerant deprecation trajectory (§3.3) as a heads-up.

## 11. Ordering recommendation

**Ship `redact` first (with the fallback flag, default mode still `strict`), then `resolve` (which flips the default to `resolve` with `redact` fallback).** Both in v0.8.x. The tolerant-mode stderr warning ships with whichever PR lands first.

The default flip waits for the `resolve` PR because the chosen production default is the `resolve → redact` pairing (§3.0). Flipping the default before resolve lands would briefly make `redact` the default, then flip again — two breaking changes for one upgrade window. One coordinated flip in the `resolve` PR is cleaner for adopters.

`redact` first because:
- Smaller scope. Two new `ConflictTier` variants (`SafetyNetRedacted`, `Fallback`), one new `FallbackReason` enum, one post-emission stream rewrite, one new audit row source. No resolver interaction.
- Manifest contract is untouched. The change cannot regress restore-round-trip for any existing token.
- The fallback flag is wired end-to-end in this PR so that the `resolve` PR only adds the resolve action itself, not the fallback plumbing.
- Estimated 1.5–2 days of impl + tests + audit-row xtask gate update + tolerant stderr warning.

`resolve` second because:
- Larger scope. Synthetic custom-recognizer entry registration, one-shot loop guard, threshold flag and policy field, adversarial fixtures around collision-family + validator-veto interaction (which surface as `FallbackReason::ValidatorVeto` and `AnchorMissing`).
- Lower behavioral risk than the original synthetic-Candidate approach (because it reuses existing code paths), but still meaningful.
- Ships the default flip (`--safety-net-mode resolve`, `--safety-net-fallback redact`) as a single coordinated change.
- Estimated 2–3 days of impl + tests + default-flip migration tests, partly absorbed by the impl-alt choice in §8.4.

If maintainer review prefers a single bundled PR, that is fine — the two contracts are independent and can be reviewed as two commits in one PR. Two separate PRs is preferred for blast-radius reasons but not required.

## 12. Five-axis alignment for this design

- **A1 (never leak)**: resolve closes the leak by re-tokenization through the manifest; redact closes by deletion + audit row; the default fallback (`redact`) preserves axis-1 even when the primary action cannot be honored. The `tolerant`-mode and `tolerant`-fallback "ship the leak" paths are preserved for adopters who explicitly chose them and explicitly set `GAZE_ALLOW_TOLERANT=1` but are documented as non-production (§3, §6.5) and get a stderr warning on every invocation. The flipped default does not weaken axis-1: the `resolve → redact` pairing is axis-1-safe by construction.
- **A2 (reversible)**: **strengthened** versus the redact-only path. The default attempts resolve first, which produces a manifest entry for every successfully promoted suspect — fully restorable through `gaze restore`. Only suspects that resolve cannot honor (validator-veto, missing anchor, residual suspect) cascade to the redact fallback. Redact's reversibility break is named, scoped, documented, and surfaced in `gaze restore`. The fallback flag's `redact` default explicitly accepts the residual axis-2 trade-off in exchange for axis-1 + axis-3 on those suspects only.
- **A3 (agentic-first)**: both new modes and the resolve-default flip are designed for agent loops. Resolve-default eliminates the strict-mode stall as the runtime default; the +1 pipeline pass is well within budget for the dominant deployment shapes. Agents in multi-turn conversation no longer encounter exit-3 on every safety-net suspect. Latency-sensitive callers opt into `--safety-net-mode redact`.
- **A4 (trust)**: every action — successful resolve, fallback redaction, fallback strict-exit — emits a typed `ConflictTier` audit row. Sentinel string is validated at policy load. Loop count is bounded at one resolve pass plus at most one fallback hop. The fallback flag adds at most one extra audit row per suspect and is one-hop only. No silent fallbacks — every failure path is named in §4.5, §5.4, and §6.3, and every fallback emits a typed `FallbackReason`. Closed-enum surface delta is minimized (two new `ConflictTier` variants + one new `FallbackReason` enum) by the §8.4 impl choice.
- **A5 (ergonomics)**: the matrix in §2, the production-posture table in §3, and the composition matrix in §6.2 give adopters a posture-based decision guide. Defaults reduce friction for the most common deployment shapes (agent loop, batch pseudonymization) while `strict` remains a one-flag opt-in for adopters who prefer hard-fail, and `redact` remains a one-flag opt-in for latency-sensitive adopters. The `resolve` naming aligns CLI and marketing copy. The fallback flag's one-hop cascade gives adopters a predictable mental model.

## 13. Scope coordination with pid-82's `feedback-loop.md`

The pid-82 README-rewrite track proposed a companion `docs/architecture/feedback-loop.md` to capture the *mechanics* of how safety-net suspects are promoted into the resolver. This doc — `safety-net-modes.md` — is the *catalog and contract*. The split is intentional:

- This doc answers "which mode do I pick, and what does each one promise?" Audience: adopters making a deployment decision.
- `feedback-loop.md` answers "how does `resolve` actually work, what's the candidate lifecycle, what's the failure-mode fixture surface?" Audience: contributors implementing or reviewing the resolver re-entry / custom-recognizer-promotion code.

Recommend: this doc lands first (design-only PR, no impl). `feedback-loop.md` lands with the `resolve`-mode impl PR. Both cross-link.

## 14. Open questions

**Q1 — adopter-configurable redact sentinel?** Lean: yes, per `policy.toml`, default `[REDACTED-by-safety-net]`. The literal is the right default but Laravel / agent adopters will want to pick a string the LLM is unlikely to echo. Validated at load.

**Q2 — multi-iteration resolve?** Lean: no, cap at one. Bounded behavior is auditable. If a recognizer needs the safety net to teach it twice, the recognizer should be fixed. Defer to v0.9 with adopter signal.

**Q3 — expose redacted ranges in `LeakReport` JSON?** Lean: yes, add `action_taken` per suspect (see §4.6). Agent loops can use this to self-correct.

**Q4 — chained mode `--safety-net-redact-fallback resolve` (try resolve, fall back to redact)?** **RESOLVED:** one-hop cascade only (§6.4). The fallback flag composes one hop; no multi-hop chains. If adopter signal demands a longer chain in v0.9+, the natural API is a `Vec<SafetyNetMode>` list rather than a new mode variant.

**Q5 — should `redact` act on `LeakKind::ClassMismatch`?** Lean: no, as written in §4.5. The manifest is already correct for class mismatches; overwriting a gaze-emitted token would corrupt restore.

**Q6 — `tolerant`-mode deprecation trajectory?** Lean: warn in v0.8.x, deprecate in v0.9, remove in v0.10 (§3.3). Confirm with adopter signal before committing the v0.10 removal.

**Q7 — `resolve` impl: custom-recognizer promotion vs. synthetic Candidate?** Lean: custom-recognizer promotion (§8.4). Smaller surface delta, reuses validated semantics. Confirm before impl PR.

**Q8 — naming: `resolve` vs `rerun`?** **RESOLVED:** `resolve` (§7, locked by user 2026-05-14).

**Q9 — why is `resolve` (with `redact` fallback) a safer production default than `strict` or `redact` alone?** **RESOLVED** (§3.0). The `resolve → redact` pairing is axis-1-safe by construction: every suspect either becomes a manifest token (axis 1 + axis 2 safe) or is stripped by the redact fallback (axis 1 safe, audit trail intact). Versus a strict default, this pairing wins axis-3 (no exit-3 stall) and axis-5 (low-friction default) without compromising axis-1. Versus a redact-only default, this pairing wins axis-2 (every suspect that resolve can honor is fully restorable). Strict remains a one-flag opt-in for adopters who prefer hard-fail posture; redact remains a one-flag opt-in for latency-sensitive adopters. The residual axis-2 trade-off is documented and surfaced in `gaze restore`.

**Q10 — should fallback be settable per `LeakKind`?** Lean: defer to v0.9. A future `--safety-net-fallback-uncovered redact --safety-net-fallback-partial-bleed strict` shape is conceivable, but the single-flag form covers the dominant case and keeps the matrix readable. Adopter signal required before adding the per-kind surface.
