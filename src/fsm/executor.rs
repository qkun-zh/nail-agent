use agent_client_protocol::{
    Client, ConnectionTo,
    schema::{
        ContentBlock, ContentChunk, MessageId, SessionNotification, SessionUpdate, TextContent,
        ToolCall, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    },
};
use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
    ChatCompletionRequestToolMessage, ChatCompletionRequestToolMessageContent, ChatCompletionTools,
    CreateChatCompletionRequestArgs, FunctionCall,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::Llm;
use crate::fsm::effect::Effect;
use crate::fsm::event::Event;
use crate::tool_proxy::McpHandle;
macro_rules! log_json {
    ($label:expr, $val:expr) => {
        crate::logger::log_json($label, &$val);
    };
}

pub struct ExecutorContext<'a> {
    pub llm: &'a Llm,
    pub model_name: &'a str,
    /// MCP proxy handle connected via the MCP protocol
    pub proxy_handle: &'a McpHandle,
    pub connection: &'a ConnectionTo<Client>,
}

pub async fn execute_effect(
    effect: Effect,
    ctx: &ExecutorContext<'_>,
) -> anyhow::Result<Vec<Event>> {
    match effect {
        Effect::LoadContext { session_id } => execute_load_context(session_id, ctx).await,

        Effect::CallModel {
            messages,
            tools,
            session_id,
        } => execute_call_llm_stream(messages, tools, session_id, ctx).await,

        Effect::CallTools {
            messages,
            tool_calls,
            session_id,
        } => execute_call_tools(messages, tool_calls, session_id, ctx).await,

        Effect::SaveSession {
            content,
            session_id,
        } => execute_save_session(content, session_id, ctx).await,

        Effect::DoNothing => Ok(vec![]),
    }
}

async fn execute_load_context(
    session_id: agent_client_protocol::schema::SessionId,
    ctx: &ExecutorContext<'_>,
) -> anyhow::Result<Vec<Event>> {
    let _timer = crate::logger::Timer::start("execute_load_context");
    let session_id_str = session_id.to_string();

    log::info!("[EXEC] LoadContext started session={}", session_id_str);

    // parallel loading: history + MCP tools
    log::info!("[EXEC] parallel loading: session history + MCP tool list");
    let join_timer = crate::logger::Timer::start("tokio::join(load_history, fetch_tools)");
    let (history_result, tools_result) = tokio::join!(
        crate::db::load_session_history(&session_id_str),
        fetch_openai_tools(ctx.proxy_handle)
    );
    join_timer.stop();

    let history = history_result.unwrap_or_else(|e| {
        log::error!("[EXEC] failed to load session history: {}", e);
        vec![]
    });

    let tools = tools_result.unwrap_or_else(|e| {
        log::error!("[EXEC] failed to get tool list: {}", e);
        vec![]
    });

    log::info!(
        "[EXEC] data loaded: history={}, tools={}",
        history.len(),
        tools.len()
    );

    // convert history records to LLM messages
    let convert_timer = crate::logger::Timer::start("convert_history_to_messages");
    let mut messages = Vec::with_capacity(history.len());
    let mut user_msg_count = 0usize;
    let mut asst_msg_count = 0usize;
    let mut total_chars = 0usize;
    for msg in &history {
        match msg.role.as_str() {
            "user" => {
                messages.push(ChatCompletionRequestMessage::User(
                    async_openai::types::chat::ChatCompletionRequestUserMessage {
                        content: msg.content.clone().into(),
                        name: None,
                    },
                ));
                user_msg_count += 1;
                total_chars += msg.content.len();
            }
            "assistant" => {
                messages.push(ChatCompletionRequestMessage::Assistant(
                    async_openai::types::chat::ChatCompletionRequestAssistantMessage {
                        content: Some(
                            async_openai::types::chat::
                            ChatCompletionRequestAssistantMessageContent::Text(
                                msg.content.clone(),
                            ),
                        ),
                        name: None,
                        ..Default::default()
                    },
                ));
                asst_msg_count += 1;
                total_chars += msg.content.len();
            }
            _ => {}
        }
    }
    convert_timer.stop();

    log::info!(
        "[EXEC] history message conversion complete: user={}, assistant={}, total_chars={}",
        user_msg_count,
        asst_msg_count,
        total_chars
    );

    // log: record loaded tools
    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| {
            if let ChatCompletionTools::Function(f) = t {
                Some(f.function.name.as_str())
            } else {
                None
            }
        })
        .collect();
    log_json!(
        "LoadContext",
        serde_json::json!({
            "sessionId": session_id.to_string(),
            "historyCount": messages.len(),
            "historyTotalChars": total_chars,
            "tools": tool_names,
        })
    );

    Ok(vec![Event::ContextLoaded { messages, tools }])
}

/// CallLlmStream: stream-based LLM API call with real-time text chunk push
async fn execute_call_llm_stream(
    messages: Vec<ChatCompletionRequestMessage>,
    tools: Vec<ChatCompletionTools>,
    session_id: agent_client_protocol::schema::SessionId,
    ctx: &ExecutorContext<'_>,
) -> anyhow::Result<Vec<Event>> {
    use futures::StreamExt;

    let _timer = crate::logger::Timer::start("execute_call_llm_stream");

    let messages_json_size = serde_json::to_string(&messages)
        .map(|s| s.len())
        .unwrap_or(0);
    let tools_json_size = serde_json::to_string(&tools).map(|s| s.len()).unwrap_or(0);

    log::info!(
        "[LLM] building streaming request: model={}, messages={} ({}bytes), tools={} ({}bytes)",
        ctx.model_name,
        messages.len(),
        messages_json_size,
        tools.len(),
        tools_json_size,
    );

    if messages_json_size > 1_000_000 {
        log::warn!(
            "[LLM] message body large: {} bytes, may cause slow LLM call",
            messages_json_size
        );
    }

    let mut args = CreateChatCompletionRequestArgs::default();
    args.model(ctx.model_name)
        .messages(messages.clone())
        .stream(true);
    if !tools.is_empty() {
        args.tools(tools);
    }
    let request = args.build()?;

    log::info!("[LLM] streaming request built, calling API");

    let mut stream = ctx.llm.chat().create_stream(request).await?;

    let mut events = Vec::new();
    let mut full_content = String::new();

    let mut tool_calls_acc: Vec<(i32, Option<String>, String, String)> = Vec::new();
    let mut is_first_delta = true;
    let msg_id = Some(MessageId::new(Uuid::now_v7().to_string()));

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        for choice in chunk.choices {
            if let Some(ref text) = choice.delta.content {
                if !text.is_empty() {
                    send_chunk_notification(ctx.connection, &session_id, &msg_id, text).await?;
                    full_content.push_str(text);
                }
                if is_first_delta {
                    is_first_delta = false;
                    events.push(Event::FirstChunkOfModelResponseArrived {
                        delta: text.clone(),
                    });
                } else if !text.is_empty() {
                    events.push(Event::NextChunkOfModelResponseArrived {
                        delta: text.clone(),
                    });
                }
            }

            if let Some(ref deltas) = choice.delta.tool_calls {
                for delta in deltas {
                    let idx = delta.index as i32;
                    let pos = tool_calls_acc.iter().position(|(i, _, _, _)| *i == idx);
                    if let Some(p) = pos {
                        let (_, id, name, args) = &mut tool_calls_acc[p];
                        if let Some(new_id) = &delta.id {
                            *id = Some(new_id.clone());
                        }
                        if let Some(ref f) = delta.function {
                            if let Some(n) = &f.name {
                                name.push_str(n);
                            }
                            if let Some(a) = &f.arguments {
                                args.push_str(a);
                            }
                        }
                    } else {
                        let mut id = None;
                        let mut name = String::new();
                        let mut args = String::new();
                        if let Some(new_id) = &delta.id {
                            id = Some(new_id.clone());
                        }
                        if let Some(ref f) = delta.function {
                            if let Some(n) = &f.name {
                                name.push_str(n);
                            }
                            if let Some(a) = &f.arguments {
                                args.push_str(a);
                            }
                        }
                        tool_calls_acc.push((idx, id, name, args));
                    }
                }
            }
        }
    }

    if tool_calls_acc.is_empty() {
        log::info!(
            "[LLM] stream ended, plain text response: {} chars",
            full_content.len()
        );
        events.push(Event::ModelResponseFinishedWithoutToolCalls { full_content });
    } else {
        log::info!(
            "[LLM] stream ended, detected {} tool calls",
            tool_calls_acc.len()
        );
        let tool_calls: Vec<ChatCompletionMessageToolCalls> = tool_calls_acc
            .into_iter()
            .filter_map(|(_idx, id, name, args)| {
                id.map(|id| {
                    ChatCompletionMessageToolCalls::Function(ChatCompletionMessageToolCall {
                        id,
                        function: FunctionCall {
                            name,
                            arguments: args,
                        },
                    })
                })
            })
            .collect();
        events.push(Event::ModelResponseFinishedWithToolCalls { tool_calls });
    }

    Ok(events)
}

/// CallTools: execute MCP tool calls + compose messages
async fn execute_call_tools(
    messages: Vec<ChatCompletionRequestMessage>,
    tool_calls: Vec<ChatCompletionMessageToolCalls>,
    session_id: agent_client_protocol::schema::SessionId,
    ctx: &ExecutorContext<'_>,
) -> anyhow::Result<Vec<Event>> {
    let tool_calls_count = tool_calls.len();
    let _total_timer =
        crate::logger::Timer::start(format!("execute_call_tools({} calls)", tool_calls_count));

    log::info!("[TOOL] ======== executing tools ========");
    log::info!("[TOOL] tools to execute: {}", tool_calls_count);
    log::info!("[TOOL] current message list size: {}", messages.len());

    let mut updated = messages;

    let assistant_msg = ChatCompletionRequestAssistantMessage {
        content: None,
        tool_calls: Some(tool_calls.clone()),
        ..Default::default()
    };
    updated.push(ChatCompletionRequestMessage::Assistant(assistant_msg));

    for (i, tc) in tool_calls.into_iter().enumerate() {
        match tc {
            ChatCompletionMessageToolCalls::Function(call) => {
                let func_name = call.function.name.clone();
                let func_args_str = call.function.arguments.clone();

                log::info!(
                    "[TOOL] [{}/{}] executing: {} (args_len={})",
                    i + 1,
                    tool_calls_count,
                    func_name,
                    func_args_str.len()
                );

                let acp_tool_call = ToolCall::new(call.id.clone(), func_name.clone())
                    .kind(ToolKind::Execute)
                    .status(ToolCallStatus::InProgress)
                    .raw_input(serde_json::from_str(&func_args_str).ok());
                if let Err(e) = ctx.connection.send_notification(SessionNotification::new(
                    session_id.clone(),
                    SessionUpdate::ToolCall(acp_tool_call),
                )) {
                    log::error!("[TOOL] failed to send ToolCall notification: {}", e);
                }

                let tool_timer = crate::logger::Timer::start(format!("tool_call({})", func_name));

                let args: Map<String, Value> =
                    serde_json::from_str(&func_args_str).unwrap_or_default();

                let tool_params =
                    rmcp::model::CallToolRequestParams::new(func_name.clone()).with_arguments(args);
                let result = ctx.proxy_handle.call_tool(tool_params).await;

                let (result_text, is_error) = match result {
                    Ok(res) => {
                        let text = extract_text(&res);
                        log::info!(
                            "[TOOL] [{}/{}] {} success, result length={}chars",
                            i + 1,
                            tool_calls_count,
                            func_name,
                            text.len()
                        );
                        (text, false)
                    }
                    Err(e) => {
                        let err = format!("tool call failed: {:?}", e);
                        log::error!(
                            "[TOOL] [{}/{}] {} failed, error: {}",
                            i + 1,
                            tool_calls_count,
                            func_name,
                            err
                        );
                        (err, true)
                    }
                };

                tool_timer.stop();

                let status = if is_error {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                };
                let update = ToolCallUpdate::new(
                    call.id.clone(),
                    ToolCallUpdateFields::new()
                        .status(status)
                        .raw_output(serde_json::Value::String(result_text.clone())),
                );
                if let Err(e) = ctx.connection.send_notification(SessionNotification::new(
                    session_id.clone(),
                    SessionUpdate::ToolCallUpdate(update),
                )) {
                    log::error!("[TOOL] failed to send ToolCallUpdate notification: {}", e);
                }

                log_json!(
                    "ToolCall",
                    serde_json::json!({
                        "tool": func_name,
                        "index": i,
                        "arguments": func_args_str,
                        "error": is_error,
                        "resultLength": result_text.len(),
                    })
                );

                updated.push(ChatCompletionRequestMessage::Tool(
                    ChatCompletionRequestToolMessage {
                        content: ChatCompletionRequestToolMessageContent::Text(result_text),
                        tool_call_id: call.id.clone(),
                    },
                ));
            }
            _ => log::warn!("[TOOL] skipping custom tool call"),
        }
    }

    log::info!(
        "[TOOL] tool execution complete, message list grew: {}",
        updated.len()
    );
    log::info!("[TOOL] ======== tool execution ended ========");

    Ok(vec![Event::ToolsResponseArrived {
        updated_messages: updated,
    }])
}

/// SaveSession: save final response to database (content already streamed in execute_call_llm_stream)
async fn execute_save_session(
    content: String,
    session_id: agent_client_protocol::schema::SessionId,
    _ctx: &ExecutorContext<'_>,
) -> anyhow::Result<Vec<Event>> {
    let _timer = crate::logger::Timer::start(format!(
        "execute_save_session(session={}, text_len={})",
        session_id,
        content.len()
    ));

    if content.is_empty() {
        log::info!("[SEND] response text is empty, skipping save");
        return Ok(vec![]);
    }

    log::info!(
        "[SEND] saving assistant message to database: len={}",
        content.len()
    );

    // save assistant message to database (content already stream-pushed in execute_call_llm_stream)
    let db_timer = crate::logger::Timer::start("db.append_message(assistant)");
    if let Err(e) = crate::db::append_message(&session_id.to_string(), "assistant", &content).await
    {
        log::error!("[SEND] failed to save assistant message: {}", e);
    }
    db_timer.stop();

    Ok(vec![])
}

/// send streaming text chunk notification (real-time push)
async fn send_chunk_notification(
    connection: &ConnectionTo<Client>,
    session_id: &agent_client_protocol::schema::SessionId,
    msg_id: &Option<MessageId>,
    text: &str,
) -> anyhow::Result<()> {
    let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text.to_string())))
        .message_id(msg_id.clone());
    let update = SessionUpdate::AgentMessageChunk(chunk);
    connection.send_notification(SessionNotification::new(session_id.clone(), update))?;
    Ok(())
}

/// fetch tools from Tool Proxy and convert to OpenAI format
async fn fetch_openai_tools(proxy: &McpHandle) -> anyhow::Result<Vec<ChatCompletionTools>> {
    let _timer = crate::logger::Timer::start("fetch_openai_tools");

    log::info!("[TOOL-PROXY] fetching aggregated tool list...");
    let mcp_tools = proxy.list_all_tools().await?;

    log::info!("[TOOL-PROXY] found {} tools", mcp_tools.len());

    for tool in &mcp_tools {
        let schema_size = serde_json::to_string(&serde_json::Value::Object(
            tool.input_schema.as_ref().clone(),
        ))
        .map(|s| s.len())
        .unwrap_or(0);
        log::info!(
            "[TOOL-PROXY]   tool: {} (schema_size={}bytes, desc={})",
            tool.name,
            schema_size,
            tool.description.as_deref().unwrap_or("<no description>")
        );
    }

    log_json!(
        "ProxyToolsDiscovered",
        serde_json::json!({
            "count": mcp_tools.len(),
            "tools": mcp_tools.iter().map(|t| &t.name).collect::<Vec<_>>(),
        })
    );

    let convert_timer = crate::logger::Timer::start("convert_tools_schema");
    let result: Vec<ChatCompletionTools> = mcp_tools
        .iter()
        .map(|tool| {
            let schema_value = serde_json::Value::Object(tool.input_schema.as_ref().clone());
            ChatCompletionTools::Function(async_openai::types::chat::ChatCompletionTool {
                function: async_openai::types::chat::FunctionObject {
                    name: tool.name.to_string(),
                    description: tool.description.as_ref().map(|d| d.to_string()),
                    parameters: Some(schema_value),
                    strict: None,
                },
            })
        })
        .collect();
    convert_timer.stop();

    log::info!(
        "[TOOL-PROXY] tool list conversion complete: {} tools",
        result.len()
    );
    Ok(result)
}

/// extract text from CallToolResult
fn extract_text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.raw.as_text().map(|text| text.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}
