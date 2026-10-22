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

## Source spelling

The manifest stores each value in the bytes the recognizer captured. A value
captured inside a JSON string literal of a text that is a whole JSON document
(a JSON tool result, a JSON text block) is captured in its escaped spelling,
such as `Robert \"Bob\" Smith`. When a token is minted for such a value, the
session records `SourceSpelling::JsonEscaped`; every other token is `Plain`.

- The spelling is set once, when the token is minted. A later capture of the
  same bytes in another syntax reuses the token and keeps the first reading.
- `restore_strict_text*` and `restore` always return the source bytes. Plain
  text has no syntax to decode, so it gets exactly what the client sent.
- `restore_strict_decoded_text_with_provenance` serves decoded JSON
  destinations, such as a parsed tool-call input: a JSON-escaped value comes
  back as the value its escapes spell (`Robert "Bob" Smith`). It is as strict
  as the source restore; unknown tokens still fail closed.
- A manifest holding a JSON-escaped value exports as `session_blob` envelope
  v7. Readers up to v0.17 refuse it with `InvalidSnapshotVersion(7)`. Every
  other manifest still exports as v6.

## See also

- [Repeat-value sweep and v6 `session_blob`](../detection/manifest-sweep.md)
- [Daemon `session_id` semantics](../daemon/daemon-mode.md)
- [Restore guarantees](restore-boundary.md)
- Rustdoc: [Session](https://docs.rs/gaze-pii/latest/gaze/struct.Session.html), [Scope](https://docs.rs/gaze-pii/latest/gaze/enum.Scope.html)
