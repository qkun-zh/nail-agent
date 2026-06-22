use anyhow::Context;
use rmcp::model::CallToolResult;
use serde_json::{Map, Value};

/// Low-level MCP client that communicates with an HTTP endpoint via JSON-RPC.
///
/// Headers (auth, custom) are passed at construction from the server's config function.
/// This client has no knowledge of specific API keys or auth schemes.
pub struct RawHttpClient {
    pub name: String,
    url: String,
    headers: Vec<(String, String)>,
    http_client: reqwest::Client,
    next_id: u64,
}

impl RawHttpClient {
    pub fn new(name: String, url: String, headers: Vec<(String, String)>) -> Self {
        Self {
            name,
            url,
            headers,
            http_client: reqwest::Client::new(),
            next_id: 1,
        }
    }

    pub async fn send_request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        });
        let mut req = self
            .http_client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");
        for (key, value) in &self.headers {
            req = req.header(key, value);
        }
        let response = req.json(&body).send().await.context(format!(
            "HTTP request failed ({}): POST {}",
            self.name, self.url
        ))?;
        let status = response.status();
        let body_text = response.text().await.context(format!(
            "failed to read response body ({}: {})",
            self.name, self.url
        ))?;
        log::debug!(
            "[RAW] {} <- POST {} | status={} | body={:?}",
            self.name,
            self.url,
            status,
            body_text
        );
        if !status.is_success() {
            anyhow::bail!(
                "HTTP {} ({}): status={}, body={}",
                self.name,
                self.url,
                status,
                body_text
            );
        }

        // Handle SSE format (data: ...) or plain JSON
        let json_text = body_text
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap_or(&body_text);
        let resp: Value = serde_json::from_str(json_text).context(format!(
            "failed to parse JSON ({}: {}): body={:?}",
            self.name, self.url, body_text
        ))?;
        if let Some(error) = resp.get("error") {
            anyhow::bail!("JSON-RPC error ({}): {}", self.name, error);
        }
        Ok(resp["result"].clone())
    }

    pub async fn send_notification(&self, method: &str, params: Value) -> anyhow::Result<()> {
        let body = serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let mut req = self
            .http_client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");
        for (key, value) in &self.headers {
            req = req.header(key, value);
        }
        req.json(&body).send().await?;
        Ok(())
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
        self.send_notification("notifications/initialized", serde_json::json!({}))
            .await?;
        log::info!("[HTTP] MCP handshake completed (name={}, url={})", self.name, self.url);
        Ok(())
    }
}
