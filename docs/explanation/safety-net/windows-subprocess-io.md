# Windows subprocess pipe ownership

OPF, the only subprocess safety net, retains inference with diagnostics enabled
or disabled. It uses the same existing deadline, bounded stdout, finite-memory stderr drain, and joined
worker lifecycle as Unix. No parser, detector, or registry behavior changes.

## Adapter and ownership

Only freshly spawned `ChildStdin`, `ChildStdout`, and `ChildStderr` enter the
private Windows adapter. Their handles are moved into one worker each, never
cloned or exposed to another reader. All configuration completes before workers
start; a setup error kills/reaps the direct child and drops all pipe owners.

For stdin, `SetNamedPipeHandleState(PIPE_NOWAIT)` makes writes return without
waiting for the reader. Partial writes advance normally; a successful zero-byte
write of nonempty input means backpressure and retries after a cancellation
check. Other write errors remain errors. Flush checks cancellation only because
`ChildStdin` has no userspace buffer; `FlushFileBuffers` would wait for the peer.

For stdout/stderr, `PeekNamedPipe` queries available bytes. Empty but connected
means retry, not EOF. Broken pipe means EOF; other errors remain errors. Each read
consumes at most the available count. Exclusive ownership is essential: a writer
may append or close but cannot consume the bytes between peek and read. Windows
read-only std child handles do not have the `FILE_WRITE_ATTRIBUTES` access needed
to set their wait mode, so reads use availability instead.

Rust 1.96 creates overlapped parent handles for `Stdio::piped()`, which matters
because Microsoft warns that peeking a synchronous handle in a multithreaded application
can block. This adapter must not be generalized to arbitrary files, borrowed
handles, or second consumers. Native CI exercises the actual Rust-created pipes.

There is no custom pending overlapped request, cancellation callback, duplicated
thread handle, or cancellation helper thread. The existing cancellation flag is
checked before every operation, including continuous progress. Cancellation
between a check and an operation permits at most that bounded operation before
the next check. All workers join and close their handles before the caller
returns. Unix's `O_NONBLOCK` implementation is unchanged.

## Evidence and limits

`windows_subprocess` uses only synthetic Python fixtures, no model inference.
It checks both backends, diagnostics on/off, input backpressure, noisy stderr,
Unicode prefix clipping, stdout caps, invalid output, IO errors, and inherited
stdin/stdout/stderr. Descendants report EOF/broken pipe to prove closure, rather
than accepting a fast return with a detached reader. A watchdog bounds fixtures.
The focused Windows workflow runs this suite on a native hosted runner.

This contract is about owned parent IO and direct-child cleanup. It does not
promise descendant termination or hard real-time scheduling. Other targets that
are neither Unix nor Windows have no adapter here; their capabilities have not
been exhaustively assessed, and inference fails before spawn.

## Primary sources

- [Microsoft SetNamedPipeHandleState](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-setnamedpipehandlestate): anonymous pipes, access rights, immediate nonblocking operations.
- [Microsoft pipe modes](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-type-read-and-wait-modes): byte-pipe partial writes and empty-pipe behavior. `PIPE_NOWAIT` is used for explicit polling, not as a substitute for overlapped completion delivery.
- [Microsoft PeekNamedPipe](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-peeknamedpipe): availability without consumption and synchronous-handle caveat.
- [Microsoft cancellation considerations](https://learn.microsoft.com/en-us/windows/win32/fileio/canceling-pending-i-o-operations): cancellation races and retaining overlapped storage until completion. Avoided here by not issuing pending custom requests.
- [Rust 1.96 child pipes](https://github.com/rust-lang/rust/blob/1.96.0/library/std/src/sys/process/windows/child_pipe.rs): overlapped parent handles, synchronous child handles, byte-stream pipe construction.
- [Rust ChildStdout ownership](https://doc.rust-lang.org/std/process/struct.ChildStdout.html): dropping closes the owned handle.
