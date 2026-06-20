#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod approval;
pub mod audit;
pub mod bridge;
pub mod client;
pub mod config;
pub mod error;
pub mod registry;
pub mod session_store;

pub mod policy;

pub use approval::{ApprovalDecision, ApprovalHook, ApprovalRequest, DenyApprovalHook};
pub use audit::{ArgPath, BridgeAuditEvent, BridgeAuditSink, FileBridgeAuditSink, ResultPath};
pub use bridge::{BridgeHost, BridgeHostBuilder};
pub use client::{DownstreamClient, DownstreamTool, RmcpChildClient};
pub use config::{BridgeConfig, SessionMode};
pub use error::{BridgeError, BridgeResult};
pub use policy::{DecisionOutcome, FieldPolicy, ResultMode, ResultPolicy};
pub use registry::{BridgeRegistry, NamespacedTool};
pub use session_store::{BridgeSessionStore, SessionStoreMode};
