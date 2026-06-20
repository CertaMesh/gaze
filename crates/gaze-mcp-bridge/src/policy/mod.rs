use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{BridgeError, BridgeResult};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    #[serde(default)]
    pub default: FieldPolicy,
    #[serde(default)]
    pub servers: BTreeMap<String, ServerPolicy>,
    #[serde(default)]
    pub tools: BTreeMap<String, ToolPolicy>,
}

impl PolicyConfig {
    pub fn validate(&self) -> BridgeResult<()> {
        self.default.validate("policy.default")?;
        for (name, server) in &self.servers {
            server.validate(&format!("policy.servers.{name}"))?;
        }
        for (name, tool) in &self.tools {
            tool.validate(&format!("policy.tools.{name}"))?;
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        server: &str,
        namespaced_tool: &str,
        bare_tool: &str,
    ) -> ResolvedToolPolicy {
        let mut resolved = ResolvedToolPolicy {
            base: self.default.clone(),
            arguments: BTreeMap::new(),
            result: self.default.result.clone().unwrap_or_default(),
        };

        if let Some(server_policy) = self.servers.get(server) {
            resolved.base = resolved.base.merge(&server_policy.base);
            if let Some(result) = &server_policy.result {
                resolved.result = resolved.result.merge(result);
            }
            for (path, policy) in &server_policy.arguments {
                resolved.arguments.insert(path.clone(), policy.clone());
            }
            if let Some(tool_policy) = server_policy.tools.get(bare_tool) {
                resolved.apply_tool(tool_policy);
            }
        }

        if let Some(tool_policy) = self
            .tools
            .get(namespaced_tool)
            .or_else(|| self.tools.get(bare_tool))
        {
            resolved.apply_tool(tool_policy);
        }

        resolved
    }

    pub fn contains_result_mode(&self, mode: ResultMode) -> bool {
        self.default.result.as_ref().is_some_and(|r| r.mode == mode)
            || self
                .servers
                .values()
                .any(|server| server.contains_result_mode(mode))
            || self
                .tools
                .values()
                .any(|tool| tool.result.as_ref().is_some_and(|r| r.mode == mode))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerPolicy {
    #[serde(flatten)]
    pub base: FieldPolicy,
    #[serde(default)]
    pub arguments: BTreeMap<String, FieldPolicy>,
    #[serde(default)]
    pub tools: BTreeMap<String, ToolPolicy>,
    pub result: Option<ResultPolicy>,
}

impl ServerPolicy {
    fn validate(&self, context: &str) -> BridgeResult<()> {
        self.base.validate(context)?;
        if let Some(result) = &self.result {
            result.validate(&format!("{context}.result"))?;
        }
        for (path, field) in &self.arguments {
            field.validate(&format!("{context}.arguments.{path}"))?;
        }
        for (tool, policy) in &self.tools {
            policy.validate(&format!("{context}.tools.{tool}"))?;
        }
        Ok(())
    }

    fn contains_result_mode(&self, mode: ResultMode) -> bool {
        self.result.as_ref().is_some_and(|r| r.mode == mode)
            || self
                .tools
                .values()
                .any(|tool| tool.result.as_ref().is_some_and(|r| r.mode == mode))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPolicy {
    #[serde(flatten)]
    pub base: FieldPolicy,
    #[serde(default)]
    pub arguments: BTreeMap<String, FieldPolicy>,
    pub result: Option<ResultPolicy>,
}

impl ToolPolicy {
    fn validate(&self, context: &str) -> BridgeResult<()> {
        self.base.validate(context)?;
        if let Some(result) = &self.result {
            result.validate(&format!("{context}.result"))?;
        }
        for (path, field) in &self.arguments {
            field.validate(&format!("{context}.arguments.{path}"))?;
        }
        Ok(())
    }
}

/// Policy applied at default, server, tool, and argument scopes.
///
/// Argument-specific policy is selected only by the full audit path
/// (`$.field`) or by a top-level argument key (`field`). Nested selectors such
/// as `contact.email` do not match `$.contact.email`; nested values fall back to
/// the nearest top-level or base policy.
///
/// Policy merging is monotonic for boolean guard fields. If an outer scope sets
/// `allow_sensitive_fields`, `requires_approval`, or `log_raw` to `true`, an
/// inner scope cannot narrow that value back to `false`. Keep broad scopes
/// least-privilege and allow only the smallest top-level argument needed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldPolicy {
    #[serde(default)]
    pub allow_sensitive_fields: bool,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default = "default_on_block")]
    pub on_block: BlockAction,
    #[serde(default)]
    pub log_raw: bool,
    pub result: Option<ResultPolicy>,
}

impl Default for FieldPolicy {
    fn default() -> Self {
        Self {
            allow_sensitive_fields: false,
            requires_approval: false,
            on_block: BlockAction::Refuse,
            log_raw: false,
            result: None,
        }
    }
}

impl FieldPolicy {
    fn validate(&self, context: &str) -> BridgeResult<()> {
        if self.log_raw {
            return Err(BridgeError::Config(format!(
                "{context}.log_raw=true is forbidden"
            )));
        }
        if let Some(result) = &self.result {
            result.validate(&format!("{context}.result"))?;
        }
        Ok(())
    }

    fn merge(&self, overlay: &FieldPolicy) -> FieldPolicy {
        FieldPolicy {
            allow_sensitive_fields: overlay.allow_sensitive_fields || self.allow_sensitive_fields,
            requires_approval: overlay.requires_approval || self.requires_approval,
            on_block: overlay.on_block,
            log_raw: overlay.log_raw || self.log_raw,
            result: overlay.result.clone().or_else(|| self.result.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockAction {
    Refuse,
}

fn default_on_block() -> BlockAction {
    BlockAction::Refuse
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultPolicy {
    #[serde(default)]
    pub mode: ResultMode,
}

impl ResultPolicy {
    fn validate(&self, _context: &str) -> BridgeResult<()> {
        Ok(())
    }

    fn merge(&self, overlay: &ResultPolicy) -> ResultPolicy {
        ResultPolicy { mode: overlay.mode }
    }
}

impl Default for ResultPolicy {
    fn default() -> Self {
        Self {
            mode: ResultMode::Process,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultMode {
    #[default]
    Process,
    Deny,
    Allow,
}

#[derive(Debug, Clone)]
pub struct ResolvedToolPolicy {
    pub base: FieldPolicy,
    pub arguments: BTreeMap<String, FieldPolicy>,
    pub result: ResultPolicy,
}

impl ResolvedToolPolicy {
    fn apply_tool(&mut self, tool: &ToolPolicy) {
        self.base = self.base.merge(&tool.base);
        if let Some(result) = &tool.result {
            self.result = self.result.merge(result);
        }
        for (path, field) in &tool.arguments {
            self.arguments.insert(path.clone(), field.clone());
        }
    }

    pub fn argument_policy(&self, path: &str) -> FieldPolicy {
        if let Some(policy) = self.arguments.get(path) {
            return self.base.merge(policy);
        }
        if let Some(top_level) = path
            .strip_prefix("$.")
            .and_then(|rest| rest.split('.').next())
            .and_then(|key| self.arguments.get(key))
        {
            return self.base.merge(top_level);
        }
        self.base.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionOutcome {
    Allowed,
    Blocked,
    RequiresApproval,
}
