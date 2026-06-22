use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use ascon_hash::Digest;
use rmcp::model::{CallToolResult, Tool};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient};
use serde_json::{Map, Value};
use tokio::sync::Mutex;

use super::client_to_tool_servers::{ClientConfig, ClientHandle, connect};
use super::filter::ToolFilter;

pub type McpHandle = RunningService<RoleClient, NullClientHandler>;

#[derive(Clone, Default)]
pub struct NullClientHandler;
impl ClientHandler for NullClientHandler {}

pub enum ToolServerHandle {
    Internal { name: String, handle: McpHandle },
    External(ClientHandle),
}

impl ToolServerHandle {
    pub fn name(&self) -> &str {
        match self {
            ToolServerHandle::Internal { name, .. } => name,
            ToolServerHandle::External(h) => h.name(),
        }
    }

    pub async fn list_tools(&self) -> anyhow::Result<Vec<Tool>> {
        match self {
            ToolServerHandle::Internal { handle, .. } => handle
                .list_all_tools()
                .await
                .map_err(|e| anyhow::anyhow!(e)),
            ToolServerHandle::External(h) => h.list_tools().await,
        }
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> anyhow::Result<CallToolResult> {
        match self {
            ToolServerHandle::Internal { handle, .. } => {
                let params = rmcp::model::CallToolRequestParams::new(name.to_string())
                    .with_arguments(arguments);
                handle
                    .call_tool(params)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            }
            ToolServerHandle::External(h) => h.call_tool(name, arguments).await,
        }
    }
}

struct ServerEntry {
    handle: Arc<ToolServerHandle>,
    filter: ToolFilter,
}

impl ServerEntry {
    fn name(&self) -> &str {
        self.handle.name()
    }
}

impl Clone for ServerEntry {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            filter: self.filter.clone(),
        }
    }
}

pub struct ToolProxy {
    servers: Mutex<Vec<ServerEntry>>,
    tool_index: Mutex<HashMap<String, usize>>,
    tool_original_name: Mutex<HashMap<String, String>>,
}

/// Ascon-Hash 256-bit，取前 8 字节作为 16 位十六进制后缀
fn short_hash(input: &str) -> String {
    let result = ascon_hash::AsconHash256::digest(input.as_bytes());
    let mut hex = String::with_capacity(16);
    for b in result.iter().take(8) {
        hex.push_str(&format!("{:02x}", b));
    }
    hex
}

impl ToolProxy {
    pub fn new() -> Self {
        Self {
            servers: Mutex::new(Vec::new()),
            tool_index: Mutex::new(HashMap::new()),
            tool_original_name: Mutex::new(HashMap::new()),
        }
    }

    pub async fn add_tool_server(&self, config: ClientConfig) -> anyhow::Result<()> {
        self.add_tool_server_with_filter(config, ToolFilter::allow_all())
            .await
    }

    pub async fn add_tool_server_with_filter(
        &self,
        config: ClientConfig,
        filter: ToolFilter,
    ) -> anyhow::Result<()> {
        let handle = connect(config).await?;
        let mut servers = self.servers.lock().await;
        servers.push(ServerEntry {
            handle: Arc::new(ToolServerHandle::External(handle)),
            filter,
        });
        self.tool_index.lock().await.clear();
        self.tool_original_name.lock().await.clear();
        log::info!(
            "[TOOL-PROXY] ToolServer added, current count: {}",
            servers.len()
        );
        Ok(())
    }

    pub async fn list_all_tools(&self) -> anyhow::Result<Vec<Tool>> {
        let timer = crate::logger::Timer::start("ToolProxy.list_all_tools");

        let snapshot = {
            let servers = self.servers.lock().await;
            servers.clone()
        };

        if snapshot.is_empty() {
            log::warn!("[TOOL-PROXY] no registered ToolServer, tool list is empty");
            return Ok(vec![]);
        }

        // Step 1: collect all tools from each server (with filter applied)
        let mut all_entries: Vec<(String, usize, Tool)> = Vec::new();

        for (idx, entry) in snapshot.iter().enumerate() {
            let timer_b =
                crate::logger::Timer::start(format!("list_tools(server={})", entry.name()));
            match entry.handle.list_tools().await {
                Ok(tools) => {
                    timer_b.stop();
                    let filtered = entry.filter.apply(tools);
                    log::info!(
                        "[TOOL-PROXY] ToolServer '{}' returned {} -> exposed {} tools",
                        entry.name(),
                        filtered.len(),
                        filtered.len(),
                    );
                    for tool in &filtered {
                        log::info!(
                            "[TOOL-PROXY]   |-- {} (from: {})",
                            tool.name.as_ref(),
                            entry.name()
                        );
                    }
                    all_entries.extend(
                        filtered
                            .into_iter()
                            .map(|tool| (entry.name().to_string(), idx, tool)),
                    );
                }
                Err(e) => {
                    timer_b.stop();
                    log::error!(
                        "[TOOL-PROXY] failed to list tools for ToolServer '{}': {}",
                        entry.name(),
                        e
                    );
                }
            }
        }

        // Step 2: suffix all tool names with 8-char Ascon hash of "{server}-{tool}"
        let mut all_tools: Vec<Tool> = Vec::new();
        let mut tool_index: HashMap<String, usize> = HashMap::new();
        let mut tool_original_name: HashMap<String, String> = HashMap::new();

        for (server_name, server_idx, mut tool) in all_entries {
            let original_name = tool.name.as_ref().to_string();
            let hash = short_hash(&format!("{}-{}", server_name, original_name));
            let exposed_name = format!("{}-{}", original_name, hash);
            tool.name = exposed_name.clone().into();
            tool_original_name.insert(exposed_name.clone(), original_name);
            tool_index.insert(exposed_name, server_idx);
            all_tools.push(tool);
        }

        *self.tool_index.lock().await = tool_index;
        *self.tool_original_name.lock().await = tool_original_name;
        timer.stop();
        log::info!(
            "[TOOL-PROXY] tool aggregation completed: {} tools",
            all_tools.len()
        );
        Ok(all_tools)
    }

    /// Calls a tool (looked up via the exposed name).
    pub async fn call_tool(
        &self,
        exposed_name: &str,
        arguments: Map<String, Value>,
    ) -> anyhow::Result<CallToolResult> {
        let timer = crate::logger::Timer::start(format!("ToolProxy.call_tool({})", exposed_name));

        let snapshot = {
            let servers = self.servers.lock().await;
            servers.clone()
        };

        let server_idx = self.tool_index.lock().await.get(exposed_name).copied();

        if let Some(idx) = server_idx {
            if idx < snapshot.len() {
                let entry = &snapshot[idx];
                let original_name = self
                    .tool_original_name
                    .lock()
                    .await
                    .get(exposed_name)
                    .cloned()
                    .unwrap_or_else(|| exposed_name.to_string());

                let result = entry
                    .handle
                    .call_tool(&original_name, arguments)
                    .await
                    .context(format!(
                        "ToolServer '{}' failed to call tool '{}' (original: '{}')",
                        entry.name(),
                        exposed_name,
                        original_name,
                    ))?;
                timer.stop();
                return Ok(result);
            }
        }

        log::warn!(
            "[TOOL-PROXY] tool '{}' not found in cache index, searching servers...",
            exposed_name
        );
        for entry in snapshot.iter() {
            if let Ok(r) = entry
                .handle
                .call_tool(exposed_name, arguments.clone())
                .await
            {
                timer.stop();
                return Ok(r);
            }
        }

        anyhow::bail!("no ToolServer found tool '{}'", exposed_name);
    }

    pub async fn server_count(&self) -> usize {
        self.servers.lock().await.len()
    }

    pub async fn server_names(&self) -> Vec<String> {
        let servers = self.servers.lock().await;
        servers.iter().map(|s| s.name().to_string()).collect()
    }
}

impl Default for ToolProxy {
    fn default() -> Self {
        Self::new()
    }
}
