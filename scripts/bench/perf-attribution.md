# Performance attribution

`perf_attribution.py` separates source cost from policy/model cost without changing
protection. Specification: **gaze-perf-attribution-v2**. It does not replace the
detection gain gate or alter the release benchmark specification.

The runner requires an explicit `--dry-run` or `--measure`. Dry runs clean five
documents per side per arm, execute no end-to-end timing clocks, omit percentile
summaries, and always say `NOT A MEASUREMENT`, `publishable: false`. Timer builds
still exercise their diagnostic clocks to check the protocol. Never use those
values as measurements.

## Inputs and arms

The default sources are v0.16.0 and v0.17.0. Supply `--binary-a` and `--binary-b`
to use existing executables, or omit either to build that revision in an Anvil
checkout with Rust 1.96.0, two build jobs and `nice -n 10`. Builds run sequentially
and refuse `CARGO_TARGET_DIR`. Supplied binaries' source revisions are declarations;
the receipt independently hashes their bytes. Identical binary hashes are refused.
Verify downloaded release binaries against their published `.sha256` assets first.
Label them with `--binary-kind-a published-release` and
`--binary-kind-b published-release`. Automatic builds require a clean checkout.

All five arms use the same two binaries:

| Arm | Policy | Process mode |
| --- | --- | --- |
| `common` | One frozen `--common-policy` and its exact model files | Warm daemon |
| `shipped` | Each executable's own `gaze setup` output | Warm daemon |
| `vernio` | Davlan mBERT, threshold 0.3; core; de-DE/en-US/global; address blocks; no safety net or secrets pack | Warm daemon |
| `source-tree` | Same as Vernio | New process/model load for each file |
| `source-tree-daemon` | Same as Vernio | One resident daemon, same ordered files as `source-tree` |

`--shipped-policy-a` and `--shipped-policy-b` accept policies previously generated
by those versions' setup commands. Otherwise the runner invokes setup. Setup may
install missing models; prepare bundles before reserving the benchmark machine.
Setup, daemon, and one-shot children receive the same scrubbed environment.
The runner keeps only `PATH`, `HOME`, `XDG_DATA_HOME`, `SYSTEMROOT`, `WINDIR`,
`TMPDIR`, `TMP`, `TEMP`, `LANG`, and `LC_ALL`, then sets `GAZE_PERF=1`.
Inherited `GAZE_*` overrides, `OPF_CHECKPOINT`, and other variables cannot select
unrecorded models or policies. The receipt hashes this resolved environment
without exposing its values. Setup resolves default model locations into the
policy before the runner inventories those files.

Active model sections must pin a literal, absolute model directory.
Home shorthand and environment-variable expansion are unsupported. Select a
different model by editing an explicit policy path or using `--ner-model-dir`
for the Vernio and source-tree arms; these inputs are hashed.
Policies use bundled rulepacks and inline dictionary terms. The runner rejects
external rulepack paths and terms files because this specification does not pin
those extra inputs. Every file in every
referenced model directory is hashed before and after the capture. The runner
also rejects policies or executables that change during a capture.

The corpus is read directly from committed Git blobs, defaulting to coverage-loop
text documents at v0.17.0. `--corpus-rev` and `--corpus-subdir` change that pinned
selection. The throughput tree defaults to commit `5ac6c06`; `--tree-rev` changes
it. It selects 400 UTF-8 text files, preferring testdata, with a seeded order.
Eligible suffixes are `.txt`, `.md`, `.json`, `.jsonl`, `.toml`, `.yaml`, `.yml`,
`.rs`, `.py`, `.csv` and `.tsv`; symlinks, NUL-containing files, empty files and
files larger than 1,000,000 bytes are excluded. Fewer than 400 eligible files is
an error even during dry runs. The receipt pins the resolved commit and every
selected document hash. Both source-tree arms bind the same ordered IDs, exact UTF-8 bytes, duplicate occurrences, and policy with `input_sha256`. Dry runs use the same first five files in both arms. File identities are hashed rather than exposing paths.

## Run

Use a prepared checkout and install models with `gaze setup` first. For example,
with verified binaries in a local `artifacts` directory:

```sh
uv run --project scripts/bench python scripts/bench/perf_attribution.py \
  --binary-a artifacts/v0.16.0/gaze --binary-b artifacts/v0.17.0/gaze \
  --common-policy artifacts/common.toml \
  --ner-model-dir "$GAZE_NER_MODEL_DIR" \
  --dry-run --out artifacts/attribution-dry-run.json
```

For an actual comparison, acquire the Solo **bench-machine** lease, ensure no
other benchmark or build is active, and replace `--dry-run` with `--measure`.
Default: four complete passes, two warm-up documents per daemon process, seed
20260710. The first side is seeded, then whole passes alternate AB/BA. Each
side starts a fresh daemon for each pass; each document uses a distinct session,
so repeated-value state cannot accumulate across corpus documents. One-shot
warmups warm filesystem/process caches only; every measured file still starts
a fresh process and loads its models.

The receipt records hardware, OS, `uptime`, load before/after each side, binary,
policy and model hashes, the combined runner/clock-contract hash, document identities, numeric samples, work counters,
p50/p95/p99 per document and stage, and files/minute. End-to-end daemon latency
includes request serialization, pipe transport and CLI response serialization.
It excludes Python response parsing. `startup_e2e_ns` measures process launch through receipt of the startup record on stderr. The daemon emits that record before waiting for input. `startup_total_ns` sums child startup clocks, while `processing_total_ns` sums child request clocks. Neither total includes warmups. One-shot rows retain startup records for every file; daemon rows retain one startup record per pass.
One-shot latency includes process and model startup. Files/minute uses the sum
of request latencies as its denominator, excluding warmups and receipt work. Stage times are inclusive and
must not be summed: `registry_ns` includes recognizers and NER; `rules_ns` sums
rule-evidence recognizer calls; `sweep_ns` includes any sweep-triggered second
resolution. Further address/house-number resolutions contribute to `resolve2_ns`.
A missing timer protocol on old binaries means unknown stages, never zero-cost
stages. A known but unused stage on an instrumented document has zero counters.

The quiet-host threshold is **load1 < 2.0**, imported from `cli-latency.py`.
`nym-warm-latency.py` records load but defines no threshold. Any sampled load at
or above the threshold, fewer than two passes, no warmup, or a timer protocol on only one side prevents a publishable
receipt. Do not derive performance claims from a failed gate.

## Timer builds and privacy

Build `gaze-cli` with `--features perf-timers`; the feature is default-off across
core, assembly, recognizers and shared types. Default builds expand the instrumentation
macros to no-ops: no clock, TLS, identity allocation, environment lookup or diagnostic IO from the instrumentation.
An instrumented CLI additionally needs `GAZE_PERF=1` to collect startup and requests. It
writes one startup `GAZE_PERF {json}` record and one record per request to stderr.
The startup clock starts at the first operation in `main`; it excludes OS loader
work before `main`. Stdout retains its existing protocol. A failed diagnostic
write does not change protection behavior.

Schema **2** adds `phase` (`startup` or `request`) and a process-local `ordinal`.
Startup is ordinal 0. Requests count from 1, including warmups. The runner rejects
schema 1, partial traces, missing hooks, wrong ordinals, processing clocks in
startup, and model initialization attached to request records. Nym timer backports must
include both split clocks below; aggregate-only Nym traces are refused. Old executables that emit no records retain unknown stage
costs. `--binary-kind-a local-timer`, `--binary-kind-b local-timer`, and
`--build-timers` require the full protocol; a missing trace is an error.
Old v1 receipts remain historical records and cannot be relabeled as v2 evidence.
The old patch below emits v1 and needs the new boundaries before it can supply
v2 stage attribution.

The record contains fixed metric keys, durations, invocation/candidate/window
counts, input byte length and process-local recognizer ordinals (`r1`, `r2`, …).
It never carries input text, matched values, token mappings, session IDs, raw
custom IDs, or unsalted hashes of custom IDs. Ordinals follow first encounter
order and remain stable across requests in one process. They do not prove that
the same ordinal refers to the same recognizer in different processes or builds.
The summary does not aggregate recognizer ordinals across processes.
Use aggregate stage clocks for cross-build comparisons. Resolve custom IDs only
locally; public receipts carry no reversible recognizer-name lookup.

Treat local source, policy, and model files as trusted inputs. The receipt does
not copy their contents or paths. The runner consumes then discards CLI
responses and suppresses child diagnostics on failure because both can contain
reversible values.

`manifest_ns` covers ledger allocation/insertion, projection and integrity
validation. `audit_ns` covers synchronous primary/safety/restore audit sink calls.
NER records tokenization, encodes, ORT calls/windows and session-mutex wait. Nym
records tokenization, ORT windows and mutex wait, plus separate stable/neutral
view durations and window counts. Stable and neutral views both remain active.
No allocation profiler is installed. Constructor counts are the `invocations`
fields of the lifecycle metrics below. They count attempted calls, including
failed calls; the runner refuses failed documents.

## Lifecycle boundaries

All `ns` values are inclusive wall-clock durations. Parent and child durations
must not be added together. The following leaf stages run serially without
overlap inside a constructor: bundle verification, tokenizer load/configuration,
and ORT model/session load. Their sum omits constructor bookkeeping. The runner
checks invocation counts against the policy and checks parent/child bounds.
The Nym assembly check and constructor check remain separate invocations of
`bundle_verify_ns`; this change preserves both checks. NER and DOB constructors
use `model_constructor_ns`; Nym uses `nym_model_constructor_ns`. The extra outer
Nym check is enclosed by `nym_outer_verify_ns`, outside the constructor. The
aggregate verification + tokenizer + model duration must fit within the combined
constructors plus that outer check. The outer check's enclosing clock includes
its timer bookkeeping, so this aggregate bound conservatively includes that
small overhead. Tokenizer + model durations must also fit within the constructors
alone. No aggregate constructor duration is charged to the wrong sibling.

The phase table and clock graph below are generated from
`perf_lifecycle_contract.py`, which validation imports directly. Refresh with
`python scripts/bench/perf_lifecycle_contract.py`; verify with `--check`.

<!-- BEGIN GENERATED CLOCK CONTRACT -->
| Metric | Phase | Boundary |
| --- | --- | --- |
| `process_to_ready_ns` | startup | First operation in main through completion of startup |
| `process_to_policy_ns` | startup | First operation in main through policy load and overrides |
| `policy_load_ns` | startup | CLI policy load and overrides, including the policy-less default |
| `pipeline_build_ns` | startup | CLI policy resolution through PipelineBuilder::build |
| `assembly_ns` | startup or request | build_pipeline_builder_inner, including NER and DOB constructors |
| `recognizer_construction_ns` | startup or request | Recognizer registration before model registration |
| `rule_construction_ns` | startup or request | Policy action rule construction after model registration |
| `nym_attach_ns` | startup | Nym outer verification, constructor and checked attachment |
| `model_constructor_ns` | startup | NER and DOB constructors, including their children |
| `nym_model_constructor_ns` | startup | Nym constructor, including its children |
| `nym_outer_verify_ns` | startup | Assembly's extra Nym bundle verification call |
| `bundle_verify_ns` | startup | All complete pinned-bundle verifications, including Nym's extra call |
| `tokenizer_load_ns` | startup | All constructor tokenizer loads and configuration |
| `model_load_ns` | startup | All constructor ORT session builds and model loads |
| `daemon_startup_ns` | startup | Daemon entry through setup, before the request loop |
| `request_ns` | request | One-shot processing after startup, or one daemon handle_line call |

The hook graph below declares serial children (sum <= parent capacity) or
overlapping children (each <= parent capacity, with no sum). Admission
checks every parent edge and every serial sum. A parent capacity containing
multiple clocks pools their durations; it is not a new measured clock.
Daemon-build and request-assembly apply only in daemon mode.
`recognizers[*]` expands to each emitted recognizer ordinal. Parent edges
check each ordinal; serial groups sum them. As a parent, it pools their durations.

| Group | Phase | Relationship | Children | Parent capacity | Hook sites and exceptions |
| --- | --- | --- | --- | --- | --- |
| ready-build | startup | serial | `pipeline_build_ns` + `nym_attach_ns` | `process_to_ready_ns` | pipeline/run.rs: prepare_clean_pipeline; cli/perf.rs: Document::from_env |
| ready-policy | startup | serial | `process_to_policy_ns` + `assembly_ns` + `nym_attach_ns` | `process_to_ready_ns` | pipeline/build.rs: policy_loaded before assembly; pipeline/run.rs: Nym after build. pipeline_build overlaps process_to_policy; use two alternative serial groups |
| ready-daemon | startup | overlapping | `daemon_startup_ns` | `process_to_ready_ns` | commands/daemon.rs: run; cli/perf.rs: ready. daemon_startup overlaps the policy prefix and contains pipeline construction |
| policy-load | startup | overlapping | `policy_load_ns` | `process_to_policy_ns` | pipeline/build.rs: resolve_pipeline_builder |
| pipeline | startup | serial | `policy_load_ns` + `assembly_ns` | `pipeline_build_ns` | pipeline/build.rs: resolve_pipeline and resolve_pipeline_builder |
| daemon-build | startup | serial | `pipeline_build_ns` + `nym_attach_ns` | `daemon_startup_ns` | commands/daemon.rs: run and Daemon::new |
| assembly | startup | serial | `recognizer_construction_ns` + `model_constructor_ns` + `rule_construction_ns` | `assembly_ns` | gaze-assembly/lib.rs: build_pipeline_builder_inner |
| nym-attach | startup | serial | `nym_outer_verify_ns` + `nym_model_constructor_ns` | `nym_attach_ns` | gaze-assembly/lib.rs: attach_nym_safety_net |
| constructor-loads | startup | serial | `tokenizer_load_ns` + `model_load_ns` | `model_constructor_ns` + `nym_model_constructor_ns` | ner/detector.rs; ner/backend/ort.rs; dob_judge.rs; safety_net/nym/ort.rs. leaf totals pool NER, DOB and Nym constructors; no per-constructor leaf split exists |
| constructor-leaves | startup | serial | `bundle_verify_ns` + `tokenizer_load_ns` + `model_load_ns` | `model_constructor_ns` + `nym_model_constructor_ns` + `nym_outer_verify_ns` | ner/loader.rs; bundle.rs; gaze-assembly/lib.rs; constructor hooks above. bundle_verify also includes Nym outer verification; load-only group excludes that capacity |
| request-sweep | request | serial | `assembly_ns` + `registry_ns` + `resolve1_ns` + `sweep_ns` + `safety_view_build_ns` + `safety_model_ns` + `manifest_ns` + `audit_ns` | `request_ns` | pipeline.rs: redact_text_with_manifest_uncached, sweep_repeat_values, run_safety_nets_in_context, log_*; pipeline/occurrence.rs: Ledger; safety_net_scan.rs: new. resolve2 partly runs inside sweep and partly afterwards; never sum sweep + resolve2 |
| request-resolve | request | serial | `assembly_ns` + `registry_ns` + `resolve1_ns` + `resolve2_ns` + `safety_view_build_ns` + `safety_model_ns` + `manifest_ns` + `audit_ns` | `request_ns` | pipeline.rs: recovery::plan at sweep, house-number and address-piece sites. alternative serial group accounts for resolve2 without double counting sweep |
| request-assembly | request | serial | `recognizer_construction_ns` + `rule_construction_ns` | `assembly_ns` | commands/daemon.rs: clean_request; gaze-assembly/lib.rs: pipeline_with_context. context rebuilds recognizers and rules; resident NER, Nym and DOB handles are reused |
| request-overlap | request | overlapping | `validators_ns` | `request_ns` | gaze-types/lib.rs: ValidatorKind. validators run inside recognizers, registry vetoes, recovery and other request work |
| registry-recognizers | request | serial | `recognizers[*]` | `registry_ns` | registry.rs: detect_candidate_pool/detect_declared; perf.rs: RecognizerSpan. built-in clean runs recognizers serially inside registry |
| recognizer-work | request | serial | `rules_ns` + `ner_tokenize_ns` + `ner_mutex_wait_ns` + `ner_ort_ns` | `recognizers[*]` | perf.rs: RecognizerSpan::drop; ner/backend/preparation.rs and ort.rs. rules is the exact rule-evidence subset; serial NER leaves belong to learned evidence. The protocol cannot identify the NER ordinal, so these disjoint durations use the pooled recognizer capacity |
| nym-views | request | serial | `nym_stable_ns` + `nym_neutral_ns` | `safety_model_ns` | pipeline.rs: run_safety_nets_in_context; safety_net/nym/mod.rs: check_with_neutral_and_telemetry |
| nym-inference | request | serial | `nym_tokenize_ns` + `nym_mutex_wait_ns` + `nym_ort_ns` | `nym_stable_ns` + `nym_neutral_ns` | safety_net/nym/ort.rs: score_pieces and run_window. leaf totals pool both serial views; the protocol does not split leaves by view |

A startup record must have zero processing durations, invocation/candidate
counts, work counters and recognizer entries. Daemon context requests may
emit assembly, recognizer-construction and rule-construction clocks, once each.
One-shot requests cannot emit those clocks. All request records must have
zero model initialization and other startup-only durations and counts.
<!-- END GENERATED CLOCK CONTRACT -->

`pipeline_build_ns` includes `policy_load_ns` and `assembly_ns`. Nym attaches
after that CLI builder returns, so `nym_attach_ns` belongs to
`process_to_ready_ns`, not to `pipeline_build_ns`. One-shot startup includes
stdin reading and initial session creation. Daemon startup excludes input wait.
The request clock includes clean response construction; daemon wire serialization
runs after `handle_line` and belongs only to end-to-end latency. A daemon request
with a context constructs request-specific recognizers and rules, even when the
context is empty. Admission includes that assembly in the request clock and checks
its serial children. Resident NER, Nym and DOB models remain initialized at startup.
The benchmark arms send no context.

No speedup or regression follows from adding these clocks. Controlled paired
measurements need the benchmark-machine lease and a quiet host.

The capture is thread-local and synchronous. Library callers can use
`gaze_types::perf::Capture`; it must cover execution on the same thread and cannot
move across threads. The CLI instruments its synchronous document boundaries.
Nested captures restore their parent. This is diagnostic instrumentation with
its own overhead; compare equally instrumented revisions for stage attribution,
and use unmodified releases for end-to-end regression confirmation.

## Earlier revisions

Published v0.16.0/v0.17.0 executables have no timer protocol and are measured only
end to end. `perf-timers-v0.16.0.patch` applies directly to the v0.16.0 tag without
cherry-picking implementation commits. It includes the earlier v1 timer layer
and document-only boundaries, adapted to that version's recognizer return path
and one-shot clean path. That historical patch alone is incompatible with this
v2 runner. Backport the current schema-2 protocol and lifecycle hooks to the tag
before building in an isolated Anvil checkout with `--features perf-timers`.
Use `--binary-kind-a local-timer --timer-patch-a <adapted-v2-patch>` for that side.
The receipt records the adapted patch hash alongside its binary hash. For a
revision already containing schema 2, `--build-timers` enables it during automatic
builds. Do not pass that flag to an unpatched old revision.

## Plumbing validation

The model-free Python tests use fake daemon protocols to prove lifecycle and request pairing, old-binary fallback, schema rejection, identical-binary refusal, privacy, provenance, exact ordered source-tree identity, percentiles and the quiet-host gate. `GAZE_ATTRIBUTION_TEST_BINARY` enables a synthetic test of actual CLI hooks through the runner. `cli_pipe` compares clean text, manifest,
entries and safety disposition byte for byte with capture disabled/enabled from
the same initial session snapshot, and verifies exact restore for each fixture. The capture includes construction as well as processing.
Nonzero witnesses cover every parent edge, serial group and wrong-phase
placement. Each serial witness violates exactly one sum while all parent edges
still fit. Each parent witness is rejected by the full graph. Parent mutation
tests also exercise that edge alone through lifecycle admission, because the
serial sum or an ancestor edge would otherwise reject the same record.

After committing, run `perf_lifecycle_mutation.py --out <directory>`. It removes
each parent check, serial sum and phase, assembly-count and startup-work guard in an isolated
Python copy. Each mutation must fail one executed assertion, then pass the same
test after restoration. The receipt pins both counts; logs retain every result.


For a synthetic comparison across separate feature-off and timer executables, run
`perf_instrumentation_proof.py --plain <binary> --timers <binary> --out <proof.json>`.
It runs seven fixtures through one-shot and daemon clean/restore with no latency
clocks. The proof normalizes only random eight-hex session prefixes inside declared
ordinary tokens. It compares token spelling/index, mapping values, clean text,
one-shot stats/disposition and daemon manifest spans exactly within each mode.
It does not claim cross-mode byte identity. The Rust fixed-snapshot test retains
exact byte comparison without prefix normalization.

`perf_hook_identity.py --base <commit> --out <proof.json>` hashes the ten runtime
files changed by lifecycle hooks. It removes only the named hooks and refuses if
the remaining bytes differ from the base. Timer implementation, feature metadata,
tests and this diagnostic runner are outside that text-path proof.
