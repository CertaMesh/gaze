//! Fail-closed verification of a pinned model bundle directory.
//!
//! A bundle is a directory holding a checksum file plus the artifacts it lists. The runtime pins
//! the SHA-256 of the checksum file itself, then checks every required artifact against the
//! digest the checksum file records. Any missing file, extra or malformed checksum entry, digest
//! mismatch, symlink, foreign owner or loose permission is an error; nothing loads until the whole
//! tree verifies.

use std::path::Path;

use gaze_types::SafetyNetError;
use sha2::{Digest, Sha256};

/// What a pinned bundle must contain.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BundleSpec {
    /// Short backend name used in sanitized error reasons (`"nym"`).
    pub(crate) backend: &'static str,
    /// Name of the checksum file inside the bundle.
    pub(crate) checksum_file: &'static str,
    /// Every file the bundle must contain, including the checksum file.
    pub(crate) required: &'static [&'static str],
}

/// Verifies `dir` against `spec` and the pinned checksum-file digest.
pub(crate) fn verify_bundle(
    dir: &Path,
    spec: BundleSpec,
    expected_sha256: &str,
) -> Result<(), SafetyNetError> {
    if !dir.exists() {
        return Err(SafetyNetError::WeightsMissing {
            path: sanitize_path(dir),
        });
    }
    verify_sensitive_tree(dir, spec.backend)?;
    for required in spec.required {
        let artifact = dir.join(required);
        if !artifact.exists() {
            return Err(SafetyNetError::WeightsMissing {
                path: sanitize_path(&artifact),
            });
        }
    }

    let sums_path = dir.join(spec.checksum_file);
    let sums = std::fs::read(&sums_path).map_err(|_| SafetyNetError::WeightsMissing {
        path: sanitize_path(&sums_path),
    })?;
    let actual = hex_sha256(&sums);
    if actual != expected_sha256 {
        return Err(SafetyNetError::ModelIntegrityMismatch {
            expected: expected_sha256.to_string(),
            actual,
        });
    }

    let entries = parse_sha256sums(&sums, spec)?;
    for required in spec.required {
        if *required == spec.checksum_file {
            continue;
        }
        let Some(expected_artifact) = entries
            .iter()
            .find_map(|(sha256, name)| (name == required).then_some(sha256.as_str()))
        else {
            return Err(SafetyNetError::ModelIntegrityMismatch {
                expected: format!("<checksum-entry:{required}>"),
                actual: "<missing>".to_string(),
            });
        };
        let artifact = dir.join(required);
        let bytes = std::fs::read(&artifact).map_err(|_| SafetyNetError::WeightsMissing {
            path: sanitize_path(&artifact),
        })?;
        let actual_artifact = hex_sha256(&bytes);
        if actual_artifact != expected_artifact {
            return Err(SafetyNetError::ModelIntegrityMismatch {
                expected: expected_artifact.to_string(),
                actual: actual_artifact,
            });
        }
    }
    Ok(())
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn parse_sha256sums(
    bytes: &[u8],
    spec: BundleSpec,
) -> Result<Vec<(String, String)>, SafetyNetError> {
    let malformed = || SafetyNetError::ModelIntegrityMismatch {
        expected: "canonical SHA256SUMS entries".to_string(),
        actual: "<invalid>".to_string(),
    };
    let text = std::str::from_utf8(bytes).map_err(|_| malformed())?;
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let sha256 = fields.next().unwrap_or_default();
        let name = fields.next().unwrap_or_default();
        if fields.next().is_some()
            || sha256.len() != 64
            || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !spec.required.contains(&name)
            || name == spec.checksum_file
            || entries.iter().any(|(_, seen)| seen == name)
        {
            return Err(malformed());
        }
        entries.push((sha256.to_ascii_lowercase(), name.to_string()));
    }
    Ok(entries)
}

fn verify_sensitive_tree(path: &Path, backend: &str) -> Result<(), SafetyNetError> {
    let metadata = verify_one_sensitive_path(path, backend)?;
    if metadata.is_dir() {
        let unreadable = || SafetyNetError::ModelUnavailable {
            reason: format!("failed to read {backend} sensitive directory"),
        };
        for entry in std::fs::read_dir(path).map_err(|_| unreadable())? {
            verify_sensitive_tree(&entry.map_err(|_| unreadable())?.path(), backend)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn verify_one_sensitive_path(
    path: &Path,
    backend: &str,
) -> Result<std::fs::Metadata, SafetyNetError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let unavailable = |reason: &str| SafetyNetError::ModelUnavailable {
        reason: format!("{backend} {reason}"),
    };
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| unavailable("sensitive path is unreadable"))?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(unavailable("sensitive path must not be a symlink"));
    }
    // Ownership is judged against who runs the process, not the current directory's owner.
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(unavailable("sensitive path owner mismatch"));
    }
    let mode = metadata.permissions().mode() & 0o777;
    if file_type.is_dir() {
        if mode != 0o700 {
            return Err(unavailable("sensitive directory must be mode 0700"));
        }
    } else if file_type.is_file() {
        if mode & 0o022 != 0 {
            return Err(unavailable(
                "sensitive file must not be group/world writable",
            ));
        }
    } else {
        return Err(unavailable(
            "sensitive path must be a regular file or directory",
        ));
    }
    Ok(metadata)
}

#[cfg(windows)]
fn verify_one_sensitive_path(
    path: &Path,
    backend: &str,
) -> Result<std::fs::Metadata, SafetyNetError> {
    let unavailable = |reason: &str| SafetyNetError::ModelUnavailable {
        reason: format!("{backend} {reason}"),
    };
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| unavailable("sensitive path is unreadable"))?;
    if metadata.file_type().is_symlink() {
        return Err(unavailable("sensitive path must not be a symlink"));
    }
    if !(metadata.file_type().is_file() || metadata.file_type().is_dir()) {
        return Err(unavailable(
            "sensitive path must be a regular file or directory",
        ));
    }
    if metadata.permissions().readonly() {
        return Ok(metadata);
    }
    Err(unavailable("sensitive Windows ACL could not be verified"))
}

fn sanitize_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("<missing:{name}>"))
        .unwrap_or_else(|| "<missing:model>".to_string())
}

#[cfg(all(test, unix))]
pub(crate) mod test_bundle {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::hex_sha256;

    /// Writes `files` plus a canonical checksum file into a private `dir` and returns the
    /// checksum file's digest.
    pub(crate) fn write(dir: &Path, checksum_file: &str, files: &[(&str, &[u8])]) -> String {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut sums = String::new();
        for (name, body) in files {
            write_private(&dir.join(name), body);
            sums.push_str(&format!("{}  {name}\n", hex_sha256(body)));
        }
        write_private(&dir.join(checksum_file), sums.as_bytes());
        hex_sha256(sums.as_bytes())
    }

    pub(crate) fn write_private(path: &Path, body: &[u8]) {
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::test_bundle::{write, write_private};
    use super::*;

    const SPEC: BundleSpec = BundleSpec {
        backend: "test",
        checksum_file: "SHA256SUMS",
        required: &["SHA256SUMS", "a.bin", "b.json"],
    };

    fn bundle() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let sha = write(
            dir.path(),
            "SHA256SUMS",
            &[("a.bin", b"a"), ("b.json", b"b")],
        );
        (dir, sha)
    }

    #[test]
    fn a_pinned_bundle_verifies() {
        let (dir, sha) = bundle();
        verify_bundle(dir.path(), SPEC, &sha).unwrap();
    }

    #[test]
    fn checksum_file_digest_mismatch_fails_closed() {
        let (dir, _) = bundle();
        let error = verify_bundle(dir.path(), SPEC, &"0".repeat(64)).unwrap_err();
        assert!(matches!(
            error,
            SafetyNetError::ModelIntegrityMismatch { .. }
        ));
    }

    #[test]
    fn tampered_artifact_fails_closed() {
        let (dir, sha) = bundle();
        write_private(&dir.path().join("a.bin"), b"tampered");
        let error = verify_bundle(dir.path(), SPEC, &sha).unwrap_err();
        assert!(matches!(
            error,
            SafetyNetError::ModelIntegrityMismatch { .. }
        ));
    }

    #[test]
    fn missing_artifact_or_entry_fails_closed() {
        let (dir, sha) = bundle();
        std::fs::remove_file(dir.path().join("b.json")).unwrap();
        assert!(matches!(
            verify_bundle(dir.path(), SPEC, &sha),
            Err(SafetyNetError::WeightsMissing { .. })
        ));

        let dir = tempfile::tempdir().unwrap();
        let sha = write(dir.path(), "SHA256SUMS", &[("a.bin", b"a")]);
        write_private(&dir.path().join("b.json"), b"b");
        assert!(matches!(
            verify_bundle(dir.path(), SPEC, &sha),
            Err(SafetyNetError::ModelIntegrityMismatch { .. })
        ));
    }

    #[test]
    fn loose_modes_and_symlinks_fail_closed() {
        let (dir, sha) = bundle();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            verify_bundle(dir.path(), SPEC, &sha),
            Err(SafetyNetError::ModelUnavailable { .. })
        ));

        let (dir, sha) = bundle();
        std::fs::remove_file(dir.path().join("a.bin")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("b.json"), dir.path().join("a.bin")).unwrap();
        assert!(matches!(
            verify_bundle(dir.path(), SPEC, &sha),
            Err(SafetyNetError::ModelUnavailable { .. })
        ));
    }
}
