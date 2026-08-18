# Safety nets

Safety nets are observer-only privacy backends that audit Gaze's clean output
for PII the deterministic pipeline missed. They never replace bytes, never
mutate the [`Manifest`](../../../crates/gaze-types/src/lib.rs), and never reach
the restore path. They exist to surface leak suspects so the deterministic
detectors and rulepacks can be improved.

For step-by-step setup with Kiji DistilBERT, see [`docs/how-to/safety-net/set-up-kiji-safetynet.md`](../../how-to/safety-net/set-up-kiji-safetynet.md).

Validator-backed self-validation is handled earlier by the deterministic
[`validator-veto`](../detection/validator-veto.md) stage. Safety nets do not veto candidates
and do not participate in conflict resolution.

## Benchmark

The v0.9 benchmark populates direct-detector and observer-residual cells for
both shipped backends against the same 150-fixture coverage-loop corpus. Kiji
DistilBERT fp32 stays at `0.125000` macro strict recall across locales; the
opt-in int8 dynamic-quantized Kiji artifact also stays at `0.125000` macro
strict recall across locales in direct-detector mode. Full numbers, pins, and
caveats are in [`docs/reference/benchmarks/v0.9-safety-net-benchmark.md`](../../reference/benchmarks/v0.9-safety-net-benchmark.md).

This document describes the safety-net contract introduced in v0.6 through
PR #91. The first shipped backend is the OpenAI Privacy Filter
(`opf`) subprocess adapter; the contract is generic so additional backends
can land without changing the trait shape or audit schema.

```text
                    GAZE CLEAN INVOCATION
                            │
                            ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │ PASS 1 — REGEX + DICTIONARY (deterministic)                     │
   │   "Contact alice@example.invalid"                               │
   │     → recognizers (email.global, name.de, iban, …)              │
   │     → Candidate { class=Email, score=1.0, span=(8,28), … }      │
   └────────────────────────────┬────────────────────────────────────┘
                                ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │ PASS 2 — NER (optional, opt-in feature)                         │
   │   mBERT (Davlan) emits B-PER / I-PER / B-LOC … per token        │
   │   → Candidate { class=Name, score=0.91, span=(0,7) }            │
   └────────────────────────────┬────────────────────────────────────┘
                                ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │ CONFLICT RESOLUTION + TOKENIZATION                              │
   │   class-priority > rule-priority > score > span-len > id        │
   │   emit tokens → "Contact <{sess}:Email_1>" + Manifest           │
   └────────────────────────────┬────────────────────────────────────┘
                                │ clean_text + manifest committed
                                │ (this is what restore will reverse)
                                ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │ PASS 3 — SAFETYNET (observer-only, opt-in)                      │
  │   Mode selector: --safety-net-backend or registry dispatch       │
   │                  ↓                  ↓                           │
   │   ┌──────────────────────┐  ┌────────────────────────────┐     │
   │   │  openai-filter       │  │  kiji-distilbert (v0.8+)   │     │
   │   │  (OPF binary)        │  │  (Kiji ONNX model)         │     │
   │   │                      │  │                            │     │
   │   │  ─ heavier weights   │  │  ─ 8.8 MB DistilBERT       │     │
   │   │  ─ OpenAI's PII set  │  │  ─ 26 PII classes (Kiji)   │     │
   │   │  ─ requires `opf`    │  │  ─ subprocess or ORT       │     │
   │   │    binary install    │  │  ─ tokenizers crate        │     │
   │   └──────────┬───────────┘  └────────────┬───────────────┘     │
   │              │                            │                     │
   │              └──────────────┬─────────────┘                     │
   │                             ▼                                   │
   │   External subprocess contract:                                 │
   │       stdin  ← clean_text (post-tokenization!)                  │
   │       stdout → JSON span array [{start, end, label, score}, …]  │
   │                                                                 │
   │   Gaze compares the SafetyNet spans against the manifest:       │
   │     ─ Span overlaps an emitted token → covered (no leak)        │
   │     ─ Span outside every token       → "Uncovered" suspect      │
   │     ─ Span overlaps partial token    → "PartialBleed" suspect   │
   │     ─ Span overlaps wrong class      → "ClassMismatch" suspect  │
   │                                                                 │
   │   Result: leak_report attached to JSON output. Manifest         │
   │   UNCHANGED. Restore UNAFFECTED. (Axis 2 reversibility intact.) │
   └─────────────────────────────────────────────────────────────────┘
```

## Locale-Aware Registry Dispatch

Kiji DistilBERT has two runtime backends under the same observer-only safety-net contract. `--kiji-backend=subprocess` remains the default for backwards compatibility and for adopters who already pin an external Kiji command; `--kiji-backend=ort` loads the same tokenizer and ONNX model in-process through ONNX Runtime, removing the Python/subprocess install path. Both backends must verify the pinned bundle first: fp32 uses `SHA256SUMS` and `KIJI_DISTILBERT_BUNDLE_SHA256`; int8 uses `SHA256SUMS.int8` and `KIJI_DISTILBERT_INT8_BUNDLE_SHA256`. Every listed artifact is re-hashed before model load, and any mismatch returns a typed `SafetyNetError` without silent fallback.

The ORT backend also accepts `--kiji-distilbert-precision {fp32,int8}`. `fp32`
is the default and remains the compatibility posture. `int8` loads
`model.int8.onnx`, shares the same tokenizer and class map, and is allowed only
with `--kiji-backend=ort`; requesting int8 without the quantized artifact fails
closed at construction. The committed safety-net matrix enforces the precision
trade-off: int8 macro recall must remain within `0.02` of fp32 for every
locale and mode, otherwise the bench gate fails and the int8 path must not
ship.

`Pipeline::with_safety_net(single_backend)` remains the compatibility path. For deployments with language-specific safety nets, `Pipeline::with_safety_net_registry(LocaleAwareModelRegistry)` activates locale-aware Pass-3 dispatch instead. The registry resolves one backend per clean segment using the existing four-tier order: exact locale, parent language, `Global`, then fail-closed.

The v1 dispatch contract is first-match wins. If a tier resolves multiple backends, Gaze invokes only the first registered backend and records that resolved backend id on the safety-net audit row. Multi-backend aggregation is intentionally left as follow-up work so the audit trail stays simple and deterministic.

CLI registry activation is explicit:

```sh
gaze clean \
  --policy quickstart-policy.toml \
  --locale de-DE \
  --safety-net-registry \
  --safety-net-add kiji-distilbert \
  --kiji-distilbert-command /opt/kiji/bin/kiji \
  --kiji-distilbert-model-dir ~/.local/share/gaze/models/kiji-distilbert \
  --kiji-distilbert-locales en-US,en-GB \
  --safety-net-add openai-filter \
  --opf-command /opt/opf/bin/opf \
  --opf-checkpoint ~/.local/share/gaze/models/opf \
  --opf-locales de-DE,de-AT
```

`--safety-net-registry` cannot be combined with `--safety-net-backend`; the registry is the backend selector in that mode.

## North-star fit

Safety nets exist because of axis 1 (reliability — never leak) but must
not weaken axes 2–4. The contract therefore mandates:

- **A1 — never leak.** Safety nets read clean text after pseudonymization
  and report metadata-only suspects. Raw input never leaves the deterministic
  core. Backend-side raw bytes never cross the adapter boundary.
- **A2 — reversibility preserved.** Safety nets do not mutate the manifest
  or emit tokens, so restore round-trips are unaffected by their presence,
  failure, or absence.
- **A3 — agentic-first.** Per-field structured-document traversal lets agent
  tool-call JSON be checked field-by-field, producing field-pathed suspects
  that downstream FP-adjudication tooling can route to the right team.
- **A4 — auditable + deterministic.** Suspects carry the backend id, version,
  decoding-params hash, and an optional replay hash. The closed
  [`SafetyNetError`](../../../crates/gaze-types/src/lib.rs) variant set keeps
  failures typed; the optional `safety_net_log` SQLite table records
  metadata-only rows that the `gaze audit safety-net` subcommand can replay.

If a safety net cannot be initialized, the strict-mode CLI fails closed with
exit `3` and an error variant; tolerant mode logs the suspects and continues.
Both modes preserve the manifest contract.

## Trait shape

`gaze-types` defines the public contract.

```rust
pub trait SafetyNet: Send + Sync {
    fn id(&self) -> &str;
    fn supported_locales(&self) -> &[LocaleTag];
    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError>;
}
```

`SafetyNetContext` is `#[derive(Copy)]`, byte-free, and exposes:

- `manifest: &Manifest` — emitted token spans for the clean text segment,
  used by `Manifest::diff_against` to classify each suspect as
  `Uncovered`, `PartialBleed`, or `ClassMismatch`.
- `locale_chain: &[LocaleTag]` — session-level locale fallback chain.
  `RawDocument::Structured` shares one chain across all fields by design;
  per-field locale annotations are out of scope for v0.6.
- `document_kind: DocumentKind` — `Text` or `Structured`.
- `session_id: Option<&str>` — opaque audit session id.
- `field_path: Option<&str>` — JSONPath-style field selector for structured
  fields, e.g. `$.user.email`.

`SafetyNet::check` returns `Vec<LeakSuspect>`. A suspect carries a clean-text
byte span, a mapped Gaze `PiiClass`, the backend id, an optional confidence
score, the `LeakKind` produced by manifest correlation, the validated
`raw_label`, and an optional `field_path`. Raw payload bytes never appear on
this struct.

### Observer-only contract

The *backend* is observer-only; the *pipeline* may still act on what it reports.
A `SafetyNet` can never rewrite bytes itself — the trait has no return channel
for replacement text and no mutable handle to the manifest, by construction —
but the `SafetyNetPolicy` the caller passes decides what the deterministic core
does with the resulting `LeakReport`: nothing (`Strict`, `Tolerant`), delete the
suspect spans (`Redact`), or tokenize them reversibly and re-run
(`Resolve`). The policy-less entry points below use
`SafetyNetPolicy::default()`, which is `Resolve` + `Redact` — the shipped
production default since v0.8.1. Pass an explicit `Strict` policy to
`Pipeline::clean_with_safety_net_policy_detect_context`, or use
`Pipeline::scan_safety_nets`, when you want report-only behaviour. Mode catalog
and the full lowering table:
[`safety-net-modes.md`](safety-net-modes.md#6-fallback-flag).

The pipeline calls
`Pipeline::clean_with_safety_net_detect_context`, which:

1. Runs the deterministic detection-and-redaction pipeline.
2. Records the emitted token spans into a `Manifest`.
3. Iterates the registered safety nets after a successful clean. Each
   backend receives the clean text and the immutable manifest snapshot.
4. Returns `(CleanDocument, LeakReport)` to the caller.

The bytes on `CleanDocument` are produced exclusively by the deterministic
core. A safety net cannot rewrite, append to, or veto the clean text: under an
enforcing policy it is still the core's tokenizer and redactor that mutate the
document, driven by the report, never the backend.

### Locale gating

Each `SafetyNet` declares `supported_locales`. When the session-level locale
chain does not intersect the backend's supported locales, the orchestrator
emits a `LeakReportTelemetry::LocaleSkipped` event instead of running the
backend. Skip telemetry is bytes-free and is recorded against the same
`safety_net_log` table as suspects.

`LocaleTag::Other(_)` matches strictly against the wire form, not the BCP-47
prefix; this matches the locale-chain semantics described in
[`docs/explanation/policy/locale-chain.md`](../policy/locale-chain.md).

### Closed error variant set

[`SafetyNetError`](../../../crates/gaze-types/src/lib.rs) is an exhaustive,
serde-stable enum:

| Variant | Meaning |
|---------|---------|
| `Unavailable { reason }` | Safety net was requested but is not configured. |
| `WeightsMissing { path }` | Required checkpoint or model file is missing. Path is sanitized to `<missing:filename>`. |
| `ModelUnavailable { reason }` | Backend could not be loaded, perms verification failed, or runtime is missing. |
| `InputTooLarge { limit, actual }` | Clean text exceeded the configured input cap. |
| `Runtime { message }` | Backend execution failed, including subprocess timeouts. |
| `InvalidOutput { message }` | Backend returned malformed output (non-UTF-8 stdout, non-finite score, unknown label). |

The CLI maps each variant to a stable `SafetyNetFailure` exit-3 sub-variant
so adopters can branch on `Unavailable` versus `Timeout` versus
`InvalidOutput` without parsing free-form text.

## OpenAI Privacy Filter adapter

The first shipped backend is the
[`OpenAiFilterSafetyNet`](../../../crates/gaze-recognizers/src/safety_net/openai_filter/mod.rs).
It calls the official `openai/privacy-filter` CLI as a subprocess.

### Command choice

Gaze binds to the **official** `openai/privacy-filter` repository. Adopters
must install `opf` from a pinned upstream Git revision or an official release
tarball. The official CLI was chosen over the `chiefautism/privacy-parser`
fork because it documents pipe input, exposes a stable JSON schema, and
publishes a reproducible Git history. The fork could be re-evaluated if a
later review confirms native byte spans and the absence of PII-bearing JSON
fields, but it is not the v0.6 default.

The adapter always invokes `opf --format json --output-mode typed`. Output
mode `typed` is the only accepted shape; other modes are not parsed.

### Subprocess configuration

[`SubprocessOpenAiFilterConfig`](../../../crates/gaze-recognizers/src/safety_net/openai_filter/backend/subprocess.rs)
is a builder with the following defaults:

- `timeout`: 5 seconds. Configurable via `--safety-net-timeout-ms`.
- `max_input_bytes`: 1 MiB. Configurable via `--safety-net-input-limit-bytes`.
- `max_stdout_bytes`: 4 MiB.
- `capture_stderr`: `false`. Stderr is routed to `Stdio::null()` by default.
- Decoding params: `format=json`, `output_mode=typed`. Operating-point flags
  add `min_score` and `operating_point` entries.

`SubprocessOpenAiFilterConfig::from_env()` reads `GAZE_OPENAI_FILTER_OPF` so
adopters can pin the install path centrally.

### PII-bearing upstream JSON fields are stripped at the boundary

Upstream OPF emits per-span `text` and `placeholder` fields that carry the
literal source bytes. These are private deserialization details inside the
adapter:

- `PrivateOpfSpan` and `PrivatePiiString` are private structs in the
  adapter module. The crate root re-exports the public trait shape and the
  config builder, but never the private structs themselves.
- `PrivatePiiString::Debug` writes `<private-opf-field>` instead of the raw
  contents.
- `PrivatePiiString::Drop` clears the underlying string buffer when the
  span is dropped.
- After `serde_json::from_str` returns, the adapter calls
  `PrivateOpfSpan::into_raw_span`, which produces a `RawSpan` containing
  only `start`, `end`, `label`, and `score`. The `_text` and `_placeholder`
  fields drop on the same statement, with their `Drop` impl scrubbing the
  buffer.
- Top-level `_text` and `_redacted_text` on the redaction-output shape
  follow the same pattern.

After this projection, no part of Gaze that consumes safety-net output sees
upstream raw bytes. The adversarial regression
`safety_net_correlates_raw_spans_with_manifest_without_source_text` covers
this invariant.

### Stderr discipline

By default `child.stderr` is `Stdio::null()`, so verbose backend logs cannot
race with the JSON adapter or appear in operator logs. Adopters who need
diagnostics can enable `SubprocessOpenAiFilterConfig::with_stderr_diagnostics(true)`,
which:

1. Captures stderr in a bounded buffer of at most 256 bytes.
2. Maps non-printable bytes to spaces.
3. Sanitizes whitespace-separated tokens with the same redactor used for
   error messages: any token containing `@` or seven or more ASCII digits
   is replaced with `<redacted>`. This catches the most common email and
   phone shapes that backends might log.
4. Truncates to the 256-byte cap.

The `verbose_stderr_is_stripped_and_capped` test locks both the cap and the
sanitization rule.

### Subprocess timeout and resource isolation

The subprocess runner enforces a single deadline that covers stdin write,
stdout read, stderr read, and child wait. On timeout the adapter:

1. Sends `SIGKILL` (`Child::kill`) and reaps the process via `wait` to
   prevent zombies.
2. Drains the writer/reader threads so file descriptors are not leaked.
3. Returns `SafetyNetError::Runtime` with the message
   `"opf subprocess timed out and was killed"`. The CLI maps this branch to
   exit-code `3` with `variant = "Timeout"`.

Initialization failures are cached in a `OnceLock<Result<Arc<_>, Arc<_>>>`
so deterministic problems (missing checkpoint, malformed config) are not
retried on every safety-net check. This is the explicit fix for "retry storm
on every clean" and is locked by `empty_command_failure_is_cached`.

### Checkpoint and cache permission verification

`SubprocessOpenAiFilterBackend::new` runs path-safety checks before any
spawn:

- The `opf` command path must be a regular file (not a symlink) when an
  absolute path is supplied. Bare command names are accepted so adopters
  can rely on `PATH` resolution when the host is hardened.
- `--openai-filter-checkpoint` must exist; missing files produce
  `WeightsMissing { path: "<missing:<filename>>" }`. The path is sanitized
  to the file basename so logs cannot leak operator directory layout.
- Checkpoint files and directories must be owned by the current uid, must
  not be symlinks, and must not be group/world writable on Unix. Directories
  must be mode `0700`. Windows enforces non-symlink + readonly ACL.
- The optional cache directory is created mode `0700` if it does not exist
  and is then verified by the same recursive walk.

The `group_writable_checkpoint_file_fails_closed` test pins the perm rule.

### Class mapping

`map_openai_label` accepts the closed set `private_person`,
`private_address`, `private_email`, `private_phone`, `private_url`,
`private_date`, `account_number`, `secret`. Unknown labels return
`SafetyNetError::InvalidOutput`. The mapping into Gaze's `PiiClass` lives
in [`class_map.rs`](../../../crates/gaze-recognizers/src/safety_net/openai_filter/class_map.rs);
the `class-map-override-safety` xtask gate runs the
`all_official_labels_map_exactly_to_gaze_classes` test on every PR.

## Structured-document per-field behavior

`Pipeline::clean_with_safety_net_detect_context` traverses
`RawDocument::Structured` field by field. For each scalar string field it:

1. Cleans the field through the deterministic pipeline.
2. Builds a per-field `Manifest` from the emitted token spans.
3. Runs each registered safety net with `field_path = Some(<JSONPath>)`.
4. Aggregates the per-field reports into the run-level `LeakReport`.

This means a class mismatch detected on `$.user.email` is reported with
that field path, and the FP-adjudication query
`gaze audit safety-net query --field-path '$.user.email'` can isolate
it. Locale-skip telemetry is also recorded per field when the session-level
locale chain does not match.

The integration coverage lives in
`crates/gaze/tests/safety_net.rs::structured_safety_net_traverses_nested_fields_and_preserves_shape`.

## Replay hash

`LeakReport.replay_hash` is an `Option<String>`. When set, it is a stable
hash over the backend id, backend version, decoding-params, and operating
point used for the run. The hash supports replaying the same input through
the same configuration to see whether the FP set has stabilized.

Replay determinism is only guaranteed when the operator fixes the command
path, checkpoint, operating point, minimum score, and decode parameters
**externally**. The adapter emits and stores the hash; it does not pin
upstream weights or downloads. Adopters using a different `opf` checkpoint
will see a different hash and a different suspect set, by design.

## `safety_net_log` audit table

When `gaze clean --audit-db <path>` is combined with `--safety-net <kind>`,
each suspect plus each `LocaleSkipped` telemetry event is appended to the
`safety_net_log` table in the same SQLite database that holds the
deterministic redaction log.

```sql
CREATE TABLE IF NOT EXISTS safety_net_log (
    id INTEGER PRIMARY KEY,
    safety_net_id TEXT NOT NULL,
    raw_label TEXT NOT NULL,
    mapped_class TEXT NOT NULL,
    leak_kind TEXT NOT NULL,
    span_len INTEGER NOT NULL,
    document_kind TEXT NOT NULL,
    field_path TEXT NULL,
    score REAL NULL,
    created_at INTEGER NOT NULL,
    session_id TEXT NULL,
    pipeline_class TEXT NULL,
    safety_net_replay_hash TEXT NULL,
    backend_id TEXT NULL,
    backend_version TEXT NULL,
    decoding_params_hash TEXT NULL,
    telemetry_kind TEXT NULL
);
```

The schema stores **metadata only**:

- `raw_label` is the validated upstream label, such as `private_email` —
  not the upstream raw text.
- `mapped_class` is the Gaze `PiiClass` produced by the class map.
- `span_len` is the byte length of the suspect span; the offsets are not
  persisted.
- `field_path` is the structured field selector when applicable.
- `pipeline_class` is the manifest class for `ClassMismatch` rows.
- `telemetry_kind` is set for `LocaleSkipped` rows so downstream queries
  can filter telemetry from suspects.

The `safety_net_log_does_not_persist_suspect_or_placeholder_bytes` and
`restricted_columns_have_no_raw_payload_fields` tests are run on every PR
by the `safety-net-sanity` gate; both lock that no upstream raw text or
placeholder bytes are stored.

The schema lives in `gaze-audit`; the protected-path Dylint gate keeps
`gaze` core free of `gaze-audit` imports outside the explicit
audit-responsible allowlist
(see [`docs/explanation/contributing/xtask-gates.md`](../contributing/xtask-gates.md#cargo_metadata_audit_isolation-self-test)).

### Querying suspects

`gaze audit safety-net query --audit-db <path>` filters the
`safety_net_log` table by leak kind, raw label, mapped class, structured
field path, and creation time. The query is opened
`SQLITE_OPEN_READ_ONLY` so the CLI cannot mutate the log even if compromised.

## CI gate

`safety-net-sanity` is the canonical local pre-push gate for the safety-net
surface.

The xtask command lives at
[`crates/xtask/src/safety_net_sanity.rs`](../../../crates/xtask/src/safety_net_sanity.rs)
and batches required behavioral tests across four target suites:

- `gaze` — manifest diff and structured traversal invariants.
- `gaze-cli` — strict/tolerant exit-code behavior.
- `gaze-recognizers` — OPF subprocess boundary, stderr sanitization, label
  mapping.
- `gaze-audit` — `safety_net_log` schema and bytes-free invariants.

Run the xtask gate manually before shared-branch pushes; the gate is **not**
scheduled nightly in v0.6 and the live-model nightly workflow is deferred —
see the "Future work" section below.

## Activation surface

v0.6 activates the safety net through the CLI or the programmatic builder
on `Pipeline`. There is **no policy-TOML surface** in v0.6. Policy support
is a deliberately deferred decision so the activation contract can be locked
down before TOML adopters take a dependency on the shape.

The minimum CLI form is:

```sh
gaze clean \
  --policy=policy.toml \
  --safety-net=openai-filter \
  --openai-filter-command=/opt/opf/bin/opf \
  --openai-filter-checkpoint=/opt/opf/checkpoint \
  --safety-net-mode=strict
```

Programmatic adopters call `Pipeline::with_safety_net(OpenAiFilterSafetyNet::new(config))`
behind the `safety-net` feature on `gaze` and `safety-net-openai` on
`gaze-recognizers`. Both features are off by default; the safety-net code
path is excluded from the default `cargo build` graph.

`crates/gaze-cli/README.md` documents every flag, the strict/tolerant exit
codes, and synthetic examples. `docs/reference/policy.md` notes the explicit absence
of a TOML surface.

## Future work (deferred to a post-v0.6.0 release)

The following items are filed for a release after v0.6.0 and intentionally
not in the v0.6 SafetyNet rollup scope:

- **Live-model nightly workflow.** A scheduled cron that runs the safety
  net against a non-empty synthetic corpus to detect FP-rate drift between
  checkpoint upgrades.
- **Native `ort` backend.** A first-party in-process backend that loads OPF
  weights through `ort` plus a `weights.rs` SHA-pinned scaffolding module,
  removing the subprocess hop. The trait shape on `OpenAiFilterBackend`
  was designed so the same adapter API serves both subprocess and in-process
  implementations.
- **Fetch / download command.** A `gaze safety-net fetch` UX that pulls a
  pinned `opf` build into a private cache directory and verifies the
  checksum offline. Closes the "first-run requires manual install" gap.
- **Long-lived subprocess / daemon mode.** The current adapter spawns one
  `opf` invocation per clean. A persistent helper would amortize startup
  cost when latency budgets tighten.
- **False-positive adjudication dashboard.** A UI on top of
  `gaze audit safety-net query` and `audit export` that lets reviewers
  triage suspects across runs.

Cross-references:

- PR #91 — Pass-3 SafetyNet rollup.
- v0.6.0 audit feature shim drop. Adopters must import concrete audit sinks
  from `gaze-audit` directly.

## See also

- [`docs/reference/policy.md`](../../reference/policy.md) — explicit note that the v0.6 safety
  nets are CLI-only.
- [`crates/gaze-cli/README.md`](../../../crates/gaze-cli/README.md) — full flag
  reference, exit-code map, and synthetic examples.
- [`docs/reference/crates.md`](../../reference/crates.md) — workspace map, including the
  safety-net feature gates on `gaze`, `gaze-recognizers`, and `gaze-cli`.
- [`docs/explanation/contributing/xtask-gates.md`](../contributing/xtask-gates.md) — `safety-net-sanity` and
  `class-map-override-safety` gates.
- [`AGENTS.md`](../../../AGENTS.md#project-north-star) —
  the five-axis north star that the safety-net contract is checked against.
