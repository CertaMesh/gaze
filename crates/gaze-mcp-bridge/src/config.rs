use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::error::{BridgeError, BridgeResult};
use crate::policy::{PolicyConfig, ResultMode};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeConfig {
    pub session: SessionCfg,
    #[serde(default)]
    pub limits: LimitCfg,
    #[serde(default)]
    pub servers: BTreeMap<String, ServerSpec>,
    #[serde(default)]
    pub policy: PolicyConfig,
}

impl BridgeConfig {
    pub fn from_path(path: &Path) -> BridgeResult<Self> {
        let raw = std::fs::read_to_string(path).map_err(|err| {
            BridgeError::Config(format!("failed to read `{}`: {err}", path.display()))
        })?;
        Self::from_toml_str(&raw)
    }

    pub fn from_toml_str(raw: &str) -> BridgeResult<Self> {
        let config: Self = toml::from_str(raw)
            .map_err(|err| BridgeError::Config(format!("TOML parse failed: {err}")))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> BridgeResult<()> {
        self.session.validate()?;
        if self.servers.is_empty() {
            return Err(BridgeError::Config(
                "at least one [servers.<name>] entry is required".to_string(),
            ));
        }
        for (name, server) in &self.servers {
            if name.trim().is_empty() {
                return Err(BridgeError::Config(
                    "server name cannot be empty".to_string(),
                ));
            }
            server.validate(name)?;
        }
        self.policy.validate()?;
        if self.policy.contains_result_mode(ResultMode::Allow) {
            tracing::warn!(
                "gaze-mcp-bridge result.mode=\"allow\" is an explicit unsafe opt-in; raw downstream output may reach the agent"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCfg {
    pub mode: SessionMode,
    pub dir: Option<PathBuf>,
    pub key_env: Option<String>,
}

impl SessionCfg {
    fn validate(&self) -> BridgeResult<()> {
        match self.mode {
            SessionMode::File => {
                let Some(key_env) = self.key_env.as_deref().filter(|value| !value.is_empty())
                else {
                    return Err(BridgeError::Config(
                        "session.mode=\"file\" requires non-empty session.key_env".to_string(),
                    ));
                };
                let key = std::env::var(key_env).map_err(|_| {
                    BridgeError::Config(format!(
                        "session.mode=\"file\" requires environment key `{key_env}`"
                    ))
                })?;
                if key.trim().is_empty() {
                    return Err(BridgeError::Config(format!(
                        "session key env `{key_env}` is empty"
                    )));
                }
                if self.dir.is_none() {
                    return Err(BridgeError::Config(
                        "session.mode=\"file\" requires session.dir".to_string(),
                    ));
                }
            }
            SessionMode::Ephemeral => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    File,
    Ephemeral,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

impl ServerSpec {
    fn validate(&self, name: &str) -> BridgeResult<()> {
        if self.command.trim().is_empty() {
            return Err(BridgeError::Config(format!(
                "servers.{name}.command cannot be empty"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitCfg {
    #[serde(default = "default_call_timeout_ms")]
    pub call_timeout_ms: u64,
    #[serde(default = "default_response_bytes")]
    pub response_bytes: usize,
    #[serde(default = "default_content_blocks")]
    pub content_blocks: usize,
}

impl LimitCfg {
    pub fn call_timeout(&self) -> Duration {
        Duration::from_millis(self.call_timeout_ms)
    }
}

impl Default for LimitCfg {
    fn default() -> Self {
        Self {
            call_timeout_ms: default_call_timeout_ms(),
            response_bytes: default_response_bytes(),
            content_blocks: default_content_blocks(),
        }
    }
}

fn default_call_timeout_ms() -> u64 {
    30_000
}

fn default_response_bytes() -> usize {
    1_048_576
}

fn default_content_blocks() -> usize {
    64
}
