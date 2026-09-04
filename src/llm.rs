//! Model backends and session modes.
//!
//! One [`Llm`] per process (client is cheaply cloneable and pooled
//! internally). [`chat_once`] runs a single streaming turn with standard
//! function calling: text deltas stream to the caller, tool calls accumulate
//! by index and return for the tools layer to execute. Cancellation drops
//! the request mid-stream.
//!
//! Key lookup order: `NAIL_API_KEY` env → `~/.config/nail-agent/api_key` →
//! legacy `~/.config/zacp/api_key`. Endpoint/model overridable via
//! `NAIL_BASE_URL` / `NAIL_MODEL`.

use async_openai::{
    Client as OpenAIClient,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
        ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageContent,
        ChatCompletionRequestMessage, ChatCompletionTools, CreateChatCompletionRequestArgs,
        FunctionCall,
    },
};
use futures_util::StreamExt;
use tokio::sync::watch;

/// Default endpoint: this project's DashScope workspace (Beijing region).
pub const DEFAULT_BASE_URL: &str =
    "https://ws-dh0kie08xivrnzho.cn-beijing.maas.aliyuncs.com/compatible-mode/v1";
/// Default model: cheapest agent-capable tier on this endpoint.
pub const DEFAULT_MODEL: &str = "qwen3.7-flash";

/// One selectable model, exposed to the client as an ACP session mode.
#[derive(Debug, Clone)]
pub struct ModelMode {
    /// Mode id (= model id on the endpoint).
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

/// All models this agent can switch between. Only lists models verified
/// against the workspace endpoint.
pub fn available_modes() -> Vec<ModelMode> {
    vec![
        ModelMode {
            id: "qwen3.7-flash",
            name: "Qwen3.7 Flash",
            description: "默认，最便宜",
        },
        ModelMode {
            id: "deepseek-v4-flash",
            name: "DeepSeek V4 Flash",
            description: "便宜备选",
        },
        ModelMode {
            id: "qwen3-coder-flash",
            name: "Qwen Coder Flash",
            description: "代码特化",
        },
    ]
}

/// Returns the model id for a mode id, or `None` when unknown.
pub fn model_for_mode(mode_id: &str) -> Option<&'static str> {
    available_modes()
        .into_iter()
        .find(|m| m.id == mode_id)
        .map(|m| m.id)
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl LlmConfig {
    pub fn load() -> Result<Self, String> {
        let api_key = std::env::var("NAIL_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .or_else(|| read_key_file(&nail_key_file()))
            .or_else(|| read_key_file(&legacy_key_file()));
        match api_key {
            Some(api_key) => Ok(Self {
                base_url: std::env::var("NAIL_BASE_URL")
                    .ok()
                    .filter(|u| !u.is_empty())
                    .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
                api_key,
                model: std::env::var("NAIL_MODEL")
                    .ok()
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            }),
            None => Err(format!(
                "没有找到 API Key：设置环境变量 NAIL_API_KEY，或把 key 写入 {}（chmod 600）",
                nail_key_file().display()
            )),
        }
    }

    /// Override the model (used when the session picked a mode).
    pub fn with_model(mut self, model: &str) -> Self {
        if !model.is_empty() {
            self.model = model.to_string();
        }
        self
    }
}

fn home_dir() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
}

fn nail_key_file() -> std::path::PathBuf {
    std::env::var("NAIL_KEY_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".config").join("nail-agent").join("api_key"))
}

fn legacy_key_file() -> std::path::PathBuf {
    home_dir().join(".config").join("zacp").join("api_key")
}

fn read_key_file(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

/// A model-requested tool call with raw (unparsed) JSON arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Outcome of a single model turn (no looping here; the caller loops).
/// Token usage accumulated from one streaming turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
}

impl Usage {
    pub fn total(self) -> u64 {
        self.input + self.output
    }

    pub fn add(&mut self, other: Usage) {
        self.input += other.input;
        self.output += other.output;
    }
}

pub enum TurnResult {
    /// Plain answer streamed out, nothing else requested.
    TextDone {
        usage: Usage,
    },
    /// The model wants these tools run; results must be appended as
    /// `tool` messages and the conversation continued.
    ToolCalls {
        assistant: ChatCompletionRequestMessage,
        calls: Vec<PendingToolCall>,
        usage: Usage,
    },
    Cancelled,
    Error(String),
}

#[derive(Clone)]
pub struct Llm {
    client: OpenAIClient<OpenAIConfig>,
}

impl Llm {
    pub fn new(config: &LlmConfig) -> Self {
        let openai = OpenAIConfig::new()
            .with_api_base(&config.base_url)
            .with_api_key(&config.api_key);
        Self {
            client: OpenAIClient::with_config(openai),
        }
    }

    /// Resolve when the cancellation flag becomes true (or the sender drops).
    /// Holds no `Ref` across an await, so it stays `Send`.
    async fn is_cancelled(rx: &mut watch::Receiver<bool>) {
        loop {
            if *rx.borrow_and_update() {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    /// Cancellation future for other layers (see [`Self::is_cancelled`]).
    pub(crate) async fn cancelled(rx: &mut watch::Receiver<bool>) {
        Self::is_cancelled(rx).await
    }

    /// Run one streaming turn over `messages`, offering `tools`.
    /// Each text delta goes to `on_text`; a `false` return stops early.
    pub async fn chat_once(
        &self,
        model: &str,
        messages: Vec<ChatCompletionRequestMessage>,
        tools: Vec<ChatCompletionTools>,
        cancel: &mut watch::Receiver<bool>,
        mut on_text: impl AsyncFnMut(&str) -> bool,
    ) -> TurnResult {
        let mut args = CreateChatCompletionRequestArgs::default();
        args.model(model).messages(messages).stream(true).stream_options(
            async_openai::types::chat::ChatCompletionStreamOptions {
                include_usage: Some(true),
                include_obfuscation: None,
            },
        );
        if !tools.is_empty() {
            args.tools(tools);
        }
        let request = match args.build() {
            Ok(request) => request,
            Err(err) => return TurnResult::Error(format!("构造模型请求失败：{err}")),
        };
        let mut stream = match self.client.chat().create_stream(request).await {
            Ok(stream) => stream,
            Err(err) => return TurnResult::Error(format!("请求模型失败：{err}")),
        };

        // Tool deltas accumulate by index: id/name arrive first, argument
        // fragments in the following chunks.
        let mut tool_ids: Vec<Option<String>> = Vec::new();
        let mut tool_names: Vec<String> = Vec::new();
        let mut tool_args: Vec<String> = Vec::new();
        let mut full_content = String::new();
        let mut usage = Usage::default();

        loop {
            tokio::select! {
                chunk = stream.next() => {
                    let Some(chunk) = chunk else { break };
                    let chunk = match chunk {
                        Ok(chunk) => chunk,
                        Err(err) => return TurnResult::Error(format!("读取流失败：{err}")),
                    };
                    if let Some(report) = chunk.usage {
                        usage.input += u64::from(report.prompt_tokens);
                        usage.output += u64::from(report.completion_tokens);
                    }
                    for choice in chunk.choices {
                        if let Some(text) = choice.delta.content.filter(|t| !t.is_empty()) {
                            full_content.push_str(&text);
                            if !on_text(&text).await {
                                return TurnResult::Cancelled;
                            }
                        }
                        if let Some(deltas) = choice.delta.tool_calls {
                            for delta in deltas {
                                let index = delta.index as usize;
                                while tool_ids.len() <= index {
                                    tool_ids.push(None);
                                    tool_names.push(String::new());
                                    tool_args.push(String::new());
                                }
                                if let Some(id) = delta.id {
                                    tool_ids[index] = Some(id);
                                }
                                if let Some(f) = delta.function {
                                    if let Some(name) = f.name {
                                        tool_names[index].push_str(&name);
                                    }
                                    if let Some(args) = f.arguments {
                                        tool_args[index].push_str(&args);
                                    }
                                }
                            }
                        }
                    }
                }
                _ = Self::is_cancelled(cancel) => {
                    return TurnResult::Cancelled;
                }
            }
        }

        let mut calls = Vec::new();
        for ((id, name), args) in tool_ids.into_iter().zip(tool_names).zip(tool_args) {
            if let (Some(id), name) = (id, name)
                && !name.is_empty()
            {
                calls.push(PendingToolCall { id, name, arguments: args });
            }
        }
        if calls.is_empty() {
            TurnResult::TextDone { usage }
        } else {
            let content = if full_content.is_empty() {
                None
            } else {
                Some(ChatCompletionRequestAssistantMessageContent::Text(
                    full_content,
                ))
            };
            let assistant = ChatCompletionRequestMessage::Assistant(
                ChatCompletionRequestAssistantMessage {
                    content,
                    tool_calls: Some(
                        calls
                            .iter()
                            .map(|c| {
                                ChatCompletionMessageToolCalls::Function(
                                    ChatCompletionMessageToolCall {
                                        id: c.id.clone(),
                                        function: FunctionCall {
                                            name: c.name.clone(),
                                            arguments: c.arguments.clone(),
                                        },
                                    },
                                )
                            })
                            .collect(),
                    ),
                    ..Default::default()
                },
            );
            TurnResult::ToolCalls { assistant, calls, usage }
        }
    }

    /// Describe one local tool in OpenAI function format.
    /// Phase 3 (tools layer) calls this.
    #[allow(dead_code)]
    pub fn tool_def(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> ChatCompletionTools {
        use async_openai::types::chat::{ChatCompletionTool, FunctionObject};
        ChatCompletionTools::Function(ChatCompletionTool {
            function: FunctionObject {
                name: name.into(),
                description: Some(description.into()),
                parameters: Some(parameters),
                strict: None,
            },
        })
    }

    /// Tool result message for a finished call.
    /// Phase 3 (tools layer) calls this.
    #[allow(dead_code)]
    pub fn tool_result(call_id: &str, content: String) -> ChatCompletionRequestMessage {
        use async_openai::types::chat::{
            ChatCompletionRequestToolMessage, ChatCompletionRequestToolMessageContent,
        };
        ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
            content: ChatCompletionRequestToolMessageContent::Text(content),
            tool_call_id: call_id.to_string(),
        })
    }

    /// User message constructor (keeps call sites tidy).
    pub fn user(text: &str) -> ChatCompletionRequestMessage {
        use async_openai::types::chat::ChatCompletionRequestUserMessageArgs;
        ChatCompletionRequestMessage::User(
            ChatCompletionRequestUserMessageArgs::default()
                .content(text)
                .build()
                .expect("user message builds"),
        )
    }

    /// Assistant text message constructor (for transcript recording).
    pub fn assistant(text: &str) -> ChatCompletionRequestMessage {
        use async_openai::types::chat::ChatCompletionRequestAssistantMessage;
        ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
            content: Some(ChatCompletionRequestAssistantMessageContent::Text(
                text.to_string(),
            )),
            ..Default::default()
        })
    }
}
