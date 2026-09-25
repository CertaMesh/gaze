# OpenAI Privacy Filter adapter

The first shipped backend is the
[`OpenAiFilterSafetyNet`](../../../crates/gaze-recognizers/src/safety_net/openai_filter/mod.rs).
It calls the official `openai/privacy-filter` CLI as a subprocess.
It is one of the two [safety-net](safety-nets.md) backends.

## Command choice

Gaze binds to the **official** `openai/privacy-filter` repository. Adopters
must install `opf` from a pinned upstream Git revision or an official release
tarball. The official CLI was chosen over the `chiefautism/privacy-parser`
fork because it documents pipe input, exposes a stable JSON schema, and
publishes a reproducible Git history. The fork could be re-evaluated if a
later review confirms native byte spans and the absence of PII-bearing JSON
fields, but it is not the v0.6 default.

The adapter always invokes `opf --format json --output-mode typed`. Output
mode `typed` is the only accepted shape; other modes are not parsed.

## Whole-text input

Piped stdin is not one input for the stock CLI: `opf` reads it line by line,
skips blank lines, and prints one JSON result per line with offsets relative
to that line. After the configured arguments (and `--checkpoint`), the adapter
therefore always appends:

```text
--no-print-color-coded-text --text-file /dev/stdin
```

The text still travels over the stdin pipe and is never written to disk. The
colour flag stops the ANSI section `opf` otherwise prints after the JSON.
`opf` reads the file in Python text mode, so `\r\n` and a lone `\r` each become
one `\n` character in the offsets it returns; the adapter maps those offsets
back to UTF-8 byte offsets in the exact clean text it sent.

The adapter accepts a result only if it is exactly one JSON object whose echoed
`text` equals the text `opf` should have read. A second document, a missing
`text`, or any other `text` (a line, a trimmed or rewritten input) is
`InvalidOutput`, so offsets relative to some other text can never be applied.
Empty clean text returns no spans without starting `opf`.

A wrapper command configured instead of `opf` must accept these arguments and
echo the analysed text. Windows has no `/dev/stdin`: there only the colour flag
is appended, `opf` still splits lines, and the echo check refuses multi-line
text rather than mis-mapping it.

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
  only `start`, `end`, `label`, and `score`. The per-span `text` and
  `placeholder` fields are never deserialized, so their contents are skipped
  by the parser and never held in memory.
- The top-level `text` echo is held in a `PrivatePiiString`, compared with
  the text the adapter sent, and scrubbed on drop. `redacted_text` is never
  deserialized.

After this projection, no part of Gaze that consumes safety-net output sees
upstream raw bytes. The adversarial regression
`safety_net_correlates_raw_spans_with_manifest_without_source_text` covers
this invariant.

## Stderr discipline

By default `child.stderr` is `Stdio::null()`, so verbose backend logs cannot
race with the JSON adapter or appear in operator logs. Adopters who need
diagnostics can enable `SubprocessOpenAiFilterConfig::with_stderr_diagnostics(true)`,
which:

1. Captures a stderr prefix in a bounded buffer of at most 256 bytes and
   drains/discards the rest to EOF so a verbose child cannot block its pipe.
   Diagnostic overflow alone does not fail inference. An incomplete trailing
   token is discarded back to the last captured ASCII whitespace before
   redaction. Arbitrary Unicode bytes and control bytes are not evidence of
   a complete raw token; a partial email or phone token could evade the sanitizer.
2. Maps non-printable bytes to spaces.
3. Sanitizes whitespace-separated tokens with the same redactor used for
   error messages: any token containing `@` or seven or more ASCII digits
   is replaced with `<redacted>`. This catches the most common email and
   phone shapes that backends might log.
4. Truncates sanitized output to the 256-byte cap, including a
   `[truncated]` marker when capture or display was shortened.

Stdout still has
a hard byte cap: overflow, I/O errors, invalid model output, and timeouts remain
errors. Diagnostics stay disabled by default. The heuristic redactor does not
provide general PII-detection completeness.

The `verbose_stderr_is_stripped_and_capped` test locks both the cap and the
sanitization rule.

## Subprocess timeout and resource isolation

Both subprocess adapters provide cancellable pipe I/O on Unix and Windows.
Windows uses exclusively owned parent pipes with nonblocking writes and
availability-bounded reads; see [Windows pipe ownership](windows-subprocess-io.md).
Targets that are neither Unix nor Windows currently have no adapter and return
`ModelUnavailable` before spawn. This describes this implementation, not a claim
that those targets cannot support subprocesses. In-process backends are unaffected.

The subprocess runner enforces a single deadline that covers stdin write,
stdout read, stderr read, and child wait. Parent pipe ends are nonblocking,
with cancellation checked between I/O operations and at most 5 ms of idle
polling delay (plus OS scheduling). On timeout or worker failure the adapter:

1. Signals cancellation to all pipe workers.
2. Sends `SIGKILL` (`Child::kill`) and reaps the direct child via `wait` to
   prevent zombies.
3. Joins every worker, closing all parent pipe ends without waiting for EOF
   from descendants. Descendant processes themselves are not killed.
4. Returns the original failure; for a deadline, `SafetyNetError::Runtime`
   with the message
   `"opf subprocess timed out and was killed"`. The CLI maps this branch to
   exit-code `3` with `variant = "Timeout"`.

Success still requires completed stdin, stdout/stderr EOF, and successful
direct-child exit. A descendant keeping a pipe open produces a timeout, never
partial successful output. Cleanup is cooperative, not a hard real-time
guarantee against OS scheduling delays or an uninterruptible child wait.

Initialization failures are cached in a `OnceLock<Result<Arc<_>, Arc<_>>>`
so deterministic problems (missing checkpoint, malformed config) are not
retried on every safety-net check. This is the explicit fix for "retry storm
on every clean" and is locked by `empty_command_failure_is_cached`.

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
