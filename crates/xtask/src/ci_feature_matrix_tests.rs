use super::{
    configured_command, library_graph_commands, MatrixCommand, CORE_NO_PHONE_PARSER_LOAD_GUARD,
    DEFAULT_WORKSPACE_SUITE, FEATURE_GRAPH_PROBES, FEATURE_GRAPH_PROBE_PACKAGE, FEATURE_MATRIX,
    REQUIRED_EMBEDDED_CORE_LOAD_RESULT,
};
use std::fs;
use std::process::Command;

#[test]
fn readme_version_check_is_owned_by_feature_gates() {
    use super::{partition_commands, Partition};

    let command = &["run", "-p", "xtask", "--", "readme-version-check"];
    assert!(partition_commands(Partition::CiGates).any(|entry| entry.args == command));
    assert!(!partition_commands(Partition::CiDefault).any(|entry| entry.args == command));
}

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
        [
            "test",
            "--config",
            "build.rustflags=['-D','warnings']",
            "--workspace",
            "--lib",
            "--bins",
            "--tests"
        ]
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
fn ci_gates_partition_runs_each_xtask_gate_once_with_its_full_argv() {
    use super::{partition_commands, Partition};

    let expected = [
        "bundle-tokenization-drift --verify-ack",
        "cargo-metadata-audit-isolation",
        "class-map-override-safety",
        "fixture-citation-lint",
        "trybuild-fixture-hygiene",
        "family-policy-table-coherence",
        "locale-cue-bundle-coherence",
        "readme-version-check",
        "dashboard-isolation",
        "mcp-tier-isolation",
        "no-tenant-knowledge",
        "safety-net-sanity",
        "tokenbridge-no-raw-index",
        "tokenbridge-encrypted-index",
        "symmetric-potemkin",
        "recognizer-composition-validator",
    ]
    .map(|gate| {
        ["run", "-p", "xtask", "--"]
            .into_iter()
            .chain(gate.split_whitespace())
            .collect::<Vec<_>>()
    });
    let actual: Vec<_> = partition_commands(Partition::CiGates)
        .filter(|command| command.args.starts_with(&["run", "-p", "xtask", "--"]))
        .map(|command| {
            assert_eq!(command.program, "cargo");
            assert_eq!(command.label, format!("cargo {}", command.args.join(" ")));
            command.args.to_vec()
        })
        .collect();
    assert_eq!(actual, expected);
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
        .env("CARGO_BUILD_JOBS", "2")
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
    assert_eq!(std::env::var("CARGO_BUILD_JOBS").unwrap(), "2");
    assert_eq!(std::env::var("GAZE_DISABLE_OS_KEYSTORE").unwrap(), "1");
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
    assert_eq!(std::env::var("CARGO_BUILD_JOBS").unwrap(), "2");
    assert_eq!(std::env::var("GAZE_DISABLE_OS_KEYSTORE").unwrap(), "1");
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

fn published(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn library_graphs_check_every_published_crate_both_graphs_with_denied_warnings() {
    let crates = published(&["gaze-types", "gaze-pii", "gaze-assembly", "gaze-cli"]);
    let commands = library_graph_commands(&crates).unwrap();
    let checks: Vec<_> = commands
        .iter()
        .filter(|command| command.args[0] == "check")
        .collect();
    assert_eq!(checks.len(), crates.len() * 2);
    for name in &crates {
        for graph in [None, Some("--no-default-features")] {
            let mut expected = vec!["check", "--locked", "-p", name.as_str()];
            expected.extend(graph);
            let command = checks
                .iter()
                .find(|command| command.args == expected)
                .unwrap_or_else(|| panic!("missing {expected:?}"));
            assert_eq!(command.rustflags, Some("-D warnings"));
            assert_eq!(command.required_output, None);
            assert_eq!(
                command.label,
                format!("RUSTFLAGS='-D warnings' cargo {}", expected.join(" "))
            );
        }
    }
}

#[test]
fn library_graph_commands_never_select_dev_dependency_targets() {
    // Tests, examples and benches pull dev-dependencies into the graph and let
    // feature unification hide a broken library graph.
    let crates = published(&["gaze-types", "gaze-pii", "gaze-assembly"]);
    for command in library_graph_commands(&crates).unwrap() {
        for forbidden in [
            "test",
            "--tests",
            "--all-targets",
            "--examples",
            "--example",
            "--benches",
            "--workspace",
            "--all-features",
        ] {
            assert!(
                !command.args.iter().any(|arg| arg == forbidden),
                "{} selects {forbidden}",
                command.label
            );
        }
        assert!(command.args.iter().any(|arg| arg == "--locked"));
        assert!(
            matches!(command.args[0].as_str(), "check" | "run"),
            "{}",
            command.label
        );
    }
}

#[test]
fn library_graph_probes_run_the_probe_bin_with_exactly_one_graph_feature() {
    let crates = published(&["gaze-types", "gaze-pii", "gaze-assembly"]);
    let probes: Vec<_> = library_graph_commands(&crates)
        .unwrap()
        .into_iter()
        .filter(|command| command.args[0] == "run")
        .collect();
    assert_eq!(probes.len(), FEATURE_GRAPH_PROBES.len());
    for (command, probe) in probes.iter().zip(FEATURE_GRAPH_PROBES) {
        assert_eq!(
            command.args,
            [
                "run",
                "--locked",
                "-p",
                FEATURE_GRAPH_PROBE_PACKAGE,
                "--no-default-features",
                "--features",
                probe.feature,
            ]
        );
        assert_eq!(command.required_output, Some(probe.required_output));
        assert_eq!(command.rustflags, None);
    }
    let all = library_graph_commands(&crates).unwrap();
    let last_probe = all.iter().rposition(|command| command.args[0] == "run");
    let first_check = all.iter().position(|command| command.args[0] == "check");
    assert!(
        last_probe < first_check,
        "cheap probes must run before checks"
    );
    // The historical break: the bundled core rulepack must load with
    // gaze-types/phone-parser off, and the probe must prove it was off.
    assert!(FEATURE_GRAPH_PROBES
        .iter()
        .any(|probe| probe.library == "gaze-pii"
            && probe.feature == "gaze-pii-no-default"
            && probe
                .required_output
                .ends_with("core rulepack loaded, phone-parser=off ok")));
    assert!(FEATURE_GRAPH_PROBES
        .iter()
        .any(|probe| probe.library == "gaze-pii"
            && probe.required_output.ends_with("phone-parser=on ok")));
}

#[test]
fn library_graph_probes_must_target_published_crates() {
    let err = library_graph_commands(&published(&["gaze-types", "gaze-pii"]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("unpublished crate gaze-assembly"), "{err}");
    assert!(library_graph_commands(&[]).is_err());
}

#[test]
fn library_graph_probe_features_exist_in_the_probe_manifest() {
    let manifest: toml::Value = toml::from_str(
        &fs::read_to_string(
            crate::repo::repo_root()
                .unwrap()
                .join("crates/feature-graph-probe/Cargo.toml"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest["package"]["name"].as_str(),
        Some(FEATURE_GRAPH_PROBE_PACKAGE)
    );
    assert_eq!(manifest["package"]["publish"].as_bool(), Some(false));
    assert!(
        manifest.get("dev-dependencies").is_none(),
        "the probe must not carry dev-dependencies"
    );
    let features = manifest["features"].as_table().unwrap();
    assert_eq!(features["default"].as_array().unwrap().len(), 0);
    for probe in FEATURE_GRAPH_PROBES {
        assert!(features.contains_key(probe.feature), "{}", probe.feature);
    }
}

#[test]
fn library_graph_plan_covers_the_real_publish_plan() {
    let root = crate::repo::repo_root().unwrap();
    let crates = crate::publish_plan::published_package_names(&root).unwrap();
    assert!(!crates
        .iter()
        .any(|name| name == FEATURE_GRAPH_PROBE_PACKAGE));
    let commands = library_graph_commands(&crates).unwrap();
    assert_eq!(
        commands.len(),
        crates.len() * 2 + FEATURE_GRAPH_PROBES.len()
    );
    for name in ["gaze-pii", "gaze-recognizers", "gaze-types", "gaze-cli"] {
        assert!(crates.iter().any(|published| published == name), "{name}");
    }
}

fn configured_rustflags(command: MatrixCommand) -> Option<String> {
    configured_command(command)
        .unwrap()
        .get_envs()
        .find(|(key, _)| *key == "RUSTFLAGS")
        .and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
}

#[test]
fn default_workspace_suite_denies_warnings_and_other_commands_do_not() {
    use super::{partition_commands, Partition};

    let defaults: Vec<_> = partition_commands(Partition::CiDefault).collect();
    assert_eq!(defaults, [&DEFAULT_WORKSPACE_SUITE]);
    let args = DEFAULT_WORKSPACE_SUITE.args;
    let config = args
        .iter()
        .position(|arg| *arg == "--config")
        .expect("default suite passes --config");
    assert_eq!(args[config + 1], "build.rustflags=['-D','warnings']");
    assert!(args.contains(&"--workspace"));
    for command in FEATURE_MATRIX {
        // Test binaries inherit the environment; RUSTFLAGS there would leak
        // `-D warnings` into nested cargo/trybuild builds inside tests.
        assert_eq!(configured_rustflags(*command), None, "{}", command.label);
        if *command != DEFAULT_WORKSPACE_SUITE {
            assert!(
                !command.args.contains(&"build.rustflags=['-D','warnings']"),
                "{}",
                command.label
            );
        }
    }
}
