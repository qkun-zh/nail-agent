















use super::super::McpToolCall;
use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Mutex;




static SHELL_CHILDREN: Mutex<Option<HashSet<u32>>> = Mutex::new(None);

fn register_child(pid: u32) {
	SHELL_CHILDREN
		.lock()
		.unwrap()
		.get_or_insert_with(HashSet::new)
		.insert(pid);
}

fn unregister_child(pid: u32) {
	if let Some(set) = SHELL_CHILDREN.lock().unwrap().as_mut() {
		set.remove(&pid);
	}
}



#[cfg(unix)]
pub fn kill_all_shell_children() {
	let pids: Vec<u32> = SHELL_CHILDREN
		.lock()
		.unwrap()
		.as_mut()
		.map(|set| set.drain().collect())
		.unwrap_or_default();

	for pid in pids {
		let pgid = pid as libc::pid_t;


		unsafe {
			libc::kill(-pgid, libc::SIGKILL);
		}
	}
}

#[cfg(not(unix))]
pub fn kill_all_shell_children() {

	if let Some(set) = SHELL_CHILDREN.lock().unwrap().as_mut() {
		set.clear();
	}
}



static SHELL_MISUSE_HINTS: &[(&[&str], &str, &str)] = &[
	(
		&["cat", "head", "tail", "less", "more"],
		"view",
		"⚠️ Prefer `view` for reading files (line-numbered, supports ranges). Use shell only when piping output.",
	),
	(
		&["grep", "egrep", "fgrep", "rg"],
		"view",
		"⚠️ Use `view` with content= to search for text in files or directories (gitignore-aware, context lines, line numbers).",
	),
	(
		&["find", "ls"],
		"view",
		"⚠️ Prefer `view` for directory listing (.gitignore-aware, pattern/content filtering). Use shell only for system paths outside the project.",
	),
	(
		&["sed", "awk"],
		"text_editor",
		"⚠️ Prefer `text_editor` str_replace/line_replace for file edits (atomic, tracked). Use sed/awk only for stream transforms in pipelines.",
	),
];






static NONINTERACTIVE_ENVS: &[(&str, &str)] = &[
	("GIT_TERMINAL_PROMPT", "0"),
	("DEBIAN_FRONTEND", "noninteractive"),
	("PAGER", "cat"),
	("GIT_PAGER", "cat"),
];



fn detect_shell_misuse(command: &str) -> Option<&'static str> {
	let cmd = command.trim();

	// Check if cmd is exactly `prog` or starts with `prog ` / `prog\t`
	let is_prog = |prog: &str| -> bool {
		cmd == prog || cmd.starts_with(&format!("{prog} ")) || cmd.starts_with(&format!("{prog}\t"))
	};

	for (progs, _tool, hint) in SHELL_MISUSE_HINTS {
		if progs.iter().any(|p| is_prog(p)) {
			// Always emit hint — no tool_map in octofs
			return Some(hint);
		}
	}

	None
}

// Execute a shell command
pub async fn execute_shell_command(call: &McpToolCall) -> Result<String> {
	use tokio::process::Command as TokioCommand;

	// Extract command parameter
	let command = match call.parameters.get("command") {
		Some(Value::String(cmd)) => {
			if cmd.trim().is_empty() {
				bail!("Command parameter cannot be empty");
			}
			cmd.clone()
		}
		Some(_) => {
			bail!("Command parameter must be a string");
		}
		None => {
			bail!("Missing required 'command' parameter");
		}
	};

	// Extract background parameter
	let background = call
		.parameters
		.get("background")
		.and_then(|v| v.as_bool())
		.unwrap_or(false);

	// NOTE: We do NOT add MCP tool commands to shell history
	// NOTE: We do NOT add MCP tool commands to shell history
	// Only direct user commands via `octomind shell` CLI should persist to history
	// (see src/commands/shell.rs for user-initiated shell history)

	// Get the working directory from the call context
	let working_dir = call.workdir.clone();
	// Use tokio::process::Command for better cancellation support
	let mut cmd = if cfg!(target_os = "windows") {
		let mut cmd = TokioCommand::new("cmd");
		cmd.args(["/C", &command]);
		cmd.current_dir(&working_dir);
		cmd
	} else {
		let mut cmd = TokioCommand::new("sh");
		cmd.args(["-c", &command]);
		cmd.current_dir(&working_dir);
		cmd
	};

	// Force non-interactive: put the child in its own process group so it
	// cannot access the controlling terminal (/dev/tty opens fail with ENXIO
	// when combined with stdin=null set below). We use process_group(0)
	// instead of setsid() — setsid() creates a new *session* which makes the
	// child unreachable by the parent's process-group signals (e.g. when the


	#[cfg(unix)]
	{
		cmd.process_group(0);
	}




	for (key, value) in NONINTERACTIVE_ENVS {
		cmd.env(key, value);
	}


	if background {

		cmd.stdout(std::process::Stdio::null())
			.stderr(std::process::Stdio::null())
			.stdin(std::process::Stdio::null())
			.kill_on_drop(false);
	} else {

		cmd.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::piped())
			.stdin(std::process::Stdio::null())
			.kill_on_drop(true);
	}


	let child = cmd
		.spawn()
		.map_err(|e| anyhow!("Failed to spawn command: {}", e))?;


	if background {

		let pid = child
			.id()
			.ok_or_else(|| anyhow!("Failed to get process ID"))?;



		std::mem::forget(child);

		return Ok(format!(
			"Command started in background with PID {pid}\nUse 'kill {pid}' to terminate this background process if needed"
		));
	}



	let child_pid = child.id();
	if let Some(pid) = child_pid {
		register_child(pid);
	}


	let result = child.wait_with_output().await;


	if let Some(pid) = child_pid {
		unregister_child(pid);
	}

	match result.map_err(|e| anyhow!("Command execution failed: {}", e)) {
		Ok(output) => {
			let stdout = String::from_utf8_lossy(&output.stdout).to_string();
			let stderr = String::from_utf8_lossy(&output.stderr).to_string();


			let combined = if stderr.is_empty() {
				stdout
			} else if stdout.is_empty() {
				stderr
			} else {
				format!("{stdout}\n\nError: {stderr}")
			};


			let final_output = combined;


			let status_code = output.status.code().unwrap_or(-1);
			let success = output.status.success();


			if let Some(hint) = detect_shell_misuse(&command) {
				crate::mcp::hint_accumulator::push_hint(hint);
			}


			if success {
				Ok(final_output)
			} else {
				bail!(
					"Command failed with exit code {status_code}\nCommand: {command}\n\nOutput:\n{final_output}"
				)
			}
		}
		Err(e) => bail!("Error: {e}"),
	}
}
