use std::sync::Arc;

use rmcp::{ServiceExt, transport::stdio};

use nail_agent::tool_proxy::ToolProxy;
use nail_agent::tool_proxy::server_to_agent::ToolProxyServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    eprintln!("========================================");
    eprintln!("[nail-tool-proxy] starting");
    eprintln!("[nail-tool-proxy] version: {}", env!("CARGO_PKG_VERSION"));
    eprintln!("========================================");

    let proxy = ToolProxy::new();
    nail_agent::tool_proxy::client_to_tool_servers::register_all(&proxy).await;

    let server = ToolProxyServer::new(Arc::new(proxy));

    eprintln!("[nail-tool-proxy] serving MCP on stdio...");
    eprintln!("[nail-tool-proxy] ready, waiting for MCP client...");

    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
