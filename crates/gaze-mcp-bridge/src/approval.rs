use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::audit::ArgPath;

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequest {
    pub call_id: String,
    pub session_id: String,
    pub redacted_arg_sha256: String,
    pub token_classes: BTreeSet<String>,
    pub token_list_sha256: String,
    pub downstream_tool: String,
    pub discovery_hash: String,
    pub reason: String,
    pub arg_paths: Vec<ArgPath>,
}

impl ApprovalRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        call_id: impl Into<String>,
        session_id: impl Into<String>,
        redacted_args: &serde_json::Value,
        token_classes: BTreeSet<String>,
        tokens: &[String],
        downstream_tool: impl Into<String>,
        discovery_hash: impl Into<String>,
        reason: impl Into<String>,
        arg_paths: Vec<ArgPath>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            session_id: session_id.into(),
            redacted_arg_sha256: json_sha256(redacted_args),
            token_classes,
            token_list_sha256: string_list_sha256(tokens),
            downstream_tool: downstream_tool.into(),
            discovery_hash: discovery_hash.into(),
            reason: reason.into(),
            arg_paths,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Granted,
    Denied,
    Pending,
}

#[async_trait]
pub trait ApprovalHook: Send + Sync {
    async fn decide(&self, request: &ApprovalRequest) -> ApprovalDecision;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DenyApprovalHook;

#[async_trait]
impl ApprovalHook for DenyApprovalHook {
    async fn decide(&self, _request: &ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Pending
    }
}

fn json_sha256(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    sha256_hex(&bytes)
}

fn string_list_sha256(values: &[String]) -> String {
    let mut sorted = values.to_vec();
    sorted.sort();
    sha256_hex(sorted.join("\n").as_bytes())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowApprovalHook;

#[cfg(test)]
#[async_trait]
impl ApprovalHook for AllowApprovalHook {
    async fn decide(&self, _request: &ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Granted
    }
}
