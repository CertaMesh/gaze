//! Tool registry — the only legal way to put a [`Tool`] in the dispatcher's reach.
//!
//! Registration is `register::<T: Tool + 'static>(t)` only; there is no
//! `register_raw(Box<dyn Fn(JsonValue) -> JsonValue>)` escape hatch that would
//! let an adopter sneak unsealed handlers past the chokepoint. The registry
//! is also the single source of truth for `tools/list`-style transport
//! endpoints — [`Frontend`](crate::frontend) impls call [`ToolRegistry::list`]
//! to advertise tools.

use std::collections::HashMap;
use std::sync::Arc;

use crate::tool::{ResponseRedaction, Tool, ToolDescriptor, ToolTier};

/// Errors returned by [`ToolRegistry::register`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolRegistryError {
    /// A tool with the same `descriptor().name` is already registered.
    /// Registries reject duplicate names rather than silently overwriting so
    /// adopter mistakes (two `register` calls for the same name from
    /// different feature flags) surface at startup, not under load.
    #[error("tool already registered: {0}")]
    DuplicateName(String),
    /// The tool's descriptor failed validation (currently: empty `name`).
    /// Adopter-supplied schemas are NOT validated here — gaze-mcp-core does
    /// not load a JSON schema validator; tools self-validate args in
    /// [`Tool::invoke`].
    #[error("invalid tool descriptor: {0}")]
    InvalidDescriptor(String),
}

/// Registry of [`Tool`] implementations keyed by [`ToolDescriptor::name`].
///
/// Constructed empty via [`ToolRegistry::new`] / [`ToolRegistry::default`].
/// Adopters typically build the registry once at startup, register all tools,
/// and then hand it to [`crate::dispatch::PiiEnvelope`] for the lifetime of
/// the host. The registry is `Send + Sync` so it can sit behind an `Arc` for
/// shared dispatchers.
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn Tool` does not require `Debug`; render the descriptor list so
        // the registry stays inspectable without forcing a supertrait bound.
        f.debug_struct("ToolRegistry")
            .field(
                "tools",
                &self
                    .tools
                    .values()
                    .map(|t| t.descriptor())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ToolRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a [`Tool`] implementation. Owns the tool by value, wrapping
    /// it in an `Arc<dyn Tool>` for shared lookup. Returns
    /// [`ToolRegistryError::DuplicateName`] if a tool with the same name is
    /// already registered, [`ToolRegistryError::InvalidDescriptor`] if the
    /// descriptor's name is empty.
    pub fn register<T>(&mut self, tool: T) -> Result<(), ToolRegistryError>
    where
        T: Tool + 'static,
    {
        let descriptor = tool.descriptor();
        let name = descriptor.name().to_string();
        if name.is_empty() {
            return Err(ToolRegistryError::InvalidDescriptor(
                "descriptor.name is empty".into(),
            ));
        }
        if descriptor.tier() == ToolTier::Agent
            && descriptor.response_redaction() == ResponseRedaction::BypassByOperator
        {
            return Err(ToolRegistryError::InvalidDescriptor(format!(
                "agent tool `{name}` cannot bypass response redaction"
            )));
        }
        if self.tools.contains_key(&name) {
            return Err(ToolRegistryError::DuplicateName(name));
        }
        self.tools.insert(name, Arc::new(tool));
        Ok(())
    }

    /// Look up a registered tool by wire name. Returns `None` if no tool
    /// with that name is registered. The dispatcher uses this to fail
    /// closed on `tools/call` for unknown tools.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Snapshot of every registered tool's descriptor. Stable iteration
    /// order is not guaranteed (adopter caches that order should sort
    /// after `list()`).
    pub fn list(&self) -> Vec<&ToolDescriptor> {
        self.tools.values().map(|t| t.descriptor()).collect()
    }

    /// Number of registered tools. Useful for adopter health checks.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// True when no tools are registered.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::ToolCtx;
    use crate::tool::{ToolError, ToolResponse, ToolTier};
    use async_trait::async_trait;
    use serde_json::json;

    struct StubTool {
        descriptor: ToolDescriptor,
    }

    impl StubTool {
        fn new(name: &str, tier: ToolTier) -> Self {
            let descriptor = match tier {
                ToolTier::Agent => ToolDescriptor::agent(name, json!({"type": "object"})),
                ToolTier::Operator => ToolDescriptor::operator(name, json!({"type": "object"})),
            };
            Self { descriptor }
        }
    }

    #[async_trait]
    impl Tool for StubTool {
        fn descriptor(&self) -> &ToolDescriptor {
            &self.descriptor
        }

        async fn invoke(&self, _ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
            Ok(ToolResponse::text("ok"))
        }
    }

    #[test]
    fn register_and_get_round_trip() {
        let mut reg = ToolRegistry::new();
        reg.register(StubTool::new("clean", ToolTier::Agent))
            .unwrap();
        assert!(reg.get("clean").is_some());
        assert!(reg.get("missing").is_none());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut reg = ToolRegistry::new();
        reg.register(StubTool::new("clean", ToolTier::Agent))
            .unwrap();
        let err = reg
            .register(StubTool::new("clean", ToolTier::Agent))
            .unwrap_err();
        assert_eq!(err, ToolRegistryError::DuplicateName("clean".into()));
    }

    #[test]
    fn empty_name_rejected_as_invalid_descriptor() {
        let mut reg = ToolRegistry::new();
        let err = reg
            .register(StubTool::new("", ToolTier::Agent))
            .unwrap_err();
        assert!(matches!(err, ToolRegistryError::InvalidDescriptor(_)));
    }

    #[test]
    fn agent_tool_cannot_bypass_response_redaction() {
        let mut reg = ToolRegistry::new();
        let err = reg
            .register(StubTool {
                descriptor: ToolDescriptor::agent("unsafe", json!({"type": "object"}))
                    .with_response_redaction(ResponseRedaction::BypassByOperator),
            })
            .unwrap_err();
        assert!(matches!(err, ToolRegistryError::InvalidDescriptor(_)));
    }

    #[test]
    fn list_returns_all_descriptors() {
        let mut reg = ToolRegistry::new();
        reg.register(StubTool::new("clean", ToolTier::Agent))
            .unwrap();
        reg.register(StubTool::new("restore", ToolTier::Operator))
            .unwrap();
        let mut names: Vec<&str> = reg.list().iter().map(|d| d.name()).collect();
        names.sort();
        assert_eq!(names, vec!["clean", "restore"]);
    }
}
