use std::env;
use std::sync::Arc;

use agent_client_protocol::{
    Agent, Client, ConnectTo, ConnectionTo, Dispatch, Responder, Result, Stdio,
    schema::{
        AgentCapabilities, ContentChunk, InitializeRequest, InitializeResponse, LoadSessionRequest,
        LoadSessionResponse, MessageId, NewSessionRequest, NewSessionResponse, PromptRequest,
        PromptResponse, SessionId, SessionNotification, SessionUpdate, StopReason,
    },
};
use anyhow::Context;
use dotenvy::dotenv;
use nail_agent::{
    AgentConfig, AppState, Llm, db, fsm, logger,
    tool_proxy::server_to_agent::ToolProxyServer,
    tool_proxy::{McpHandle, NullClientHandler, ToolProxy},
};
use rmcp::ServiceExt;
use tokio::io::duplex;

async fn handle_initialize(req: InitializeRequest) -> InitializeResponse {
    logger::log_json("InitializeRequest", &req);
    let mut caps = AgentCapabilities::new();
    caps.load_session = true;
    let response = InitializeResponse::new(req.protocol_version).agent_capabilities(caps);
    logger::log_json("InitializeResponse", &response);
    response
}

async fn handle_new_session(req: NewSessionRequest) -> NewSessionResponse {
    logger::log_json("NewSessionRequest", &req);
    let session_id = SessionId::new(uuid::Uuid::now_v7().to_string());
    let response = NewSessionResponse::new(session_id);
    logger::log_json("NewSessionResponse", &response);
    response
}

async fn handle_load_session(
    req: LoadSessionRequest,
    connection: ConnectionTo<Client>,
) -> LoadSessionResponse {
    let history = match db::load_session_history(&req.session_id.to_string()).await {
        Ok(h) => h,
        Err(e) => {
            log::error!("failed to load session history: {}", e);
            return LoadSessionResponse::default();
        }
    };

    for msg in history {
        let chunk = ContentChunk::new(agent_client_protocol::schema::ContentBlock::Text(
            agent_client_protocol::schema::TextContent::new(msg.content),
        ))
        .message_id(Some(MessageId::new(msg.id)));

        let update = match msg.role.as_str() {
            "user" => SessionUpdate::UserMessageChunk(chunk),
            "assistant" => SessionUpdate::AgentMessageChunk(chunk),
            _ => continue,
        };
        let notification = SessionNotification::new(req.session_id.clone(), update);
        if let Err(e) = connection.send_notification(notification) {
            log::error!("failed to send history notification: {}", e);
        }
    }

    LoadSessionResponse::default()
}

async fn init_tool_proxy() -> anyhow::Result<McpHandle> {
    log::info!("[TOOL-PROXY] initializing Tool Proxy...");
    let proxy = ToolProxy::new();

    // 注册所有 tool server（失败自动 warning 跳过，不影响其他 server）
    nail_agent::tool_proxy::client_to_tool_servers::register_all(&proxy).await;

    let proxy = Arc::new(proxy);

    log::info!("[TOOL-PROXY] creating MCP duplex channel...");
    let (server_io, client_io) = duplex(1024 * 64);

    let proxy_server = ToolProxyServer::new(proxy);
    tokio::spawn(async move {
        log::info!("[TOOL-PROXY] Tool Proxy server started (in-process duplex)");
        match proxy_server.serve(server_io).await {
            Ok(service) => {
                if let Err(e) = service.waiting().await {
                    log::error!("[TOOL-PROXY] Tool Proxy server waiting error: {}", e);
                }
            }
            Err(e) => {
                log::error!("[TOOL-PROXY] Tool Proxy server error: {}", e);
            }
        }
    });

    let client_handle = NullClientHandler
        .serve(client_io)
        .await
        .context("failed to connect to Proxy via MCP protocol")?;

    log::info!("[TOOL-PROXY] Tool Proxy initialized (communicating via MCP protocol)");
    Ok(client_handle)
}

// -------------------- run ACP Agent --------------------

async fn run_agent(
    transport: impl ConnectTo<Agent>,
    state: Arc<AppState>,
) -> Result<(), agent_client_protocol::Error> {
    Agent
        .builder()
        .on_receive_request(
            move |req: InitializeRequest, responder: Responder<InitializeResponse>, _connection| {
                async move { responder.respond(handle_initialize(req).await) }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            move |req: NewSessionRequest, responder: Responder<NewSessionResponse>, _connection| {
                async move { responder.respond(handle_new_session(req).await) }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            |req: LoadSessionRequest,
             responder: Responder<LoadSessionResponse>,
             conn: ConnectionTo<Client>| async move {
                responder.respond(handle_load_session(req, conn).await)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                move |req: PromptRequest,
                      responder: Responder<PromptResponse>,
                      connection: ConnectionTo<Client>| {
                    let state = state.clone();
                    async move {
                        let result = fsm::driver::run(req, connection, state).await;
                        match result {
                            Ok(resp) => responder.respond(resp),
                            Err(e) => {
                                log::error!("prompt processing failed: {}", e);
                                responder.respond(PromptResponse::new(StopReason::EndTurn))
                            }
                        }
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_dispatch(
            move |msg: Dispatch, cx: ConnectionTo<Client>| async move {
                msg.respond_with_error(agent_client_protocol::Error::method_not_found(), cx)
            },
            agent_client_protocol::on_receive_dispatch!(),
        )
        .connect_to(transport)
        .await
}

// -------------------- main entry point --------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let main_timer = logger::Timer::start("main");

    dotenv().context("failed to load .env file")?;
    logger::init_logger().context("failed to initialize logging system")?;

    log::info!("========================================");
    log::info!("[MAIN] nail-agent starting");
    log::info!("[MAIN] version: {}", env!("CARGO_PKG_VERSION"));
    log::info!("========================================");

    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "<not set>".into());
    let model_name = env::var("MODEL_NAME").unwrap_or_else(|_| "<not set>".into());
    log::info!(
        "[MAIN] LLM config: BASE_URL={}, MODEL_NAME={}",
        base_url,
        model_name
    );
    log::info!("[MAIN] API_KEY set: {}", env::var("API_KEY").is_ok());

    log::info!("[MAIN] initializing database...");
    let db_path = "./agent_data";
    db::init_db(db_path)
        .await
        .context("failed to initialize database")?;
    log::info!("[MAIN] database started, path: {}", db_path);

    log::info!("[MAIN] starting Tool Proxy (MCP protocol)...");
    let proxy_handle = init_tool_proxy().await?;
    log::info!("[MAIN] Tool Proxy started");

    let config = AgentConfig::from_env()?;
    log::info!("[MAIN] creating LLM client...");
    let llm = Llm::with_config(
        async_openai::config::OpenAIConfig::new()
            .with_api_base(config.base_url)
            .with_api_key(config.api_key),
    );

    let state = Arc::new(AppState {
        llm,
        model_name: config.model_name,
        proxy_handle,
    });

    log::info!("[MAIN] initialization complete, entering ACP Agent main loop");
    log::info!("========================================");

    run_agent(Stdio::new(), state)
        .await
        .map_err(|e| anyhow::anyhow!("running agent failed: {}", e))?;

    main_timer.stop();
    log::info!("[MAIN] nail-agent normal exit");
    Ok(())
}
