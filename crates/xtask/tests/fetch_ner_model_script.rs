#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempTree(PathBuf);

impl TempTree {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "gaze-fetch-ner-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn fetch_ner_model_resolves_repository_root_from_nested_script_dir() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf();
    let temp = TempTree::new();
    let fake_bin = temp.0.join("bin");
    let destination = temp.0.join("model");
    fs::create_dir(&fake_bin).unwrap();

    write_executable(
        &fake_bin.join("curl"),
        r#"#!/usr/bin/env bash
set -eu
output=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) output="$2"; shift 2 ;;
    *) shift ;;
  esac
done
if [ -n "$output" ]; then
  : > "$output"
fi
"#,
    );
    write_executable(&fake_bin.join("shasum"), "#!/usr/bin/env bash\nexit 0\n");

    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![fake_bin];
    paths.extend(std::env::split_paths(&original_path));
    let path = std::env::join_paths(paths).unwrap();
    let output = Command::new("bash")
        .arg(repo_root.join("scripts/fetch/fetch-ner-model.sh"))
        .args(["--gaze-version", "v0.0.0"])
        .arg(&destination)
        .env("PATH", path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "fetch script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(destination.join("labels.json").is_file());
    assert!(repo_root.join("Cargo.toml").is_file());
}
