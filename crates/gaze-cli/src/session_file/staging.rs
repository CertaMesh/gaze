//! Protect the staging directory before a session inode exists, then publish it.

use std::ffi::CString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::acl;

pub(super) fn write(target: &Path, overwrite: bool, body: &[u8]) -> io::Result<()> {
    let staging = Staging::new(target)?;
    let result = (|| {
        let mut file = staging.create_file()?;
        file.write_all(body)?;
        file.sync_all()?;
        staging.publish(target, overwrite)
    })();
    let cleanup = staging.cleanup();
    result.and(cleanup)
}

struct Staging {
    path: PathBuf,
    directory: File,
}

impl Staging {
    fn new(target: &Path) -> io::Result<Self> {
        let path = temp_path(target);
        DirBuilder::new().mode(0o700).create(&path)?;
        let directory = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(&path)
        {
            Ok(directory) => directory,
            Err(err) => {
                let _ = fs::remove_dir(&path);
                return Err(err);
            }
        };
        let staging = Self { path, directory };
        // The directory can inherit grants, but contains no session inode yet.
        staging.verify_directory()?;
        acl::clear_and_verify(&staging.directory)?;
        verify_owner_mode(&staging.directory, true, 0o700)?;
        staging.verify_directory()?;
        Ok(staging)
    }

    fn verify_directory(&self) -> io::Result<()> {
        let opened = self.directory.metadata()?;
        let named = fs::symlink_metadata(&self.path)?;
        // SAFETY: geteuid has no preconditions.
        if !opened.is_dir()
            || opened.uid() != unsafe { libc::geteuid() }
            || named.file_type().is_symlink()
            || opened.dev() != named.dev()
            || opened.ino() != named.ino()
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }

    fn create_file(&self) -> io::Result<File> {
        // Anchor all file operations to the verified directory, even if its
        // pathname is moved by someone with write access to the target's parent.
        // SAFETY: directory is live, the name is terminated, mode is supplied.
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                c"session".as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned a new descriptor whose ownership transfers here.
        let file = unsafe { File::from_raw_fd(fd) };
        acl::clear_and_verify(&file)?;
        verify_owner_mode(&file, false, 0o600)?;
        Ok(file)
    }

    fn publish(&self, target: &Path, overwrite: bool) -> io::Result<()> {
        let target = CString::new(target.as_os_str().as_bytes())?;
        // SAFETY: both names are terminated, directory is live, and AT_FDCWD
        // resolves the caller's target. The exclusive flag refuses any racer.
        let result = unsafe {
            if overwrite {
                libc::renameat(
                    self.directory.as_raw_fd(),
                    c"session".as_ptr(),
                    libc::AT_FDCWD,
                    target.as_ptr(),
                )
            } else {
                #[cfg(target_os = "macos")]
                {
                    libc::renameatx_np(
                        self.directory.as_raw_fd(),
                        c"session".as_ptr(),
                        libc::AT_FDCWD,
                        target.as_ptr(),
                        libc::RENAME_EXCL,
                    )
                }
                #[cfg(target_os = "linux")]
                {
                    libc::renameat2(
                        self.directory.as_raw_fd(),
                        c"session".as_ptr(),
                        libc::AT_FDCWD,
                        target.as_ptr(),
                        libc::RENAME_NOREPLACE,
                    )
                }
            }
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn cleanup(&self) -> io::Result<()> {
        // SAFETY: directory is live and the constant name is terminated. Never
        // traverse a possibly replaced staging pathname to remove session data.
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), c"session".as_ptr(), 0) } != 0 {
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::NotFound {
                return Err(err);
            }
        }
        self.verify_directory()?;
        fs::remove_dir(&self.path)
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        // Also covers directory ACL failure before new() returns the guard.
        let _ = self.cleanup();
    }
}

fn verify_owner_mode(file: &File, directory: bool, mode: u32) -> io::Result<()> {
    let metadata = file.metadata()?;
    // SAFETY: geteuid has no preconditions.
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != mode
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file() || metadata.nlink() != 1
        }
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}

fn temp_path(target: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    target.with_file_name(format!(".gaze-session.{}.{nanos}.tmp", std::process::id()))
}
