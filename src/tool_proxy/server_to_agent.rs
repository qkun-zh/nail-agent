




use std::future::Future;
use std::sync::Arc;

use rmcp::{
    RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, ErrorData, Implementation, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo,
    },
    service::{MaybeSendFuture, RequestContext},
};

use crate::tool_proxy::ToolProxy;





pub struct ToolProxyServer {
    proxy: Arc<ToolProxy>,
}

impl ToolProxyServer {
    pub fn new(proxy: Arc<ToolProxy>) -> Self {
        Self { proxy }
    }
}

impl ServerHandler for ToolProxyServer {
    fn get_info(&self) -> ServerInfo {

        ServerInfo::new(ServerCapabilities::default()).with_server_info(Implementation::new(
            "nail-tool-proxy",
            env!("CARGO_PKG_VERSION"),
        ))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + MaybeSendFuture + '_ {
        let proxy = self.proxy.clone();
        async move {
            let tools = proxy
                .list_all_tools()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            Ok(ListToolsResult::with_all_items(tools))
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + MaybeSendFuture + '_ {
        let proxy = self.proxy.clone();
        let name = request.name.clone().into_owned();
        let arguments = request.arguments.unwrap_or_default();
        async move {
            proxy
                .call_tool(&name, arguments)
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))
        }
    }
}
