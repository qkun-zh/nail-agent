//! Minimal MCP client (stdio transport only).
//!
//! Speaks just enough of the Model Context Protocol to use Zed-forwarded
//! tool servers: `initialize` → `notifications/initialized` → `tools/list`
//! → `tools/call`. Hand-rolled JSON-RPC over pipes on purpose: no extra
//! dependency, full control over timeouts and cancellation, and stdio is
//! the one transport every ACP agent MUST support. SSE/HTTP servers are
//! skipped with a warning (documented limitation).
//!
//! Tool names are namespaced `{server}__{tool}`; on collision a
//! `DefaultHasher` suffix disambiguates.

use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::process::Stdio;
use std::time::Duration;

use agent_client_protocol::schema::v1::McpServer;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// One tool offered by one MCP server.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub server: String,
    pub name: String,
    /// `{server}__{tool}` (plus hash suffix on collision).
    pub namespaced: String,
    pub description: String,
    pub parameters: Value,
}

struct ServerConn {
    child: Child,
    stdin: ChildStdin,
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    next_id: u64,
}

impl ServerConn {
    async fn start(
        name: &str,
        command: &std::path::Path,
        args: &[String],
        envs: &[(String, String)],
    ) -> Result<Self, String> {
        let mut child = Command::new(command)
            .args(args)
            .envs(envs.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn {name}: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let mut conn = Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 1,
        };
        // MCP handshake.
        conn.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "nail-agent", "version": env!("CARGO_PKG_VERSION")},
            }),
            HANDSHAKE_TIMEOUT,
        )
        .await?;
        conn.notify("notifications/initialized", json!({})).await?;
        Ok(conn)
    }

    async fn send(&mut self, frame: &Value) -> Result<(), String> {
        let mut line = serde_json::to_string(frame).map_err(|e| format!("encode: {e}"))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("write: {e}"))?;
        self.stdin.flush().await.map_err(|e| format!("flush: {e}"))?;
        Ok(())
    }

    async fn next_frame(&mut self, timeout: Duration) -> Result<Value, String> {
        let line = tokio::time::timeout(timeout, self.lines.next_line())
            .await
            .map_err(|_| "timed out waiting for MCP server".to_string())?
            .map_err(|e| format!("read: {e}"))?
            .ok_or("MCP server closed stdout")?;
        serde_json::from_str(&line).map_err(|e| format!("bad frame: {e}"))
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    async fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        loop {
            let frame = self.next_frame(timeout).await?;
            if frame.get("id") == Some(&json!(id)) {
                if let Some(err) = frame.get("error") {
                    return Err(format!("MCP error: {err}"));
                }
                return Ok(frame["result"].clone());
            }
            // Skip server-initiated notifications while waiting.
        }
    }

    async fn list_tools(&mut self) -> Result<Vec<McpToolDef>, String> {
        let result = self
            .request("tools/list", json!({}), HANDSHAKE_TIMEOUT)
            .await?;
        let mut defs = Vec::new();
        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            for tool in tools {
                let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                defs.push(McpToolDef {
                    server: String::new(),
                    name: name.to_string(),
                    namespaced: name.to_string(),
                    description: tool
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string(),
                    parameters: tool.get("inputSchema").cloned().unwrap_or(json!({})),
                });
            }
        }
        Ok(defs)
    }

    async fn call_tool(&mut self, name: &str, args: Value) -> Result<String, String> {
        let result = self
            .request("tools/call", json!({"name": name, "arguments": args}), CALL_TIMEOUT)
            .await?;
        let mut out = String::new();
        if let Some(items) = result.get("content").and_then(|c| c.as_array()) {
            for item in items {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(text);
                }
            }
        }
        if out.is_empty() {
            out = result.to_string();
        }
        Ok(out)
    }
}

fn hash8(input: &str) -> String {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())[..8].to_string()
}

/// Connected MCP servers, keyed by ACP session.
pub struct McpPool {
    inner: Mutex<HashMap<String, Vec<ServerEntry>>>,
}

/// Name of the embedded filesystem server.
pub const OCTOFS_SERVER: &str = "octofs";

/// Locate the octofs binary.
///
/// Order: `OCTOFS_BIN` env → next to the agent executable itself (release
/// archives ship both binaries together) → `PATH`. Returns `None` when
/// absent — the caller falls back to built-in tools.
pub fn octofs_command() -> Option<std::path::PathBuf> {
    if let Ok(bin) = std::env::var("OCTOFS_BIN")
        && !bin.is_empty()
    {
        let path = std::path::PathBuf::from(bin);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        #[cfg(windows)]
        let sibling = dir.join("octofs.exe");
        #[cfg(not(windows))]
        let sibling = dir.join("octofs");
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    find_on_path("octofs", &std::env::var("PATH").unwrap_or_default())
}

fn find_on_path(name: &str, path_var: &str) -> Option<std::path::PathBuf> {
    for dir in std::env::split_paths(path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Build the auto-attached octofs server entry scoped to `cwd`.
/// Returns `None` when the binary is unavailable.
pub fn octofs_server(
    cwd: &std::path::Path,
    command: std::path::PathBuf,
) -> agent_client_protocol::schema::v1::McpServer {
    use agent_client_protocol::schema::v1::{McpServer, McpServerStdio};
    McpServer::Stdio(
        McpServerStdio::new(OCTOFS_SERVER, command)
            .args(vec!["mcp".to_string(), "--path".to_string(), cwd.to_string_lossy().into_owned()]),
    )
}

/// One connected server: its name plus a shared handle.
type ServerEntry = (String, std::sync::Arc<Mutex<ServerConn>>);

impl McpPool {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Connect servers the session doesn't have yet; return all tool defs.
    /// Non-stdio servers are skipped with a warning.
    pub async fn tools_for(
        &self,
        session_key: &str,
        servers: &[McpServer],
    ) -> Vec<McpToolDef> {
        for server in servers {
            let McpServer::Stdio(cfg) = server else {
                tracing::warn!("skipping non-stdio MCP server (unsupported transport)");
                continue;
            };
            let already = self
                .inner
                .lock()
                .await
                .get(session_key)
                .map(|v| v.iter().any(|(name, _)| name == &cfg.name))
                .unwrap_or(false);
            if already {
                continue;
            }
            let envs: Vec<(String, String)> = cfg
                .env
                .iter()
                .map(|e| (e.name.clone(), e.value.clone()))
                .collect();
            match ServerConn::start(&cfg.name, &cfg.command, &cfg.args, &envs).await {
                Ok(conn) => {
                    tracing::info!(server = %cfg.name, "MCP server connected");
                    self.inner.lock().await.entry(session_key.to_string()).or_default().push((
                        cfg.name.clone(),
                        std::sync::Arc::new(Mutex::new(conn)),
                    ));
                }
                Err(err) => {
                    tracing::warn!(server = %cfg.name, error = %err, "MCP server failed, skipped");
                }
            }
        }
        // List tools from every connected server.
        let conns: Vec<ServerEntry> = self
            .inner
            .lock()
            .await
            .get(session_key)
            .cloned()
            .unwrap_or_default();
        let mut defs = Vec::new();
        for (server_name, conn) in &conns {
            let mut conn = conn.lock().await;
            match conn.list_tools().await {
                Ok(mut tools) => {
                    for tool in &mut tools {
                        tool.server = server_name.clone();
                        tool.namespaced = format!("{}__{}", server_name, tool.name);
                    }
                    defs.extend(tools);
                }
                Err(err) => {
                    tracing::warn!(server = %server_name, error = %err, "tools/list failed");
                }
            }
        }
        // Disambiguate collisions with a hash suffix.
        let mut seen: HashMap<String, usize> = HashMap::new();
        for def in &mut defs {
            let count = seen.entry(def.namespaced.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                def.namespaced = format!(
                    "{}__{}",
                    def.namespaced,
                    hash8(&format!("{}__{}", def.server, def.name))
                );
            }
        }
        defs
    }

    /// Call a namespaced tool. The caller resolves `namespaced` first.
    pub async fn call(
        &self,
        session_key: &str,
        server: &str,
        tool: &str,
        args: Value,
    ) -> Result<String, String> {
        let conn = self
            .inner
            .lock()
            .await
            .get(session_key)
            .and_then(|v| {
                v.iter()
                    .find(|(name, _)| name == server)
                    .map(|(_, conn)| conn.clone())
            });
        let Some(conn) = conn else {
            return Err(format!("MCP server {server} not connected"));
        };
        conn.lock().await.call_tool(tool, args).await
    }

    /// Drop (and kill) one server connection, e.g. after cancellation.
    pub async fn drop_server(&self, session_key: &str, server: &str) {
        let conn = self.inner.lock().await.get_mut(session_key).and_then(|v| {
            v.iter()
                .position(|(name, _)| name == server)
                .map(|i| v.remove(i).1)
        });
        if let Some(conn) = conn {
            let mut conn = conn.lock().await;
            let _ = conn.child.kill().await;
        }
    }

    /// Drop a whole session (kills all its servers).
    pub async fn drop_session(&self, session_key: &str) {
        let conns = self.inner.lock().await.remove(session_key).unwrap_or_default();
        for (_, conn) in conns {
            let mut conn = conn.lock().await;
            let _ = conn.child.kill().await;
        }
    }

    /// Split `server__tool` back into parts.
    ///
    /// Server names containing a double underscore are unsupported and will
    /// mis-split; such names are pathological in practice.
    pub fn split_namespaced(namespaced: &str) -> Option<(&str, &str)> {
        namespaced.split_once("__")
    }
}

/// Outcome of one MCP tool call.
pub struct McpToolOutcome {
    pub output: String,
    pub cancelled: bool,
}

/// Run one model-requested MCP tool: permission first, then the server call.
/// Cancellation drops (and kills) the server connection; it reconnects lazily.
pub async fn execute_mcp(
    ctx: &mut super::CallCtx<'_>,
    pool: &McpPool,
    namespaced: &str,
    arguments: &str,
) -> McpToolOutcome {
    use super::{clip, ensure_permission, tool_result_update, tool_update};
    use agent_client_protocol::schema::v1::{ToolCallContent, ToolCallStatus};

    let done = |output: String| McpToolOutcome { output, cancelled: false };
    let Some((server, tool)) = McpPool::split_namespaced(namespaced) else {
        return done(format!("未知工具：{namespaced}"));
    };
    let args: serde_json::Value = match serde_json::from_str(arguments) {
        Ok(args) => args,
        Err(_) => return done(format!("工具参数不是合法 JSON（{namespaced}）：{arguments}")),
    };
    let tool_id = format!("mcp-{namespaced}");
    let title = format!("{server}: {tool}");
    // Defense in depth: our own screens still apply to MCP calls, including
    // the embedded octofs shell (octofs scopes itself with --path on top).
    if tool == "shell"        && let Some(args_obj) = args.as_object()
        && let Some(command) = args_obj.get("command").and_then(|c| c.as_str())
        && let Some(reason) = super::screen_command(command)
    {
        super::audit(ctx.session_key, namespaced, &title, "blocked");
        return done(reason);
    }
    super::audit(ctx.session_key, namespaced, &title, "exec");
    if !ensure_permission(ctx, &tool_id,
        namespaced,
        &title,
        Some(args.clone()),
    )
    .await
    {
        let _ = ctx.cx.send_notification(tool_update(
            ctx.session_id,
            &tool_id,
            &title,
            ToolCallStatus::Failed,
        ));
        return done("用户拒绝了这次工具调用。".to_string());
    }
    let _ = ctx.cx.send_notification(tool_update(
        ctx.session_id,
        &tool_id,
        &title,
        ToolCallStatus::InProgress,
    ));
    let cancel: &mut tokio::sync::watch::Receiver<bool> = &mut *ctx.cancel;
    let output = tokio::select! {
        result = pool.call(ctx.session_key, server, tool, args) => {
            match result {
                Ok(output) => {
                    let _ = ctx.cx.send_notification(tool_result_update(
                        ctx.session_id,
                        &tool_id,
                        &title,
                        ToolCallStatus::Completed,
                        vec![ToolCallContent::from(clip(&output, 2000))],
                        Vec::new(),
                        Some(serde_json::Value::String(clip(&output, 4000))),
                    ));
                    output
                }
                Err(err) => {
                    let _ = ctx.cx.send_notification(tool_update(
                        ctx.session_id,
                        &tool_id,
                        &title,
                        ToolCallStatus::Failed,
                    ));
                    format!("MCP 工具调用失败：{err}")
                }
            }
        }
        _ = crate::llm::Llm::cancelled(cancel) => {
            pool.drop_server(ctx.session_key, server).await;
            let _ = ctx.cx.send_notification(tool_update(
                ctx.session_id,
                &tool_id,
                &title,
                ToolCallStatus::Failed,
            ));
            return McpToolOutcome {
                output: "调用被取消。".to_string(),
                cancelled: true,
            };
        }
    };
    done(output)
}
