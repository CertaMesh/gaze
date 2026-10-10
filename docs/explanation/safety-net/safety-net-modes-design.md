# Safety-net modes: v0.8 design record (historical)

This is a summary of the v0.8 proposal, not the current contract. Read
[Safety-net modes](safety-net-modes.md) for shipped behavior. The full proposal
is available in Git history.

## Decision

Use `resolve` with a `redact` fallback by default. Try reversible tokenization
first; replace only suspects that cannot be resolved with a visible one-way
marker. Keep `strict` for deployments that must stop on an uncovered suspect.
`tolerant` ships flagged bytes and is development-only.

The proposal added action policy after the observer-only safety net. It did
not propose new backends or change deterministic restore semantics. The name
`resolve` was chosen over `rerun` on 2026-05-14 because it describes the action;
the extra scan cost belongs in the help text.

| Mode | Proposed action | Tradeoff |
|---|---|---|
| `strict` | Refuse the document, exit `3`, empty stdout | Stops an agent loop |
| `tolerant` | Warn and continue | Flagged bytes remain raw |
| `redact` | Replace suspect bytes and audit the action | Those bytes cannot restore |
| `resolve` | Make suspects restorable, then scan again | Extra scan when suspects exist |

These actions protect only what detection finds. They do not establish zero
leaks on arbitrary input.

## Proposed mechanisms

The proposal compared synthetic candidate injection with temporary custom
recognizers. It favored custom recognizers to reuse collision-family,
validator-veto, and locale behavior, with lower priority than deterministic
recognizers. Temporary rules would be scoped to one invocation, never saved.
Neither mechanism shipped: the core tokenizes suspect spans directly.

The design bounded resolution to one pass and fallback to one hop. Longer
chains and per-`LeakKind` fallbacks were deferred until adopters needed them.
Existing backend class maps would supply the token class.

For redaction, it considered a fixed `[REDACTED-by-safety-net]` string,
a manifest-shaped token, and an adopter-configurable sentinel. It favored a
configurable string with a fixed default, validated as nonempty and outside
token grammar. A token-shaped marker was rejected because it would imply a
restorable mapping.

The proposed redaction path left the manifest unchanged and overwrote only
uncovered bytes. Current markers instead have non-owned `Action::Redact`
manifest entries so clean/raw offsets remain aligned; overlaps may consume
whole tokens. See [the marker contract](safety-nets.md#the-redaction-marker).

## Proposal versus implementation

| Proposed surface | Shipped behavior |
|---|---|
| Resolver re-entry through custom recognizers | Direct tokenization in family `safety_net`; no resolver, validator, or anchor rerun |
| Configurable sentinel | Fixed `[REDACTED:<class>]` marker |
| `ConflictTier::SafetyNetRedacted` | `ConflictTier::Redact`, `Resolve`, `Fallback`, rendered `redact`, `resolve`, `fallback` |
| `--safety-net-resolve-threshold`, default `0.7` | No such flag |
| `[policy.safety_net]` mode/fallback/threshold keys | No such keys; `[safety_net]` accepts `backend` and `nym` |
| Fallback after either `redact` or `resolve` | Only `resolve` consults fallback; `redact` handles spans or fails closed |
| `ValidatorVeto` / `AnchorMissing` fallback reasons | Accepted enum variants, never emitted; runtime emits `OverlapConflict` / `ResidualSuspect` |
| `LeakReport.action_taken` | Not implemented; actions are in audit rows |
| Tolerant opt-in failure as `CliError::PolicyConfig` | `TolerantModeDisabled`; both tolerant flags require `GAZE_ALLOW_TOLERANT=1` |
| Warn in v0.8, deprecate in v0.9, remove in v0.10 | Tolerant remains gated and prints a deprecation warning; removal was only proposed |
| Companion `feedback-loop.md` explaining resolve | That page is the synthetic detection regression loop |

## Fallback and audit

The proposed flag was `--safety-net-fallback <strict|tolerant|redact>`, default
`redact`, with no recursive `resolve` value. The shipped lowering is canonical:

| Primary mode | Fallback use |
|---|---|
| `strict` | Ignored; return report, reject uncovered/partial-bleed suspects at CLI boundary |
| `tolerant` | Ignored; return report and warn |
| `redact` | Ignored; replace spans or return a typed error |
| `resolve` | Tokenize, scan again, apply the selected fallback to residuals |

Fallback must use the re-run report at post-resolve positions. Reusing the
primary report could overwrite a new token while leaving the residual raw.
The regression test is
`resolve_followup_uses_the_residual_report_not_the_stale_primary_report`.

Audit actions contain metadata, never suspect bytes. Fallback rows use
`decided_by = Fallback`, `conflict_loser = true`, and
`fallback_triggered: Option<FallbackReason>`. `Redact` denotes replacement;
`Preserve` denotes bytes left in place under strict/tolerant. Strict writes
its row before rejection. `LeakKind` still describes the finding rather than
the action; existing `Action::Tokenize`, `Redact`, and `Preserve` suffice.

## Migration and rollout

The intended rollout was redaction/fallback first with strict still default,
then resolution with one coordinated default flip. Strict-default adopters
must pass `--safety-net-mode strict` explicitly. Restore leaves markers
unchanged. Audit consumers must accept the shipped tier strings and optional
fallback reason; old rows have no reason.

The remaining proposal questions concerned configurable markers, per-suspect
report actions, more resolution iterations, and per-kind fallbacks. None is
an adopter-facing configuration promise. Current behavior, structured-document
limits, and tolerant warnings are in [Safety-net modes](safety-net-modes.md).
