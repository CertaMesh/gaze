use std::borrow::Cow;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use gaze::{CleanDocument, DictionaryBundle, LocaleChain, Policy, RawDocument, Session};
use gaze_recognizers::safety_net::kiji_distilbert::{
    KIJI_DISTILBERT_BUNDLE_SHA256, KIJI_DISTILBERT_HF_COMMIT, KIJI_DISTILBERT_HF_REPO,
    KIJI_DISTILBERT_SHA256SUMS,
};
use sha2::{Digest, Sha256};

use crate::error::CliError;
use crate::pipeline::build::{
    build_pipeline_from_policy, dictionary_terms_from_rulepacks, load_rulepacks,
    map_pipeline_error, map_policy_error, resolve_ner_threshold,
};

const DEFAULT_POLICY_FILE: &str = "gaze.toml";
const DEFAULT_MODEL_DIR_NAME: &str = "kiji-distilbert";
const OPF_UNPINNED_NOTICE: &str = "OPF safety-net is not pinned in this build; defaulting to NER.";
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
const DOCTOR_INPUT: &str =
    "From: Alice Example <alice@example.invalid>\nContact Alice Example about Example Ltd.";

#[derive(Debug)]
pub(crate) struct Args {
    pub(crate) safety_net: Option<SetupSafetyNet>,
    pub(crate) policy_out: Option<PathBuf>,
    pub(crate) model_dir: Option<PathBuf>,
    pub(crate) non_interactive: bool,
    pub(crate) force: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SetupSafetyNet {
    Ner,
    Opf,
}

pub(crate) fn run(args: Args) -> Result<(), CliError> {
    let summary = run_with_manifest(args, &kiji_manifest())?;
    print_summary(&summary);
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
enum ModelInstallStatus {
    AlreadyPresent,
    Downloaded,
}

#[derive(Debug)]
struct SetupSummary {
    model_dir: PathBuf,
    policy_path: PathBuf,
    model_status: ModelInstallStatus,
    doctor_clean_text: String,
    opf_notice: Option<String>,
    opf_checkpoint: Option<PathBuf>,
}

#[derive(Clone)]
struct ArtifactFile<'a> {
    source_path: Option<&'a str>,
    file_name: &'a str,
    inline_contents: Option<&'a str>,
}

#[derive(Clone)]
struct ArtifactManifest<'a> {
    hf_repo: &'a str,
    hf_commit: &'a str,
    bundle_sha256: &'a str,
    sha256sums: Cow<'a, str>,
    files: Vec<ArtifactFile<'a>>,
}

#[derive(Clone, Copy)]
struct OpfBundlePin<'a> {
    bundle_sha256: Option<&'a str>,
    required_artifacts: &'a [&'a str],
}

#[derive(Clone, Copy)]
struct OpfSetup<'a> {
    pin: OpfBundlePin<'a>,
    checkpoint_dir: Option<&'a Path>,
}

#[derive(Debug)]
struct ResolvedSetupSafetyNet {
    opf_notice: Option<String>,
    opf_checkpoint: Option<PathBuf>,
}

fn kiji_manifest() -> ArtifactManifest<'static> {
    ArtifactManifest {
        hf_repo: KIJI_DISTILBERT_HF_REPO,
        hf_commit: KIJI_DISTILBERT_HF_COMMIT,
        bundle_sha256: KIJI_DISTILBERT_BUNDLE_SHA256,
        sha256sums: Cow::Borrowed(KIJI_DISTILBERT_SHA256SUMS),
        files: vec![
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
        ],
    }
}

fn run_with_manifest(
    args: Args,
    manifest: &ArtifactManifest<'_>,
) -> Result<SetupSummary, CliError> {
    run_with_manifest_and_opf(args, manifest, default_opf_setup())
}

fn run_with_manifest_and_opf(
    args: Args,
    manifest: &ArtifactManifest<'_>,
    opf_setup: OpfSetup<'_>,
) -> Result<SetupSummary, CliError> {
    let resolved_safety_net = resolve_safety_net(args.safety_net, args.non_interactive, opf_setup)?;
    let model_dir = resolve_model_dir(args.model_dir)?;
    let policy_path = resolve_policy_path(args.policy_out, args.non_interactive)?;

    let model_status = ensure_model_dir(manifest, &model_dir)?;

    write_policy(&policy_path, &model_dir, args.force)?;
    let doctor_clean_text = doctor_check(&policy_path)?;

    Ok(SetupSummary {
        model_dir,
        policy_path,
        model_status,
        doctor_clean_text,
        opf_notice: resolved_safety_net.opf_notice,
        opf_checkpoint: resolved_safety_net.opf_checkpoint,
    })
}

fn resolve_safety_net(
    requested: Option<SetupSafetyNet>,
    non_interactive: bool,
    opf_setup: OpfSetup<'_>,
) -> Result<ResolvedSetupSafetyNet, CliError> {
    let choice = match requested {
        Some(choice) => choice,
        None if non_interactive => SetupSafetyNet::Ner,
        None => prompt_safety_net()?,
    };

    match choice {
        SetupSafetyNet::Ner => Ok(ResolvedSetupSafetyNet {
            opf_notice: None,
            opf_checkpoint: None,
        }),
        SetupSafetyNet::Opf => resolve_opf_safety_net(opf_setup),
    }
}

#[cfg(feature = "safety-net-openai")]
fn opf_bundle_pin() -> OpfBundlePin<'static> {
    use gaze_recognizers::safety_net::openai_filter::{
        OPF_CHECKPOINT_BUNDLE_SHA256, REQUIRED_OPF_ARTIFACTS,
    };

    OpfBundlePin {
        bundle_sha256: OPF_CHECKPOINT_BUNDLE_SHA256,
        required_artifacts: REQUIRED_OPF_ARTIFACTS,
    }
}

#[cfg(not(feature = "safety-net-openai"))]
fn opf_bundle_pin() -> OpfBundlePin<'static> {
    OpfBundlePin {
        bundle_sha256: None,
        required_artifacts: &[],
    }
}

fn default_opf_setup() -> OpfSetup<'static> {
    OpfSetup {
        pin: opf_bundle_pin(),
        checkpoint_dir: None,
    }
}

fn resolve_opf_safety_net(opf_setup: OpfSetup<'_>) -> Result<ResolvedSetupSafetyNet, CliError> {
    if opf_setup.pin.bundle_sha256.is_none() {
        return Ok(ResolvedSetupSafetyNet {
            opf_notice: Some(OPF_UNPINNED_NOTICE.to_string()),
            opf_checkpoint: None,
        });
    }

    let checkpoint_dir = match opf_setup.checkpoint_dir {
        Some(path) => absolute_path(path)?,
        None => default_opf_checkpoint_dir()?,
    };

    verify_opf_checkpoint_dir(opf_setup.pin, &checkpoint_dir).map_err(|err| {
        setup_error(format!(
            "OPF checkpoint is pinned but not installed or SHA-valid at `{}`: {err}. Run `opf download` then re-run `gaze setup --safety-net opf`.",
            checkpoint_dir.display()
        ))
    })?;

    Ok(ResolvedSetupSafetyNet {
        opf_notice: None,
        opf_checkpoint: Some(canonical_or_absolute(&checkpoint_dir)?),
    })
}

fn default_opf_checkpoint_dir() -> Result<PathBuf, CliError> {
    if let Some(checkpoint) = std::env::var_os("OPF_CHECKPOINT").filter(|value| !value.is_empty()) {
        return absolute_path(&PathBuf::from(checkpoint));
    }
    let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) else {
        return Err(setup_error(
            "cannot resolve OPF checkpoint dir: neither OPF_CHECKPOINT nor HOME is set; run `opf download` then re-run `gaze setup --safety-net opf`".to_string(),
        ));
    };
    Ok(PathBuf::from(home).join(".opf").join("privacy_filter"))
}

fn verify_opf_checkpoint_dir(pin: OpfBundlePin<'_>, checkpoint_dir: &Path) -> Result<(), String> {
    let expected_bundle_sha256 = pin
        .bundle_sha256
        .ok_or_else(|| "OPF checkpoint bundle SHA is not pinned".to_string())?;
    if pin.required_artifacts.is_empty() {
        return Err("OPF required artifact list is empty".to_string());
    }
    if !checkpoint_dir.is_dir() {
        return Err(format!("`{}` is not a directory", checkpoint_dir.display()));
    }
    reject_symlink(checkpoint_dir)?;

    let mut manifest = String::new();
    for required in pin.required_artifacts {
        if required.contains('/') || required.contains('\\') {
            return Err("OPF required artifacts must be flat file names".to_string());
        }
        let artifact = checkpoint_dir.join(required);
        reject_symlink(&artifact)?;
        let bytes = fs::read(&artifact)
            .map_err(|err| format!("cannot read `{}`: {err}", artifact.display()))?;
        push_sha256sum_manifest_line(&mut manifest, required, &hex_sha256(&bytes));
    }

    let actual_bundle_sha256 = hex_sha256(manifest.as_bytes());
    if actual_bundle_sha256 != expected_bundle_sha256 {
        return Err(format!(
            "checkpoint bundle SHA mismatch: expected {} got {}",
            expected_bundle_sha256, actual_bundle_sha256
        ));
    }
    Ok(())
}

fn push_sha256sum_manifest_line(manifest: &mut String, artifact: &str, sha256: &str) {
    manifest.push_str(sha256);
    manifest.push_str("  ");
    manifest.push_str(artifact);
    manifest.push('\n');
}

fn prompt_safety_net() -> Result<SetupSafetyNet, CliError> {
    loop {
        let input = prompt_line("Safety net [ner/opf] (default ner): ")?;
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("ner") {
            return Ok(SetupSafetyNet::Ner);
        }
        if trimmed.eq_ignore_ascii_case("opf") {
            return Ok(SetupSafetyNet::Opf);
        }
        println!("Enter `ner` or `opf`.");
    }
}

fn resolve_model_dir(model_dir: Option<PathBuf>) -> Result<PathBuf, CliError> {
    match model_dir {
        Some(path) => absolute_path(&path),
        None => default_model_dir(),
    }
}

fn resolve_policy_path(
    policy_out: Option<PathBuf>,
    non_interactive: bool,
) -> Result<PathBuf, CliError> {
    match policy_out {
        Some(path) => absolute_path(&path),
        None if non_interactive => absolute_path(Path::new(DEFAULT_POLICY_FILE)),
        None => {
            let input = prompt_line("Policy path (default ./gaze.toml): ")?;
            let trimmed = input.trim();
            if trimmed.is_empty() {
                absolute_path(Path::new(DEFAULT_POLICY_FILE))
            } else {
                absolute_path(Path::new(trimmed))
            }
        }
    }
}

fn prompt_line(prompt: &str) -> Result<String, CliError> {
    print!("{prompt}");
    io::stdout()
        .flush()
        .map_err(|err| setup_error(format!("failed to flush prompt: {err}")))?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|err| setup_error(format!("failed to read prompt input: {err}")))?;
    Ok(line)
}

fn default_model_dir() -> Result<PathBuf, CliError> {
    if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(xdg_data_home)
            .join("gaze")
            .join("models")
            .join(DEFAULT_MODEL_DIR_NAME));
    }
    let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) else {
        return Err(setup_error(
            "cannot resolve model dir: neither XDG_DATA_HOME nor HOME is set".to_string(),
        ));
    };
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("gaze")
        .join("models")
        .join(DEFAULT_MODEL_DIR_NAME))
}

fn absolute_path(path: &Path) -> Result<PathBuf, CliError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|err| setup_error(format!("cannot resolve current directory: {err}")))
    }
}

fn ensure_model_dir(
    manifest: &ArtifactManifest<'_>,
    model_dir: &Path,
) -> Result<ModelInstallStatus, CliError> {
    if model_dir.exists() {
        if verify_model_dir(manifest, model_dir).is_ok() {
            secure_model_permissions(manifest, model_dir)?;
            return Ok(ModelInstallStatus::AlreadyPresent);
        }
        if !is_empty_dir(model_dir)? {
            return Err(setup_error(format!(
                "existing model dir `{}` is not SHA-valid; remove it or choose --model-dir",
                model_dir.display()
            )));
        }
    }

    download_model_dir(manifest, model_dir)?;
    verify_model_dir(manifest, model_dir).map_err(setup_error)?;
    secure_model_permissions(manifest, model_dir)?;
    Ok(ModelInstallStatus::Downloaded)
}

fn is_empty_dir(path: &Path) -> Result<bool, CliError> {
    if !path.is_dir() {
        return Ok(false);
    }
    let mut entries = fs::read_dir(path)
        .map_err(|err| setup_error(format!("cannot read `{}`: {err}", path.display())))?;
    Ok(entries.next().is_none())
}

fn download_model_dir(manifest: &ArtifactManifest<'_>, model_dir: &Path) -> Result<(), CliError> {
    let parent = model_dir.parent().ok_or_else(|| {
        setup_error(format!(
            "model dir `{}` has no parent directory",
            model_dir.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|err| {
        setup_error(format!(
            "cannot create model parent `{}`: {err}",
            parent.display()
        ))
    })?;

    let tmp_dir = parent.join(format!(
        ".{}.download-{}",
        model_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(DEFAULT_MODEL_DIR_NAME),
        unique_suffix()
    ));
    fs::create_dir(&tmp_dir).map_err(|err| {
        setup_error(format!(
            "cannot create temporary model dir `{}`: {err}",
            tmp_dir.display()
        ))
    })?;
    set_dir_private(&tmp_dir)?;

    let result = (|| {
        for file in &manifest.files {
            let destination = tmp_dir.join(file.file_name);
            if let Some(contents) = file.inline_contents {
                fs::write(&destination, contents.as_bytes()).map_err(|err| {
                    setup_error(format!("cannot write `{}`: {err}", destination.display()))
                })?;
                set_file_private(&destination)?;
                continue;
            }

            let source_path = file.source_path.ok_or_else(|| {
                setup_error(format!("artifact `{}` has no source path", file.file_name))
            })?;
            let url = format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                manifest.hf_repo, manifest.hf_commit, source_path
            );
            download_url_to_file(&url, &destination)?;
            set_file_private(&destination)?;
        }

        let sums_path = tmp_dir.join("SHA256SUMS");
        fs::write(&sums_path, manifest.sha256sums.as_bytes())
            .map_err(|err| setup_error(format!("cannot write `{}`: {err}", sums_path.display())))?;
        set_file_private(&sums_path)?;
        verify_model_dir(manifest, &tmp_dir).map_err(setup_error)
    })();

    if let Err(err) = result {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(err);
    }

    if model_dir.exists() {
        if is_empty_dir(model_dir)? {
            fs::remove_dir(model_dir).map_err(|err| {
                setup_error(format!(
                    "cannot replace empty model dir `{}`: {err}",
                    model_dir.display()
                ))
            })?;
        } else {
            let _ = fs::remove_dir_all(&tmp_dir);
            return Err(setup_error(format!(
                "model dir `{}` became non-empty during setup",
                model_dir.display()
            )));
        }
    }
    fs::rename(&tmp_dir, model_dir).map_err(|err| {
        let _ = fs::remove_dir_all(&tmp_dir);
        setup_error(format!(
            "cannot install model dir `{}`: {err}",
            model_dir.display()
        ))
    })
}

fn download_url_to_file(url: &str, destination: &Path) -> Result<(), CliError> {
    let mut last_error = String::new();
    for _ in 0..3 {
        match try_download_url_to_file(url, destination) {
            Ok(()) => return Ok(()),
            Err(err) => last_error = err,
        }
    }
    Err(setup_error(format!(
        "failed to download `{url}` after 3 attempts: {last_error}"
    )))
}

fn try_download_url_to_file(url: &str, destination: &Path) -> Result<(), String> {
    let response = ureq::get(url).call().map_err(|err| err.to_string())?;
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(u64::MAX)
        .reader();
    let tmp = destination.with_extension("download");
    let mut file = fs::File::create(&tmp).map_err(|err| err.to_string())?;
    io::copy(&mut reader, &mut file).map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())?;
    fs::rename(&tmp, destination).map_err(|err| err.to_string())
}

fn verify_model_dir(manifest: &ArtifactManifest<'_>, model_dir: &Path) -> Result<(), String> {
    if !model_dir.is_dir() {
        return Err(format!("`{}` is not a directory", model_dir.display()));
    }
    reject_symlink(model_dir)?;

    let sums_path = model_dir.join("SHA256SUMS");
    reject_symlink(&sums_path)?;
    let sums = fs::read(&sums_path)
        .map_err(|err| format!("cannot read `{}`: {err}", sums_path.display()))?;
    let actual_bundle_sha = hex_sha256(&sums);
    if actual_bundle_sha != manifest.bundle_sha256 {
        return Err(format!(
            "SHA256SUMS integrity mismatch: expected {} got {}",
            manifest.bundle_sha256, actual_bundle_sha
        ));
    }

    let entries = parse_sha256sums(&sums)?;
    for file in &manifest.files {
        if !entries
            .iter()
            .any(|(name, _)| name.as_str() == file.file_name)
        {
            return Err(format!("SHA256SUMS missing entry for {}", file.file_name));
        }
    }

    for (file_name, expected_sha) in entries {
        let path = model_dir.join(&file_name);
        reject_symlink(&path)?;
        let bytes =
            fs::read(&path).map_err(|err| format!("cannot read `{}`: {err}", path.display()))?;
        let actual_sha = hex_sha256(&bytes);
        if actual_sha != expected_sha {
            return Err(format!(
                "artifact `{}` SHA mismatch: expected {} got {}",
                file_name, expected_sha, actual_sha
            ));
        }
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| format!("cannot inspect `{}`: {err}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("`{}` must not be a symlink", path.display()));
    }
    Ok(())
}

fn parse_sha256sums(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "SHA256SUMS is not UTF-8".to_string())?;
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut fields = trimmed.split_whitespace();
        let sha = fields.next().unwrap_or_default();
        let name = fields.next().unwrap_or_default();
        if fields.next().is_some()
            || sha.len() != 64
            || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
            || name.is_empty()
            || name.contains('/')
            || name.contains('\\')
        {
            return Err(format!("malformed SHA256SUMS line {}", index + 1));
        }
        entries.push((name.to_string(), sha.to_ascii_lowercase()));
    }
    Ok(entries)
}

fn secure_model_permissions(
    manifest: &ArtifactManifest<'_>,
    model_dir: &Path,
) -> Result<(), CliError> {
    set_dir_private(model_dir)?;
    set_file_private(&model_dir.join("SHA256SUMS"))?;
    for file in &manifest.files {
        set_file_private(&model_dir.join(file.file_name))?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_dir_private(path: &Path) -> Result<(), CliError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|err| setup_error(format!("cannot chmod 0700 `{}`: {err}", path.display())))
}

#[cfg(not(unix))]
fn set_dir_private(_path: &Path) -> Result<(), CliError> {
    Ok(())
}

#[cfg(unix)]
fn set_file_private(path: &Path) -> Result<(), CliError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|err| setup_error(format!("cannot chmod 0600 `{}`: {err}", path.display())))
}

#[cfg(not(unix))]
fn set_file_private(_path: &Path) -> Result<(), CliError> {
    Ok(())
}

fn write_policy(policy_path: &Path, model_dir: &Path, force: bool) -> Result<(), CliError> {
    if policy_path.exists() && !force {
        return Err(setup_error(format!(
            "policy `{}` already exists; pass --force to overwrite",
            policy_path.display()
        )));
    }
    if let Some(parent) = policy_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            setup_error(format!(
                "cannot create policy directory `{}`: {err}",
                parent.display()
            ))
        })?;
    }

    let model_dir = canonical_or_absolute(model_dir)?;
    let policy = setup_policy_toml(&model_dir);
    fs::write(policy_path, policy).map_err(|err| {
        setup_error(format!(
            "cannot write policy `{}`: {err}",
            policy_path.display()
        ))
    })
}

fn setup_policy_toml(model_dir: &Path) -> String {
    let model_dir = toml_basic_string(&model_dir.to_string_lossy());
    format!(
        r#"schema_version = "0.1.0"

[session]
scope = "conversation"

[locale]
active = ["en-US"]

[ner]
model_dir = "{model_dir}"
locale = "en-US"
threshold = 0.3

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "class"
class = "name"
action = "tokenize"

[[rule]]
kind = "class"
class = "location"
action = "generalize"

[[rule]]
kind = "class"
class = "organization"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
"#
    )
}

fn doctor_check(policy_path: &Path) -> Result<String, CliError> {
    let policy = Policy::load_for_cli(policy_path).map_err(map_policy_error)?;
    let rulepacks = load_rulepacks(&policy).map_err(map_pipeline_error)?;
    let rulepack_dictionaries =
        dictionary_terms_from_rulepacks(&rulepacks).map_err(map_pipeline_error)?;
    let mut dictionary_terms = policy.dictionaries.clone();
    dictionary_terms.extend(rulepack_dictionaries);
    let dictionaries = DictionaryBundle::from_rulepack_terms(&dictionary_terms);
    let locale_chain =
        LocaleChain::merge_cli_policy_rulepack_default(None, policy.locale.as_deref(), None);
    let pipeline = build_pipeline_from_policy(
        &policy,
        &rulepacks,
        None,
        &locale_chain,
        resolve_ner_threshold(None, Some(&policy)),
    )?;
    let session = Session::from_policy(&policy)
        .map_err(|err| setup_error(format!("doctor session init failed: {err}")))?;
    let clean = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(DOCTOR_INPUT.to_string()),
            locale_chain.as_slice(),
            &dictionaries,
        )
        .map_err(|err| setup_error(format!("doctor clean failed: {err}")))?;
    let CleanDocument::Text(clean_text) = clean else {
        return Err(setup_error(
            "doctor produced a non-text clean document".to_string(),
        ));
    };

    if !clean_text.contains(":Name_") || !clean_text.contains(":Email_") {
        return Err(setup_error(format!(
            "doctor did not tokenize expected synthetic Name and Email spans: {clean_text}"
        )));
    }
    Ok(clean_text)
}

fn print_summary(summary: &SetupSummary) {
    if let Some(notice) = &summary.opf_notice {
        println!("{notice}");
    }
    if let Some(opf_checkpoint) = &summary.opf_checkpoint {
        println!("OPF checkpoint verified {}", opf_checkpoint.display());
    }
    match summary.model_status {
        ModelInstallStatus::AlreadyPresent => {
            println!("model unchanged {}", summary.model_dir.display());
        }
        ModelInstallStatus::Downloaded => {
            println!("model installed {}", summary.model_dir.display());
        }
    }
    println!("policy written {}", summary.policy_path.display());
    println!("doctor pass {}", summary.doctor_clean_text);
    println!("Setup complete.");
    println!("Model: {}", summary.model_dir.display());
    println!("Policy: {}", summary.policy_path.display());
    println!(
        "Try: printf 'From: Alice Example <alice@example.invalid>\\nContact Alice Example about Example Ltd.\\n' | gaze clean --policy {}",
        shell_quote_path(&summary.policy_path)
    );
    println!(
        "For gaze index: export GAZE_KIJI_DISTILBERT_MODEL_DIR={}",
        shell_quote_path(&summary.model_dir)
    );
    if let Some(opf_checkpoint) = &summary.opf_checkpoint {
        println!(
            "For OPF safety net: gaze clean --policy {} --safety-net openai-filter --opf-command $(command -v opf) --opf-checkpoint {}",
            shell_quote_path(&summary.policy_path),
            shell_quote_path(opf_checkpoint)
        );
    }
    println!("For gaze index: set GAZE_INDEX_KEY before ingest/search.");
}

fn canonical_or_absolute(path: &Path) -> Result<PathBuf, CliError> {
    path.canonicalize().or_else(|_| absolute_path(path))
}

fn toml_basic_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn shell_quote_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
    {
        value.into_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        write!(&mut out, "{byte:02x}").expect("writing to string cannot fail");
    }
    out
}

fn unique_suffix() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("{}-{millis}", std::process::id())
}

fn setup_error(detail: String) -> CliError {
    CliError::SetupDetail(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn synthetic_manifest<'a>(
        model_bytes: &'a str,
        tokenizer_bytes: &'a str,
        labels_bytes: &'a str,
    ) -> ArtifactManifest<'a> {
        let sums = format!(
            "{}  labels.json\n{}  model.onnx\n{}  tokenizer.json\n",
            hex_sha256(labels_bytes.as_bytes()),
            hex_sha256(model_bytes.as_bytes()),
            hex_sha256(tokenizer_bytes.as_bytes()),
        );
        let bundle_sha = hex_sha256(sums.as_bytes());
        ArtifactManifest {
            hf_repo: "example.invalid/gaze-test",
            hf_commit: "0000000000000000000000000000000000000000",
            bundle_sha256: Box::leak(bundle_sha.into_boxed_str()),
            sha256sums: Cow::Owned(sums),
            files: vec![
                ArtifactFile {
                    source_path: Some("model.onnx"),
                    file_name: "model.onnx",
                    inline_contents: Some(model_bytes),
                },
                ArtifactFile {
                    source_path: Some("tokenizer.json"),
                    file_name: "tokenizer.json",
                    inline_contents: Some(tokenizer_bytes),
                },
                ArtifactFile {
                    source_path: None,
                    file_name: "labels.json",
                    inline_contents: Some(labels_bytes),
                },
            ],
        }
    }

    fn write_manifest_dir(manifest: &ArtifactManifest<'_>, model_dir: &Path) {
        fs::create_dir_all(model_dir).unwrap();
        for file in &manifest.files {
            fs::write(
                model_dir.join(file.file_name),
                file.inline_contents.unwrap().as_bytes(),
            )
            .unwrap();
        }
        fs::write(model_dir.join("SHA256SUMS"), manifest.sha256sums.as_bytes()).unwrap();
    }

    #[test]
    fn non_interactive_existing_model_skips_download_writes_policy_and_doctor_passes() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("gaze.toml");
        let manifest = synthetic_manifest("fake-model", "fake-tokenizer", "{}");
        write_manifest_dir(&manifest, &model_dir);

        let summary = run_with_manifest(
            Args {
                safety_net: None,
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir.clone()),
                non_interactive: true,
                force: false,
            },
            &manifest,
        )
        .unwrap();

        assert_eq!(summary.model_status, ModelInstallStatus::AlreadyPresent);
        assert_eq!(summary.policy_path, policy_out);
        let policy = fs::read_to_string(&summary.policy_path).unwrap();
        assert!(policy.contains("[ner]"));
        assert!(policy.contains(&toml_basic_string(&model_dir.to_string_lossy())));
        assert!(summary.doctor_clean_text.contains(":Name_"));
        assert!(summary.doctor_clean_text.contains(":Email_"));
    }

    #[test]
    fn generated_policy_tokenizes_with_clean_pipeline() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        let manifest = synthetic_manifest("fake-model", "fake-tokenizer", "{}");
        write_manifest_dir(&manifest, &model_dir);

        run_with_manifest(
            Args {
                safety_net: Some(SetupSafetyNet::Ner),
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir),
                non_interactive: true,
                force: false,
            },
            &manifest,
        )
        .unwrap();

        let clean_text = doctor_check(&policy_out).unwrap();
        assert!(clean_text.contains(":Name_"), "{clean_text}");
        assert!(clean_text.contains(":Email_"), "{clean_text}");
    }

    #[test]
    fn sha_mismatch_existing_model_fails_closed() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        let manifest = synthetic_manifest("fake-model", "fake-tokenizer", "{}");
        write_manifest_dir(&manifest, &model_dir);
        fs::write(model_dir.join("model.onnx"), b"corrupt").unwrap();

        let err = run_with_manifest(
            Args {
                safety_net: Some(SetupSafetyNet::Ner),
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir),
                non_interactive: true,
                force: false,
            },
            &manifest,
        )
        .unwrap_err();

        assert!(matches!(err, CliError::SetupDetail(detail) if detail.contains("not SHA-valid")));
        assert!(!policy_out.exists());
    }

    #[test]
    fn opf_request_defaults_to_ner_when_bundle_is_not_pinned() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        let checkpoint_dir = dir.path().join("missing-opf");
        let manifest = synthetic_manifest("fake-model", "fake-tokenizer", "{}");
        write_manifest_dir(&manifest, &model_dir);

        let summary = run_with_manifest_and_opf(
            Args {
                safety_net: Some(SetupSafetyNet::Opf),
                policy_out: Some(policy_out),
                model_dir: Some(model_dir),
                non_interactive: true,
                force: false,
            },
            &manifest,
            OpfSetup {
                pin: OpfBundlePin {
                    bundle_sha256: None,
                    required_artifacts: &[],
                },
                checkpoint_dir: Some(&checkpoint_dir),
            },
        )
        .unwrap();

        assert_eq!(summary.opf_notice.as_deref(), Some(OPF_UNPINNED_NOTICE));
        assert_eq!(summary.opf_checkpoint, None);
        assert_eq!(summary.model_status, ModelInstallStatus::AlreadyPresent);
    }

    #[test]
    fn opf_request_with_pinned_bundle_requires_downloaded_checkpoint() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        let checkpoint_dir = dir.path().join("missing-opf");
        let manifest = synthetic_manifest("fake-model", "fake-tokenizer", "{}");
        write_manifest_dir(&manifest, &model_dir);

        let err = run_with_manifest_and_opf(
            Args {
                safety_net: Some(SetupSafetyNet::Opf),
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir),
                non_interactive: true,
                force: false,
            },
            &manifest,
            OpfSetup {
                pin: OpfBundlePin {
                    bundle_sha256: Some(
                        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    ),
                    required_artifacts: &["config.json"],
                },
                checkpoint_dir: Some(&checkpoint_dir),
            },
        )
        .unwrap_err();

        assert!(
            matches!(err, CliError::SetupDetail(detail) if detail.contains("Run `opf download`") && detail.contains("not installed or SHA-valid"))
        );
        assert!(!policy_out.exists());
    }

    #[test]
    fn opf_request_with_sha_mismatched_checkpoint_fails_closed() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        let checkpoint_dir = dir.path().join("privacy_filter");
        let manifest = synthetic_manifest("fake-model", "fake-tokenizer", "{}");
        write_manifest_dir(&manifest, &model_dir);
        fs::create_dir_all(&checkpoint_dir).unwrap();
        fs::write(checkpoint_dir.join("config.json"), b"corrupt").unwrap();

        let err = run_with_manifest_and_opf(
            Args {
                safety_net: Some(SetupSafetyNet::Opf),
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir),
                non_interactive: true,
                force: false,
            },
            &manifest,
            OpfSetup {
                pin: OpfBundlePin {
                    bundle_sha256: Some(
                        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    ),
                    required_artifacts: &["config.json"],
                },
                checkpoint_dir: Some(&checkpoint_dir),
            },
        )
        .unwrap_err();

        assert!(
            matches!(err, CliError::SetupDetail(detail) if detail.contains("checkpoint bundle SHA mismatch") && detail.contains("Run `opf download`"))
        );
        assert!(!policy_out.exists());
    }

    #[test]
    fn opf_request_with_pinned_checkpoint_records_runtime_wiring() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        let checkpoint_dir = dir.path().join("privacy_filter");
        let manifest = synthetic_manifest("fake-model", "fake-tokenizer", "{}");
        write_manifest_dir(&manifest, &model_dir);
        fs::create_dir_all(&checkpoint_dir).unwrap();
        fs::write(checkpoint_dir.join("config.json"), b"{}").unwrap();
        fs::write(
            checkpoint_dir.join("model.safetensors"),
            b"synthetic-weights",
        )
        .unwrap();

        let mut opf_manifest = String::new();
        push_sha256sum_manifest_line(&mut opf_manifest, "config.json", &hex_sha256(b"{}"));
        push_sha256sum_manifest_line(
            &mut opf_manifest,
            "model.safetensors",
            &hex_sha256(b"synthetic-weights"),
        );
        let bundle_sha = hex_sha256(opf_manifest.as_bytes());

        let summary = run_with_manifest_and_opf(
            Args {
                safety_net: Some(SetupSafetyNet::Opf),
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir),
                non_interactive: true,
                force: false,
            },
            &manifest,
            OpfSetup {
                pin: OpfBundlePin {
                    bundle_sha256: Some(Box::leak(bundle_sha.into_boxed_str())),
                    required_artifacts: &["config.json", "model.safetensors"],
                },
                checkpoint_dir: Some(&checkpoint_dir),
            },
        )
        .unwrap();

        let checkpoint_dir = checkpoint_dir.canonicalize().unwrap();
        assert_eq!(summary.opf_notice, None);
        assert_eq!(
            summary.opf_checkpoint.as_deref(),
            Some(checkpoint_dir.as_path())
        );
        assert_eq!(summary.model_status, ModelInstallStatus::AlreadyPresent);
        let policy = fs::read_to_string(policy_out).unwrap();
        assert!(policy.contains("[ner]"));
    }

    #[test]
    #[ignore = "hits Hugging Face; run manually when validating the real network fetch path"]
    fn downloads_pinned_kiji_bundle_from_hugging_face() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("kiji-distilbert");
        ensure_model_dir(&kiji_manifest(), &model_dir).unwrap();
        verify_model_dir(&kiji_manifest(), &model_dir).unwrap();
    }
}
