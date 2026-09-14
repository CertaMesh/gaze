//! Poll exclusively owned std child pipes without leaving pending I/O to cancel.
//!
//! Rust creates overlapped parent handles for Stdio::piped(). Readers peek and
//! consume only available bytes; stdin uses byte-mode nonblocking writes. See
//! docs/explanation/safety-net/windows-subprocess-io.md for the platform contract.

use super::*;
use std::os::windows::io::AsRawHandle;
use std::process::{ChildStderr, ChildStdin, ChildStdout};
use windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE;
use windows_sys::Win32::System::Pipes::{PeekNamedPipe, SetNamedPipeHandleState, PIPE_NOWAIT};

// Only these owned, freshly spawned std pipes enter the adapter. In particular,
// no File, borrowed handle, clone, or second reader may consume peeked bytes.
pub(in crate::safety_net) trait ChildPipe: AsRawHandle {
    fn configure(&self) -> io::Result<()>;
}

impl ChildPipe for ChildStdin {
    fn configure(&self) -> io::Result<()> {
        let mode = PIPE_NOWAIT;
        // SAFETY: self owns the live handle; mode is valid for the byte pipe.
        // This happens before workers start and changes only the parent end.
        let result = unsafe {
            SetNamedPipeHandleState(
                self.as_raw_handle(),
                &mode,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl ChildPipe for ChildStdout {
    fn configure(&self) -> io::Result<()> {
        Ok(())
    }
}

impl ChildPipe for ChildStderr {
    fn configure(&self) -> io::Result<()> {
        Ok(())
    }
}

impl<T: Read + ChildPipe> Read for Pipe<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.retry(|inner| {
            if bytes.is_empty() {
                return Ok(0);
            }
            let mut available = 0;
            // SAFETY: the owner outlives this call. Only available is written;
            // all optional output buffers are null. No operation is pending on
            // this handle, and no other thread can consume its buffered bytes.
            let result = unsafe {
                PeekNamedPipe(
                    inner.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if result == 0 {
                let error = io::Error::last_os_error();
                return if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                    Ok(0)
                } else {
                    Err(error)
                };
            }
            if available == 0 {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let count = bytes.len().min(available as usize);
            // A writer can append or close, but cannot remove these bytes.
            inner.read(&mut bytes[..count])
        })
    }
}

impl Write for Pipe<ChildStdin> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.retry(|inner| match inner.write(bytes) {
            // PIPE_NOWAIT can successfully write zero bytes when full. This is
            // backpressure, not WriteZero. Partial writes retain write_all's
            // normal retry behavior, including a fresh cancellation check.
            Ok(0) if !bytes.is_empty() => Err(io::ErrorKind::WouldBlock.into()),
            result => result,
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        // ChildStdin is unbuffered. Never call FlushFileBuffers: that waits for
        // the peer to consume bytes and would reintroduce an unbounded wait.
        self.cancellation.check()
    }
}
