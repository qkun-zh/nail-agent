use std::future::Future;
use std::sync::Arc;

use anyhow::Context;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ErrorData, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{MaybeSendFuture, RequestContext};
use rmcp::{ClientHandler, RoleClient, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Map, Value};
use tokio::io::duplex;

pub trait ToolHandler: Send + Sync {
    fn tool_spec(&self) -> rmcp::model::Tool;
    fn call(&self, arguments: Map<String, Value>) -> anyhow::Result<CallToolResult>;
}

pub struct SimpleTool {
    spec: rmcp::model::Tool,
    handler: Box<dyn Fn(Map<String, Value>) -> anyhow::Result<CallToolResult> + Send + Sync>,
}

impl SimpleTool {
    pub fn new(
        spec: rmcp::model::Tool,
        handler: impl Fn(Map<String, Value>) -> anyhow::Result<CallToolResult> + Send + Sync + 'static,
    ) -> Self {
        Self {
            spec,
            handler: Box::new(handler),
        }
    }
}

impl ToolHandler for SimpleTool {
    fn tool_spec(&self) -> rmcp::model::Tool {
        self.spec.clone()
    }

    fn call(&self, arguments: Map<String, Value>) -> anyhow::Result<CallToolResult> {
        (self.handler)(arguments)
    }
}

// ── In-process MCP Server ──────────────────────────────────────────────

struct FunctionCallServer {
    name: String,
    handlers: Vec<Arc<dyn ToolHandler>>,
}

impl FunctionCallServer {
    fn new(name: String, handlers: Vec<Arc<dyn ToolHandler>>) -> Self {
        Self { name, handlers }
    }
}

impl ServerHandler for FunctionCallServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(&self.name, env!("CARGO_PKG_VERSION")))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + MaybeSendFuture + '_ {
        let tools: Vec<rmcp::model::Tool> = self.handlers.iter().map(|h| h.tool_spec()).collect();
        async move { Ok(ListToolsResult::with_all_items(tools)) }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + MaybeSendFuture + '_ {
        let name = request.name.clone().into_owned();
        let arguments = request.arguments.unwrap_or_default();

        async move {
            for handler in &self.handlers {
                if handler.tool_spec().name == name {
                    return handler
                        .call(arguments)
                        .map_err(|e| ErrorData::internal_error(e.to_string(), None));
                }
            }
            Err(ErrorData::invalid_params(
                format!(
                    "Tool '{}' not found in FunctionCall server '{}'",
                    name, self.name
                ),
                None,
            ))
        }
    }
}

// ── In-process MCP Client ─────────────────────────────────────────────

#[derive(Clone, Default)]
struct LocalClientHandler;
impl ClientHandler for LocalClientHandler {}

pub struct FunctionCallClient {
    inner: rmcp::service::RunningService<RoleClient, LocalClientHandler>,
    pub name: String,
}

impl FunctionCallClient {
    pub async fn list_tools(&mut self) -> anyhow::Result<Vec<rmcp::model::Tool>> {
        self.inner
            .list_all_tools()
            .await
            .context("FunctionCallClient tools/list failed")
    }

    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> anyhow::Result<CallToolResult> {
        let params = CallToolRequestParams::new(name.to_string()).with_arguments(arguments);
        self.inner
            .call_tool(params)
            .await
            .context("FunctionCallClient tools/call failed")
    }
}

/// Creates an in-process MCP connection via a duplex channel.
pub async fn connect_duplex(
    name: String,
    handlers: Vec<Arc<dyn ToolHandler>>,
) -> anyhow::Result<FunctionCallClient> {
    let (server_io, client_io) = duplex(1024 * 64);

    let server_name = name.clone();
    let server = FunctionCallServer::new(name.clone(), handlers);
    tokio::spawn(async move {
        log::info!("[FUNC] in-process server '{}' started", server_name);
        match server.serve(server_io).await {
            Ok(service) => {
                if let Err(e) = service.waiting().await {
                    log::error!("[FUNC] server '{}' waiting error: {}", server_name, e);
                }
            }
            Err(e) => {
                log::error!("[FUNC] server '{}' error: {}", server_name, e);
            }
        }
    });

    let inner = LocalClientHandler
        .serve(client_io)
        .await
        .context("failed to connect to in-process MCP server")?;

    log::info!("[FUNC] in-process client '{}' connected", name);
    Ok(FunctionCallClient { inner, name })
}
