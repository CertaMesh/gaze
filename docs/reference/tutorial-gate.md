# Tutorial execution gate

The `tutorial execution` CI job runs on every pull request, including docs-only
changes, and pushes to `main`. It is independent of the required `test` job.
It builds `gaze` from the checked-out source with `proxy` and `mcp` features.
The Cargo cache retains dependency builds between runs. The uploaded
`tutorial-execution` artifact contains per-page counts and timings plus the
elapsed time through verification, including dependency installation and the
CLI build. The Actions job's `started_at` and `completed_at` timestamps give the
full wall clock, including artifact upload and cache saving.

Run it locally from the repository root:

```sh
cargo build --locked -p gaze-cli --no-default-features --features proxy,mcp
python3 scripts/ci/tutorial_gate.py --gaze target/debug/gaze \
  --output target/tutorial-gate/receipts.json
TUTORIAL_GATE_BINARY=target/debug/gaze python3 -m unittest discover \
  -s scripts/ci -p test_tutorial_gate.py -v
```

Use Python 3.10 or newer. CI pins Python 3.13.9, Ubuntu 24.04's
`tesseract-ocr=5.3.4-1build5` and `tesseract-ocr-eng=1:4.1.0-2`.
The proxy tutorial pins `openai==2.31.0` in its own virtual environment.
No provider key or model download is needed.

## Markdown contract

Every `docs/tutorials/*.md` page except the `README.md` index must execute at
least one `sh` fence. Adding a page includes it automatically. Commands run in
order in one Bash shell per page, with `set -euo pipefail`, a ten-minute timeout,
and a fresh empty working directory. Shell variables, directory changes, and
exit traps persist between blocks. The runner kills remaining child processes
after the page, including on timeout.

A page that starts in a source checkout declares this marker outside a fence:

```html
<!-- tutorial-gate: start=checkout -->
```

The runner copies tracked source files into a fresh directory, without `.git`
or existing build outputs. Working directories use the system temporary
directory, outside the repository's Cargo workspace. The Rust example's build
cache and saved receipts live under `target/tutorial-gate/`. Each page gets a private home and XDG
directories. Cargo keeps the invoking user's registry and toolchain caches.
The Rust tutorial's `cargo add gaze-pii gaze-assembly` command resolves to
`--path` dependencies on the checked-out `crates/gaze` and
`crates/gaze-assembly`; other Cargo commands execute unchanged.
This substitution leaves the published-crate installation path untested.
After publishing, maintainers
[replay the Rust tutorial with the published version](../how-to/maintainers/release-process.md#replay-the-published-rust-tutorial).

The first `text` fence following an `sh` fence is that command's expected stdout.
Explanatory prose may separate the fences. Another code fence ends the pairing.
Comparison covers the entire stdout, including whitespace and final newlines.
Stderr contains diagnostics and is reported when a command fails. Commands
without a paired `text` fence still run and must succeed. Prose-only output
expectations, including the byte count and MCP installer paths, are not checked.

Only these output placeholders are accepted:

| Placeholder | Regex |
| --- | --- |
| `{session_hex}` | `[0-9a-f]{8}` |
| `{session_a}` | `[0-9a-f]{8}` |
| `{session_b}` | `[0-9a-f]{8}` |
| `{original_address}` | `alice@example\.invalid` |

A placeholder keeps its first matched value across the page. Repeated
occurrences must match the same bytes. Every unknown `{...}` placeholder is
an error. There is no whitespace normalization, substring check, or wildcard
for arbitrary output.

## Account-bound steps

A shell block that needs an external account must have an immediately preceding
marker with a nonempty reason:

```html
<!-- tutorial-gate: manual reason="Requires a signed-in account; replay before release." -->
```

The job prints each skipped block with its filename, line, and reason. It still
executes the other blocks on that page. A marker is not permission to skip:
[`tutorial_manual_steps.json`](../../scripts/ci/tutorial_manual_steps.json)
pins each approved page, the SHA-256 of the exact shell block body, and its
reason. Unapproved markers, changed or missing approved blocks, duplicate
approved blocks, and manual blocks without an account command fail the gate.
Changing an approved command or its reason requires review of the allowlist
change. The Claude Code call is currently the only approved skip.

The parser rejects unmarked Claude calls, including `command claude`,
`env claude`, and simple environment assignments before the command. A blocking
`claude` executable also comes first on the page's PATH. This runtime guard
catches PATH-resolved shell indirection and records attempts even when a
command ignores its exit status. It never invokes the installed account CLI.
Absolute-path calls and deliberate replacement of PATH or the shim are outside
this guard. Tutorials are trusted code; the runner is not an OS sandbox.
Other unmarked foreground commands fail the gate on nonzero exit.
A page with only skipped blocks fails.
Replay all marked blocks as part of the
[release checklist](../how-to/maintainers/release-process.md#replay-account-bound-tutorial-steps).

The runner tests mutate real tutorials: changing an expected output line,
breaking a `gaze` command, removing the Claude Code manual marker, and marking
every clean/restore block manual must each fail the gate.
`TUTORIAL_GATE_BINARY` enables these real-binary probes; CI always supplies it.
Other probes cover wrappers, runtime account-command indirection, zero
executed blocks, unknown placeholders, invalid session prefixes, and output
drift between blocks.

## Background processes

Bash's `set -e` and the block completion record cover foreground commands.
They do not propagate a background process's failure. Tutorials must check
background-job readiness, verify observable results, and check `wait` exit
statuses themselves. The proxy tutorial verifies its request and restored
response, then accepts only normal exit or the expected SIGTERM status after
stopping both services. A negative probe starts a failing background job and
proves that its checked `wait` fails the page. An unchecked background failure
is outside the shell completion proof.
