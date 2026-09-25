//! Runs one ignored test of the current test binary in a child process and returns its whole
//! stderr, so a test can pin every byte the proxy writes to its log. libtest captures
//! `eprintln!` in-process, so only a `--nocapture` child exposes a real stderr.
use std::process::Command;

pub fn stderr_of(child_test: &str) -> String {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            child_test,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stdout}\n{stderr}");
    // A filter that matches nothing also exits 0; require the child to have run and passed.
    assert!(
        stdout.contains(&format!("test {child_test} ... ok")),
        "{stdout}"
    );
    assert!(stdout.contains("test result: ok. 1 passed;"), "{stdout}");
    stderr
}
