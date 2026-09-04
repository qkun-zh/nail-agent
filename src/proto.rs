//! ACP wire handlers: one typed handler per client request.
//!
//! The prompt handler only starts the turn: the turn itself runs in a spawned
//! task (via [`ConnectionTo::spawn`]) so the dispatch loop keeps servicing
//! `session/cancel` while chunks stream out. Turn logic lives in later
//! phases; this phase echoes the prompt back in streamed chunks.

use agent_client_protocol::schema::v1::{
    AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateRequest, AuthenticateResponse,
    CancelNotification, CloseSessionRequest, CloseSessionResponse, ContentBlock, ContentChunk,
    CurrentModeUpdate, Implementation, InitializeRequest, InitializeResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
    ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities, SessionId, SessionMode,
    SessionModeState, SessionNotification, SessionResumeCapabilities, SessionUpdate,
    SetSessionModeRequest, SetSessionModeResponse, StopReason, TextContent, UsageUpdate,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Responder, Stdio};
use tokio::sync::watch;

use crate::core::{SessionState, Sessions};
use crate::llm::{self, Llm, TurnResult};
use crate::tools::mcp::McpPool;

/// Modes broadcast on every new session.
fn advertised_modes() -> SessionModeState {
    SessionModeState::new(
        llm::DEFAULT_MODEL,
        llm::available_modes()
            .into_iter()
            .map(|m| SessionMode::new(m.id, m.name).description(m.description))
            .collect(),
    )
}

/// Register every handler and serve stdio until the transport closes.
pub async fn serve(sessions: std::sync::Arc<Sessions>) -> Result<(), agent_client_protocol::Error> {
    let pool = std::sync::Arc::new(McpPool::new());
    Agent
        .builder()
        .name("nail-agent")
        .on_receive_request(
            async move |request: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(request.protocol_version)
                        .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                            "api-key",
                            "Model API key",
                        ))])
                        .agent_capabilities(
                            AgentCapabilities::new()
                                .load_session(true)
                                .session_capabilities(
                                    SessionCapabilities::new()
                                        .resume(SessionResumeCapabilities::new()),
                                ),
                        )
                        .agent_info(Implementation::new(
                            "nail-agent",
                            env!("CARGO_PKG_VERSION"),
                        )),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        // `authenticate` — the registry-required agent-type method: succeeds
        // when the server itself holds a model key (env or key file).
        .on_receive_request(
            async move |request: AuthenticateRequest, responder, _cx| {
                let method: &str = &request.method_id.0;
                if method != "api-key" {
                    return responder.respond_with_error(
                        agent_client_protocol::Error::invalid_request()
                            .data(format!("unknown auth method {method}")),
                    );
                }
                match llm::LlmConfig::load() {
                    Ok(_) => responder.respond(AuthenticateResponse::new()),
                    Err(hint) => responder.respond_with_error(
                        agent_client_protocol::Error::invalid_request().data(hint),
                    ),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = sessions.clone();
                async move |request: NewSessionRequest, responder, _cx| {
                    let id =
                        SessionId::from(sessions.create(request.cwd, request.mcp_servers));
                    responder.respond(NewSessionResponse::new(id).modes(advertised_modes()))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = sessions.clone();
                let pool = pool.clone();
                async move |request: PromptRequest,
                            responder: Responder<PromptResponse>,
                            cx: ConnectionTo<Client>| {
                    let session_id = request.session_id.clone();
                    let key = session_id.0.to_string();
                    if !sessions.exists(&key) {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_request()
                                .data(format!("unknown session {key}")),
                        );
                    }
                    let text = prompt_text(&request.prompt);
                    let _ = cx.spawn(run_turn(
                        sessions.clone(),
                        pool.clone(),
                        cx.clone(),
                        session_id,
                        key,
                        text,
                        responder,
                    ));
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = sessions.clone();
                async move |request: SetSessionModeRequest, responder, cx: ConnectionTo<Client>| {
                    let key: &str = &request.session_id.0;
                    let mode = request.mode_id.0.to_string();
                    if !sessions.exists(key) {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_request()
                                .data(format!("unknown session {key}")),
                        );
                    }
                    if llm::model_for_mode(&mode).is_none() {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_request()
                                .data(format!("unknown mode {mode}")),
                        );
                    }
                    sessions.set_mode(key, &mode);
                    let _ = cx.send_notification(SessionNotification::new(
                        request.session_id,
                        SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode)),
                    ));
                    responder.respond(SetSessionModeResponse::new())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = sessions.clone();
                let pool = pool.clone();
                async move |request: CloseSessionRequest, responder, _cx| {
                    let key: &str = &request.session_id.0;
                    sessions.remove(key);
                    pool.drop_session(key).await;
                    responder.respond(CloseSessionResponse::new())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // `session/load` — restore the session and replay its transcript so
        // the client can render history (user/assistant text; tool traffic
        // is execution detail and stays out of the replay).
        .on_receive_request(
            {
                let sessions = sessions.clone();
                async move |request: LoadSessionRequest,
                            responder,
                            cx: ConnectionTo<Client>| {
                    let key: &str = &request.session_id.0;
                    if !sessions.restore(key) {
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_request()
                                .data(format!("unknown session {key}")),
                        );
                    }
                    sessions.set_mcp_servers(key, request.mcp_servers);
                    let transcript = sessions.transcript_of(key).unwrap_or_default();
                    for message in &transcript {
                        let Some((is_user, text)) = message_text(message) else {
                            continue;
                        };
                        let update = if is_user {
                            SessionUpdate::UserMessageChunk(ContentChunk::new(
                                ContentBlock::Text(TextContent::new(text)),
                            ))
                        } else {
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                ContentBlock::Text(TextContent::new(text)),
                            ))
                        };
                        let _ = cx.send_notification(SessionNotification::new(
                            request.session_id.clone(),
                            update,
                        ));
                    }
                    responder
                        .respond(LoadSessionResponse::new().modes(advertised_modes()))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = sessions.clone();
                async move |request: ResumeSessionRequest, responder, _cx| {                    let key: &str = &request.session_id.0;
                    if sessions.restore(key) {
                        // Fresh server list wins over whatever was stored.
                        sessions.set_mcp_servers(key, request.mcp_servers);
                        responder.respond(
                            ResumeSessionResponse::new().modes(advertised_modes()),
                        )
                    } else {
                        responder.respond_with_error(
                            agent_client_protocol::Error::invalid_request()
                                .data(format!("unknown session {key}")),
                        )
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let sessions = sessions.clone();
                async move |notification: CancelNotification, _cx| {
                    let key: &str = &notification.session_id.0;
                    if sessions.exists(key) {
                        sessions.cancel(key);
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await
}

/// Pull printable text out of a prompt's content blocks.
fn prompt_text(prompt: &[ContentBlock]) -> String {
    let mut out = String::new();
    for block in prompt {
        match block {
            ContentBlock::Text(text) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&text.text);
            }
            ContentBlock::ResourceLink(link) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&link.uri);
            }
            _ => {}
        }
    }
    out
}

/// Turn logic: history in, streamed answer out, transcript saved.
/// Tools arrive with the tools layer; until then an (unexpected) tool call
/// is reported as text and the turn ends.
async fn run_turn(
    sessions: std::sync::Arc<Sessions>,
    pool: std::sync::Arc<McpPool>,
    cx: ConnectionTo<Client>,
    session_id: SessionId,
    session_key: String,
    text: String,
    responder: Responder<PromptResponse>,
) -> agent_client_protocol::Result<()> {
    let mut cancel = match sessions.cancel_watcher(&session_key) {
        Some(watcher) => watcher,
        None => {
            let _ = responder.respond_with_error(agent_client_protocol::Error::invalid_request()
                .data(format!("unknown session {session_key}")));
            return Ok(());
        }
    };
    sessions.set_state(&session_key, SessionState::Active);

    let model = sessions
        .mode_of(&session_key)
        .unwrap_or_else(|| llm::DEFAULT_MODEL.to_string());
    let mut transcript = sessions.transcript_of(&session_key).unwrap_or_default();
    transcript.push(Llm::user(&text));

    let stop = match llm::LlmConfig::load().map(|c| llm::Llm::new(&c.with_model(&model))) {
        Ok(llm) => {
            let (stop, usage) = run_chat_loop(&sessions, &pool, &cx, &session_id, &session_key, &mut cancel, &model, &llm, &mut transcript).await;
            // Report cumulative session usage so the client can display cost.
            // 1M context window is what all three session models offer.
            let used = sessions.add_usage(&session_key, usage).total();
            if used > 0 {
                let _ = cx.send_notification(SessionNotification::new(
                    session_id.clone(),
                    SessionUpdate::UsageUpdate(UsageUpdate::new(used, 1_000_000)),
                ));
            }
            stop
        }
        Err(hint) => {
            if stream_text(&cx, &session_id, &mut cancel, &hint).await {
                StopReason::EndTurn
            } else {
                StopReason::Cancelled
            }
        }
    };

    sessions.save_transcript(&session_key, transcript);
    sessions.set_state(
        &session_key,
        match stop {
            StopReason::EndTurn => SessionState::Completed,
            StopReason::Cancelled => SessionState::Cancelled,
            _ => SessionState::Failed,
        },
    );
    sessions.bump_turn(&session_key);
    let _ = responder.respond(PromptResponse::new(stop));
    Ok(())
}

/// Extract replayable (user, text) or (assistant, text) from a transcript
/// message. Tool traffic is skipped: it is execution detail, and replaying
/// stale tool calls could confuse the client. Returns the role flag plus text.
fn message_text(
    message: &async_openai::types::chat::ChatCompletionRequestMessage,
) -> Option<(bool, String)> {
    use async_openai::types::chat::{
        ChatCompletionRequestAssistantMessageContent, ChatCompletionRequestMessage,
        ChatCompletionRequestUserMessageContent,
    };
    match message {
        ChatCompletionRequestMessage::User(msg) => {
            let text = match &msg.content {
                ChatCompletionRequestUserMessageContent::Text(text) => text.clone(),
                ChatCompletionRequestUserMessageContent::Array(parts) => parts
                    .iter()
                    .filter_map(|p| match p {
                        async_openai::types::chat::ChatCompletionRequestUserMessageContentPart::Text(
                            t,
                        ) => Some(t.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            if text.is_empty() { None } else { Some((true, text)) }
        }
        ChatCompletionRequestMessage::Assistant(msg) => {
            let content = msg.content.as_ref()?;
            let text = match content {
                ChatCompletionRequestAssistantMessageContent::Text(text) => text.clone(),
                ChatCompletionRequestAssistantMessageContent::Array(parts) => parts
                    .iter()
                    .filter_map(|p| match p {
                        async_openai::types::chat::ChatCompletionRequestAssistantMessageContentPart::Text(
                            t,
                        ) => Some(t.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            if text.is_empty() {
                None
            } else {
                Some((false, text))
            }
        }
        _ => None,
    }
}

/// Upper bound on model→tool→model rounds per prompt turn.
const MAX_TOOL_TURNS: usize = 8;

/// Run model turns until the answer is final: each round streams text (also
/// buffered into the transcript), executes requested local tools, and feeds
/// the results back as `tool` messages.
#[allow(clippy::too_many_arguments)]
async fn run_chat_loop(
    sessions: &Sessions,
    pool: &McpPool,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    session_key: &str,
    cancel: &mut watch::Receiver<bool>,
    model: &str,
    llm: &Llm,
    transcript: &mut Vec<async_openai::types::chat::ChatCompletionRequestMessage>,
) -> (StopReason, crate::llm::Usage) {
    use crate::tools::local_tool_defs;

    let mut tools = local_tool_defs();
    // Merge MCP tools (connects missing servers lazily).
    if let Some(servers) = sessions.mcp_servers_of(session_key)
        && !servers.is_empty()
    {
        for def in pool.tools_for(session_key, &servers).await {
            tools.push(Llm::tool_def(
                def.namespaced,
                def.description,
                def.parameters,
            ));
        }
    }
    let mut turn_usage = crate::llm::Usage::default();
    for _ in 0..MAX_TOOL_TURNS {
        // Fresh owned captures per round: the callback moves them and the
        // stream loop keeps its own receiver clone, so nothing is borrowed
        // twice and the future stays `Send`.
        let reply_sink = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let sink = reply_sink.clone();
        let cx_owned = cx.clone();
        let session_owned = session_id.clone();
        let mut cancel_stream = cancel.clone();
        // Thought chunks stream alongside the answer; they are display-only
        // and never enter the transcript (OpenAI convention).
        let thought_cx = cx.clone();
        let thought_session = session_id.clone();
        let mut thought_cancel = cancel.clone();
        match llm
            .chat_once(
                model,
                transcript.clone(),
                tools.clone(),
                cancel,
                async move |delta| {
                    if let Ok(mut reply) = sink.lock() {
                        reply.push_str(delta);
                    }
                    stream_text(&cx_owned, &session_owned, &mut cancel_stream, delta).await
                },
                async move |thought| {
                    stream_thought(&thought_cx, &thought_session, &mut thought_cancel, thought)
                        .await
                },
            )
            .await
        {
            TurnResult::TextDone { usage } => {
                turn_usage.add(usage);
                let full = reply_sink.lock().map(|r| r.clone()).unwrap_or_default();
                if !full.is_empty() {
                    transcript.push(Llm::assistant(&full));
                }
                return (StopReason::EndTurn, turn_usage);
            }
            TurnResult::Cancelled => return (StopReason::Cancelled, turn_usage),
            TurnResult::Error(detail) => {
                return if stream_text(cx, session_id, cancel, &detail).await {
                    (StopReason::EndTurn, turn_usage)
                } else {
                    (StopReason::Cancelled, turn_usage)
                };
            }
            TurnResult::ToolCalls { assistant, calls, usage } => {
                turn_usage.add(usage);
                transcript.push(assistant);
                let mut stopped = false;
                for call in &calls {
                    let mut ctx = crate::tools::CallCtx {
                        sessions,
                        cx,
                        session_id,
                        session_key,
                        cancel,
                    };
                    let (output, was_cancelled) = match call.name.as_str() {
                        "run" | "read" | "write" => {
                            let result =
                                crate::tools::execute_local(&mut ctx, &call.name, &call.arguments)
                                    .await;
                            (result.output, result.cancelled)
                        }
                        namespaced => {
                            let result = crate::tools::mcp::execute_mcp(
                                &mut ctx,
                                pool,
                                namespaced,
                                &call.arguments,
                            )
                            .await;
                            (result.output, result.cancelled)
                        }
                    };
                    if was_cancelled {
                        stopped = true;
                        break;
                    }
                    transcript.push(Llm::tool_result(&call.id, output));
                }
                if stopped {
                    return (StopReason::Cancelled, turn_usage);
                }
            }
        }
    }
    if stream_text(cx, session_id, cancel, "（工具调用轮数已达上限，本轮结束）").await {
        (StopReason::EndTurn, turn_usage)
    } else {
        (StopReason::Cancelled, turn_usage)
    }
}

/// Stream one thinking chunk (display-only, like Zed's native agent).
/// Returns `false` when cancelled or the transport broke.
async fn stream_thought(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    cancel: &mut watch::Receiver<bool>,
    text: &str,
) -> bool {
    if *cancel.borrow_and_update() {
        return false;
    }
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new(text),
        ))),
    ))
    .is_ok()
}

fn text_chunk(session_id: &SessionId, text: &str) -> SessionNotification {    SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new(text),
        ))),
    )
}

/// Stream `text` in small pieces so cancellation has a window to land.
/// Returns `true` when the whole text went out, `false` when cancelled.
async fn stream_text(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    cancel: &mut watch::Receiver<bool>,
    text: &str,
) -> bool {
    let mut buf = String::new();
    for ch in text.chars() {
        buf.push(ch);
        if buf.len() >= 24 || ch == '\n' {
            if *cancel.borrow_and_update() {
                return false;
            }
            if cx.send_notification(text_chunk(session_id, &buf)).is_err() {
                return false;
            }
            buf.clear();
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }
    if !buf.is_empty() {
        if *cancel.borrow_and_update() {
            return false;
        }
        if cx.send_notification(text_chunk(session_id, &buf)).is_err() {
            return false;
        }
    }
    true
}
