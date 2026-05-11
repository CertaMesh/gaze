use std::path::{Path, PathBuf};

use pdfium_render::prelude::Pdfium;
use serde::Serialize;
use serde_json::Value;

use crate::error::CliError;

use super::install::{targets, ClientTarget};
use super::serve::default_manifest_dir;
use super::{default_agents_md, Client, DoctorArgs};

const BEGIN_MARKER: &str = "<!-- BEGIN GAZE MCP -->";

pub(crate) fn run(args: DoctorArgs) -> Result<(), CliError> {
    let mut checks = vec![
        check_binary_on_path(),
        check_tesseract(),
        check_pdfium(),
        check_manifest_dir(&default_manifest_dir().join("calls")),
    ];
    for target in targets(Client::All)? {
        checks.push(check_client(target));
    }
    checks.push(check_agents_md(
        args.agents_md.unwrap_or_else(default_agents_md),
    ));

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&checks)
                .map_err(|err| CliError::McpDetail(format!("doctor JSON failed: {err}")))?
        );
    } else {
        println!("{:<5}\t{:<28}\tevidence", "state", "check");
        for check in &checks {
            println!(
                "{:<5}\t{:<28}\t{}",
                check.state.as_str(),
                check.name,
                check.evidence
            );
        }
    }

    let has_fail = checks.iter().any(|check| check.state == CheckState::Fail);
    let has_warn = checks.iter().any(|check| check.state == CheckState::Warn);
    if has_fail || (args.strict && has_warn) {
        return Err(CliError::McpDetail(
            "one or more MCP doctor checks failed".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct Check {
    name: String,
    state: CheckState,
    evidence: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CheckState {
    Pass,
    Warn,
    Fail,
}

impl CheckState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

fn pass(name: impl Into<String>, evidence: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        state: CheckState::Pass,
        evidence: evidence.into(),
    }
}

fn warn(name: impl Into<String>, evidence: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        state: CheckState::Warn,
        evidence: evidence.into(),
    }
}

fn fail(name: impl Into<String>, evidence: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        state: CheckState::Fail,
        evidence: evidence.into(),
    }
}

fn check_binary_on_path() -> Check {
    let current = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => return fail("binary on PATH", format!("current_exe failed: {err}")),
    };
    let Some(found) = find_on_path("gaze") else {
        return warn(
            "binary on PATH",
            format!(
                "`gaze` not found on PATH; MCP configs can still use `{}`",
                current.display()
            ),
        );
    };
    if paths_match(&current, &found) {
        pass("binary on PATH", found.display().to_string())
    } else {
        warn(
            "binary on PATH",
            format!(
                "PATH resolves `{}` but current executable is `{}`",
                found.display(),
                current.display()
            ),
        )
    }
}

fn check_tesseract() -> Check {
    match std::process::Command::new("tesseract")
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => pass("tesseract", "tesseract --version exited 0"),
        Ok(output) => warn(
            "tesseract",
            format!("tesseract --version exited {}", output.status),
        ),
        Err(err) => warn(
            "tesseract",
            format!("missing or not executable; gaze_read_text still works: {err}"),
        ),
    }
}

fn check_pdfium() -> Check {
    match Pdfium::bind_to_system_library() {
        Ok(_) => pass("pdfium dynamic lib", "pdfium system library loaded"),
        Err(err) => warn(
            "pdfium dynamic lib",
            format!("missing; PDF input unavailable until installed: {err}"),
        ),
    }
}

fn check_manifest_dir(path: &Path) -> Check {
    if !path.exists() {
        return warn(
            "manifest dir",
            format!(
                "`{}` does not exist yet; serve will create it",
                path.display()
            ),
        );
    }
    if !path.is_dir() {
        return fail(
            "manifest dir",
            format!("`{}` is not a directory", path.display()),
        );
    }
    let probe = path.join(".gaze-doctor-write-test");
    match std::fs::write(&probe, b"ok").and_then(|_| std::fs::remove_file(&probe)) {
        Ok(()) => pass("manifest dir", format!("`{}` is writable", path.display())),
        Err(err) => fail(
            "manifest dir",
            format!("`{}` is not writable: {err}", path.display()),
        ),
    }
}

fn check_client(target: ClientTarget) -> Check {
    let label = target.label();
    let name = format!("{label} config");
    let path = match target.config_path() {
        Ok(path) => path,
        Err(err) => return fail(name, format!("cannot resolve config path: {err:?}")),
    };
    if !path.exists() {
        return warn(name, format!("`{}` does not exist", path.display()));
    }
    let root = match read_config(&path) {
        Ok(root) => root,
        Err(err) => return fail(name, err),
    };
    let Some(server) = root
        .get("mcpServers")
        .and_then(|servers| servers.get("gaze"))
    else {
        return warn(
            name,
            format!("`{}` has no mcpServers.gaze entry", path.display()),
        );
    };
    let Some(command) = server.get("command").and_then(Value::as_str) else {
        return fail(name, "mcpServers.gaze.command is missing or not a string");
    };
    let current = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => return fail(name, format!("current_exe failed: {err}")),
    };
    if paths_match(&current, Path::new(command)) {
        pass(name, format!("gaze entry points at `{command}`"))
    } else {
        warn(
            name,
            format!(
                "gaze entry points at `{command}`, current executable is `{}`",
                current.display()
            ),
        )
    }
}

fn check_agents_md(path: PathBuf) -> Check {
    match std::fs::read_to_string(&path) {
        Ok(content) if content.contains(BEGIN_MARKER) => pass(
            "AGENTS.md skill section",
            format!("`{}` contains Gaze MCP section", path.display()),
        ),
        Ok(_) => warn(
            "AGENTS.md skill section",
            format!("`{}` lacks Gaze MCP section", path.display()),
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => warn(
            "AGENTS.md skill section",
            format!("`{}` does not exist", path.display()),
        ),
        Err(err) => fail(
            "AGENTS.md skill section",
            format!("cannot read `{}`: {err}", path.display()),
        ),
    }
}

fn read_config(path: &Path) -> Result<Value, String> {
    let bytes =
        std::fs::read(path).map_err(|err| format!("cannot read `{}`: {err}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("cannot parse `{}`: {err}", path.display()))
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{binary}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn paths_match(a: &Path, b: &Path) -> bool {
    let a = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let b = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    a == b
}
