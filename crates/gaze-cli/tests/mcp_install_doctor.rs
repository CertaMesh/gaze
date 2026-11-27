#![cfg(feature = "mcp")]

use assert_cmd::Command;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn mcp_install_writes_project_config_and_agents_section_idempotently() {
    let temp = tempdir().expect("tempdir");
    let agents = temp.path().join("AGENTS.md");
    let config = temp.path().join(".mcp.json");
    std::fs::write(
        &config,
        r#"{
  "mcpServers": {
    "other": {
      "command": "node",
      "args": ["server.js"],
      "env": {}
    }
  },
  "otherTopLevel": true
}"#,
    )
    .expect("seed config");

    Command::cargo_bin("gaze")
        .expect("gaze bin")
        .current_dir(temp.path())
        .args([
            "mcp",
            "install",
            "--client",
            "claude-code",
            "--agents-md",
            agents.to_str().expect("agents path"),
        ])
        .assert()
        .success();

    let first_config = std::fs::read_to_string(&config).expect("config");
    let first_agents = std::fs::read_to_string(&agents).expect("agents");
    assert_config_has_gaze_server_and_preserves_other(&first_config);
    assert_eq!(first_agents.matches("BEGIN GAZE MCP").count(), 1);

    Command::cargo_bin("gaze")
        .expect("gaze bin")
        .current_dir(temp.path())
        .args([
            "mcp",
            "install",
            "--client",
            "claude-code",
            "--agents-md",
            agents.to_str().expect("agents path"),
        ])
        .assert()
        .success();

    let second_config = std::fs::read_to_string(&config).expect("config");
    let second_agents = std::fs::read_to_string(&agents).expect("agents");
    assert_eq!(first_config, second_config);
    assert_eq!(first_agents, second_agents);

    Command::cargo_bin("gaze")
        .expect("gaze bin")
        .current_dir(temp.path())
        .args([
            "mcp",
            "doctor",
            "--agents-md",
            agents.to_str().expect("agents path"),
        ])
        .assert()
        .success();

    let output = Command::cargo_bin("gaze")
        .expect("gaze bin")
        .current_dir(temp.path())
        .args([
            "mcp",
            "doctor",
            "--json",
            "--agents-md",
            agents.to_str().expect("agents path"),
        ])
        .output()
        .expect("doctor runs");
    let checks: Value = serde_json::from_slice(&output.stdout).expect("doctor json");
    let check = |name: &str| {
        checks
            .as_array()
            .expect("checks")
            .iter()
            .find(|check| check["name"] == name)
            .unwrap_or_else(|| panic!("doctor reports {name}"))
            .clone()
    };
    let profile = check("protection profile");
    assert_eq!(profile["state"], "pass");
    assert!(profile["evidence"].as_str().unwrap().contains("[core]"));
    assert!(profile["evidence"]
        .as_str()
        .unwrap()
        .contains("no NER or Nym"));
    let scope = check("session scope");
    assert_eq!(scope["state"], "pass");
    assert!(scope["evidence"]
        .as_str()
        .unwrap()
        .contains("per stdio connection"));
}

#[test]
fn mcp_install_dry_run_does_not_write() {
    let temp = tempdir().expect("tempdir");
    let agents = temp.path().join("AGENTS.md");

    Command::cargo_bin("gaze")
        .expect("gaze bin")
        .current_dir(temp.path())
        .args([
            "mcp",
            "install",
            "--client",
            "claude-code",
            "--agents-md",
            agents.to_str().expect("agents path"),
            "--dry-run",
        ])
        .assert()
        .success();

    assert!(!temp.path().join(".mcp.json").exists());
    assert!(!agents.exists());
}

fn assert_config_has_gaze_server_and_preserves_other(config: &str) {
    let parsed: Value = serde_json::from_str(config).expect("valid json");
    assert_eq!(parsed["otherTopLevel"], true);
    assert_eq!(parsed["mcpServers"]["other"]["command"], "node");

    let gaze = &parsed["mcpServers"]["gaze"];
    let command = gaze["command"].as_str().expect("gaze command");
    assert!(!command.is_empty());
    assert!(std::path::Path::new(command).is_absolute());
    assert_eq!(gaze["args"], serde_json::json!(["mcp", "serve"]));
    assert_eq!(gaze["env"], serde_json::json!({}));
}
