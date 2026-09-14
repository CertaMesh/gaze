//! Parent pipe ownership and cooperative cancellation for subprocess workers.

use std::io::{self, Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

#[derive(Clone, Default)]
pub(super) struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub(super) fn check_platform() -> io::Result<()> {
        if cfg!(any(unix, windows)) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no cancellable subprocess pipe adapter for this target",
            ))
        }
    }

    pub(super) fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn check(&self) -> io::Result<()> {
        if self.0.load(Ordering::Relaxed) {
            // Interrupted would make Write::write_all retry cancellation forever.
            Err(io::Error::new(io::ErrorKind::TimedOut, "pipe cancelled"))
        } else {
            Ok(())
        }
    }
}

pub(super) struct Pipe<T> {
    inner: T,
    cancellation: Cancellation,
}

impl<T> Pipe<T> {
    #[cfg(unix)]
    pub(super) fn new(inner: T, cancellation: &Cancellation) -> io::Result<Self>
    where
        T: std::os::fd::AsRawFd,
    {
        let fd = inner.as_raw_fd();
        // SAFETY: inner owns the live descriptor throughout both calls. Only the
        // parent's pipe end changes; no thread has received this handle yet.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            inner,
            cancellation: cancellation.clone(),
        })
    }

    #[cfg(windows)]
    pub(super) fn new(inner: T, cancellation: &Cancellation) -> io::Result<Self>
    where
        T: windows::ChildPipe,
    {
        inner.configure()?;
        Ok(Self {
            inner,
            cancellation: cancellation.clone(),
        })
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn new(_inner: T, _cancellation: &Cancellation) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no cancellable subprocess pipe adapter for this target",
        ))
    }

    fn retry<R>(&mut self, mut operation: impl FnMut(&mut T) -> io::Result<R>) -> io::Result<R> {
        loop {
            // Check even during continuous progress, not just when a pipe is idle.
            self.cancellation.check()?;
            match operation(&mut self.inner) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                result => return result,
            }
        }
    }
}

#[cfg(not(windows))]
impl<T: Read> Read for Pipe<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.retry(|inner| inner.read(bytes))
    }
}

#[cfg(not(windows))]
impl<T: Write> Write for Pipe<T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.retry(|inner| inner.write(bytes))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.retry(Write::flush)
    }
}

#[cfg(windows)]
mod windows;
