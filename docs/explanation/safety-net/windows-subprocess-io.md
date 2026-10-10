# Windows subprocess pipe ownership

OPF uses the same deadline, bounded stdout, finite-memory stderr drain, and
joined worker lifecycle on Windows with diagnostics enabled or disabled.

## Adapter and ownership

Freshly spawned stdin/stdout/stderr handles each move into one private worker.
They are never cloned or shared with another reader. Configure before starting
workers; setup failure kills/reaps the direct child and drops all pipe owners.

| Pipe | Nonblocking operation |
|---|---|
| stdin | `SetNamedPipeHandleState(PIPE_NOWAIT)`; advance partial writes, retry zero-byte nonempty writes after cancellation check, preserve other errors |
| stdout/stderr | `PeekNamedPipe`; retry empty-connected pipes, treat broken pipe as EOF, read at most available bytes, preserve other errors |

Exclusive readers prevent consumption between peek and read. Read-only std
handles lack `FILE_WRITE_ATTRIBUTES`, so they cannot set wait mode. Stdin flush
only checks cancellation: `ChildStdin` has no userspace buffer and
`FlushFileBuffers` would wait for the peer.

Rust 1.96 creates overlapped parent `Stdio::piped()` handles. Synchronous
handles can block while peeking in multithreaded code; do not generalize this
adapter to arbitrary files, borrowed handles, or second consumers.

No custom pending request, callback, duplicate thread handle, or helper thread
is used. Cancellation is checked before every operation, including continuous
progress; a race permits at most one bounded operation before the next check.
All workers join and close handles before return. Unix `O_NONBLOCK` is unchanged.

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
