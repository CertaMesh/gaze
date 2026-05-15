use std::path::Path;

use gaze_types::SafetyNetError;
use sha2::{Digest, Sha256};

use super::KijiDistilbertPrecision;

/// Files Kiji must ship under `model_dir` for the pinned-artifact contract.
///
/// Missing `SHA256SUMS` is an Axis-1 reliability defect: the runtime must
/// fail closed rather than silently degrade. Mirrors the NER load contract
/// in `crates/gaze/testdata/ner/`.
pub const REQUIRED_KIJI_ARTIFACTS: &[&str] =
    &["SHA256SUMS", "labels.json", "model.onnx", "tokenizer.json"];

/// Files Kiji must ship for the opt-in int8 dynamic-quantized artifact.
pub const REQUIRED_KIJI_INT8_ARTIFACTS: &[&str] = &[
    "SHA256SUMS.int8",
    "labels.json",
    "model.int8.onnx",
    "tokenizer.json",
];

/// Hugging Face repository for the canonical Kiji DistilBERT ONNX bundle.
pub const KIJI_DISTILBERT_HF_REPO: &str = "onnx-community/distilbert-NER-ONNX";

/// Immutable upstream revision used by `scripts/fetch-kiji-safetynet-model.sh`.
pub const KIJI_DISTILBERT_HF_COMMIT: &str = "3a19fe9404a4469d91aa3d551558a97f68872f67";

/// SHA256 of the canonical `SHA256SUMS` file for the bundle.
///
/// The runtime first verifies this digest, then verifies every artifact listed
/// in the checksum file. That keeps the release-published checksum contract and
/// local bytes tied together at load time.
pub const KIJI_DISTILBERT_BUNDLE_SHA256: &str =
    "c129e135d86698e67c4836456212666f94a56ceaf995acd60532f557b3120d2f";

/// SHA256 of the canonical `SHA256SUMS.int8` file for the quantized bundle.
pub const KIJI_DISTILBERT_INT8_BUNDLE_SHA256: &str =
    "6e7f238f38c5ee7977052ec391f6a8c68bbef038091f2ecff4747cc2268210cb";

/// Canonical checksum file content. The model hash is the Hugging Face LFS
/// SHA256 for `onnx/model.onnx` at `KIJI_DISTILBERT_HF_COMMIT`.
pub const KIJI_DISTILBERT_SHA256SUMS: &str = concat!(
    "d3753ce580a9d43b113d779c712494bd61341285317beec49cc1e848b86f9a97  labels.json\n",
    "b5f77096d0d9f425d34a2e263f8a2dfb845cdc757dc00c7a1e69e9cbb93115d5  model.onnx\n",
    "cb26b43c98e8266ae3e99c2a583cf8315d73b33a17e6b20b4df7ff1f22392d34  tokenizer.json\n",
);

/// Canonical int8 checksum file content generated from the pinned fp32 ONNX.
pub const KIJI_DISTILBERT_INT8_SHA256SUMS: &str = concat!(
    "d3753ce580a9d43b113d779c712494bd61341285317beec49cc1e848b86f9a97  labels.json\n",
    "bd909cc924b7e10a8d2cdffb0e3bb6036f12d6ae6740cc45420fe82682f5a867  model.int8.onnx\n",
    "cb26b43c98e8266ae3e99c2a583cf8315d73b33a17e6b20b4df7ff1f22392d34  tokenizer.json\n",
);

/// Pinned-artifact contract: every entry in `REQUIRED_KIJI_ARTIFACTS` must exist
/// under `model_dir`. Missing `SHA256SUMS` is a fail-closed condition (Axis 1).
pub(crate) fn verify_model_dir(
    model_dir: Option<&Path>,
    expected_sha256: &str,
) -> Result<(), SafetyNetError> {
    verify_model_dir_for_precision(model_dir, KijiDistilbertPrecision::Fp32, expected_sha256)
}

pub(crate) fn verify_model_dir_for_precision(
    model_dir: Option<&Path>,
    precision: KijiDistilbertPrecision,
    expected_sha256: &str,
) -> Result<(), SafetyNetError> {
    let Some(dir) = model_dir else {
        return Ok(());
    };

    if !dir.exists() {
        return Err(SafetyNetError::WeightsMissing {
            path: sanitize_path(dir),
        });
    }
    verify_sensitive_tree(dir)?;

    for required in required_artifacts(precision) {
        let artifact = dir.join(required);
        if !artifact.exists() {
            return Err(SafetyNetError::WeightsMissing {
                path: sanitize_path(&artifact),
            });
        }
    }
    verify_bundle_integrity_for_precision(dir, precision, expected_sha256)?;
    Ok(())
}

pub(crate) fn verify_bundle_integrity_for_precision(
    dir: &Path,
    precision: KijiDistilbertPrecision,
    expected_sha256: &str,
) -> Result<(), SafetyNetError> {
    let sha256sums_path = dir.join(checksum_filename(precision));
    let sha256sums =
        std::fs::read(&sha256sums_path).map_err(|_| SafetyNetError::WeightsMissing {
            path: sanitize_path(&sha256sums_path),
        })?;
    let actual_sha256 = hex_sha256(&sha256sums);
    if actual_sha256 != expected_sha256 {
        return Err(SafetyNetError::ModelIntegrityMismatch {
            expected: expected_sha256.to_string(),
            actual: actual_sha256,
        });
    }

    let checksums = parse_sha256sums(&sha256sums, precision)?;
    for required in required_artifacts(precision) {
        if *required == checksum_filename(precision) {
            continue;
        }
        let Some(expected_artifact_sha256) = checksums
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
        let actual_artifact_sha256 = hex_sha256(&bytes);
        if actual_artifact_sha256 != expected_artifact_sha256 {
            return Err(SafetyNetError::ModelIntegrityMismatch {
                expected: expected_artifact_sha256.to_string(),
                actual: actual_artifact_sha256,
            });
        }
    }

    Ok(())
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

fn parse_sha256sums(
    bytes: &[u8],
    precision: KijiDistilbertPrecision,
) -> Result<Vec<(String, String)>, SafetyNetError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SafetyNetError::ModelIntegrityMismatch {
        expected: "valid UTF-8 SHA256SUMS".to_string(),
        actual: "<invalid-utf8>".to_string(),
    })?;
    let mut entries = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let sha256 = fields.next().unwrap_or_default();
        let name = fields.next().unwrap_or_default();
        if fields.next().is_some()
            || sha256.len() != 64
            || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !required_artifacts(precision).contains(&name)
            || name == checksum_filename(precision)
            || name.contains('/')
            || name.contains('\\')
        {
            return Err(SafetyNetError::ModelIntegrityMismatch {
                expected: "canonical SHA256SUMS entries".to_string(),
                actual: "<invalid>".to_string(),
            });
        }
        entries.push((sha256.to_ascii_lowercase(), name.to_string()));
    }
    Ok(entries)
}

pub(crate) fn required_artifacts(precision: KijiDistilbertPrecision) -> &'static [&'static str] {
    match precision {
        KijiDistilbertPrecision::Fp32 => REQUIRED_KIJI_ARTIFACTS,
        KijiDistilbertPrecision::Int8 => REQUIRED_KIJI_INT8_ARTIFACTS,
    }
}

pub(crate) fn checksum_filename(precision: KijiDistilbertPrecision) -> &'static str {
    match precision {
        KijiDistilbertPrecision::Fp32 => "SHA256SUMS",
        KijiDistilbertPrecision::Int8 => "SHA256SUMS.int8",
    }
}

pub(crate) fn model_filename(precision: KijiDistilbertPrecision) -> &'static str {
    match precision {
        KijiDistilbertPrecision::Fp32 => "model.onnx",
        KijiDistilbertPrecision::Int8 => "model.int8.onnx",
    }
}

fn verify_sensitive_tree(path: &Path) -> Result<(), SafetyNetError> {
    let metadata = verify_one_sensitive_path(path)?;
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|_| SafetyNetError::ModelUnavailable {
            reason: "failed to read kiji sensitive directory".to_string(),
        })? {
            let entry = entry.map_err(|_| SafetyNetError::ModelUnavailable {
                reason: "failed to read kiji sensitive directory entry".to_string(),
            })?;
            verify_sensitive_tree(&entry.path())?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn verify_one_sensitive_path(path: &Path) -> Result<std::fs::Metadata, SafetyNetError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| SafetyNetError::ModelUnavailable {
            reason: "failed to inspect kiji sensitive path".to_string(),
        })?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "kiji sensitive path must not be a symlink".to_string(),
        });
    }

    let uid = current_uid();
    if metadata.uid() != uid {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "kiji sensitive path owner mismatch".to_string(),
        });
    }

    let mode = metadata.permissions().mode() & 0o777;
    if file_type.is_dir() {
        if mode != 0o700 {
            return Err(SafetyNetError::ModelUnavailable {
                reason: "kiji sensitive directory must be mode 0700".to_string(),
            });
        }
    } else if file_type.is_file() {
        if mode & 0o022 != 0 {
            return Err(SafetyNetError::ModelUnavailable {
                reason: "kiji sensitive file must not be group/world writable".to_string(),
            });
        }
    } else {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "kiji sensitive path must be a regular file or directory".to_string(),
        });
    }

    Ok(metadata)
}

#[cfg(unix)]
fn current_uid() -> u32 {
    std::fs::metadata(".")
        .map(|metadata| {
            use std::os::unix::fs::MetadataExt;
            metadata.uid()
        })
        .unwrap_or(0)
}

#[cfg(windows)]
fn verify_one_sensitive_path(path: &Path) -> Result<std::fs::Metadata, SafetyNetError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| SafetyNetError::ModelUnavailable {
            reason: "failed to inspect kiji sensitive path".to_string(),
        })?;
    if metadata.file_type().is_symlink() {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "kiji sensitive path must not be a symlink".to_string(),
        });
    }
    if !(metadata.file_type().is_file() || metadata.file_type().is_dir()) {
        return Err(SafetyNetError::ModelUnavailable {
            reason: "kiji sensitive path must be a regular file or directory".to_string(),
        });
    }
    if metadata.permissions().readonly() {
        return Ok(metadata);
    }
    Err(SafetyNetError::ModelUnavailable {
        reason: "kiji sensitive Windows ACL could not be verified".to_string(),
    })
}

fn sanitize_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("<missing:{name}>"))
        .unwrap_or_else(|| "<missing:model>".to_string())
}
