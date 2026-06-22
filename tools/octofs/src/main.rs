













use anyhow::Result;
use clap::Parser;
use tracing::Level;

mod cli;
pub mod mcp;
pub mod utils;

use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
	let cli = Cli::parse();

	match cli.command {
		Commands::Mcp {
			path,
			bind,
			line_mode,
		} => {

			let working_directory = if let Some(p) = path {
				p.canonicalize().unwrap_or(p)
			} else {
				std::env::current_dir()?
			};


			init_mcp_logging();


			let mode = match line_mode.as_str() {
				"hash" => utils::line_hash::LineMode::Hash,
				_ => utils::line_hash::LineMode::Number,
			};
			utils::line_hash::set_line_mode(mode);


			mcp::set_session_root_directory(working_directory.clone());


			let server = mcp::server::OctofsServer::new();

			match bind {
				Some(addr) => {

					run_http_server(&addr).await?;
				}
				None => {

					run_stdio_server(server).await?;
				}
			}
		}
	}

	Ok(())
}






async fn run_stdio_server(server: mcp::server::OctofsServer) -> Result<()> {
	use rmcp::{transport::stdio, ServiceExt};

	tracing::info!("Starting MCP server (STDIO mode)");

	let service = server.serve(stdio()).await.inspect_err(|e| {
		tracing::error!("serving error: {:?}", e);
	})?;




	#[cfg(unix)]
	{
		use tokio::signal::unix::{signal, SignalKind};
		let ct = service.cancellation_token();
		let mut sigterm =
			signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
		tokio::select! {
			_ = service.waiting() => {

			}
			_ = sigterm.recv() => {
				tracing::debug!("SIGTERM received, shutting down gracefully");
				ct.cancel();
			}
		}
	}
	#[cfg(not(unix))]
	{
		service.waiting().await.ok();
	}


	mcp::fs::shell::kill_all_shell_children();

	Ok(())
}


async fn run_http_server(bind_addr: &str) -> Result<()> {
	use rmcp::transport::streamable_http_server::{
		session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
	};

	let ct = tokio_util::sync::CancellationToken::new();



	let service = StreamableHttpService::new(
		|| Ok(mcp::server::OctofsServer::new()),
		LocalSessionManager::default().into(),
		StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
	);

	let router = axum::Router::new().nest_service("/mcp", service);
	let addr = bind_addr
		.parse::<std::net::SocketAddr>()
		.map_err(|e| anyhow::anyhow!("Invalid bind address '{}': {}", bind_addr, e))?;

	let tcp_listener = tokio::net::TcpListener::bind(&addr)
		.await
		.map_err(|e| anyhow::anyhow!("Failed to bind to {}: {}", addr, e))?;

	tracing::info!("MCP HTTP server listening on {}", addr);

	let _ = axum::serve(tcp_listener, router)
		.with_graceful_shutdown(async move {
			tokio::signal::ctrl_c().await.unwrap();
			ct.cancel();
		})
		.await;

	Ok(())
}

fn init_mcp_logging() {

	let level = std::env::var("RUST_LOG")
		.ok()
		.and_then(|v| v.parse::<Level>().ok())
		.unwrap_or(Level::WARN);

	tracing::subscriber::set_global_default(StderrSubscriber { level })
		.expect("failed to set tracing subscriber");
}



struct StderrSubscriber {
	level: Level,
}

impl tracing::Subscriber for StderrSubscriber {
	fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
		metadata.level() <= &self.level
	}

	fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
		tracing::span::Id::from_u64(1)
	}

	fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

	fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

	fn event(&self, event: &tracing::Event<'_>) {
		use std::fmt::Write;
		let mut msg = String::new();
		let _ = write!(msg, "[{}] ", event.metadata().level());
		event.record(&mut MessageVisitor(&mut msg));
		eprintln!("{}", msg);
	}

	fn enter(&self, _span: &tracing::span::Id) {}
	fn exit(&self, _span: &tracing::span::Id) {}
}

struct MessageVisitor<'a>(&'a mut String);

impl tracing::field::Visit for MessageVisitor<'_> {
	fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
		if field.name() == "message" {
			let _ = std::fmt::write(self.0, format_args!("{:?}", value));
		}
	}

	fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
		if field.name() == "message" {
			self.0.push_str(value);
		}
	}
}
