use agent_client_protocol::schema::SessionId;
use async_openai::types::chat::{ChatCompletionRequestMessage, ChatCompletionTools};

#[derive(Clone)]
pub enum State {
    Idle {
        session_id: SessionId,
    },

    ContextLoading {
        session_id: SessionId,
    },

    ModelCalling {
        session_id: SessionId,
        messages: Vec<ChatCompletionRequestMessage>,
        tools: Vec<ChatCompletionTools>,
    },

    ModelResponseStreaming {
        session_id: SessionId,
        messages: Vec<ChatCompletionRequestMessage>,
        tools: Vec<ChatCompletionTools>,
        accumulated: String,
    },

    ToolsCalling {
        session_id: SessionId,
        tools: Vec<ChatCompletionTools>,
    },

    Done,
}
