//! nail-agent: registry-grade ACP agent for Zed, built clean.
//!
//! Layers (each only talks to the one below through small interfaces):
//! - `proto` — ACP wire handlers (this phase: echo turns)
//! - `core` — session registry and lifecycle states
//! - `llm` — model backends and session modes (later phase)
//! - `tools` — function-calling toolbox, permission, safety (later phase)
//! - `store` — AgDb persistence (later phase)

mod core;
mod llm;
mod proto;
mod store;
mod tools;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), agent_client_protocol::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let sessions = core::Sessions::new().unwrap_or_else(|err| {
        eprintln!("nail-agent: cannot open session store: {err}");
        std::process::exit(1);
    });
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        persistent = sessions.is_persistent(),
        "nail-agent starting on stdio"
    );
    proto::serve(sessions).await
}
