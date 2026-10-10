# Session contract

A `Session` owns one pseudonym namespace. Use one per conversation or other
isolation boundary. `Scope` controls persistence.

## What a Session guarantees

Each new session has fresh per-class counters (`Name_1`, `Email_1`) and a fresh
`session_hex` prefix. Sessions never share counters or value-keyed lookups,
regardless of scope. Later copies of rule-found values in the same session are
also tokenized; changed case or spacing gets a sibling token. See the
[repeat-value sweep](../detection/manifest-sweep.md).

## `Scope` variants

| Variant | Lifetime | `export()` |
| --- | --- | --- |
| `Scope::Ephemeral` | Until the session is dropped | No |
| `Scope::Conversation(id)` | Keyed multi-turn session; reopening across restarts depends on storage | Yes |
| `Scope::Persistent { ttl: Duration }` | Across restarts | Yes |

## Single shared session across conversations

Sharing one `Session::new(Scope::Ephemeral)` across conversations reuses
pseudonyms, grows counters and maps across the app lifetime, and lets observers
link independent contexts.

Use a separate session for each boundary. Use `Scope::Conversation(conv_id)`
for a namespace that can reopen across restarts, or `Scope::Ephemeral` for
one-shot work.

## See also

- [Repeat-value sweep and v6 `session_blob`](../detection/manifest-sweep.md)
- [Daemon `session_id` semantics](../daemon/daemon-mode.md)
- [Restore guarantees](restore-boundary.md)
- Rustdoc: [Session](https://docs.rs/gaze-pii/latest/gaze/struct.Session.html), [Scope](https://docs.rs/gaze-pii/latest/gaze/enum.Scope.html)
