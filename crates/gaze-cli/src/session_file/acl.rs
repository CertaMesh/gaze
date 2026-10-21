//! Extended ACL removal must happen on the created descriptor before any write.

#[cfg(target_os = "macos")]
pub(super) fn clear_and_verify(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let empty = darwin::Acl::empty()?;
    let empty_size = empty.size()?;
    // SAFETY: file owns a live descriptor and empty owns an initialized ACL.
    if unsafe { darwin::acl_set_fd(file.as_raw_fd(), empty.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Darwin reports ENOENT for a descriptor without an extended ACL.
    if let Some(actual) = darwin::Acl::read(file)? {
        // The external representation has a fixed header plus one record per
        // entry. Comparing with a freshly initialized empty ACL avoids Darwin's
        // acl_get_entry ambiguity (EINVAL means both empty and invalid).
        if actual.size()? != empty_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "session file retains extended ACL entries",
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod darwin {
    use std::ffi::{c_int, c_void};
    use std::io;
    use std::os::fd::AsRawFd;
    use std::ptr::NonNull;

    // macOS sys/acl.h: opaque acl_t, with storage owned by acl_init/acl_get_fd.
    unsafe extern "C" {
        fn acl_init(count: c_int) -> *mut c_void;
        fn acl_free(acl: *mut c_void) -> c_int;
        fn acl_size(acl: *mut c_void) -> isize;
        fn acl_get_fd(fd: c_int) -> *mut c_void;
        pub(super) fn acl_set_fd(fd: c_int, acl: *mut c_void) -> c_int;
    }

    pub(super) struct Acl(NonNull<c_void>);

    impl Acl {
        pub(super) fn empty() -> io::Result<Self> {
            // SAFETY: zero is a valid initial entry count; ownership is captured.
            NonNull::new(unsafe { acl_init(0) })
                .map(Self)
                .ok_or_else(io::Error::last_os_error)
        }

        pub(super) fn read(file: &std::fs::File) -> io::Result<Option<Self>> {
            // SAFETY: file owns the descriptor for the duration of this call.
            match NonNull::new(unsafe { acl_get_fd(file.as_raw_fd()) }) {
                Some(acl) => Ok(Some(Self(acl))),
                None => {
                    let err = io::Error::last_os_error();
                    if err.raw_os_error() == Some(libc::ENOENT) {
                        Ok(None)
                    } else {
                        Err(err)
                    }
                }
            }
        }

        pub(super) fn as_ptr(&self) -> *mut c_void {
            self.0.as_ptr()
        }

        pub(super) fn size(&self) -> io::Result<isize> {
            // SAFETY: this wrapper always owns a live initialized ACL.
            let size = unsafe { acl_size(self.as_ptr()) };
            if size < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(size)
            }
        }
    }

    impl Drop for Acl {
        fn drop(&mut self) {
            // SAFETY: the allocation is owned here and is freed exactly once.
            unsafe { acl_free(self.as_ptr()) };
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn clear_and_verify(file: &std::fs::File) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    use std::os::fd::AsRawFd;

    let name = c"system.posix_acl_access";
    // SAFETY: file owns the descriptor and name is a NUL-terminated constant.
    if unsafe { libc::fremovexattr(file.as_raw_fd(), name.as_ptr()) } != 0 {
        let err = Error::last_os_error();
        if err.raw_os_error() != Some(libc::ENODATA) {
            return Err(err);
        }
    }
    // SAFETY: size zero requests only the attribute's size, with no buffer.
    let size = unsafe { libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
    if size >= 0 {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "session file retains an extended ACL",
        ));
    }
    let err = Error::last_os_error();
    if err.raw_os_error() == Some(libc::ENODATA) {
        Ok(())
    } else {
        Err(err)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(super) fn clear_and_verify(_file: &std::fs::File) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owner-only session creation is unsupported on this platform",
    ))
}
