# Session Contract

A `Session` is the boundary of a pseudonym namespace in gaze. This document describes the runtime contract - what `Session` and `Scope` guarantee, what they do not guarantee, and the common pitfall that triggered issue #275.

## The Contract

- A `Session` is the pseudonym namespace boundary.
- Each new `Session` starts with fresh per-class counters (`Person_1`, `Email_1`, etc.) and a fresh `session_hex` prefix.
- Two `Session`s never share counters or value-keyed lookups, regardless of `Scope` variant.
- `Scope` variants choose *persistence*, not *isolation*.

## `Scope` Variants

| Variant | Use case | `export()` allowed |
|---------|----------|--------------------|
| `Scope::Ephemeral` | Process-bound one-off redaction; namespace lives until the `Session` is dropped. | No |
| `Scope::Conversation(id)` | Keyed multi-turn LLM sessions that can be re-opened across process restarts, storage backend-dependent. | Yes |
| `Scope::Persistent { ttl: Duration }` | Long-lived sessions across restarts. | Yes |

## Common Pitfalls

### Single Shared Session Across Conversations

**Symptom:** the same email or person name in two adapter-side conversations
produces the same pseudonym. Per-class counters (`Email_N`, `Person_N`) grow
monotonically across the entire app lifetime. Internal value-to-token maps grow
unboundedly.

**Cause:** one `Session::new(Scope::Ephemeral)` shared across all calls. The
`Scope` variant controls *persistence* (whether the namespace survives process
restart), not *isolation* (whether two logical conversations share a namespace).

**Fix:** use one `Session` per logical isolation boundary. For chat or agent
threads, `Scope::Conversation(conv_id)` re-opens the same namespace on a key,
which is useful across restarts. For ad-hoc one-shot redaction with no reuse,
`Scope::Ephemeral` is fine.

**Why this matters (axis 1):** cross-context linkability through pseudonym reuse
is the failure mode that GDPR Art. 4(5) pseudonymization is meant to prevent. If
two contexts that should be independent share a `Session`, the pseudonym becomes
a stable identifier across them, which is exactly the property an attacker
correlating two logs would exploit.

## Cross-References

- [`docs/explanation/daemon/daemon-mode.md`](../daemon/daemon-mode.md) for daemon-mode-specific `session_id` semantics.
- [`docs/explanation/core/restore-boundary.md`](restore-boundary.md) for restore-side guarantees.
- Rustdoc for [`Session`](https://docs.rs/gaze-pii/latest/gaze/struct.Session.html) and [`Scope`](https://docs.rs/gaze-pii/latest/gaze/enum.Scope.html).
