pub mod functioncall_tool_servers;

mod client_to_functioncall_tool_servers;
mod client_to_http_tool_servers;
mod client_to_stdio_tool_servers;

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::Context;
use rmcp::model::CallToolResult;
use serde_json::{Map, Value};
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::Mutex;

pub use client_to_functioncall_tool_servers::{FunctionCallClient, SimpleTool, ToolHandler};
pub use client_to_http_tool_servers::RawHttpClient;
pub use client_to_stdio_tool_servers::RawStdioClient;

use crate::tool_proxy::ToolProxy;
use crate::tool_proxy::filter::ToolFilter;

// ============================================================================
// ClientConfig
// ============================================================================

#[derive(Clone)]
pub enum ClientConfig {
    Stdio {
        name: String,
        command: String,
        args: Vec<String>,
    },
    Http {
        name: String,
        url: String,
        headers: Vec<(String, String)>,
    },
    FunctionCall {
        name: String,
        handlers: Vec<Arc<dyn ToolHandler>>,
    },
}

impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdio {
                name,
                command,
                args,
            } => f
                .debug_struct("Stdio")
                .field("name", name)
                .field("command", command)
                .field("args", args)
                .finish(),
            Self::Http { name, url, headers } => {
                let mut s = f.debug_struct("Http");
                s.field("name", name).field("url", url);
                if !headers.is_empty() {
                    s.field("headers_count", &headers.len());
                }
                s.finish()
            }
            Self::FunctionCall { name, handlers } => f
                .debug_struct("FunctionCall")
                .field("name", name)
                .field("handlers_count", &handlers.len())
                .finish(),
        }
    }
}

impl ClientConfig {
    pub fn name(&self) -> &str {
        match self {
            ClientConfig::Stdio { name, .. } => name,
            ClientConfig::Http { name, .. } => name,
            ClientConfig::FunctionCall { name, .. } => name,
        }
    }

    pub fn stdio(name: impl Into<String>, command: impl Into<String>, args: Vec<String>) -> Self {
        ClientConfig::Stdio {
            name: name.into(),
            command: command.into(),
            args,
        }
    }

    pub fn http(name: impl Into<String>, url: impl Into<String>) -> Self {
        ClientConfig::Http {
            name: name.into(),
            url: url.into(),
            headers: vec![],
        }
    }

    /// Add headers to an Http config. Used by config functions to pass auth tokens.
    pub fn with_headers(self, extra: Vec<(String, String)>) -> Self {
        match self {
            ClientConfig::Http {
                name,
                url,
                mut headers,
            } => {
                headers.extend(extra);
                ClientConfig::Http { name, url, headers }
            }
            other => other,
        }
    }

    pub fn function_call(name: impl Into<String>, handlers: Vec<Arc<dyn ToolHandler>>) -> Self {
        ClientConfig::FunctionCall {
            name: name.into(),
            handlers,
        }
    }
}

// ============================================================================
// ClientHandle
// ============================================================================

pub enum ClientHandle {
    Stdio {
        name: String,
        inner: Mutex<RawStdioClient>,
    },
    Http {
        name: String,
        inner: Mutex<RawHttpClient>,
    },
    FunctionCall {
        name: String,
        inner: Mutex<FunctionCallClient>,
    },
}

impl ClientHandle {
    pub fn name(&self) -> &str {
        match self {
            ClientHandle::Stdio { name, .. } => name,
            ClientHandle::Http { name, .. } => name,
            ClientHandle::FunctionCall { name, .. } => name,
        }
    }

    pub async fn list_tools(&self) -> anyhow::Result<Vec<rmcp::model::Tool>> {
        match self {
            ClientHandle::Stdio { inner, .. } => inner.lock().await.list_tools().await,
            ClientHandle::Http { inner, .. } => inner.lock().await.list_tools().await,
            ClientHandle::FunctionCall { inner, .. } => inner.lock().await.list_tools().await,
        }
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> anyhow::Result<CallToolResult> {
        match self {
            ClientHandle::Stdio { inner, .. } => {
                inner.lock().await.call_tool(name, arguments).await
            }
            ClientHandle::Http { inner, .. } => inner.lock().await.call_tool(name, arguments).await,
            ClientHandle::FunctionCall { inner, .. } => {
                inner.lock().await.call_tool(name, arguments).await
            }
        }
    }
}

// ============================================================================
// connect() — 根据 ClientConfig 创建 ClientHandle
// ============================================================================

pub async fn connect(config: ClientConfig) -> anyhow::Result<ClientHandle> {
    match config {
        ClientConfig::Stdio {
            name,
            command,
            args,
        } => connect_stdio(name, command, args).await,
        ClientConfig::Http { name, url, headers } => connect_http(name, url, headers).await,
        ClientConfig::FunctionCall { name, handlers } => {
            log::info!(
                "[CLIENT] connecting FunctionCallToolServer: name={}, tools={}",
                name,
                handlers.len()
            );
            let client =
                client_to_functioncall_tool_servers::connect_duplex(name.clone(), handlers).await?;
            Ok(ClientHandle::FunctionCall {
                name,
                inner: Mutex::new(client),
            })
        }
    }
}

async fn connect_stdio(
    name: String,
    command: String,
    args: Vec<String>,
) -> anyhow::Result<ClientHandle> {
    log::info!(
        "[CLIENT] connecting StdioToolServer: name={}, cmd={}",
        name,
        command
    );

    let mut child = Command::new(&command)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(format!("failed to start child process {}", command))?;

    let stdout = child.stdout.take().context("failed to get child stdout")?;
    let stdin = child.stdin.take().context("failed to get child stdin")?;
    let mut stderr = child.stderr.take().context("failed to get child stderr")?;

    let bg_name = name.clone();
    tokio::spawn(async move {
        let mut buf = String::new();
        if let Err(e) = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buf).await {
            log::error!("[CLIENT] reading {} stderr failed: {}", bg_name, e);
        } else if !buf.is_empty() {
            for line in buf.lines() {
                log::info!("[CLIENT] [{} stderr] {}", bg_name, line);
            }
        }
        let status = child.wait().await;
        log::info!(
            "[CLIENT] {} child process exited: {:?}",
            bg_name,
            status.map(|s| s.code())
        );
    });

    let mut client = RawStdioClient::new(name.clone(), BufReader::new(stdout), stdin);
    client.initialize().await?;

    log::info!("[CLIENT] StdioToolServer {} connected successfully", name);
    Ok(ClientHandle::Stdio {
        name,
        inner: Mutex::new(client),
    })
}

async fn connect_http(
    name: String,
    url: String,
    headers: Vec<(String, String)>,
) -> anyhow::Result<ClientHandle> {
    log::info!(
        "[CLIENT] connecting HttpToolServer: name={}, url={}",
        name,
        url
    );

    let mut client = RawHttpClient::new(name.clone(), url, headers);
    client.initialize().await?;

    log::info!("[CLIENT] HttpToolServer {} connected successfully", name);
    Ok(ClientHandle::Http {
        name,
        inner: Mutex::new(client),
    })
}

// ============================================================================
// TOML 配置结构
// ============================================================================

/// TOML 顶层结构：`[[servers]]`
#[derive(serde::Deserialize)]
struct ConfigFile {
    servers: Vec<ConfigEntry>,
}

#[derive(serde::Deserialize)]
struct ConfigEntry {
    name: String,
    transport: String,

    // stdio
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,

    // http
    url: Option<String>,
    #[serde(default)]
    headers: Vec<HeaderEntry>,

    // override
    #[serde(rename = "override")]
    filter: Option<ToolFilter>,
}

#[derive(serde::Deserialize)]
struct HeaderEntry {
    name: String,
    value: String,
}

/// 替换字符串中的 `${ENV_VAR}` 为实际环境变量值。
/// 未设置的变量保留原样，不报错。
fn resolve_env_vars(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // skip '{'
            let mut var_name = String::new();
            for ch in chars.by_ref() {
                if ch == '}' {
                    break;
                }
                var_name.push(ch);
            }
            let val = std::env::var(&var_name).unwrap_or_else(|_| format!("${{{}}}", var_name));
            result.push_str(&val);
        } else {
            result.push(c);
        }
    }
    result
}

/// 查找配置文件路径：`TOOL_SERVERS_CONFIG` 环境变量 → `./config/tool_servers.toml` → 跳过
fn config_path() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("TOOL_SERVERS_CONFIG") {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
        log::warn!(
            "[CONFIG] TOOL_SERVERS_CONFIG={} 不存在，尝试默认路径",
            p.display()
        );
    }

    // 默认路径：项目根目录下的 tool_servers.toml
    let default = Path::new("./tool_servers.toml");
    if default.exists() {
        return Some(default.to_path_buf());
    }

    None
}

/// 读取 TOML 配置，返回配置条目列表。
fn load_config() -> Vec<ConfigEntry> {
    let path = match config_path() {
        Some(p) => p,
        None => {
            log::info!("[CONFIG] 未找到 tool_servers.toml，仅注册内置 server");
            return vec![];
        }
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            log::error!(
                "[CONFIG] 读取 {} 失败: {}，仅注册内置 server",
                path.display(),
                e
            );
            return vec![];
        }
    };

    match toml::from_str::<ConfigFile>(&content) {
        Ok(config) => {
            log::info!(
                "[CONFIG] 从 {} 加载了 {} 个 tool server 配置",
                path.display(),
                config.servers.len()
            );
            config.servers
        }
        Err(e) => {
            log::error!(
                "[CONFIG] 解析 {} 失败: {}，仅注册内置 server",
                path.display(),
                e
            );
            vec![]
        }
    }
}

// ============================================================================
// register_all() — 从 TOML 配置注册所有 tool server
// ============================================================================

/// 注册所有 tool server：
/// 1. 注册内置 server（FunctionCall，无法通过 TOML 表达）
/// 2. 读取 tool_servers.toml，逐条连接
/// 3. 导出 tool_list.txt 供用户查看
///
/// 每条 server 独立连接，失败只 warning 不影响其他。
pub async fn register_all(proxy: &ToolProxy) {
    // 1. 注册内置 FunctionCall server
    register_one(
        proxy,
        functioncall_tool_servers::builtin::config(),
        &ToolFilter::allow_all(),
    )
    .await;

    // 2. 从 TOML 配置加载外部 server
    let entries = load_config();
    for entry in entries {
        match entry_to_config(&entry).await {
            Ok(config) => {
                let filter = entry.filter.unwrap_or_default();
                register_one(proxy, config, &filter).await;
            }
            Err(e) => {
                log::warn!("[CONFIG] server '{}' 配置无效: {} (跳过)", entry.name, e);
            }
        }
    }

    // 3. 导出 tool_list.txt
    export_tool_list(proxy).await;
}

/// 导出 tools.json，列出所有可用 tool 供用户查看。
async fn export_tool_list(proxy: &ToolProxy) {
    let tools = match proxy.list_all_tools().await {
        Ok(t) => t,
        Err(e) => {
            log::warn!("[EXPORT] 获取 tool 列表失败: {}", e);
            return;
        }
    };

    let json = serde_json::to_string_pretty(&tools).unwrap_or_default();
    match std::fs::write("./tools.json", &json) {
        Ok(_) => log::info!("[EXPORT] tools.json 已生成 ({} tools)", tools.len()),
        Err(e) => log::warn!("[EXPORT] 写入 tools.json 失败: {}", e),
    }
}

/// 将 JSON 配置条目转为 ClientConfig。
async fn entry_to_config(entry: &ConfigEntry) -> anyhow::Result<ClientConfig> {
    match entry.transport.as_str() {
        "stdio" => {
            let command = entry
                .command
                .as_deref()
                .context("stdio transport 需要 command 字段")?;
            let command = resolve_env_vars(command);
            let args: Vec<String> = entry.args.iter().map(|a| resolve_env_vars(a)).collect();
            Ok(ClientConfig::stdio(&entry.name, command, args))
        }
        "http" => {
            let url = entry
                .url
                .as_deref()
                .context("http transport 需要 url 字段")?;
            let url = resolve_env_vars(url);
            let headers: Vec<(String, String)> = entry
                .headers
                .iter()
                .map(|h| (resolve_env_vars(&h.name), resolve_env_vars(&h.value)))
                .collect();
            Ok(ClientConfig::http(&entry.name, url).with_headers(headers))
        }
        other => anyhow::bail!("不支持的 transport: {}", other),
    }
}

/// 连接并注册一个 tool server，失败只 warning。
async fn register_one(proxy: &ToolProxy, config: ClientConfig, filter: &ToolFilter) {
    let name = config.name().to_string();
    log::info!("[REGISTER] connecting tool server: {} ({:?})", name, config);
    if let Err(e) = proxy
        .add_tool_server_with_filter(config, filter.clone())
        .await
    {
        log::warn!("[REGISTER] failed to connect '{}': {} (skipping)", name, e);
    }
}
