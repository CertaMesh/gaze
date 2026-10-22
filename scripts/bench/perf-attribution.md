# Performance attribution

`perf_attribution.py` separates source cost from policy/model cost without changing
protection. Specification: **gaze-perf-attribution-v1**. It does not replace the
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

All four arms use the same two binaries:

| Arm | Policy | Process mode |
| --- | --- | --- |
| `common` | One frozen `--common-policy` and its exact model files | Warm daemon |
| `shipped` | Each executable's own `gaze setup` output | Warm daemon |
| `vernio` | Davlan mBERT, threshold 0.3; core; de-DE/en-US/global; address blocks; no safety net or secrets pack | Warm daemon |
| `source-tree` | Same as Vernio | New process/model load for each file |

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
selected document hash. File identities are hashed rather than exposing paths.

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
policy and model hashes, the runner hash, document identities, numeric samples, work counters,
p50/p95/p99 per document and stage, and files/minute. End-to-end daemon latency
includes request serialization, pipe transport and CLI response serialization.
It excludes Python response parsing. Stage clocks cover the library only.
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
core, recognizers and shared types. Default builds expand the instrumentation
macros to no-ops: no clock, TLS, ID hashing, environment lookup or diagnostic IO.
An instrumented CLI additionally needs `GAZE_PERF=1` to collect a document. It
writes exactly one `GAZE_PERF {json}` record to stderr per daemon request or
one-shot clean. Stdout retains its existing protocol. A failed diagnostic write
does not change protection behavior.

The record contains fixed metric keys, durations, invocation/candidate/window
counts, input byte length and SHA3-256 recognizer IDs. It never carries input
text, matched values, token mappings, session IDs or raw custom IDs. To locate a
specific recognizer, hash its declared ID locally using SHA3-256. These hashes
are unsalted: guessable custom IDs can be recovered by dictionary matching.
This remains a privacy limitation of local attribution receipts. Before sharing
an adopter receipt, remove custom recognizer IDs or replace them with locally
keyed hashes. Per-receipt keyed IDs are a follow-up, outside this change.

Treat local source, policy, and model files as trusted inputs. The receipt does
not copy their contents or paths. The runner consumes then discards CLI
responses and suppresses child diagnostics on failure because both can contain
reversible values.

`manifest_ns` covers ledger allocation/insertion, projection and integrity
validation. `audit_ns` covers synchronous primary/safety/restore audit sink calls.
NER records tokenization, encodes, ORT calls/windows and session-mutex wait. Nym
records tokenization, ORT windows and mutex wait, plus separate stable/neutral
view durations and window counts. Stable and neutral views both remain active.
No allocation profiler is installed; allocation counts and model-constructor
counts are outside this specification.

The capture is thread-local and synchronous. Library callers can use
`gaze_types::perf::Capture`; it must cover execution on the same thread and cannot
move across threads. The CLI instruments its synchronous document boundaries.
Nested captures restore their parent. This is diagnostic instrumentation with
its own overhead; compare equally instrumented revisions for stage attribution,
and use unmodified releases for end-to-end regression confirmation.

## Earlier revisions

Published v0.16.0/v0.17.0 executables have no timer protocol and are measured only
end to end. `perf-timers-v0.16.0.patch` applies directly to the v0.16.0 tag without
cherry-picking implementation commits. It includes the same shared timer layer
and measurement boundaries, adapted to that version's recognizer return path
and one-shot clean path. Apply it in an isolated Anvil checkout, build with
`--features perf-timers`, and label the resulting binary as a local timer build.
Use `--binary-kind-a local-timer --timer-patch-a <patch>` for the patched side.
The receipt records the patch hash alongside its binary hash. For a later revision containing
this feature, `--build-timers` enables it during automatic builds. Do not pass
that flag to an unpatched old revision.

## Plumbing validation

The model-free Python tests use fake daemon protocols to prove request/record
pairing, old-binary fallback, identical-binary refusal, privacy, provenance,
percentiles and the quiet-host gate. `cli_pipe` compares clean text, manifest,
entries and safety disposition byte for byte with capture disabled/enabled from
the same initial session snapshot, and verifies exact restore for each fixture.
No new Rust test binary is introduced.
