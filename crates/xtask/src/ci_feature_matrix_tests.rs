use super::{configured_command, MatrixCommand};
use std::fs;
use std::process::Command;

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
