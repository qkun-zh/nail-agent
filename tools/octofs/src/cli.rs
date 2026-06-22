













use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "octofs")]
#[command(version, author = "Muvon Un Limited <contact@muvon.io>")]
#[command(about = "Standalone MCP filesystem tools server", long_about = None)]
pub struct Cli {
	#[command(subcommand)]
	pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {

	Mcp {

		#[arg(long, value_name = "PATH")]
		path: Option<PathBuf>,


		#[arg(long, value_name = "HOST:PORT")]
		bind: Option<String>,


		#[arg(long, value_name = "MODE", default_value = "number")]
		line_mode: String,
	},
}
