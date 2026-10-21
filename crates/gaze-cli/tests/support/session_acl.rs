use super::{assert_session_file_error, file_mode, Command};
use std::fs;
use std::path::Path;

fn run(overwrite: bool, fault: Option<&str>) {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("inherited");
    fs::create_dir(&parent).unwrap();
    let chmod = std::process::Command::new("chmod")
        .args(["+a", "everyone allow read,file_inherit"])
        .arg(&parent)
        .output()
        .unwrap();
    assert!(chmod.status.success(), "chmod: {chmod:?}");

    // Prove this filesystem actually reproduces the reviewer's inherited grant.
    let control = parent.join("control");
    fs::write(&control, b"").unwrap();
    assert!(acl(&control).contains("inherited allow read"));
    fs::remove_file(control).unwrap();

    let library = root.path().join("probe.dylib");
    let build = std::process::Command::new("cc")
        .args(["-dynamiclib", "-Wall", "-Wextra", "-Werror"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/session_acl_probe.c"
        ))
        .arg("-o")
        .arg(&library)
        .output()
        .unwrap();
    assert!(build.status.success(), "cc: {build:?}");

    let session = parent.join("owner.json");
    if overwrite {
        fs::write(&session, b"keep existing session").unwrap();
    }
    let log = root.path().join("calls.log");
    let mut command = Command::cargo_bin("gaze").unwrap();
    command.args(["clean", "--session-out", session.to_str().unwrap()]);
    if overwrite {
        command.arg("--overwrite-session-out");
    }
    command
        .env("DYLD_INSERT_LIBRARIES", &library)
        .env("GAZE_ACL_PROBE_LOG", &log);
    if let Some(fault) = fault {
        command.env(fault, "1");
    }
    let result = command
        .write_stdin(b"alice@example.invalid".to_vec())
        .output()
        .unwrap();
    let calls = fs::read_to_string(log).unwrap_or_default();
    if fault.is_some() {
        assert_session_file_error(&result, "cannot write session file");
        if overwrite {
            assert_eq!(fs::read(&session).unwrap(), b"keep existing session");
        } else {
            assert!(!session.exists(), "unsafe empty file must be removed");
        }
        assert_eq!(
            fs::read_dir(&parent).unwrap().count(),
            usize::from(overwrite)
        );
        assert!(calls.starts_with("clear-before-write\n"), "calls={calls:?}");
    } else {
        assert!(result.status.success(), "result={result:?}");
        assert_eq!(file_mode(&session), 0o600);
        assert_eq!(acl(&session).lines().count(), 1, "no extended ACL entries");
        assert_eq!(calls, "clear-before-write\nverify-before-write\n");
        assert!(!String::from_utf8_lossy(&result.stdout).contains("alice@example.invalid"));
        let restored = super::gaze_with_stdin(
            &["restore", "--session-file", session.to_str().unwrap()],
            &result.stdout,
        );
        assert!(restored.status.success());
        assert_eq!(restored.stdout, b"alice@example.invalid");
    }
}

fn acl(path: &Path) -> String {
    let output = std::process::Command::new("ls")
        .arg("-le")
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn session_out_clears_inherited_acl_before_writing() {
    run(false, None);
}

#[test]
fn session_out_overwrite_clears_inherited_acl_before_writing() {
    run(true, None);
}

#[test]
fn session_out_acl_failures_remove_empty_file_and_preserve_existing_session() {
    for overwrite in [false, true] {
        for fault in [
            "GAZE_ACL_PROBE_DENY_CLEAR",
            "GAZE_ACL_PROBE_DENY_VERIFY",
            "GAZE_ACL_PROBE_KEEP_ACL",
        ] {
            run(overwrite, Some(fault));
        }
    }
}
