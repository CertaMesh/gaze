#[cfg(target_os = "macos")]
use std::env;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
fn main() {
    let Ok(target) = env::var("TARGET") else {
        return;
    };
    if !target.contains("apple-darwin") {
        return;
    }

    if let Some(dir) = clang_runtime_dir() {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn clang_runtime_dir() -> Option<PathBuf> {
    let output = Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let resource_dir = String::from_utf8(output.stdout).ok()?;
    let candidate = PathBuf::from(resource_dir.trim())
        .join("lib")
        .join("darwin");
    if candidate.join("libclang_rt.osx.a").is_file() {
        Some(candidate)
    } else {
        None
    }
}
