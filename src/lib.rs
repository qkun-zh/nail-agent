pub mod db;
pub mod fsm;
pub mod logger;
pub mod tool_proxy;

use async_openai::{Client as OpenAIClient, config::OpenAIConfig};
use tool_proxy::McpHandle;
use anyhow::Context;

pub type Llm = OpenAIClient<OpenAIConfig>;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub base_url: String,
    pub api_key: String,
    pub model_name: String,
}

impl AgentConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        use std::env;
        Ok(Self {
            base_url: env::var("BASE_URL").context("missing BASE_URL environment variable")?,
            api_key: env::var("API_KEY").context("missing API_KEY environment variable")?,
            model_name: env::var("MODEL_NAME")
                .context("missing MODEL_NAME environment variable")?,
        })
    }
}

pub struct AppState {
    pub llm: Llm,
    pub model_name: String,

    pub proxy_handle: McpHandle,
}
