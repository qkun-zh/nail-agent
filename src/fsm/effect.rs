use agent_client_protocol::schema::SessionId;
use async_openai::types::chat::{
    ChatCompletionMessageToolCalls, ChatCompletionRequestMessage, ChatCompletionTools,
};

pub enum Effect {
    LoadContext { session_id: SessionId },

    CallModel {
        messages: Vec<ChatCompletionRequestMessage>,
        tools: Vec<ChatCompletionTools>,
        session_id: SessionId,
    },

    CallTools {
        messages: Vec<ChatCompletionRequestMessage>,
        tool_calls: Vec<ChatCompletionMessageToolCalls>,
        session_id: SessionId,
    },

    SaveSession {
        content: String,
        session_id: SessionId,
    },

    DoNothing,
}
