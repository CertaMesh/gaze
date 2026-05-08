# Gaze

**Reversible PII pseudonymization for agentic LLM workflows.**

Gaze sits between your data and the LLM. It swaps PII for stable, session-scoped tokens on the way out, and restores the originals on the way back. The agent never sees raw personal data; the data owner never loses the ability to read the agent's reply.

```sh
cargo install gaze-cli
echo 'Email alice@example.invalid about ORD-789012.' | gaze clean
```

```json
{
  "clean_text": "Email <{session_hex}:Email_1> about ORD-789012.",
  "session_blob": "<base64>",
  "stats": {"detections": 1}
}
```

Send `clean_text` to the LLM. Keep `session_blob` server-side — it is the signed restore manifest, and it must never reach the model.

Round-trip the model's reply through restore on the same manifest:

```sh
echo '{"session_blob":"<base64>","text":"Confirmation sent to <{session_hex}:Email_1>."}' \
  | gaze restore
```

```json
{"text":"Confirmation sent to alice@example.invalid."}
```

Full CLI surface — flags, structured-document mode, audit logging, policy TOML — is in [`crates/gaze-cli/README.md`](crates/gaze-cli/README.md).

## Why this exists

PII handling in LLM apps usually falls into one of three buckets:

1. **No redaction.** Real emails, phone numbers, and order IDs end up in the model provider's logs.
2. **One-way redaction.** You strip PII, the agent replies "I've sent the confirmation to `<REDACTED>`", and you have no way to thread the reply back to the actual user.
3. **LLM-based redaction.** A second model call decides what's PII. Non-deterministic, non-auditable, costs another round trip per turn.

Gaze takes a fourth path: deterministic, rule-based detection with a signed restore manifest. Reversible without giving up an audit trail.

## Guarantees

- **Fail closed.** Ambiguous matches are tokenized, never silently passed. Unknown rulepack validators or normalizers fail at policy load — no degraded mode.
- **Reversible by design.** Tokens like `<{session_hex}:Email_1>` are session-scoped and counted by class. Restore goes through a signed `SensitiveSnapshot`, not string substitution.
- **Auditable.** Every emitted token traces to a recognizer + rule. Optional metadata-only SQLite log via `gaze clean --audit-db`; raw PII is never written to the log.
- **Deterministic.** Detection is regex/dictionary-first. NER and the OpenAI-filter safety net are opt-in observers. They cannot mutate the manifest or the restore path.

## Install

```sh
cargo install gaze-cli
```

Pre-built binaries for Apple Silicon macOS and Linux x86_64 (glibc 2.39+) are attached to each [GitHub release](https://github.com/EmpireTwo/gaze/releases). Other targets: build from source with `cargo build --release -p gaze-cli`.

For library use — linking the Rust runtime directly instead of shelling out — see [Use from Rust](#use-from-rust) below.

## Pipeline shape

```text
                       regex (always-on)  ─┐
                       dictionary (opt-in) ├──► resolver ──► tokens ──► CleanDocument
                       NER (opt-in)        ─┘     │
                                                  │  conflict tiers:
                                                  │  class > rule > score > length > id
                                                  │
                                                  ├──► Pass-3 SafetyNet (observer)
                                                  │    reads clean text + manifest
                                                  │    emits LeakReport, never mutates
                                                  │
                                                  └──► SensitiveSnapshot (signed)
                                                              │
                                                              ▼
                                                          restore
```

Three deterministic detection passes plus an optional observer pass. The safety net cannot modify the clean text or the restore path; it only emits suspect reports against the manifest of emitted tokens.

## Workspace

Six published crates. Pick the smallest surface that does the job.

| Crate | Use when |
|-------|----------|
| [`gaze-pii`](crates/gaze/) (lib name `gaze`) | You want the runtime: `Pipeline`, `Session`, `Policy`, `Recognizer`, restore. |
| [`gaze-assembly`](crates/gaze-assembly/) | You want bundled defaults without hand-wiring recognizers. |
| [`gaze-recognizers`](crates/gaze-recognizers/) | You're writing a custom recognizer or rulepack. |
| [`gaze-audit`](crates/gaze-audit/) | You want SQLite-backed metadata audit logging. Adopt directly; `gaze` core has no `rusqlite` dep in any feature graph. |
| [`gaze-cli`](crates/gaze-cli/) | You want a process boundary for non-Rust adapters (Laravel, Python, etc.). |
| [`gaze-types`](crates/gaze-types/) | You want the value contracts (`RedactionLogger`, `Manifest`, `LeakReport`) without ML deps. |

Crate boundaries and the audit-isolation gate: [`docs/architecture/crates.md`](docs/architecture/crates.md).

## Detection coverage

Bundled rulepacks (composable through `CorePipelineConfig::with_bundled_rulepack` or `[policy.rulepacks]`):

- **`core` — always-on.** Email (RFC-validated), and locale-aware `Name` coverage cued off forwarded headers, agent reply preambles, and auto-footer sender lines.
- **`core-extended` — opt-in.** Phone (E.164 + national), IPv4/IPv6, postal codes, IBAN (MOD-97), credit card (Luhn).

Validators are a closed enum (`EmailRfc`, `E164Phone`, `Luhn`, `IbanMod97`); unknown validator names in a rulepack fail at load with a typed error. Locale chain is strict and ordered: CLI > policy > rulepack default > system default.

Tenant-specific PII (order IDs, song titles, artist names) needs a dictionary or custom regex recognizer. See [`docs/policy.md`](docs/policy.md).

## Audit and restore

Restore is manifest-first. Tokens are session-scoped, counted by class, and only resolvable through a signed `SensitiveSnapshot`. There is no string-map fallback.

Optional metadata audit log:

```sh
gaze clean --policy policy.toml --audit-db audit.sqlite < input.txt
gaze audit query --audit-db audit.sqlite --class email --action tokenize
gaze audit export --audit-db audit.sqlite --format jsonl --output redactions.jsonl
gaze audit purge --audit-db audit.sqlite --before 2026-01-01T00:00:00Z
```

The audit DB is opened read-only by `query` and `export`. The exported column set excludes raw PII payloads. There is no policy-level retention default and no background auto-purge — adopters drive retention explicitly.

## Status

- **Version:** v0.6.4 (2026-05).
- **MSRV:** Rust 1.89.
- **License:** dual `Apache-2.0 OR MIT`.
- **crates.io:** published as `gaze-pii`. The bare `gaze` name is in transfer; until that completes, depend on `gaze-pii`. Source-compat is preserved via `[lib].name = "gaze"`.
- **Contract surface:** `Pipeline`, `Session`, `Policy`, rulepack schema, and token shape are stable across the v0.6 line. SafetyNet contract: [`docs/architecture/safety-nets.md`](docs/architecture/safety-nets.md).

## North star

> Zero PII leaks between the agent and the data owner — ever. Any byte of PII that reaches an LLM outside the manifest contract is a critical defect.

Five axes are evaluated on every PR: reliability, reversibility, agentic-first design, trust (auditable + deterministic), and adopter ergonomics. Correctness axes always beat performance. Full text: [AGENTS.md](AGENTS.md#project-north-star).

## Limits

- Bundled detection is strongest for emails, names, locations, organizations, IBANs, credit cards, IPv4/IPv6, and DACH/EN postal + phone shapes. Tenant-specific PII needs a custom recognizer.
- `--rulepack-bundled core-extended` without a policy activates `phone.national.de`, `phone.national.us`, `postal.us`, `postal.de`. Adopters wanting narrower scope must supply a policy or pass `--locale=global`.
- Linux x86_64 binaries link against glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer). Older distros: build from source.
- No Intel macOS, no musl, no Windows binaries shipped today; build from source.

## Use from Rust

The CLI is a process boundary around the Rust runtime; you can link the runtime directly:

```toml
[dependencies]
gaze-pii = "0.6"
gaze-assembly = "0.6"
```

The crate is published as `gaze-pii` because the bare `gaze` name is in transfer; the import path stays `use gaze::...` because `[lib].name = "gaze"` is preserved.

- Minimal example and the API surface table: [`crates/gaze/README.md`](crates/gaze/README.md) (also rendered on [`crates.io/crates/gaze-pii`](https://crates.io/crates/gaze-pii)).
- Full walk-through with structured documents, tenant-specific recognizers, and policy TOML: [`docs/getting-started.md`](docs/getting-started.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Repository gates (xtask + Dylint) enforce the contracts in [`docs/architecture/`](docs/architecture/). Run them locally before pushing:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- ci-feature-matrix
```

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
