use anyhow::Context;
use rmcp::model::CallToolResult;
use serde_json::{Map, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Low-level MCP client that communicates with a subprocess via stdin/stdout (JSON-RPC).
pub struct RawStdioClient {
    pub name: String,
    reader: BufReader<tokio::process::ChildStdout>,
    writer: tokio::process::ChildStdin,
    next_id: u64,
}

impl RawStdioClient {
    pub fn new(
        name: String,
        reader: BufReader<tokio::process::ChildStdout>,
        writer: tokio::process::ChildStdin,
    ) -> Self {
        Self {
            name,
            reader,
            writer,
            next_id: 1,
        }
    }

    pub async fn send_request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        });
        let mut msg = serde_json::to_string(&request)?;
        msg.push('\n');
        self.writer.write_all(msg.as_bytes()).await?;
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).await?;
            if line.trim().is_empty() {
                continue;
            }
            let resp: Value = serde_json::from_str(&line)?;
            if resp.get("id").is_none() {
                continue;
            }
            if resp["id"] != serde_json::json!(id) {
                continue;
            }
            if let Some(error) = resp.get("error") {
                anyhow::bail!("JSON-RPC error: {}", error);
            }
            return Ok(resp["result"].clone());
        }
    }

    pub async fn list_tools(&mut self) -> anyhow::Result<Vec<rmcp::model::Tool>> {
        let result = self
            .send_request("tools/list", serde_json::json!({}))
            .await?;
        serde_json::from_value(result["tools"].clone()).context("failed to parse tools/list")
    }

    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> anyhow::Result<CallToolResult> {
        let params = serde_json::json!({ "name": name, "arguments": arguments });
        let result = self.send_request("tools/call", params).await?;
        serde_json::from_value(result).context("failed to parse tools/call")
    }

    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": { "name": "nail-agent", "version": env!("CARGO_PKG_VERSION") },
        });
        let _result = self.send_request("initialize", params).await?;
        let notif = serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let mut msg = serde_json::to_string(&notif)?;
        msg.push('\n');
        self.writer.write_all(msg.as_bytes()).await?;
        log::info!("[STDIO] MCP handshake completed (name={})", self.name);
        Ok(())
    }
}
