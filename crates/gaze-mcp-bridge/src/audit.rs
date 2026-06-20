use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::error::{BridgeError, BridgeResult};
use crate::policy::DecisionOutcome;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ArgPath(String);

impl ArgPath {
    pub fn root() -> Self {
        Self("$".to_string())
    }

    pub fn child(&self, segment: impl AsRef<str>) -> BridgeResult<Self> {
        path_child(&self.0, segment.as_ref()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ResultPath(String);

impl ResultPath {
    pub fn root() -> Self {
        Self("$".to_string())
    }

    pub fn child(&self, segment: impl AsRef<str>) -> BridgeResult<Self> {
        path_child(&self.0, segment.as_ref()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn path_child(base: &str, segment: &str) -> BridgeResult<String> {
    if base.is_empty() || !base.starts_with('$') {
        return Err(BridgeError::Audit("invalid audit path base".to_string()));
    }
    if segment.is_empty()
        || segment.len() > 128
        || !segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'[' | b']'))
    {
        return Err(BridgeError::Audit(
            "invalid audit path segment rejected".to_string(),
        ));
    }
    Ok(format!("{base}.{segment}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeAuditEvent {
    pub ts_unix_ms: u128,
    pub upstream_request_id: String,
    pub session_id_hash: String,
    pub server: String,
    pub tool: String,
    pub arg_paths_affected: Vec<ArgPath>,
    pub result_paths_affected: Vec<ResultPath>,
    pub decision: String,
    pub outcome: DecisionOutcome,
    pub deciding_rule: String,
}

impl BridgeAuditEvent {
    pub fn new(
        upstream_request_id: String,
        session_id: &str,
        server: String,
        tool: String,
        outcome: DecisionOutcome,
        deciding_rule: impl Into<String>,
    ) -> Self {
        Self {
            ts_unix_ms: unix_ms(SystemTime::now()),
            upstream_request_id,
            session_id_hash: sha256_hex(session_id.as_bytes()),
            server,
            tool,
            arg_paths_affected: Vec::new(),
            result_paths_affected: Vec::new(),
            decision: format!("{outcome:?}"),
            outcome,
            deciding_rule: deciding_rule.into(),
        }
    }
}

#[async_trait]
pub trait BridgeAuditSink: Send + Sync {
    async fn record(&self, event: &BridgeAuditEvent) -> BridgeResult<()>;
}

#[derive(Debug, Clone)]
pub struct FileBridgeAuditSink {
    path: PathBuf,
}

impl FileBridgeAuditSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl BridgeAuditSink for FileBridgeAuditSink {
    async fn record(&self, event: &BridgeAuditEvent) -> BridgeResult<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| BridgeError::Audit(format!("create audit dir failed: {err}")))?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .map_err(|err| BridgeError::Audit(format!("open audit sink failed: {err}")))?;
        let mut line = serde_json::to_vec(event)
            .map_err(|err| BridgeError::Audit(format!("serialize audit failed: {err}")))?;
        line.push(b'\n');
        file.write_all(&line)
            .await
            .map_err(|err| BridgeError::Audit(format!("write audit failed: {err}")))?;
        file.flush()
            .await
            .map_err(|err| BridgeError::Audit(format!("flush audit failed: {err}")))
    }
}

fn unix_ms(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
