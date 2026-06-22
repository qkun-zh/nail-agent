use async_openai::types::chat::{
    ChatCompletionMessageToolCalls, ChatCompletionRequestMessage, ChatCompletionTools,
};

#[derive(Clone)]
pub enum Event {
    UserMessageArrived,

    ContextLoaded {
        messages: Vec<ChatCompletionRequestMessage>,
        tools: Vec<ChatCompletionTools>,
    },

    FirstChunkOfModelResponseArrived {
        delta: String,
    },

    NextChunkOfModelResponseArrived {
        delta: String,
    },

    ModelResponseFinishedWithoutToolCalls {
        full_content: String,
    },

    ModelResponseFinishedWithToolCalls {
        tool_calls: Vec<ChatCompletionMessageToolCalls>,
    },

    ToolsResponseArrived {
        updated_messages: Vec<ChatCompletionRequestMessage>,
    },
}
