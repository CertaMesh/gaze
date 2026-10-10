# OpenAI Privacy Filter adapter

[`OpenAiFilterSafetyNet`](../../../crates/gaze-recognizers/src/safety_net/openai_filter/mod.rs)
is the opt-in official `openai/privacy-filter` subprocess backend.

## Command choice

Install official `opf` from a pinned Git revision or official release tarball.
The adapter accepts only `opf --format json --output-mode typed`. The
`chiefautism/privacy-parser` fork is not selected: its byte spans and
PII-bearing output would need separate review.

## Whole-text input

Stock piped input is line-by-line, with blank lines skipped and line-relative
offsets. After configured arguments and `--checkpoint`, Gaze appends:

```text
--no-print-color-coded-text --text-file /dev/stdin
```

Input remains on the pipe, never disk. The colour flag suppresses ANSI output.
Python text mode turns CRLF and lone CR into LF; Gaze maps returned character
offsets back to exact clean-text UTF-8 bytes.

Output must be one JSON object whose `text` equals the expected analyzed text.
Missing/mismatched echo, trimmed input, or a second document is `InvalidOutput`.
Empty clean text returns no spans without spawn. Wrappers must accept the flags
and echo contract. Windows appends only the colour flag because `/dev/stdin`
is absent; multi-line echo mismatch refuses rather than applying wrong spans.

## Subprocess configuration

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

## PII-bearing upstream JSON fields are stripped at the boundary

Private adapter types never escape. `RawSpan` contains only start/end, label,
and score; per-span `text` / `placeholder` and top-level `redacted_text` are
skipped during deserialization. The top-level echo uses `PrivatePiiString`,
whose debug form is `<private-opf-field>` and whose buffer clears on drop.
The regression `safety_net_correlates_raw_spans_with_manifest_without_source_text`
checks correlation without source payloads.

## Stderr discipline

Stderr defaults to `Stdio::null()`. Explicit
`with_stderr_diagnostics(true)` captures at most 256 bytes and drains/discards
the rest so verbose output cannot block. Overflow alone does not fail inference.
Before sanitization, an incomplete trailing token is dropped back to the last
ASCII whitespace. Non-printable bytes become spaces; whitespace-delimited
tokens containing `@` or at least seven ASCII digits become `<redacted>`.
Display is capped at 256 bytes including `[truncated]` when shortened.

This heuristic is not complete PII detection; diagnostics stay default-off.
Stdout overflow, I/O errors, invalid output, and timeout remain failures.
`verbose_stderr_is_stripped_and_capped` pins the rules.

## Subprocess timeout and resource isolation

Unix and Windows have cancellable owned-pipe I/O;
[Windows ownership](windows-subprocess-io.md) covers its adapter. Other targets
return `ModelUnavailable` before spawn; in-process backends are unaffected.

One deadline covers stdin, stdout, stderr, and child wait. Cancellation is
checked between nonblocking operations with at most 5 ms idle polling plus OS
scheduling. On timeout/worker failure, cancel workers, kill/reap the direct
child, join every worker, and close parent pipe ends without waiting for
descendant EOF. Descendants are not killed.

A timeout returns `SafetyNetError::Runtime` with
`"opf subprocess timed out and was killed"`; CLI exit is `3`, variant `Timeout`.
Success requires completed stdin, stdout/stderr EOF, and successful direct-child
exit. Descendant-held pipes cause timeout, never partial success. Cooperative
cleanup is not a hard real-time guarantee.

`OnceLock<Result<Arc<_>, Arc<_>>>` caches initialization failures, avoiding
retries on every scan; `empty_command_failure_is_cached` pins this.

## Checkpoint and cache permission verification

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

## Class mapping

`map_openai_label` accepts the closed set `private_person`,
`private_address`, `private_email`, `private_phone`, `private_url`,
`private_date`, `account_number`, `secret`. Unknown labels return
`SafetyNetError::InvalidOutput`. The mapping into Gaze's `PiiClass` lives
in [`class_map.rs`](../../../crates/gaze-recognizers/src/safety_net/openai_filter/class_map.rs);
the `class-map-override-safety` xtask gate runs the
`all_official_labels_map_exactly_to_gaze_classes` test on every PR.
