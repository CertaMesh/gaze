use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gaze_recognizers::safety_net::kiji_distilbert::{
    verify_kiji_bundle, KIJI_DISTILBERT_HF_COMMIT, KIJI_DISTILBERT_HF_REPO,
    KIJI_DISTILBERT_SHA256SUMS,
};
pub use gaze_recognizers::safety_net::kiji_distilbert::{KijiDistilbertPrecision, SafetyNetError};

const DEFAULT_MODEL_DIR_NAME: &str = "kiji-distilbert";
const KIJI_LABELS_JSON: &str = r#"{
  "schema_version": 1,
  "source": "onnx-community/distilbert-NER-ONNX",
  "source_commit": "3a19fe9404a4469d91aa3d551558a97f68872f67",
  "labels": [
    {"id": "person", "upstream": ["B-PER", "I-PER"]},
    {"id": "location", "upstream": ["B-LOC", "I-LOC"]},
    {"id": "organization", "upstream": ["B-ORG", "I-ORG"]},
    {"id": "miscellaneous", "upstream": ["B-MISC", "I-MISC"]}
  ]
}
"#;

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub model_dir: Option<PathBuf>,
    pub precision: KijiDistilbertPrecision,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum InstallOutcome {
    AlreadyPresent { model_dir: PathBuf },
    Installed { model_dir: PathBuf },
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SetupError {
    #[error("I/O error at `{path}`: {message}")]
    Io { path: PathBuf, message: String },
    #[error("download failed for `{url}` to `{path}`: {message}")]
    Download {
        url: String,
        path: PathBuf,
        message: String,
    },
    #[error("model verification failed: {0}")]
    Verify(#[from] SafetyNetError),
    #[error("existing model dir `{path}` is non-empty but invalid: {reason}")]
    NonEmptyInvalidDir { path: PathBuf, reason: String },
    #[error("failed to resolve path: {message}")]
    PathResolve { message: String },
}

pub trait ArtifactFetcher {
    fn fetch_to(&self, url: &str, dest_tmp: &Path) -> Result<(), String>;
}

pub struct UreqFetcher;

impl ArtifactFetcher for UreqFetcher {
    fn fetch_to(&self, url: &str, dest_tmp: &Path) -> Result<(), String> {
        let response = ureq::get(url).call().map_err(|err| err.to_string())?;
        let mut reader = response
            .into_body()
            .into_with_config()
            .limit(u64::MAX)
            .reader();
        let mut file = fs::File::create(dest_tmp).map_err(|err| err.to_string())?;
        io::copy(&mut reader, &mut file).map_err(|err| err.to_string())?;
        file.flush().map_err(|err| err.to_string())
    }
}

#[derive(Clone, Copy)]
struct ArtifactFile {
    source_path: Option<&'static str>,
    file_name: &'static str,
    inline_contents: Option<&'static str>,
}

struct ArtifactManifest {
    hf_repo: &'static str,
    hf_commit: &'static str,
    checksum_file_name: &'static str,
    sha256sums: &'static str,
    files: &'static [ArtifactFile],
}

const KIJI_FP32_FILES: &[ArtifactFile] = &[
    ArtifactFile {
        source_path: Some("onnx/model.onnx"),
        file_name: "model.onnx",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: Some("tokenizer.json"),
        file_name: "tokenizer.json",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: None,
        file_name: "labels.json",
        inline_contents: Some(KIJI_LABELS_JSON),
    },
];

fn kiji_manifest(precision: KijiDistilbertPrecision) -> Option<ArtifactManifest> {
    match precision {
        KijiDistilbertPrecision::Fp32 => Some(ArtifactManifest {
            hf_repo: KIJI_DISTILBERT_HF_REPO,
            hf_commit: KIJI_DISTILBERT_HF_COMMIT,
            checksum_file_name: "SHA256SUMS",
            sha256sums: KIJI_DISTILBERT_SHA256SUMS,
            files: KIJI_FP32_FILES,
        }),
        KijiDistilbertPrecision::Int8 => None,
    }
}

pub fn default_kiji_model_dir() -> Result<PathBuf, SetupError> {
    if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(xdg_data_home)
            .join("gaze")
            .join("models")
            .join(DEFAULT_MODEL_DIR_NAME));
    }
    let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) else {
        return Err(SetupError::PathResolve {
            message: "cannot resolve model dir: neither XDG_DATA_HOME nor HOME is set".to_string(),
        });
    };
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("gaze")
        .join("models")
        .join(DEFAULT_MODEL_DIR_NAME))
}

pub fn install_kiji_bundle(opts: &InstallOptions) -> Result<InstallOutcome, SetupError> {
    install_kiji_bundle_with_fetcher(opts, &UreqFetcher)
}

pub fn install_kiji_bundle_with_fetcher(
    opts: &InstallOptions,
    fetcher: &dyn ArtifactFetcher,
) -> Result<InstallOutcome, SetupError> {
    let model_dir = match &opts.model_dir {
        Some(path) => absolute_path(path)?,
        None => default_kiji_model_dir()?,
    };

    if model_dir.exists() {
        match verify_kiji_bundle(&model_dir, opts.precision) {
            Ok(()) => {
                chmod_private_tree(&model_dir)?;
                return Ok(InstallOutcome::AlreadyPresent { model_dir });
            }
            Err(error) => {
                if is_mode_repairable_error(&error) && is_current_euid_owned_tree(&model_dir)? {
                    repair_current_euid_owned_tree(&model_dir)?;
                    match verify_kiji_bundle(&model_dir, opts.precision) {
                        Ok(()) => return Ok(InstallOutcome::AlreadyPresent { model_dir }),
                        Err(repaired_error) if !is_empty_dir(&model_dir)? => {
                            return Err(SetupError::NonEmptyInvalidDir {
                                path: model_dir,
                                reason: repaired_error.to_string(),
                            });
                        }
                        Err(_) => {}
                    }
                }
                if !is_empty_dir(&model_dir)? {
                    return Err(SetupError::NonEmptyInvalidDir {
                        path: model_dir,
                        reason: error.to_string(),
                    });
                }
            }
        }
    }

    let Some(manifest) = kiji_manifest(opts.precision) else {
        return Err(SetupError::Download {
            url: format!("kiji-distilbert:{}", opts.precision.as_str()),
            path: model_dir,
            message:
                "int8 setup requires a locally quantized model.int8.onnx and cannot be downloaded"
                    .to_string(),
        });
    };

    install_model_dir(&manifest, &model_dir, opts.precision, fetcher)?;
    Ok(InstallOutcome::Installed { model_dir })
}

fn install_model_dir(
    manifest: &ArtifactManifest,
    model_dir: &Path,
    precision: KijiDistilbertPrecision,
    fetcher: &dyn ArtifactFetcher,
) -> Result<(), SetupError> {
    let parent = model_dir.parent().ok_or_else(|| SetupError::PathResolve {
        message: format!(
            "model dir `{}` has no parent directory",
            model_dir.display()
        ),
    })?;
    fs::create_dir_all(parent).map_err(|err| io_error(parent, err))?;

    let tmp_dir = parent.join(format!(
        ".{}.download-{}",
        model_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(DEFAULT_MODEL_DIR_NAME),
        unique_suffix()
    ));
    fs::create_dir(&tmp_dir).map_err(|err| io_error(&tmp_dir, err))?;
    set_dir_private(&tmp_dir)?;

    let result = (|| {
        for file in manifest.files {
            write_artifact_file(manifest, file, &tmp_dir, fetcher)?;
        }

        let sums_path = tmp_dir.join(manifest.checksum_file_name);
        fs::write(&sums_path, manifest.sha256sums.as_bytes())
            .map_err(|err| io_error(&sums_path, err))?;
        set_file_private(&sums_path)?;

        verify_kiji_bundle(&tmp_dir, precision).map_err(SetupError::Verify)
    })();

    if let Err(error) = result {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(error);
    }

    if model_dir.exists() {
        if is_empty_dir(model_dir)? {
            fs::remove_dir(model_dir).map_err(|err| io_error(model_dir, err))?;
        } else {
            let _ = fs::remove_dir_all(&tmp_dir);
            return Err(SetupError::NonEmptyInvalidDir {
                path: model_dir.to_path_buf(),
                reason: "model dir became non-empty during setup".to_string(),
            });
        }
    }

    fs::rename(&tmp_dir, model_dir).map_err(|err| {
        let _ = fs::remove_dir_all(&tmp_dir);
        io_error(model_dir, err)
    })
}

fn write_artifact_file(
    manifest: &ArtifactManifest,
    file: &ArtifactFile,
    tmp_dir: &Path,
    fetcher: &dyn ArtifactFetcher,
) -> Result<(), SetupError> {
    let destination = tmp_dir.join(file.file_name);
    if let Some(contents) = file.inline_contents {
        fs::write(&destination, contents.as_bytes()).map_err(|err| io_error(&destination, err))?;
        set_file_private(&destination)?;
        return Ok(());
    }

    let source_path = file.source_path.ok_or_else(|| SetupError::Download {
        url: format!("kiji-distilbert:{}", file.file_name),
        path: destination.clone(),
        message: "artifact has no source path".to_string(),
    })?;
    let url = format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        manifest.hf_repo, manifest.hf_commit, source_path
    );
    download_url_to_file(fetcher, &url, &destination)
}

fn download_url_to_file(
    fetcher: &dyn ArtifactFetcher,
    url: &str,
    destination: &Path,
) -> Result<(), SetupError> {
    let tmp = destination.with_extension("download");
    let mut last_error = String::new();
    let mut io_failure = false;
    for _ in 0..3 {
        let _ = fs::remove_file(&tmp);
        match fetcher.fetch_to(url, &tmp) {
            Ok(()) => {
                set_file_private(&tmp)?;
                fs::rename(&tmp, destination).map_err(|err| io_error(destination, err))?;
                set_file_private(destination)?;
                return Ok(());
            }
            Err(error) => {
                io_failure |= is_io_fetch_error(&error);
                last_error = error;
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    if io_failure {
        Err(SetupError::Io {
            path: tmp,
            message: last_error,
        })
    } else {
        Err(SetupError::Download {
            url: url.to_string(),
            path: tmp,
            message: format!("failed after 3 attempts: {last_error}"),
        })
    }
}

fn is_empty_dir(path: &Path) -> Result<bool, SetupError> {
    if !path.is_dir() {
        return Ok(false);
    }
    let mut entries = fs::read_dir(path).map_err(|err| io_error(path, err))?;
    Ok(entries.next().is_none())
}

fn absolute_path(path: &Path) -> Result<PathBuf, SetupError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|err| SetupError::PathResolve {
                message: format!("cannot resolve current directory: {err}"),
            })
    }
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
}

fn is_mode_repairable_error(error: &SafetyNetError) -> bool {
    match error {
        SafetyNetError::ModelUnavailable { reason } => {
            reason == "kiji sensitive directory must be mode 0700"
                || reason == "kiji sensitive file must not be group/world writable"
        }
        _ => false,
    }
}

fn is_io_fetch_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("no space left")
        || error.contains("enospc")
        || error.contains("storagefull")
        || error.contains("cannot write")
        || error.contains("failed to write")
        || error.contains("write error")
}

fn io_error(path: &Path, error: io::Error) -> SetupError {
    SetupError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(unix)]
fn chmod_private_tree(path: &Path) -> Result<(), SetupError> {
    for entry in private_tree_entries(path)? {
        if entry.is_dir {
            set_dir_private(&entry.path)?;
        } else {
            set_file_private(&entry.path)?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn chmod_private_tree(_path: &Path) -> Result<(), SetupError> {
    Ok(())
}

#[cfg(unix)]
fn repair_current_euid_owned_tree(path: &Path) -> Result<(), SetupError> {
    if !is_current_euid_owned_tree(path)? {
        return Err(SetupError::NonEmptyInvalidDir {
            path: path.to_path_buf(),
            reason: "kiji sensitive path owner mismatch".to_string(),
        });
    }
    chmod_private_tree(path)
}

#[cfg(not(unix))]
fn repair_current_euid_owned_tree(path: &Path) -> Result<(), SetupError> {
    Err(SetupError::NonEmptyInvalidDir {
        path: path.to_path_buf(),
        reason: "permission repair is unsupported on this platform".to_string(),
    })
}

#[cfg(unix)]
fn is_current_euid_owned_tree(path: &Path) -> Result<bool, SetupError> {
    let uid = unsafe { libc::geteuid() };
    Ok(private_tree_entries(path)?
        .iter()
        .all(|entry| entry.uid == uid))
}

#[cfg(not(unix))]
fn is_current_euid_owned_tree(_path: &Path) -> Result<bool, SetupError> {
    Ok(false)
}

#[cfg(unix)]
#[derive(Debug)]
struct PrivateTreeEntry {
    path: PathBuf,
    is_dir: bool,
    uid: u32,
}

#[cfg(unix)]
fn private_tree_entries(path: &Path) -> Result<Vec<PrivateTreeEntry>, SetupError> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path).map_err(|err| io_error(path, err))?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(SetupError::NonEmptyInvalidDir {
            path: path.to_path_buf(),
            reason: "kiji sensitive path must not be a symlink".to_string(),
        });
    }
    if !(file_type.is_dir() || file_type.is_file()) {
        return Err(SetupError::NonEmptyInvalidDir {
            path: path.to_path_buf(),
            reason: "kiji sensitive path must be a regular file or directory".to_string(),
        });
    }

    let mut entries = vec![PrivateTreeEntry {
        path: path.to_path_buf(),
        is_dir: file_type.is_dir(),
        uid: metadata.uid(),
    }];

    if file_type.is_dir() {
        for entry in fs::read_dir(path).map_err(|err| io_error(path, err))? {
            let entry = entry.map_err(|err| io_error(path, err))?;
            entries.extend(private_tree_entries(&entry.path())?);
        }
    }

    Ok(entries)
}

#[cfg(unix)]
fn set_dir_private(path: &Path) -> Result<(), SetupError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|err| io_error(path, err))
}

#[cfg(not(unix))]
fn set_dir_private(_path: &Path) -> Result<(), SetupError> {
    Ok(())
}

#[cfg(unix)]
fn set_file_private(path: &Path) -> Result<(), SetupError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|err| io_error(path, err))
}

#[cfg(not(unix))]
fn set_file_private(_path: &Path) -> Result<(), SetupError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    struct SyntheticFetcher {
        calls: Mutex<Vec<(String, PathBuf)>>,
    }

    impl SyntheticFetcher {
        fn calls(&self) -> Vec<(String, PathBuf)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ArtifactFetcher for SyntheticFetcher {
        fn fetch_to(&self, url: &str, dest_tmp: &Path) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push((url.to_string(), dest_tmp.to_path_buf()));
            std::fs::write(dest_tmp, b"synthetic model bytes").map_err(|err| err.to_string())
        }
    }

    struct FailingFetcher;

    impl ArtifactFetcher for FailingFetcher {
        fn fetch_to(&self, _url: &str, _dest_tmp: &Path) -> Result<(), String> {
            Err("No space left on device (os error 28)".to_string())
        }
    }

    #[test]
    fn fake_fetcher_streams_temp_and_cleans_on_verify_failure() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("kiji-distilbert");
        let fetcher = SyntheticFetcher {
            calls: Mutex::new(Vec::new()),
        };

        let err = install_kiji_bundle_with_fetcher(
            &InstallOptions {
                model_dir: Some(model_dir.clone()),
                precision: KijiDistilbertPrecision::Fp32,
            },
            &fetcher,
        )
        .unwrap_err();

        assert!(matches!(err, SetupError::Verify(_)));
        assert!(!model_dir.exists());
        let calls = fetcher.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls
            .iter()
            .all(|(_, path)| path.extension().is_some_and(|ext| ext == "download")));
    }

    #[test]
    fn fetch_to_write_failure_maps_to_io() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("kiji-distilbert");

        let err = install_kiji_bundle_with_fetcher(
            &InstallOptions {
                model_dir: Some(model_dir.clone()),
                precision: KijiDistilbertPrecision::Fp32,
            },
            &FailingFetcher,
        )
        .unwrap_err();

        assert!(matches!(err, SetupError::Io { .. }));
        assert!(!model_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn loose_empty_dir_is_repaired_and_still_attempts_install() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("kiji-distilbert");
        std::fs::create_dir(&model_dir).unwrap();
        std::fs::set_permissions(&model_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let fetcher = SyntheticFetcher {
            calls: Mutex::new(Vec::new()),
        };

        let err = install_kiji_bundle_with_fetcher(
            &InstallOptions {
                model_dir: Some(model_dir.clone()),
                precision: KijiDistilbertPrecision::Fp32,
            },
            &fetcher,
        )
        .unwrap_err();

        assert!(matches!(err, SetupError::Verify(_)));
        assert_eq!(fetcher.calls().len(), 2);
        assert!(model_dir.exists());
        assert_eq!(
            std::fs::symlink_metadata(&model_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    #[test]
    fn repair_permissions_tightens_current_user_tree() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("kiji-distilbert");
        std::fs::create_dir(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.onnx"), b"synthetic").unwrap();
        std::fs::set_permissions(&model_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            model_dir.join("model.onnx"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();

        repair_current_euid_owned_tree(&model_dir).unwrap();

        let uid = unsafe { libc::geteuid() };
        let dir_meta = std::fs::symlink_metadata(&model_dir).unwrap();
        let file_meta = std::fs::symlink_metadata(model_dir.join("model.onnx")).unwrap();
        assert_eq!(dir_meta.uid(), uid);
        assert_eq!(file_meta.uid(), uid);
        assert_eq!(dir_meta.permissions().mode() & 0o777, 0o700);
        assert_eq!(file_meta.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    #[ignore = "hits Hugging Face; run manually when validating the real network fetch path"]
    fn downloads_and_verifies_pinned_kiji_bundle() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("kiji-distilbert");

        let outcome = install_kiji_bundle(&InstallOptions {
            model_dir: Some(model_dir.clone()),
            precision: KijiDistilbertPrecision::Fp32,
        })
        .unwrap();

        assert!(matches!(outcome, InstallOutcome::Installed { .. }));
        gaze_recognizers::safety_net::kiji_distilbert::verify_kiji_bundle(
            &model_dir,
            KijiDistilbertPrecision::Fp32,
        )
        .unwrap();
    }

    #[test]
    #[ignore = "requires a real pinned bundle that is safe to chmod"]
    fn loose_mode_current_user_dir_is_repaired_then_accepted() {
        let model_dir = PathBuf::from(
            std::env::var_os("GAZE_KIJI_LOOSE_MODEL_DIR")
                .expect("set GAZE_KIJI_LOOSE_MODEL_DIR to a loose current-euid-owned bundle"),
        );

        let outcome = install_kiji_bundle(&InstallOptions {
            model_dir: Some(model_dir),
            precision: KijiDistilbertPrecision::Fp32,
        })
        .unwrap();

        assert!(matches!(outcome, InstallOutcome::AlreadyPresent { .. }));
    }

    #[test]
    #[ignore = "requires a real pinned bundle owned by another uid"]
    fn foreign_owned_dir_fails_closed() {
        let model_dir = PathBuf::from(
            std::env::var_os("GAZE_KIJI_FOREIGN_MODEL_DIR")
                .expect("set GAZE_KIJI_FOREIGN_MODEL_DIR to a foreign-owned bundle"),
        );

        let err = install_kiji_bundle(&InstallOptions {
            model_dir: Some(model_dir),
            precision: KijiDistilbertPrecision::Fp32,
        })
        .unwrap_err();

        assert!(matches!(
            err,
            SetupError::NonEmptyInvalidDir { .. } | SetupError::Verify(_)
        ));
    }
}
