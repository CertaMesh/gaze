use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use gaze::{CleanDocument, RawDocument, Session};
use gaze_model_setup::{
    install_kiji_bundle, InstallOptions, InstallOutcome, KijiDistilbertPrecision, SetupError,
};
use sha2::{Digest, Sha256};

use crate::clean_overrides::CleanOverrides;
use crate::error::CliError;
use crate::pipeline::build::resolve_pipeline;

const DEFAULT_POLICY_FILE: &str = "gaze.toml";
const OPF_UNPINNED_NOTICE: &str = "OPF safety-net is not pinned in this build; defaulting to NER.";
const DOCTOR_INPUT: &str =
    "From: Alice Example <alice@example.invalid>\nContact Alice Example about Example Ltd."; // fixture-cited(crates/gaze-cli/src/commands/setup.rs:commands::setup::tests::non_interactive_existing_model_skips_download_writes_policy_and_doctor_passes)

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
    let summary = run_with_opf_setup(args, default_opf_setup())?;
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

fn run_with_opf_setup(args: Args, opf_setup: OpfSetup<'_>) -> Result<SetupSummary, CliError> {
    let resolved_safety_net = resolve_safety_net(args.safety_net, args.non_interactive, opf_setup)?;
    let policy_path = resolve_policy_path(args.policy_out, args.non_interactive)?;

    let (model_dir, model_status) = install_kiji_model(args.model_dir)?;

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

fn install_kiji_model(
    model_dir: Option<PathBuf>,
) -> Result<(PathBuf, ModelInstallStatus), CliError> {
    let outcome = install_kiji_bundle(&InstallOptions {
        model_dir,
        precision: KijiDistilbertPrecision::Fp32,
    })
    .map_err(map_model_setup_error)?;
    Ok(match outcome {
        InstallOutcome::AlreadyPresent { model_dir } => {
            (model_dir, ModelInstallStatus::AlreadyPresent)
        }
        InstallOutcome::Installed { model_dir } => (model_dir, ModelInstallStatus::Downloaded),
    })
}

fn map_model_setup_error(err: SetupError) -> CliError {
    setup_error(format!(
        "Kiji model setup failed: {err}. Remediation: re-run `gaze setup` to repair a current-user loose-permission bundle, or move/chown/chmod/remove the model directory and retry with `--model-dir`."
    ))
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

fn absolute_path(path: &Path) -> Result<PathBuf, CliError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|err| setup_error(format!("cannot resolve current directory: {err}")))
    }
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| format!("cannot inspect `{}`: {err}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("`{}` must not be a symlink", path.display()));
    }
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
    let resolved = resolve_pipeline(
        Some(policy_path),
        &CleanOverrides::default(),
        &[],
        None,
        None,
        None,
    )?;
    let policy = resolved.policy.expect("doctor requires a policy path");
    let pipeline = resolved.pipeline;
    let locale_chain = resolved.locale_chain;
    let dictionaries = resolved.dictionaries;
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
        "Try: printf 'From: Alice Example <alice@example.invalid>\\nContact Alice Example about Example Ltd.\\n' | gaze clean --policy {}", // fixture-cited(crates/gaze-cli/src/commands/setup.rs:commands::setup::tests::non_interactive_existing_model_skips_download_writes_policy_and_doctor_passes)
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

fn setup_error(detail: String) -> CliError {
    CliError::SetupDetail(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn write_synthetic_kiji_dir(model_dir: &Path) {
        let model_bytes = b"synthetic model bytes";
        let tokenizer_bytes = b"synthetic tokenizer bytes";
        let labels_bytes = b"{}";
        let sums = format!(
            "{}  labels.json\n{}  model.onnx\n{}  tokenizer.json\n",
            hex_sha256(labels_bytes),
            hex_sha256(model_bytes),
            hex_sha256(tokenizer_bytes),
        );
        fs::create_dir_all(model_dir).unwrap();
        fs::write(model_dir.join("labels.json"), labels_bytes).unwrap();
        fs::write(model_dir.join("model.onnx"), model_bytes).unwrap();
        fs::write(model_dir.join("tokenizer.json"), tokenizer_bytes).unwrap();
        fs::write(model_dir.join("SHA256SUMS"), sums.as_bytes()).unwrap();
    }

    #[test]
    fn existing_synthetic_model_fails_closed_before_policy_write() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        write_synthetic_kiji_dir(&model_dir);

        let err = run_with_opf_setup(
            Args {
                safety_net: Some(SetupSafetyNet::Ner),
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir),
                non_interactive: true,
                force: false,
            },
            default_opf_setup(),
        )
        .unwrap_err();

        assert!(
            matches!(err, CliError::SetupDetail(detail) if detail.contains("Kiji model setup failed")
                && detail.contains("non-empty but invalid")
                && detail.contains("Remediation"))
        );
        assert!(!policy_out.exists());
    }

    #[cfg(unix)]
    #[test]
    fn loose_existing_synthetic_model_is_repaired_then_rejected() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        write_synthetic_kiji_dir(&model_dir);
        fs::set_permissions(&model_dir, fs::Permissions::from_mode(0o755)).unwrap();
        for file_name in ["labels.json", "model.onnx", "tokenizer.json", "SHA256SUMS"] {
            fs::set_permissions(model_dir.join(file_name), fs::Permissions::from_mode(0o644))
                .unwrap();
        }

        let err = run_with_opf_setup(
            Args {
                safety_net: Some(SetupSafetyNet::Ner),
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir.clone()),
                non_interactive: true,
                force: false,
            },
            default_opf_setup(),
        )
        .unwrap_err();

        assert!(
            matches!(err, CliError::SetupDetail(detail) if detail.contains("Kiji model setup failed")
                && detail.contains("non-empty but invalid"))
        );
        assert_eq!(
            fs::symlink_metadata(&model_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for file_name in ["labels.json", "model.onnx", "tokenizer.json", "SHA256SUMS"] {
            assert_eq!(
                fs::symlink_metadata(model_dir.join(file_name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(!policy_out.exists());
    }

    #[test]
    fn generated_policy_tokenizes_with_clean_pipeline() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        write_synthetic_kiji_dir(&model_dir);
        write_policy(&policy_out, &model_dir, false).unwrap();

        let policy = fs::read_to_string(&policy_out).unwrap();
        assert!(policy.contains("[ner]"));
        assert!(policy.contains(&toml_basic_string(&model_dir.to_string_lossy())));

        let clean_text = doctor_check(&policy_out).unwrap();
        assert!(clean_text.contains(":Name_"), "{clean_text}");
        assert!(clean_text.contains(":Email_"), "{clean_text}");
    }

    #[test]
    fn opf_request_defaults_to_ner_when_bundle_is_not_pinned() {
        let dir = tempdir().unwrap();
        let checkpoint_dir = dir.path().join("missing-opf");

        let resolved = resolve_safety_net(
            Some(SetupSafetyNet::Opf),
            true,
            OpfSetup {
                pin: OpfBundlePin {
                    bundle_sha256: None,
                    required_artifacts: &[],
                },
                checkpoint_dir: Some(&checkpoint_dir),
            },
        )
        .unwrap();

        assert_eq!(resolved.opf_notice.as_deref(), Some(OPF_UNPINNED_NOTICE));
        assert_eq!(resolved.opf_checkpoint, None);
    }

    #[test]
    fn opf_request_with_pinned_bundle_requires_downloaded_checkpoint() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("__gaze_test_fixed_ner");
        let policy_out = dir.path().join("policy.toml");
        let checkpoint_dir = dir.path().join("missing-opf");

        let err = run_with_opf_setup(
            Args {
                safety_net: Some(SetupSafetyNet::Opf),
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir),
                non_interactive: true,
                force: false,
            },
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
        fs::create_dir_all(&checkpoint_dir).unwrap();
        fs::write(checkpoint_dir.join("config.json"), b"corrupt").unwrap();

        let err = run_with_opf_setup(
            Args {
                safety_net: Some(SetupSafetyNet::Opf),
                policy_out: Some(policy_out.clone()),
                model_dir: Some(model_dir),
                non_interactive: true,
                force: false,
            },
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
        let checkpoint_dir = dir.path().join("privacy_filter");
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

        let resolved = resolve_safety_net(
            Some(SetupSafetyNet::Opf),
            true,
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
        assert_eq!(resolved.opf_notice, None);
        assert_eq!(
            resolved.opf_checkpoint.as_deref(),
            Some(checkpoint_dir.as_path())
        );
    }

    #[test]
    #[ignore = "hits Hugging Face; validates CLI setup with the real pinned Kiji bundle"]
    fn non_interactive_existing_model_skips_download_writes_policy_and_doctor_passes() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("kiji-distilbert");
        let first_policy = dir.path().join("first.toml");
        let second_policy = dir.path().join("second.toml");

        let first = run_with_opf_setup(
            Args {
                safety_net: Some(SetupSafetyNet::Ner),
                policy_out: Some(first_policy),
                model_dir: Some(model_dir.clone()),
                non_interactive: true,
                force: false,
            },
            default_opf_setup(),
        )
        .unwrap();
        let second = run_with_opf_setup(
            Args {
                safety_net: Some(SetupSafetyNet::Ner),
                policy_out: Some(second_policy),
                model_dir: Some(model_dir.clone()),
                non_interactive: true,
                force: false,
            },
            default_opf_setup(),
        )
        .unwrap();

        assert_eq!(first.model_status, ModelInstallStatus::Downloaded);
        assert_eq!(second.model_status, ModelInstallStatus::AlreadyPresent);
        assert_eq!(second.model_dir, model_dir);
        assert!(second.doctor_clean_text.contains(":Name_"));
        assert!(second.doctor_clean_text.contains(":Email_"));
    }
}
