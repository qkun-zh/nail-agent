use std::sync::{Arc, RwLock};

use rmcp::{
	handler::server::{wrapper::Parameters, ServerHandler},
	model::{
		Implementation, InitializeRequestParams, InitializeResult, ProtocolVersion,
		ServerCapabilities, ServerInfo,
	},
	schemars,
	service::RequestContext,
	tool, tool_handler, tool_router, RoleServer,
};
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::fs;
use super::hint_accumulator;
use super::McpToolCall;



#[derive(Debug)]
pub struct SessionWorkdir {

	pub root: PathBuf,

	pub current: RwLock<Option<PathBuf>>,
}

impl SessionWorkdir {
	pub fn new(root: PathBuf) -> Self {
		Self {
			root,
			current: RwLock::new(None),
		}
	}


	pub fn get_current(&self) -> PathBuf {
		self.current
			.read()
			.ok()
			.and_then(|guard| guard.clone())
			.unwrap_or_else(|| self.root.clone())
	}


	pub fn set_current(&self, path: PathBuf) {
		if let Ok(mut guard) = self.current.write() {
			*guard = Some(path);
		}
	}


	pub fn reset(&self) {
		if let Ok(mut guard) = self.current.write() {
			*guard = None;
		}
	}
}


#[derive(Debug, Clone)]
pub struct OctofsServer {

	workdir: Arc<SessionWorkdir>,
}

impl OctofsServer {

	pub fn new() -> Self {
		let root = super::get_session_root_directory();
		Self {
			workdir: Arc::new(SessionWorkdir::new(root)),
		}
	}



	pub fn with_root(root: PathBuf) -> Self {
		Self {
			workdir: Arc::new(SessionWorkdir::new(root)),
		}
	}
}

impl Default for OctofsServer {
	fn default() -> Self {
		Self::new()
	}
}

use std::path::PathBuf;

#[tool_router]
impl OctofsServer {
	#[tool(
		description = "Read files, view directories, and search file content. Unified read-only tool."
	)]
	async fn view(&self, Parameters(params): Parameters<ViewParams>) -> Result<String, String> {
		let workdir = self.workdir.get_current();
		let call = McpToolCall {
			tool_name: "view".to_string(),
			parameters: serde_json::to_value(&params).unwrap_or_default(),
			tool_id: String::new(),
			workdir,
		};
		let result = fs::execute_view(&call).await.map_err(|e| e.to_string())?;
		Ok(append_hints(result))
	}

	#[tool(
		description = "Perform text editing operations on files: create, str_replace, delete, undo_edit."
	)]
	async fn text_editor(
		&self,
		Parameters(params): Parameters<TextEditorParams>,
	) -> Result<String, String> {
		let workdir = self.workdir.get_current();
		let call = McpToolCall {
			tool_name: "text_editor".to_string(),
			parameters: serde_json::to_value(&params).unwrap_or_default(),
			tool_id: String::new(),
			workdir,
		};
		let result = fs::execute_text_editor(&call)
			.await
			.map_err(|e| e.to_string())?;
		Ok(append_hints(result))
	}

	#[tool(description = "Perform multiple insert/replace operations on a SINGLE file atomically.")]
	async fn batch_edit(
		&self,
		Parameters(params): Parameters<BatchEditParams>,
	) -> Result<String, String> {
		let workdir = self.workdir.get_current();
		let call = McpToolCall {
			tool_name: "batch_edit".to_string(),
			parameters: serde_json::to_value(&params).unwrap_or_default(),
			tool_id: String::new(),
			workdir,
		};
		let result = fs::execute_batch_edit(&call)
			.await
			.map_err(|e| e.to_string())?;
		Ok(append_hints(result))
	}

	#[tool(description = "Copy lines from a source file and append them into a target file.")]
	async fn extract_lines(
		&self,
		Parameters(params): Parameters<ExtractLinesParams>,
	) -> Result<String, String> {
		let workdir = self.workdir.get_current();
		let call = McpToolCall {
			tool_name: "extract_lines".to_string(),
			parameters: serde_json::to_value(&params).unwrap_or_default(),
			tool_id: String::new(),
			workdir,
		};
		let result = fs::execute_extract_lines(&call)
			.await
			.map_err(|e| e.to_string())?;
		Ok(append_hints(result))
	}

	#[tool(description = "Execute a command in the shell.")]
	async fn shell(&self, Parameters(params): Parameters<ShellParams>) -> Result<String, String> {
		let workdir = self.workdir.get_current();
		let call = McpToolCall {
			tool_name: "shell".to_string(),
			parameters: serde_json::to_value(&params).unwrap_or_default(),
			tool_id: String::new(),
			workdir,
		};
		let result = fs::execute_shell_command(&call)
			.await
			.map_err(|e| e.to_string())?;
		Ok(append_hints(result))
	}

	#[tool(
		description = "Change the working directory used by subsequent tool calls. \
			Do NOT call this just to check the current directory — all tools accept \
			both relative and absolute paths and resolve relative paths against the \
			session's working directory automatically. Only invoke this tool when you \
			actually need to switch to a different directory (set `path`) or revert \
			to the session root (`reset: true`)."
	)]
	async fn workdir(
		&self,
		Parameters(params): Parameters<WorkdirParams>,
	) -> Result<String, String> {
		let workdir = self.workdir.get_current();
		let call = McpToolCall {
			tool_name: "workdir".to_string(),
			parameters: serde_json::to_value(&params).unwrap_or_default(),
			tool_id: String::new(),
			workdir,
		};
		let result = fs::execute_workdir_command(&call)
			.await
			.map_err(|e| e.to_string())?;


		match &result {
			fs::WorkdirResult::Set { current, .. } => {
				self.workdir.set_current(current.clone());
			}
			fs::WorkdirResult::Reset => {
				self.workdir.reset();
			}
			fs::WorkdirResult::Get { .. } => {}
		}

		Ok(result.to_json_string())
	}
}

#[tool_handler(router = Self::tool_router())]
impl ServerHandler for OctofsServer {
	fn get_info(&self) -> ServerInfo {
		ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
			.with_server_info(Implementation::from_build_env())
			.with_protocol_version(ProtocolVersion::V_2025_03_26)
			.with_instructions(
				"This server provides filesystem tools: view (read files/dirs), \
				 text_editor (create/str_replace/delete/undo), batch_edit (multi-op line edits), \
				 extract_lines (copy lines between files), shell (execute commands), \
				 workdir (get/set working directory)."
					.to_string(),
			)
	}



	async fn initialize(
		&self,
		request: InitializeRequestParams,
		_context: RequestContext<RoleServer>,
	) -> Result<InitializeResult, rmcp::ErrorData> {

		if let Some(experimental) = &request.capabilities.experimental {
			if let Some(session_obj) = experimental.get("session") {
				if let Some(workdir_str) = session_obj.get("workdir").and_then(|v| v.as_str()) {
					let path = std::path::PathBuf::from(workdir_str);
					if path.is_absolute() && path.is_dir() {
						self.workdir.set_current(path.clone());
						debug!("Session workdir set from capabilities: {}", path.display());
					} else {
						debug!(
							"Session workdir '{}' is not an absolute directory path, ignoring",
							workdir_str
						);
					}
				}
			}
		}

		Ok(self.get_info())
	}
}



fn append_hints(mut result: String) -> String {
	let hints = hint_accumulator::drain_hints();
	if !hints.is_empty() {
		result.push_str("\n\n");
		for hint in hints {
			result.push_str("⚠️ ");
			result.push_str(&hint);
			result.push('\n');
		}
	}
	result
}


#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ViewParams {

	#[serde(deserialize_with = "deserialize_string_or_vec")]
	#[schemars(length(min = 1, max = 50))]
	pub paths: Vec<String>,











	#[serde(default)]
	#[schemars(schema_with = "lines_param_schema")]
	pub lines: Option<serde_json::Value>,

	#[serde(default)]
	pub pattern: Option<String>,



	#[serde(default)]
	pub content: Option<String>,

	#[serde(default)]
	pub regex: Option<bool>,

	#[serde(default)]
	pub max_depth: Option<usize>,

	#[serde(default)]
	pub include_hidden: Option<bool>,

	#[serde(default)]
	pub context: Option<usize>,
}










fn lines_param_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
	serde_json::from_value(serde_json::json!({
		"description": "Line range(s) for file viewing. Two accepted shapes (max 2 levels of nesting):\n\
			1) Flat range: [start, end] — single range applied to single file or all files.\n\
			2) Nested ranges: [[start, end], [start, end], ...] — multiple ranges (single path) or per-file ranges (multiple paths).\n\
			Each endpoint is a 1-indexed line number (integer) or a hash string from previous view output. NEVER triple-wrap.",
		"oneOf": [
			{
				"description": "Flat range [start, end] — applied to single file or all files",
				"type": "array",
				"minItems": 2,
				"maxItems": 2,
				"items": {
					"oneOf": [
						{ "type": "integer", "format": "int64" },
						{ "type": "string" }
					]
				},
				"examples": [[1, 50], ["a3f2", "b8c1"]]
			},
			{
				"description": "Nested [[start,end], ...] — multiple ranges per file or per-file ranges",
				"type": "array",
				"minItems": 1,
				"items": {
					"type": "array",
					"minItems": 2,
					"maxItems": 2,
					"items": {
						"oneOf": [
							{ "type": "integer", "format": "int64" },
							{ "type": "string" }
						]
					}
				},
				"examples": [[[1, 50], [200, 250]], [[1, 30], [10, 40]]]
			}
		]
	}))
	.expect("static schema is valid JSON")
}


fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
	D: serde::Deserializer<'de>,
{
	use serde::de;

	struct StringOrVec;

	impl<'de> de::Visitor<'de> for StringOrVec {
		type Value = Vec<String>;

		fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
			formatter.write_str("a string or an array of strings")
		}

		fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
			Ok(vec![value.to_string()])
		}

		fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
			Ok(vec![value])
		}

		fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
			let mut vec = Vec::new();
			while let Some(elem) = seq.next_element::<String>()? {
				vec.push(elem);
			}
			Ok(vec)
		}
	}

	deserializer.deserialize_any(StringOrVec)
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextEditorCommand {
	Create,
	StrReplace,
	Delete,
	UndoEdit,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TextEditorParams {
	/// The operation to perform: create, str_replace, undo_edit
	pub command: TextEditorCommand,
	/// REQUIRED. Path to the file to operate on.
	pub path: String,
	/// File content for create command.
	#[serde(default)]
	pub content: Option<String>,
	/// Text to find (must match exactly). REQUIRED for str_replace.
	#[serde(default)]
	pub old_text: Option<String>,
	/// Replacement text. REQUIRED for str_replace.
	#[serde(default)]
	pub new_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BatchEditOperationType {
	Insert,
	Replace,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct BatchEditOperation {
	/// Type of operation: 'insert' (after line) or 'replace' (line range)
	pub operation: BatchEditOperationType,
	/// Line numbers from ORIGINAL file content.
	pub line_range: BatchEditLineRange,
	/// Raw content to insert or replace with.
	pub content: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum BatchEditLineRange {
	/// Single line number for insert (0=beginning, N=after line N, -1=after last line)
	Single(i64),
	/// Line range [start, end] for replace (1-indexed, inclusive)
	Range(Vec<i64>),
	/// Single hash identifier for insert (hash mode: insert after line with this hash)
	Hash(String),
	/// Hash range [start_hash, end_hash] for replace (hash mode)
	HashRange(Vec<String>),
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct BatchEditParams {
	/// Path to the file to edit
	pub path: String,
	/// Array of operations for batch_edit on SINGLE file. Max 50 operations.
	#[schemars(length(max = 50))]
	pub operations: Vec<BatchEditOperation>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ExtractLinesParams {
	/// Path to the source file to extract lines from
	pub from_path: String,
	/// Two-element array [start, end] with 1-indexed line numbers (inclusive)
	#[schemars(length(min = 2, max = 2))]
	pub from_range: Vec<i64>,
	/// Path to the target file where extracted lines will be appended
	pub append_path: String,
	/// Position where to append: 0=beginning, -1=end, N=after line N (1-indexed)
	pub append_line: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ShellParams {
	/// The shell command to execute
	pub command: String,
	/// Run command in background and return PID
	#[serde(default)]
	pub background: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct WorkdirParams {
	/// Absolute path or path relative to current workdir to switch into.
	/// Required unless `reset: true`. Do not pass `"."` — that is a no-op.
	#[serde(default)]
	pub path: Option<String>,
	/// If true, revert to the original session working directory.
	#[serde(default)]
	pub reset: Option<bool>,
}

// ── Server implementation ───────────────────────────────────────────────────────
