use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use directories_next::BaseDirs;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::error::CliError;

use super::{default_agents_md, Client, InstallArgs};

const BEGIN_MARKER: &str = "<!-- BEGIN GAZE MCP -->";
const END_MARKER: &str = "<!-- END GAZE MCP -->";

pub(crate) fn run(args: InstallArgs) -> Result<(), CliError> {
    let exe = std::env::current_exe()
        .map_err(|err| CliError::McpDetail(format!("cannot resolve current executable: {err}")))?;
    let targets = targets(args.client)?;
    let mut summaries = Vec::new();
    for target in targets {
        summaries.push(install_client(target, &exe, args.dry_run)?);
    }

    let agents_path = args.agents_md.unwrap_or_else(default_agents_md);
    let agents_summary = if args.skip_agents_md {
        None
    } else {
        Some(update_agents_md(&agents_path, args.dry_run)?)
    };

    for summary in summaries {
        println!(
            "{} {} {}",
            summary.client.label(),
            summary.action,
            summary.path.display()
        );
    }
    if let Some(summary) = agents_summary {
        println!("agents-md {} {}", summary.action, summary.path.display());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ClientTarget {
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
}

impl ClientTarget {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Cursor => "cursor",
        }
    }

    pub(crate) fn config_path(self) -> Result<PathBuf, CliError> {
        match self {
            Self::ClaudeCode => env_path("GAZE_MCP_CLAUDE_CODE_CONFIG")
                .or_else(|| Some(std::env::current_dir().ok()?.join(".mcp.json")))
                .ok_or_else(|| {
                    CliError::McpDetail(
                        "cannot resolve Claude Code project config path".to_string(),
                    )
                }),
            Self::ClaudeDesktop => env_path("GAZE_MCP_CLAUDE_DESKTOP_CONFIG")
                .or_else(claude_desktop_config_path)
                .ok_or_else(|| {
                    CliError::McpDetail(
                        "cannot resolve Claude Desktop config path from home/config dir"
                            .to_string(),
                    )
                }),
            Self::Cursor => env_path("GAZE_MCP_CURSOR_CONFIG")
                .or_else(|| {
                    Some(
                        std::env::current_dir()
                            .ok()?
                            .join(".cursor")
                            .join("mcp.json"),
                    )
                })
                .ok_or_else(|| {
                    CliError::McpDetail("cannot resolve Cursor project config path".to_string())
                }),
        }
    }
}

pub(crate) fn targets(client: Client) -> Result<Vec<ClientTarget>, CliError> {
    Ok(match client {
        Client::ClaudeCode => vec![ClientTarget::ClaudeCode],
        Client::ClaudeDesktop => vec![ClientTarget::ClaudeDesktop],
        Client::Cursor => vec![ClientTarget::Cursor],
        Client::All => vec![
            ClientTarget::ClaudeCode,
            ClientTarget::ClaudeDesktop,
            ClientTarget::Cursor,
        ],
    })
}

#[derive(Debug)]
struct InstallSummary {
    client: ClientTarget,
    path: PathBuf,
    action: &'static str,
}

#[derive(Debug)]
struct AgentsSummary {
    path: PathBuf,
    action: &'static str,
}

#[derive(Serialize)]
struct ServerConfig<'a> {
    command: &'a str,
    args: [&'static str; 2],
    env: BTreeMap<String, String>,
}

fn install_client(
    target: ClientTarget,
    exe: &Path,
    dry_run: bool,
) -> Result<InstallSummary, CliError> {
    let path = target.config_path()?;
    let mut root = read_json_object(&path)?;
    let action = upsert_gaze_server(&mut root, exe)?;
    if !dry_run {
        write_json_object(&path, &root)?;
    }
    Ok(InstallSummary {
        client: target,
        path,
        action,
    })
}

fn upsert_gaze_server(root: &mut Map<String, Value>, exe: &Path) -> Result<&'static str, CliError> {
    let exe = exe
        .to_str()
        .ok_or_else(|| CliError::McpDetail("current executable path is not UTF-8".to_string()))?;
    let desired = serde_json::to_value(ServerConfig {
        command: exe,
        args: ["mcp", "serve"],
        env: BTreeMap::new(),
    })
    .map_err(|err| CliError::McpDetail(format!("cannot build MCP server JSON: {err}")))?;

    let servers = root
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers) = servers else {
        return Err(CliError::McpDetail(
            "`mcpServers` exists but is not a JSON object".to_string(),
        ));
    };
    let action = match servers.get("gaze") {
        Some(existing) if existing == &desired => "unchanged",
        Some(_) => "updated",
        None => "installed",
    };
    servers.insert("gaze".to_string(), desired);
    Ok(action)
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, CliError> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let bytes = std::fs::read(path).map_err(|err| {
        CliError::McpDetail(format!("cannot read config `{}`: {err}", path.display()))
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|err| {
        CliError::McpDetail(format!("cannot parse config `{}`: {err}", path.display()))
    })?;
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(CliError::McpDetail(format!(
            "config `{}` is not a JSON object",
            path.display()
        ))),
    }
}

fn write_json_object(path: &Path, object: &Map<String, Value>) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            CliError::McpDetail(format!(
                "cannot create config directory `{}`: {err}",
                parent.display()
            ))
        })?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(object).map_err(|err| {
        CliError::McpDetail(format!(
            "cannot serialize config `{}`: {err}",
            path.display()
        ))
    })?;
    std::fs::write(&tmp, bytes).map_err(|err| {
        CliError::McpDetail(format!("cannot write config `{}`: {err}", tmp.display()))
    })?;
    std::fs::rename(&tmp, path).map_err(|err| {
        CliError::McpDetail(format!("cannot replace config `{}`: {err}", path.display()))
    })
}

fn update_agents_md(path: &Path, dry_run: bool) -> Result<AgentsSummary, CliError> {
    let existing = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(CliError::McpDetail(format!(
                "cannot read `{}`: {err}",
                path.display()
            )));
        }
    };
    let (next, action) = replace_or_append_section(&existing);
    if !dry_run && next != existing {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                CliError::McpDetail(format!(
                    "cannot create AGENTS.md directory `{}`: {err}",
                    parent.display()
                ))
            })?;
        }
        std::fs::write(path, next).map_err(|err| {
            CliError::McpDetail(format!("cannot write `{}`: {err}", path.display()))
        })?;
    }
    Ok(AgentsSummary {
        path: path.to_path_buf(),
        action,
    })
}

fn replace_or_append_section(existing: &str) -> (String, &'static str) {
    let section = skill_section();
    if let Some(start) = existing.find(BEGIN_MARKER) {
        if let Some(end_rel) = existing[start..].find(END_MARKER) {
            let end = start + end_rel + END_MARKER.len();
            let mut next = String::new();
            next.push_str(&existing[..start]);
            next.push_str(&section);
            next.push_str(&existing[end..]);
            return (next, "updated");
        }
    }

    let mut next = existing.to_string();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    if !next.is_empty() {
        next.push('\n');
    }
    next.push_str(&section);
    next.push('\n');
    (next, "installed")
}

fn skill_section() -> String {
    format!(
        "{BEGIN_MARKER}\n{}\n{END_MARKER}",
        r#"# Gaze MCP

When Gaze MCP is available, route potentially sensitive file or text reads through `gaze_read_file` or `gaze_read_text` before using the content in an LLM response.

Gaze output contains pseudonymous tokens such as `:Email_`, `:Name_`, and `:Custom:phone_`. Treat these as placeholders, not missing data. Do not invent originals. Preserve the `manifest_id` returned by Gaze so authorized restore flows can round-trip values later.

This section is agent guidance, not a security boundary. The MCP chokepoint is the server-side `PiiEnvelope::dispatch` path."#
    )
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn claude_desktop_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        BaseDirs::new().map(|dirs| {
            dirs.home_dir()
                .join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude_desktop_config.json")
        })
    }
    #[cfg(target_os = "windows")]
    {
        return directories_next::ProjectDirs::from("", "", "Claude")
            .map(|dirs| dirs.config_dir().join("claude_desktop_config.json"));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        directories_next::ProjectDirs::from("", "", "Claude")
            .map(|dirs| dirs.config_dir().join("claude_desktop_config.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_section_is_idempotent() {
        let (once, action) = replace_or_append_section("# Existing\n");
        assert_eq!(action, "installed");
        let (twice, action) = replace_or_append_section(&once);
        assert_eq!(action, "updated");
        assert_eq!(once, twice);
    }

    #[test]
    fn upsert_preserves_other_servers() {
        let mut root = Map::new();
        root.insert(
            "mcpServers".to_string(),
            serde_json::json!({ "other": { "command": "node" } }),
        );
        let action = upsert_gaze_server(&mut root, Path::new("/tmp/gaze")).unwrap();
        assert_eq!(action, "installed");
        assert!(root["mcpServers"]["other"].is_object());
        assert_eq!(
            root["mcpServers"]["gaze"]["args"],
            serde_json::json!(["mcp", "serve"])
        );
    }
}
