













use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;

pub mod fs;
pub mod hint_accumulator;
pub mod server;
pub mod shared_utils;




static SESSION_ROOT: OnceLock<PathBuf> = OnceLock::new();



pub fn set_session_root_directory(path: PathBuf) {
	SESSION_ROOT.set(path).ok();
}


pub fn get_session_root_directory() -> PathBuf {
	SESSION_ROOT
		.get()
		.cloned()
		.unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCall {
	pub tool_name: String,
	pub parameters: Value,
	#[serde(default)]
	pub tool_id: String,

	pub workdir: PathBuf,
}

impl McpToolCall {

	#[cfg(test)]
	pub fn test_call(tool_name: &str, parameters: Value) -> Self {
		Self {
			tool_name: tool_name.to_string(),
			parameters,
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
		}
	}
}
