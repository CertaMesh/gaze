use super::{
    configured_command, MatrixCommand, CORE_NO_PHONE_PARSER_LOAD_GUARD, FEATURE_MATRIX,
    REQUIRED_EMBEDDED_CORE_LOAD_RESULT,
};
use std::fs;
use std::process::Command;

#[test]
fn cli_preserves_full_local_matrix_and_rejects_unknown_partitions() {
    use super::Partition;
    use crate::{Cli, Command as XtaskCommand};
    use clap::Parser;

    let cli = Cli::try_parse_from(["xtask", "ci-feature-matrix"]).unwrap();
    assert!(matches!(
        cli.command,
        XtaskCommand::CiFeatureMatrix {
            partition: Partition::Full
        }
    ));
    assert!(
        Cli::try_parse_from(["xtask", "ci-feature-matrix", "--partition", "skip-all"]).is_err()
    );
}

#[test]
fn ci_partitions_preserve_every_local_command_with_one_owner() {
    use super::{ci_owner, partition_commands, CiOwner, Partition};

    let full: Vec<_> = partition_commands(Partition::Full).collect();
    assert_eq!(full, FEATURE_MATRIX.iter().collect::<Vec<_>>());
    let defaults: Vec<_> = partition_commands(Partition::CiDefault).collect();
    assert_eq!(defaults.len(), 1);
    assert_eq!(
        defaults[0].args,
        ["test", "--workspace", "--lib", "--bins", "--tests"]
    );
    let test_owned: Vec<_> = FEATURE_MATRIX
        .iter()
        .filter(|command| ci_owner(command) == CiOwner::Test)
        .map(|command| command.label)
        .collect();
    assert_eq!(
        test_owned,
        [
            "cargo fmt --all -- --check",
            "cargo clippy --workspace --all-features --all-targets -- -D warnings",
            "cargo test -p gaze-document --features mcp",
            "cargo test -p gaze-cli --features mcp",
            "cargo test --workspace --all-features",
        ]
    );
    let gates: Vec<_> = partition_commands(Partition::CiGates).collect();
    assert!(gates.contains(&&CORE_NO_PHONE_PARSER_LOAD_GUARD));
    assert!(gates
        .iter()
        .any(|command| command.args.contains(&"nym_no_feature")));
    for command in full {
        let owner_count = usize::from(gates.contains(&command))
            + usize::from(defaults.contains(&command))
            + usize::from(ci_owner(command) == CiOwner::Test);
        assert_eq!(owner_count, 1, "coverage owner for {}", command.label);
    }
}

#[test]
fn matrix_roster_requires_the_no_phone_parser_core_load() {
    assert!(FEATURE_MATRIX.contains(&CORE_NO_PHONE_PARSER_LOAD_GUARD));
    assert_eq!(
        CORE_NO_PHONE_PARSER_LOAD_GUARD.args,
        ["test", "-p", "gaze-pii", "--no-default-features", "--lib"]
    );
    assert_eq!(
        REQUIRED_EMBEDDED_CORE_LOAD_RESULT,
        "embedded_core_loads_without_phone_parser ... ok"
    );
}

#[test]
fn matrix_build_environment_survives_nested_cargo() {
    // Set hostile caller values in a subprocess, never in the parallel test runner.
    let cargo_home = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "ci_feature_matrix::tests::matrix_environment_child",
            "--nocapture",
        ])
        .env("CARGO_HOME", cargo_home.path())
        .env("GAZE_MATRIX_ENV_PROBE", "1")
        .env("CARGO_INCREMENTAL", "1")
        .env("CARGO_PROFILE_DEV_DEBUG", "2")
        .env("CARGO_PROFILE_TEST_DEBUG", "2")
        .env("CARGO_PROFILE_DEV_OPT_LEVEL", "3")
        .env("CARGO_PROFILE_TEST_DEBUG_ASSERTIONS", "false")
        .env("CARGO_TARGET_DIR", "hostile-target")
        .env("RUSTFLAGS", "--invalid-caller-rustflag")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "matrix environment probe failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("matrix nested environment verified"));
}

#[test]
fn matrix_environment_child() {
    if std::env::var_os("GAZE_MATRIX_ENV_PROBE").is_none() {
        return;
    }
    let fixture = tempfile::tempdir().unwrap();
    fs::create_dir(fixture.path().join("src")).unwrap();
    fs::write(
        fixture.path().join("Cargo.toml"),
        "[package]\nname = \"matrix-env-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        fixture.path().join("build.rs"),
        r#"fn main() {
    assert_eq!(std::env::var("DEBUG").unwrap(), "false");
    assert_eq!(std::env::var("OPT_LEVEL").unwrap(), "0");
    for key in ["CARGO_INCREMENTAL", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG"] {
        assert_eq!(std::env::var(key).unwrap(), "0", "{key}");
    }
}"#,
    )
    .unwrap();
    fs::write(
        fixture.path().join("src/main.rs"),
        r#"fn main() {
    for key in ["CARGO_INCREMENTAL", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG"] {
        assert_eq!(std::env::var(key).unwrap(), "0", "{key}");
    }
    assert!(cfg!(debug_assertions));
    for key in ["RUSTFLAGS", "GAZE_MATRIX_ENV_PROBE", "CARGO_PROFILE_DEV_OPT_LEVEL", "CARGO_PROFILE_TEST_DEBUG_ASSERTIONS"] {
        assert!(std::env::var_os(key).is_none(), "caller value leaked: {key}");
    }
    println!("matrix nested environment verified");
}
#[test]
fn nested_build_inherits_matrix_environment() {
    main();
    // Model a fresh adversarial target without sharing the outer build artifacts.
    let output = std::process::Command::new(std::env::var_os("CARGO").unwrap())
        .args(["run", "--offline", "--target-dir", "target/nested", "-j4"])
        .env_remove("CARGO_TARGET_DIR")
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("matrix nested environment verified"));
}"#,
    )
    .unwrap();
    let output = configured_command(MatrixCommand {
        label: "matrix environment fixture",
        program: "cargo",
        args: &[
            "test",
            "--offline",
            "--target-dir",
            "target",
            "-j4",
            "--",
            "--nocapture",
        ],
    })
    .unwrap()
    .current_dir(fixture.path())
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("matrix nested environment verified"));
    assert!(fixture
        .path()
        .join("target/nested/debug/matrix-env-probe")
        .with_extension(std::env::consts::EXE_EXTENSION)
        .exists());
    assert!(!fixture.path().join("hostile-target").exists());
    println!("matrix nested environment verified");
}
