#![cfg(feature = "mcp")]

use std::ffi::OsString;

use assert_cmd::Command;
use tempfile::tempdir;

fn doctor_output(envs: &[(&str, &str)]) -> String {
    let temp = tempdir().expect("tempdir");
    let exe = assert_cmd::cargo::cargo_bin("gaze");
    let exe_dir = exe.parent().expect("gaze bin parent");
    let agents_dir = temp.path().join("agents-dir");
    std::fs::create_dir(&agents_dir).expect("agents dir");

    let mut path = OsString::from(exe_dir);
    if let Some(existing_path) = std::env::var_os("PATH") {
        path.push(":");
        path.push(existing_path);
    }

    let mut command = Command::new(exe);
    command
        .current_dir(temp.path())
        .args([
            "mcp",
            "doctor",
            "--agents-md",
            agents_dir.to_str().expect("agents path"),
        ])
        .env("PATH", path)
        .env(
            "GAZE_MCP_CLAUDE_DESKTOP_CONFIG",
            temp.path().join("missing-claude-desktop.json"),
        )
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE");

    for (name, value) in envs {
        command.env(name, value);
    }

    let output = command.output().expect("doctor output");
    assert_eq!(
        output.status.code(),
        Some(6),
        "doctor should fail only because the test passes a directory as AGENTS.md; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("doctor stdout utf8")
}

fn assert_state_words(output: &str) {
    assert!(output.contains("pass"), "missing pass state: {output}");
    assert!(output.contains("warn"), "missing warn state: {output}");
    assert!(output.contains("fail"), "missing fail state: {output}");
}

#[test]
fn mcp_doctor_human_output_colors_state_when_forced() {
    let output = doctor_output(&[("CLICOLOR_FORCE", "1")]);

    assert!(
        output.contains("\x1b["),
        "forced-color doctor output should contain ANSI escapes: {output}"
    );
    assert_state_words(&output);
}

#[test]
fn mcp_doctor_human_output_suppresses_color_when_no_color_is_set() {
    let output = doctor_output(&[("CLICOLOR_FORCE", "1"), ("NO_COLOR", "1")]);

    assert!(
        !output.contains("\x1b["),
        "NO_COLOR must suppress ANSI escapes even with CLICOLOR_FORCE: {output}"
    );
    assert_state_words(&output);
}

#[test]
fn mcp_doctor_human_output_suppresses_color_when_stdout_is_not_tty() {
    let output = doctor_output(&[]);

    assert!(
        !output.contains("\x1b["),
        "piped doctor output must not contain ANSI escapes: {output}"
    );
    assert_state_words(&output);
}

#[test]
fn mcp_doctor_json_output_is_never_colored() {
    let temp = tempdir().expect("tempdir");
    let output = Command::cargo_bin("gaze")
        .expect("gaze bin")
        .current_dir(temp.path())
        .args(["mcp", "doctor", "--json"])
        .env("CLICOLOR_FORCE", "1")
        .output()
        .expect("doctor json output");

    assert!(
        output.status.success(),
        "doctor --json should succeed without injected fail; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("doctor stdout utf8");
    assert!(
        !stdout.contains("\x1b["),
        "doctor JSON output must not contain ANSI escapes: {stdout}"
    );
    assert!(stdout.contains("\"state\""));
}
