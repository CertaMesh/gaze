use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gaze_recognizers::safety_net::nym::{
    verify_nym_bundle, NYM_SMALL_CHECKSUM_FILE, NYM_SMALL_HF_COMMIT, NYM_SMALL_HF_REPO,
    NYM_SMALL_INT8_SHA256SUMS,
};
pub use gaze_recognizers::safety_net::SafetyNetError;
use gaze_recognizers::{
    verify_davlan_ner_bundle, DAVLAN_NER_HF_COMMIT, DAVLAN_NER_HF_REPO, DAVLAN_NER_LABELS_JSON,
    DAVLAN_NER_MODEL_DIR_NAME, DAVLAN_NER_SHA256SUMS,
};

const DEFAULT_NYM_MODEL_DIR_NAME: &str = "nym-small-int8";
const MODEL_DOWNLOAD_MAX_REDIRECTS: u32 = 5;

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
        let response = https_only_download_agent()
            .get(url)
            .call()
            .map_err(|err| err.to_string())?;
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

fn https_only_download_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(MODEL_DOWNLOAD_MAX_REDIRECTS)
        .build()
        .into()
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

const DAVLAN_NER_FILES: &[ArtifactFile] = &[
    ArtifactFile {
        source_path: Some("onnx/model_int8.onnx"),
        file_name: "model.onnx",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: Some("tokenizer.json"),
        file_name: "tokenizer.json",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: Some("tokenizer_config.json"),
        file_name: "tokenizer_config.json",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: Some("config.json"),
        file_name: "config.json",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: Some("special_tokens_map.json"),
        file_name: "special_tokens_map.json",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: Some("vocab.txt"),
        file_name: "vocab.txt",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: None,
        file_name: "labels.json",
        inline_contents: Some(DAVLAN_NER_LABELS_JSON),
    },
];

const DAVLAN_NER_MANIFEST: ArtifactManifest = ArtifactManifest {
    hf_repo: DAVLAN_NER_HF_REPO,
    hf_commit: DAVLAN_NER_HF_COMMIT,
    checksum_file_name: "SHA256SUMS",
    sha256sums: DAVLAN_NER_SHA256SUMS,
    files: DAVLAN_NER_FILES,
};

const NYM_SMALL_INT8_FILES: &[ArtifactFile] = &[
    ArtifactFile {
        source_path: Some("int8/config.json"),
        file_name: "config.json",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: Some("int8/model_int8.onnx"),
        file_name: "model_int8.onnx",
        inline_contents: None,
    },
    ArtifactFile {
        source_path: Some("int8/tokenizer.json"),
        file_name: "tokenizer.json",
        inline_contents: None,
    },
];

const NYM_SMALL_INT8_MANIFEST: ArtifactManifest = ArtifactManifest {
    hf_repo: NYM_SMALL_HF_REPO,
    hf_commit: NYM_SMALL_HF_COMMIT,
    checksum_file_name: NYM_SMALL_CHECKSUM_FILE,
    sha256sums: NYM_SMALL_INT8_SHA256SUMS,
    files: NYM_SMALL_INT8_FILES,
};

/// Default install directory of the primary NER bundle:
/// `$XDG_DATA_HOME/gaze/models/davlan-mbert-ner-hrl`, else `~/.local/share/gaze/models/davlan-mbert-ner-hrl`.
pub fn default_ner_model_dir() -> Result<PathBuf, SetupError> {
    default_model_dir(DAVLAN_NER_MODEL_DIR_NAME)
}

/// Default install directory of the Nym-small int8 bundle:
/// `$XDG_DATA_HOME/gaze/models/nym-small-int8`, else `~/.local/share/gaze/models/nym-small-int8`.
pub fn default_nym_model_dir() -> Result<PathBuf, SetupError> {
    default_model_dir(DEFAULT_NYM_MODEL_DIR_NAME)
}

fn default_model_dir(name: &str) -> Result<PathBuf, SetupError> {
    if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(xdg_data_home)
            .join("gaze")
            .join("models")
            .join(name));
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
        .join(name))
}

/// Downloads the pinned Davlan mBERT NER bundle into `model_dir` (default
/// [`default_ner_model_dir`]) and verifies it against `DAVLAN_NER_BUNDLE_SHA256`. An existing
/// verified bundle is kept; an existing invalid non-empty directory is an error.
pub fn install_ner_bundle(model_dir: Option<&Path>) -> Result<InstallOutcome, SetupError> {
    install_ner_bundle_with_fetcher(model_dir, &UreqFetcher)
}

pub fn install_ner_bundle_with_fetcher(
    model_dir: Option<&Path>,
    fetcher: &dyn ArtifactFetcher,
) -> Result<InstallOutcome, SetupError> {
    let model_dir = match model_dir {
        Some(path) => absolute_path(path)?,
        None => default_ner_model_dir()?,
    };
    if let Some(outcome) = reuse_existing_bundle(&model_dir, &verify_davlan_ner_bundle)? {
        return Ok(outcome);
    }
    install_model_dir(
        &DAVLAN_NER_MANIFEST,
        &model_dir,
        &verify_davlan_ner_bundle,
        fetcher,
    )?;
    Ok(InstallOutcome::Installed { model_dir })
}

/// Downloads the pinned Nym-small int8 bundle into `model_dir` (default
/// [`default_nym_model_dir`]) and verifies it against `NYM_SMALL_INT8_BUNDLE_SHA256`. An
/// existing verified bundle is kept; an existing invalid non-empty directory is an error.
pub fn install_nym_bundle(model_dir: Option<&Path>) -> Result<InstallOutcome, SetupError> {
    install_nym_bundle_with_fetcher(model_dir, &UreqFetcher)
}

pub fn install_nym_bundle_with_fetcher(
    model_dir: Option<&Path>,
    fetcher: &dyn ArtifactFetcher,
) -> Result<InstallOutcome, SetupError> {
    let model_dir = match model_dir {
        Some(path) => absolute_path(path)?,
        None => default_nym_model_dir()?,
    };
    if let Some(outcome) = reuse_existing_bundle(&model_dir, &verify_nym_bundle)? {
        return Ok(outcome);
    }
    install_model_dir(
        &NYM_SMALL_INT8_MANIFEST,
        &model_dir,
        &verify_nym_bundle,
        fetcher,
    )?;
    Ok(InstallOutcome::Installed { model_dir })
}

/// `Some(AlreadyPresent)` when `model_dir` already holds a verified bundle (after repairing loose
/// modes on a current-user tree), `None` when it is absent or empty and should be installed.
fn reuse_existing_bundle(
    model_dir: &Path,
    verify: &dyn Fn(&Path) -> Result<(), SafetyNetError>,
) -> Result<Option<InstallOutcome>, SetupError> {
    if !model_dir.exists() {
        return Ok(None);
    }
    let model_dir_buf = model_dir.to_path_buf();
    match verify(model_dir) {
        Ok(()) => {
            chmod_private_tree(model_dir)?;
            Ok(Some(InstallOutcome::AlreadyPresent {
                model_dir: model_dir_buf,
            }))
        }
        Err(error) => {
            if is_mode_repairable_error(&error) && is_current_euid_owned_tree(model_dir)? {
                repair_current_euid_owned_tree(model_dir)?;
                match verify(model_dir) {
                    Ok(()) => {
                        return Ok(Some(InstallOutcome::AlreadyPresent {
                            model_dir: model_dir_buf,
                        }))
                    }
                    Err(repaired_error) if !is_empty_dir(model_dir)? => {
                        return Err(SetupError::NonEmptyInvalidDir {
                            path: model_dir_buf,
                            reason: repaired_error.to_string(),
                        });
                    }
                    Err(_) => {}
                }
            }
            if !is_empty_dir(model_dir)? {
                return Err(SetupError::NonEmptyInvalidDir {
                    path: model_dir_buf,
                    reason: error.to_string(),
                });
            }
            Ok(None)
        }
    }
}

fn install_model_dir(
    manifest: &ArtifactManifest,
    model_dir: &Path,
    verify: &dyn Fn(&Path) -> Result<(), SafetyNetError>,
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
            .unwrap_or("model"),
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

        verify(&tmp_dir).map_err(SetupError::Verify)
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
        url: format!("{}:{}", manifest.hf_repo, file.file_name),
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
        SafetyNetError::ModelUnavailable { reason } => [
            "davlan-ner sensitive directory must be mode 0700",
            "davlan-ner sensitive file must not be group/world writable",
            "nym sensitive directory must be mode 0700",
            "nym sensitive file must not be group/world writable",
        ]
        .contains(&reason.as_str()),
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
            reason: "model sensitive path owner mismatch".to_string(),
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
            reason: "model sensitive path must not be a symlink".to_string(),
        });
    }
    if !(file_type.is_dir() || file_type.is_file()) {
        return Err(SetupError::NonEmptyInvalidDir {
            path: path.to_path_buf(),
            reason: "model sensitive path must be a regular file or directory".to_string(),
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
        let model_dir = root.path().join("davlan-mbert-ner-hrl");
        let fetcher = SyntheticFetcher {
            calls: Mutex::new(Vec::new()),
        };

        let err = install_ner_bundle_with_fetcher(Some(&model_dir), &fetcher).unwrap_err();

        assert!(matches!(
            err,
            SetupError::Verify(SafetyNetError::ModelIntegrityMismatch { .. })
        ));
        assert!(!model_dir.exists());
        let calls = fetcher.calls();
        assert!(calls
            .iter()
            .all(|(_, path)| path.extension().is_some_and(|ext| ext == "download")));
        let urls = calls.into_iter().map(|(url, _)| url).collect::<Vec<_>>();
        assert_eq!(
            urls,
            [
                "onnx/model_int8.onnx",
                "tokenizer.json",
                "tokenizer_config.json",
                "config.json",
                "special_tokens_map.json",
                "vocab.txt",
            ]
            .map(|path| format!(
                "https://huggingface.co/onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX/resolve/cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8/{path}"
            ))
        );
    }

    #[test]
    fn nym_install_fetches_the_pinned_revision_and_fails_closed_on_wrong_bytes() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("nym-small-int8");
        let fetcher = SyntheticFetcher {
            calls: Mutex::new(Vec::new()),
        };

        let err = install_nym_bundle_with_fetcher(Some(&model_dir), &fetcher).unwrap_err();

        assert!(matches!(
            err,
            SetupError::Verify(SafetyNetError::ModelIntegrityMismatch { .. })
        ));
        assert!(
            !model_dir.exists(),
            "a failed install leaves nothing behind"
        );
        let urls = fetcher
            .calls()
            .into_iter()
            .map(|(url, _)| url)
            .collect::<Vec<_>>();
        assert_eq!(
            urls,
            [
                "int8/config.json",
                "int8/model_int8.onnx",
                "int8/tokenizer.json"
            ]
            .map(|path| format!(
                "https://huggingface.co/Wismut/nym-pii-multilingual-small/resolve/4348999cd3c2e20c49615e9af7c6bbb45b64cd85/{path}"
            ))
        );
    }

    #[test]
    fn nym_install_refuses_a_non_empty_invalid_dir() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("nym-small-int8");
        std::fs::create_dir(&model_dir).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&model_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(model_dir.join("model_int8.onnx"), b"stale").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(
            model_dir.join("model_int8.onnx"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let fetcher = SyntheticFetcher {
            calls: Mutex::new(Vec::new()),
        };

        let err = install_nym_bundle_with_fetcher(Some(&model_dir), &fetcher).unwrap_err();

        assert!(matches!(err, SetupError::NonEmptyInvalidDir { .. }));
        assert!(fetcher.calls().is_empty());
    }

    #[test]
    fn fetch_to_write_failure_maps_to_io() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("davlan-mbert-ner-hrl");

        let err = install_ner_bundle_with_fetcher(Some(&model_dir), &FailingFetcher).unwrap_err();

        assert!(matches!(err, SetupError::Io { .. }));
        assert!(!model_dir.exists());
    }

    #[test]
    fn ureq_fetcher_is_https_only_and_redirect_bounded() {
        let agent = https_only_download_agent();

        assert!(agent.config().https_only());
        assert_eq!(agent.config().max_redirects(), MODEL_DOWNLOAD_MAX_REDIRECTS);
    }

    #[test]
    fn ureq_fetcher_rejects_plain_http_before_writing_file() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("artifact.download");

        let err = UreqFetcher
            .fetch_to("http://example.invalid/model.onnx", &dest)
            .unwrap_err();

        assert!(
            err.to_ascii_lowercase().contains("https"),
            "unexpected error: {err}"
        );
        assert!(!dest.exists());
    }

    #[cfg(unix)]
    #[test]
    fn loose_empty_dir_is_repaired_and_still_attempts_install() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("davlan-mbert-ner-hrl");
        std::fs::create_dir(&model_dir).unwrap();
        std::fs::set_permissions(&model_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let fetcher = SyntheticFetcher {
            calls: Mutex::new(Vec::new()),
        };

        let err = install_ner_bundle_with_fetcher(Some(&model_dir), &fetcher).unwrap_err();

        assert!(matches!(err, SetupError::Verify(_)));
        assert_eq!(fetcher.calls().len(), 6);
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
        let model_dir = root.path().join("davlan-mbert-ner-hrl");
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
    #[ignore = "hits Hugging Face (150 MB); run manually when validating the real Nym fetch path"]
    fn downloads_and_verifies_pinned_nym_bundle() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("nym-small-int8");

        let outcome = install_nym_bundle(Some(&model_dir)).unwrap();

        assert!(matches!(outcome, InstallOutcome::Installed { .. }));
        verify_nym_bundle(&model_dir).unwrap();
        assert!(matches!(
            install_nym_bundle(Some(&model_dir)).unwrap(),
            InstallOutcome::AlreadyPresent { .. }
        ));
    }

    #[test]
    #[ignore = "hits Hugging Face; run manually when validating the real network fetch path"]
    fn downloads_and_verifies_pinned_ner_bundle() {
        let root = tempfile::tempdir().unwrap();
        let model_dir = root.path().join("davlan-mbert-ner-hrl");

        let outcome = install_ner_bundle(Some(&model_dir)).unwrap();

        assert!(matches!(outcome, InstallOutcome::Installed { .. }));
        verify_davlan_ner_bundle(&model_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires a real pinned bundle that is safe to chmod"]
    fn loose_mode_current_user_dir_is_repaired_then_accepted() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let model_dir =
            PathBuf::from(std::env::var_os("GAZE_MODEL_SETUP_LOOSE_MODEL_DIR").expect(
                "set GAZE_MODEL_SETUP_LOOSE_MODEL_DIR to a loose current-euid-owned bundle",
            ));

        let outcome = install_ner_bundle(Some(&model_dir)).unwrap();

        assert!(matches!(outcome, InstallOutcome::AlreadyPresent { .. }));
        verify_davlan_ner_bundle(&model_dir).unwrap();
        let uid = unsafe { libc::geteuid() };
        let mut pending = vec![model_dir];
        while let Some(path) = pending.pop() {
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert_eq!(metadata.uid(), uid, "repaired path must retain its owner");
            assert!(!metadata.file_type().is_symlink());
            let mode = metadata.permissions().mode() & 0o777;
            if metadata.is_dir() {
                assert_eq!(mode, 0o700, "repaired directories must be private");
                pending.extend(
                    fs::read_dir(&path)
                        .unwrap()
                        .map(|entry| entry.unwrap().path()),
                );
            } else {
                assert!(metadata.is_file());
                assert_eq!(mode, 0o600, "repaired files must be private");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires a real pinned bundle in a readable directory owned by another uid"]
    fn foreign_owned_dir_fails_closed() {
        use std::os::unix::fs::MetadataExt;

        let model_dir = PathBuf::from(
            std::env::var_os("GAZE_MODEL_SETUP_FOREIGN_MODEL_DIR").expect(
                "set GAZE_MODEL_SETUP_FOREIGN_MODEL_DIR to a readable foreign-owned bundle",
            ),
        );
        assert_ne!(fs::symlink_metadata(&model_dir).unwrap().uid(), unsafe {
            libc::geteuid()
        });
        let verification_error = verify_davlan_ner_bundle(&model_dir).unwrap_err();
        assert!(matches!(
            &verification_error,
            SafetyNetError::ModelUnavailable { reason }
                if reason == "davlan-ner sensitive path owner mismatch"
        ));
        let expected_reason = verification_error.to_string();
        let err = install_ner_bundle(Some(&model_dir)).unwrap_err();

        assert!(matches!(
            err,
            SetupError::NonEmptyInvalidDir { reason, .. } if reason == expected_reason
        ));
    }
}
