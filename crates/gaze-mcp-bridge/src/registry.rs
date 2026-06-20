use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use gaze_mcp_core::{ToolDescriptor, ToolTier};
use sha2::{Digest, Sha256};

use crate::client::{DownstreamClient, DownstreamTool};
use crate::error::{BridgeError, BridgeResult};

#[derive(Clone)]
pub struct NamespacedTool {
    pub namespaced_name: String,
    pub server: String,
    pub raw_server: String,
    pub raw_tool: String,
    pub sanitized_tool: String,
    pub descriptor: ToolDescriptor,
    pub discovery_hash: String,
    pub client: Arc<dyn DownstreamClient>,
}

pub struct BridgeRegistry {
    tools: BTreeMap<String, NamespacedTool>,
    resources_discovered: BTreeMap<String, usize>,
    prompts_discovered: BTreeMap<String, usize>,
}

impl BridgeRegistry {
    pub async fn discover(
        clients: BTreeMap<String, Arc<dyn DownstreamClient>>,
        timeout: Duration,
    ) -> BridgeResult<Self> {
        let mut tools = BTreeMap::new();
        let mut resources_discovered = BTreeMap::new();
        let mut prompts_discovered = BTreeMap::new();
        let mut sanitized_servers = BTreeSet::new();

        for (raw_server, client) in clients {
            let server = sanitize_component(&raw_server)?;
            if !sanitized_servers.insert(server.clone()) {
                return Err(BridgeError::Config(format!(
                    "server name collision after sanitization: {raw_server}"
                )));
            }

            let downstream_tools = client.list_tools(timeout).await?;
            let mut within_server = BTreeSet::new();
            for downstream in downstream_tools {
                let tool = register_tool(
                    &raw_server,
                    &server,
                    Arc::clone(&client),
                    downstream,
                    &mut within_server,
                )?;
                if tools
                    .insert(tool.namespaced_name.clone(), tool.clone())
                    .is_some()
                {
                    return Err(BridgeError::Config(
                        "tool collision after namespacing".to_string(),
                    ));
                }
            }

            resources_discovered.insert(
                server.clone(),
                client.list_resources(timeout).await.unwrap_or_default(),
            );
            prompts_discovered.insert(
                server,
                client.list_prompts(timeout).await.unwrap_or_default(),
            );
        }

        Ok(Self {
            tools,
            resources_discovered,
            prompts_discovered,
        })
    }

    pub fn from_tools(tools: Vec<NamespacedTool>) -> BridgeResult<Self> {
        let mut map = BTreeMap::new();
        for tool in tools {
            if map.insert(tool.namespaced_name.clone(), tool).is_some() {
                return Err(BridgeError::Config("duplicate tool name".to_string()));
            }
        }
        Ok(Self {
            tools: map,
            resources_discovered: BTreeMap::new(),
            prompts_discovered: BTreeMap::new(),
        })
    }

    pub fn get(&self, name: &str) -> Option<&NamespacedTool> {
        self.tools.get(name)
    }

    pub fn list_descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools
            .values()
            .map(|tool| tool.descriptor.clone())
            .collect()
    }

    pub fn print_surface(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for tool in self.tools.keys() {
            rows.push(format!("tool allow {tool}"));
        }
        for (server, count) in &self.resources_discovered {
            rows.push(format!("resources deny {server} count={count}"));
        }
        for (server, count) in &self.prompts_discovered {
            rows.push(format!("prompts deny {server} count={count}"));
        }
        rows
    }
}

fn register_tool(
    raw_server: &str,
    server: &str,
    client: Arc<dyn DownstreamClient>,
    downstream: DownstreamTool,
    within_server: &mut BTreeSet<String>,
) -> BridgeResult<NamespacedTool> {
    let sanitized_tool = sanitize_component(&downstream.raw_name)?;
    if !within_server.insert(sanitized_tool.clone()) {
        return Err(BridgeError::Config(format!(
            "duplicate downstream tool after sanitization on server {raw_server}"
        )));
    }
    let namespaced_name = format!("{server}.{sanitized_tool}");
    validate_component(&namespaced_name)?;
    let discovery_hash = discovery_hash(server, &sanitized_tool, &downstream);
    let descriptor =
        ToolDescriptor::agent(namespaced_name.clone(), downstream.input_schema.clone())
            .with_description(downstream.description.unwrap_or_else(|| {
                format!(
                    "Gaze MCP bridge proxy for {raw_server}.{}",
                    downstream.raw_name
                )
            }));
    let descriptor = if let Some(output_schema) = downstream.output_schema {
        descriptor.with_output_schema(output_schema)
    } else {
        descriptor
    };
    if descriptor.tier() != ToolTier::Agent {
        return Err(BridgeError::Config(
            "bridge only exposes agent-tier downstream tools".to_string(),
        ));
    }
    Ok(NamespacedTool {
        namespaced_name,
        server: server.to_string(),
        raw_server: raw_server.to_string(),
        raw_tool: downstream.raw_name,
        sanitized_tool,
        descriptor,
        discovery_hash,
        client,
    })
}

pub fn sanitize_component(input: &str) -> BridgeResult<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(BridgeError::Config(
            "tool/server name cannot be empty".to_string(),
        ));
    }
    let sanitized = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    validate_component(&sanitized)?;
    Ok(sanitized)
}

fn validate_component(input: &str) -> BridgeResult<()> {
    if input.is_empty() || input.len() > 128 {
        return Err(BridgeError::Config(format!(
            "MCP name length must be 1..=128, got {}",
            input.len()
        )));
    }
    if !input
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(BridgeError::Config(
            "MCP name contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

fn discovery_hash(server: &str, tool: &str, downstream: &DownstreamTool) -> String {
    let mut hasher = Sha256::new();
    hasher.update(server.as_bytes());
    hasher.update([0]);
    hasher.update(tool.as_bytes());
    hasher.update([0]);
    hasher.update(serde_json::to_vec(&downstream.input_schema).unwrap_or_default());
    if let Some(output_schema) = &downstream.output_schema {
        hasher.update([0]);
        hasher.update(serde_json::to_vec(output_schema).unwrap_or_default());
    }
    hex::encode(hasher.finalize())
}
