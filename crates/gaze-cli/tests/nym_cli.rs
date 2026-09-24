//! `gaze clean --safety-net nym`: configuration fails closed, and (live) the net protects a plate.
#![cfg(feature = "safety-net-nym")]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::{tempdir, TempDir};

const PLATE_PROSE: &str = "Das Fahrzeug mit dem Kennzeichen M-AB 1234 wurde abgeschleppt.";

fn policy(extra: &str) -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    fs::write(
        &path,
        format!(
            r#"[session]
scope = "conversation"

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "default"
action = "tokenize"
{extra}"#
        ),
    )
    .unwrap();
    (dir, path)
}

fn clean(args: &[&str], input: &str) -> std::process::Output {
    Command::cargo_bin("gaze")
        .unwrap()
        .arg("clean")
        .args(args)
        .env_remove("GAZE_NYM_MODEL_DIR")
        .write_stdin(input.as_bytes().to_vec())
        .output()
        .unwrap()
}

fn stderr_json(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stderr)
        .unwrap_or_else(|_| panic!("stderr={}", String::from_utf8_lossy(&out.stderr)))
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn nym_without_a_model_dir_is_a_config_error() {
    let (_dir, policy) = policy("");
    let out = clean(
        &["--policy", path_str(&policy), "--safety-net", "nym"],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = stderr_json(&out);
    assert_eq!(stderr["error"], "SafetyNetConfig");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("M-AB"));
}

#[test]
fn nym_with_an_incomplete_bundle_fails_before_loading() {
    let (_dir, policy) = policy("");
    let bundle = tempdir().unwrap();
    let out = clean(
        &[
            "--policy",
            path_str(&policy),
            "--safety-net",
            "nym",
            "--nym-model-dir",
            path_str(bundle.path()),
        ],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_eq!(stderr_json(&out)["error"], "SafetyNetArtifactMissing");
}

#[test]
fn nym_policy_table_without_backend_does_not_activate() {
    let (_dir, policy) = policy(
        "\n[safety_net.nym]\nlabels = [\"LICENSE_PLATE\"]\nthreshold = { LICENSE_PLATE = 0.5 }\n",
    );
    let out = clean(&["--policy", path_str(&policy)], PLATE_PROSE);
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}

#[test]
fn policy_backend_nym_requires_a_bundle_without_cli_flags() {
    let (_dir, policy) = policy("\n[safety_net]\nbackend = \"nym\"\n");
    let out = clean(&["--policy", path_str(&policy)], PLATE_PROSE);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_eq!(stderr_json(&out)["error"], "SafetyNetConfig");
    assert!(stderr_json(&out)["detail"]
        .as_str()
        .unwrap()
        .contains("gaze setup --safety-net nym"));
}

#[test]
fn command_line_none_replaces_policy_nym_with_notice() {
    let (_dir, policy) = policy("\n[safety_net]\nbackend = \"nym\"\n");
    let out = clean(
        &["--policy", path_str(&policy), "--safety-net", "none"],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("disabled policy safety net nym"));
    assert!(!out.stdout.is_empty());
}

#[test]
fn none_disables_policy_nym_even_with_an_unused_model_path_flag() {
    let (_dir, policy) = policy("\n[safety_net]\nbackend = \"nym\"\n");
    let out = clean(
        &[
            "--policy",
            path_str(&policy),
            "--safety-net",
            "none",
            "--nym-model-dir",
            "/nonexistent/nym-bundle",
        ],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("disabled policy safety net nym"));
}

#[test]
fn none_cannot_be_combined_with_another_safety_net() {
    let (_dir, policy) = policy("");
    let out = clean(
        &[
            "--policy",
            path_str(&policy),
            "--safety-net",
            "none",
            "--safety-net",
            "nym",
        ],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stderr_json(&out)["error"], "SafetyNetUsage");
}

#[test]
fn repeatable_safety_net_list_activates_nym() {
    let (_dir, policy) = policy("");
    let out = clean(
        &[
            "--policy",
            path_str(&policy),
            "--safety-net",
            "nym",
            "--safety-net",
            "openai-filter",
        ],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stderr_json(&out)["error"], "SafetyNetConfig");
    assert!(stderr_json(&out)["detail"]
        .as_str()
        .unwrap()
        .contains("model_dir"));
}

#[test]
fn backend_selector_requires_one_explicit_safety_net() {
    let (_dir, policy) = policy("");
    for values in [
        vec!["--safety-net-backend", "nym"],
        vec![
            "--safety-net",
            "nym",
            "--safety-net",
            "openai-filter",
            "--safety-net-backend",
            "nym",
        ],
    ] {
        let mut args = vec!["--policy", path_str(&policy)];
        args.extend(values);
        let out = clean(&args, PLATE_PROSE);
        assert_eq!(out.status.code(), Some(2));
        assert_eq!(stderr_json(&out)["error"], "SafetyNetUsage");
    }
}

#[test]
fn policy_rejects_opf_activation_at_load() {
    let (_dir, policy) = policy("\n[safety_net]\nbackend = \"openai-filter\"\n");
    let out = clean(&["--policy", path_str(&policy)], PLATE_PROSE);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stderr_json(&out)["error"], "PolicyConfig");
    assert!(stderr_json(&out)["detail"]
        .as_str()
        .unwrap()
        .contains("command-line only"));
}

#[test]
fn policy_nym_digest_mismatch_is_config_error() {
    let bundle = tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bundle.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let checksum = bundle.path().join("SHA256SUMS");
    for name in [
        "SHA256SUMS",
        "config.json",
        "model_int8.onnx",
        "tokenizer.json",
    ] {
        let file = bundle.path().join(name);
        fs::write(&file, b"invalid bundle").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    assert!(checksum.exists());
    let (_dir, policy) = policy(&format!(
        "\n[safety_net]\nbackend = \"nym\"\n[safety_net.nym]\nmodel_dir = {:?}\n",
        bundle.path().to_str().unwrap()
    ));
    let out = clean(&["--policy", path_str(&policy)], PLATE_PROSE);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stderr_json(&out)["error"], "SafetyNetConfig");
    assert!(
        stderr_json(&out)["detail"]
            .as_str()
            .unwrap()
            .contains("integrity"),
        "{:?}",
        stderr_json(&out)
    );
}

#[test]
fn cli_model_dir_precedes_environment_and_policy() {
    let dir = tempdir().unwrap();
    let policy_dir = dir.path().join("policy-bundle");
    let env_dir = dir.path().join("env-bundle");
    let cli_dir = dir.path().join("cli-bundle");
    let (_policy_dir, policy) = policy(&format!(
        "\n[safety_net]\nbackend = \"nym\"\n[safety_net.nym]\nmodel_dir = {:?}\n",
        policy_dir.to_str().unwrap()
    ));
    let run = |env_path: Option<&Path>, cli_path: Option<&Path>| {
        let mut command = Command::cargo_bin("gaze").unwrap();
        command.arg("clean").args(["--policy", path_str(&policy)]);
        if let Some(path) = cli_path {
            command.args(["--nym-model-dir", path_str(path)]);
        }
        if let Some(path) = env_path {
            command.env("GAZE_NYM_MODEL_DIR", path);
        } else {
            command.env_remove("GAZE_NYM_MODEL_DIR");
        }
        command
            .write_stdin(PLATE_PROSE.as_bytes().to_vec())
            .output()
            .unwrap()
    };
    for (env_path, cli_path, expected) in [
        (
            Some(env_dir.as_path()),
            Some(cli_dir.as_path()),
            "cli-bundle",
        ),
        (Some(env_dir.as_path()), None, "env-bundle"),
        (None, None, "policy-bundle"),
    ] {
        let out = run(env_path, cli_path);
        assert_eq!(out.status.code(), Some(2));
        assert!(stderr_json(&out)["path"]
            .as_str()
            .unwrap()
            .contains(expected));
    }
}

#[test]
fn nym_policy_with_an_unmapped_label_fails_at_load() {
    let (_dir, policy) =
        policy("\n[safety_net.nym]\nlabels = [\"GIVEN_NAME\"]\nthreshold = { GIVEN_NAME = 0.5 }\n");
    let out = clean(
        &["--policy", path_str(&policy), "--safety-net", "nym"],
        PLATE_PROSE,
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("GIVEN_NAME"));
}

#[test]
fn nym_is_refused_through_the_registry() {
    let (_dir, policy) = policy("");
    let out = clean(
        &[
            "--policy",
            path_str(&policy),
            "--safety-net-registry",
            "--safety-net-add",
            "nym",
        ],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--safety-net-registry"));
}

#[test]
fn nym_flags_without_a_net_are_refused() {
    let (_dir, policy) = policy("");
    let out = clean(
        &["--policy", path_str(&policy), "--nym-intra-threads", "2"],
        PLATE_PROSE,
    );
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
}

/// The default safety net is unchanged: a bundle location in the environment never activates Nym.
/// The directory is empty, so loading the net would fail; a default run (with and without a
/// policy) must not touch it.
#[test]
fn default_runs_never_load_nym_even_with_a_bundle_dir_set() {
    let (_dir, policy) = policy("");
    let empty_bundle = tempdir().unwrap();
    for args in [vec!["--policy", path_str(&policy)], vec![]] {
        let out = Command::cargo_bin("gaze")
            .unwrap()
            .arg("clean")
            .args(&args)
            .env("GAZE_NYM_MODEL_DIR", empty_bundle.path())
            .write_stdin(PLATE_PROSE.as_bytes().to_vec())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {stderr}");
        assert!(!out.stdout.is_empty(), "{args:?}");
        assert!(!stderr.contains("nym"), "{args:?}: {stderr}");
    }
}

#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR pointing at the pinned bundle"]
fn live_nym_net_tokenizes_a_plate_the_rules_miss() {
    let model_dir = std::env::var("GAZE_NYM_MODEL_DIR").expect("GAZE_NYM_MODEL_DIR");
    let (_dir, policy) = policy("");

    let baseline = clean(&["--policy", path_str(&policy)], PLATE_PROSE);
    assert_eq!(baseline.status.code(), Some(0));
    let baseline: Value = serde_json::from_slice(&baseline.stdout).unwrap();
    assert!(
        baseline["clean_text"]
            .as_str()
            .unwrap()
            .contains("M-AB 1234"),
        "the rule floor alone must miss the plate for this test to mean anything"
    );

    let out = clean(
        &[
            "--policy",
            path_str(&policy),
            "--safety-net",
            "nym",
            "--nym-model-dir",
            &model_dir,
        ],
        PLATE_PROSE,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let clean_text = json["clean_text"].as_str().unwrap();
    assert!(!clean_text.contains("M-AB"), "{clean_text}");
    assert!(clean_text.contains("Kennzeichen <"), "{clean_text}");
    let report = json["leak_report"].to_string();
    assert!(report.contains("nym-small-int8"), "{report}");
    assert!(report.contains("LICENSE_PLATE>=0.5"), "{report}");
}

#[test]
#[ignore = "needs GAZE_NYM_MODEL_DIR pointing at the pinned bundle"]
fn policy_nym_model_dir_activates_live_bundle() {
    let model_dir = std::env::var("GAZE_NYM_MODEL_DIR").expect("GAZE_NYM_MODEL_DIR");
    let (_dir, policy) = policy(&format!(
        "\n[safety_net]\nbackend = \"nym\"\n[safety_net.nym]\nmodel_dir = {:?}\n",
        model_dir
    ));
    let out = clean(&["--policy", path_str(&policy)], PLATE_PROSE);
    assert_eq!(out.status.code(), Some(0));
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!result["clean_text"].as_str().unwrap().contains("M-AB"));
    assert!(result["leak_report"].to_string().contains("nym-small-int8"));
}
