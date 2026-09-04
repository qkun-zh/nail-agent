//! Local toolbox: `run` / `read` / `write`.
//!
//! Offered to the model through standard function calling and invokable by
//! the `/run`, `/read`, `/write` prompt prefixes. Every call is announced as
//! an ACP `ToolCall`, gated by `session/request_permission` (allow-once /
//! allow-always / reject per session), executed, then reported back.
//!
//! Safety is layered, honestly limited: paths resolve inside the session cwd
//! (escapes and sensitive locations refused), destructive shell patterns are
//! screened, and every decision lands in the audit log. This is *not* a
//! sandbox — the agent process runs with the user's privileges, so the
//! permission dialog stays the real gate.
//!
//! MCP servers live in [`mcp`]: same permission flow, namespaced tool names.

pub mod mcp;

use agent_client_protocol::schema::v1::{
    Diff, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, SessionId, SessionNotification, SessionUpdate, ToolCall,
    ToolCallContent, ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use async_openai::types::chat::ChatCompletionTools;
use serde_json::json;
use tokio::sync::watch;

use agent_client_protocol::{Client, ConnectionTo};

use crate::core::Sessions;
use crate::llm::Llm;

pub const TOOL_RUN: &str = "run";
pub const TOOL_READ: &str = "read";
pub const TOOL_WRITE: &str = "write";

/// Function declarations offered to the model.
pub fn local_tool_defs() -> Vec<ChatCompletionTools> {
    vec![
        Llm::tool_def(
            TOOL_RUN,
            "Execute a shell command (sh -c) in the session directory and return its combined stdout/stderr.",
            json!({
                "type": "object",
                "properties": {"command": {"type": "string"}},
                "required": ["command"],
            }),
        ),
        Llm::tool_def(
            TOOL_READ,
            "Read a UTF-8 text file inside the session directory and return its content (truncated past 8 KiB).",
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
            }),
        ),
        Llm::tool_def(
            TOOL_WRITE,
            "Write text to a file inside the session directory, creating or overwriting it. Returns a summary.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": ["path", "content"],
            }),
        ),
    ]
}

fn tool_kind(tool: &str) -> ToolKind {
    // Namespaced MCP tools (`server__tool`) map by suffix so clients that
    // key rendering off `kind` (Zed does) behave the same as built-ins.
    let base = tool.rsplit("__").next().unwrap_or(tool);
    match base {
        "read" | "view" => ToolKind::Read,
        "write" | "text_editor" | "batch_edit" => ToolKind::Edit,
        "run" | "shell" => ToolKind::Execute,
        _ => ToolKind::Other,
    }
}

/// Announce a new tool call (the `ToolCall` half of the create/update pair).
pub(crate) fn announce_tool_call(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    tool_call_id: &str,
    title: &str,
    tool: &str,
    raw_input: Option<serde_json::Value>,
) {
    let _ = cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCall(
            ToolCall::new(tool_call_id.to_string(), title)
                .kind(tool_kind(tool))
                .raw_input(raw_input),
        ),
    ));
}

pub(crate) fn tool_update(
    session_id: &SessionId,
    tool_call_id: &str,
    title: &str,
    status: ToolCallStatus,
) -> SessionNotification {
    SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            tool_call_id.to_string(),
            ToolCallUpdateFields::new().title(title.to_string()).status(status),
        )),
    )
}

/// Truncate display text with a marker instead of silent cutting.
fn clip(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…（截断，只显示前 {limit} 字节）", &text[..end])
}

/// Completion update carrying what Zed renders: inline content blocks,
/// file locations, and the raw output payload.
#[allow(clippy::too_many_arguments)]
fn tool_result_update(
    session_id: &SessionId,
    tool_call_id: &str,
    title: &str,
    status: ToolCallStatus,
    content: Vec<ToolCallContent>,
    locations: Vec<ToolCallLocation>,
    raw_output: Option<serde_json::Value>,
) -> SessionNotification {
    SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            tool_call_id.to_string(),
            ToolCallUpdateFields::new()
                .title(title.to_string())
                .status(status)
                .content(content)
                .locations(locations)
                .raw_output(raw_output),
        )),
    )
}

/// Ask the client for permission unless this session remembered allow-always.
/// `tool_call_id` identifies the whole create/update sequence.
/// Returns `true` when the tool may run.
pub async fn ensure_permission(
    ctx: &CallCtx<'_>,
    tool_call_id: &str,
    tool: &str,
    title: &str,
    raw_input: Option<serde_json::Value>,
) -> bool {
    announce_tool_call(ctx.cx, ctx.session_id, tool_call_id, title, tool, raw_input);
    if ctx.sessions.is_always_allowed(ctx.session_key, tool) {
        audit(ctx.session_key, tool, title, "allow-always-remembered");
        return true;
    }
    let options = vec![
        PermissionOption::new("allow-once", "Allow once", PermissionOptionKind::AllowOnce),
        PermissionOption::new(
            "allow-always",
            "Allow always in this session",
            PermissionOptionKind::AllowAlways,
        ),
        PermissionOption::new("reject", "Reject", PermissionOptionKind::RejectOnce),
    ];
    let request = RequestPermissionRequest::new(
        ctx.session_id.clone(),
        ToolCallUpdate::new(
            tool_call_id.to_string(),
            ToolCallUpdateFields::new().status(ToolCallStatus::InProgress),
        ),
        options,
    );
    let response = match ctx.cx.send_request(request).block_task().await {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!(session = ctx.session_key, error = %err, "permission request failed");
            audit(ctx.session_key, tool, title, "error");
            return false;
        }
    };
    match response.outcome {
        RequestPermissionOutcome::Selected(selected) => {
            let id: &str = &selected.option_id.0;
            if id == "allow-always" {
                ctx.sessions.remember_allow_always(ctx.session_key, tool);
            }
            let allowed = id == "allow-once" || id == "allow-always";
            audit(ctx.session_key, tool, title, if allowed { id } else { "deny" });
            allowed
        }
        RequestPermissionOutcome::Cancelled => {
            audit(ctx.session_key, tool, title, "cancelled");
            false
        }
        _ => {
            audit(ctx.session_key, tool, title, "deny");
            false
        }
    }
}

fn session_cwd(sessions: &Sessions, key: &str) -> std::path::PathBuf {
    sessions
        .cwd_of(key)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// Collapse `.`/`..` lexically (no I/O), so non-existent targets can still
/// be checked against the session root.
fn lexical_clean(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

fn resolve(cwd: &std::path::Path, path: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Guarded path resolution: relative paths anchor at the session cwd;
/// escapes from the session root and sensitive locations are refused.
/// Notably the agent's own key file can never pass through these tools.
fn check_path(cwd: &std::path::Path, path: &str) -> Result<std::path::PathBuf, String> {
    if path.trim().is_empty() {
        return Err("路径为空".to_string());
    }
    let target = resolve(cwd, path);
    let base = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let probe = target
        .canonicalize()
        .unwrap_or_else(|_| lexical_clean(&target));
    if !probe.starts_with(&base) {
        return Err(format!(
            "拒绝：{} 超出会话目录 {}",
            target.display(),
            base.display()
        ));
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = std::path::PathBuf::from(home);
        for guarded in [".ssh", ".gnupg", ".config/nail-agent", ".config/zacp", ".aws"] {
            let dir = home.join(guarded);
            if probe.starts_with(&dir) {
                return Err(format!("拒绝：敏感路径 {}", dir.display()));
            }
        }
    }
    Ok(target)
}

/// Heuristic screen for destructive shell commands. A backstop, not a
/// sandbox: the permission dialog stays the real gate.
fn screen_command(command: &str) -> Option<String> {
    const BLOCKED: &[&str] = &[
        "rm -rf /",
        "rm -fr /",
        "mkfs",
        "dd ",
        ":(){",
        "chmod -R 777 /",
        "chmod -R 777 /*",
        "chown -R ",
        "> /dev/sd",
        "> /dev/nvme",
        "of=/dev/",
    ];
    let flat: String = command.split_whitespace().collect::<Vec<_>>().join(" ");
    BLOCKED
        .iter()
        .find(|rule| flat.contains(*rule))
        .map(|rule| format!("拒绝：命令命中危险模式 `{rule}`"))
}

/// Append-only audit trail of tool decisions and executions (best effort).
fn audit(session_key: &str, tool: &str, detail: &str, decision: &str) {
    let line = serde_json::json!({
        "ts": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "session": session_key,
        "tool": tool,
        "detail": detail.chars().take(300).collect::<String>(),
        "decision": decision,
    });
    let path = crate::store::data_dir().join("audit.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::fmt::Write;
    let mut buf = serde_json::to_string(&line).unwrap_or_default();
    let _ = writeln!(buf);
    use std::io::Write as _;
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(buf.as_bytes()));
}

/// Execute a shell command in `cwd`, killing the child on cancellation.
/// Returns the combined output report and whether it was cancelled.
pub async fn exec_shell(
    session_key: &str,
    command: &str,
    cwd: &std::path::Path,
    cancel: &mut watch::Receiver<bool>,
) -> (String, bool) {
    if let Some(reason) = screen_command(command) {
        audit(session_key, TOOL_RUN, command, "blocked");
        return (reason, false);
    }
    audit(session_key, TOOL_RUN, command, "exec");
    let mut child = match tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return (format!("执行失败：{err}"), false),
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let drain = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut out = Vec::new();
        let mut err = Vec::new();
        if let Some(mut pipe) = stdout {
            let _ = pipe.read_to_end(&mut out).await;
        }
        if let Some(mut pipe) = stderr {
            let _ = pipe.read_to_end(&mut err).await;
        }
        (out, err)
    });
    let mut cancel_wait = cancel.clone();
    let status = tokio::select! {
        status = child.wait() => Some(status),
        _ = cancel_wait.wait_for(|cancelled| *cancelled) => None,
    };
    match status {
        Some(Ok(exit)) => {
            let (out, err) = drain.await.unwrap_or_default();
            let mut text = String::from_utf8_lossy(&out).into_owned();
            let stderr_text = String::from_utf8_lossy(&err);
            if !stderr_text.is_empty() {
                text.push_str("\n[stderr]\n");
                text.push_str(&stderr_text);
            }
            if !exit.success() {
                text.push_str(&format!("\n[exit {exit}]"));
            }
            (text, false)
        }
        Some(Err(err)) => (format!("执行失败：{err}"), false),
        None => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            drain.abort();
            ("命令被取消。".to_string(), true)
        }
    }
}

/// Read a text file, truncated past 8 KiB. Paths are checked first.
pub async fn exec_read(session_key: &str, cwd: &std::path::Path, path: &str) -> String {
    let target = match check_path(cwd, path) {
        Ok(target) => target,
        Err(reason) => {
            audit(session_key, TOOL_READ, path, "blocked");
            return reason;
        }
    };
    audit(session_key, TOOL_READ, path, "exec");
    const LIMIT: usize = 8000;
    match tokio::fs::read_to_string(&target).await {
        Ok(content) => {
            if content.len() > LIMIT {
                format!("{}…（截断，只显示前 {LIMIT} 字节）", &content[..LIMIT])
            } else {
                content
            }
        }
        Err(err) => format!("读取失败：{err}"),
    }
}

/// Write text to a file, creating or overwriting it. Paths are checked first.
pub async fn exec_write(
    session_key: &str,
    cwd: &std::path::Path,
    path: &str,
    content: &str,
) -> (String, Option<String>) {
    let target = match check_path(cwd, path) {
        Ok(target) => target,
        Err(reason) => {
            audit(session_key, TOOL_WRITE, path, "blocked");
            return (reason, None);
        }
    };
    audit(session_key, TOOL_WRITE, path, "exec");
    let old_text = tokio::fs::read_to_string(&target).await.ok();
    match tokio::fs::write(&target, content).await {
        Ok(()) => (
            format!("已写入 {}（{} 字节）", target.display(), content.len()),
            old_text,
        ),
        Err(err) => (format!("写入失败：{err}"), old_text),
    }
}

pub struct ToolOutcome {
    pub output: String,
    pub cancelled: bool,
}

/// Shared per-call context: one struct keeps tool entry points small.
pub(crate) struct CallCtx<'a> {
    pub(crate) sessions: &'a Sessions,
    pub(crate) cx: &'a ConnectionTo<Client>,
    pub(crate) session_id: &'a SessionId,
    pub(crate) session_key: &'a str,
    pub(crate) cancel: &'a mut watch::Receiver<bool>,
}

static TOOL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Run one model-requested local tool: permission first, then execute.
/// Every path reports ACP status updates under a single tool-call id.
pub async fn execute_local(
    ctx: &mut CallCtx<'_>,
    name: &str,
    arguments: &str,
) -> ToolOutcome {
    use std::sync::atomic::Ordering;
    let done = |output: String| ToolOutcome { output, cancelled: false };
    let args: serde_json::Value = match serde_json::from_str(arguments) {
        Ok(args) => args,
        Err(_) => return done(format!("工具参数不是合法 JSON（{name}）：{arguments}")),
    };
    let get = |key: &str| args.get(key).and_then(|v| v.as_str()).unwrap_or("");
    let (tool, title) = match name {
        "run" => (TOOL_RUN, format!("run: {}", get("command"))),
        "read" => (TOOL_READ, format!("read {}", get("path"))),
        "write" => (TOOL_WRITE, format!("write {}", get("path"))),
        other => return done(format!("未知工具：{other}（可用：run、read、write）")),
    };
    let tool_id = format!(
        "{}-{}",
        tool,
        TOOL_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    if !ensure_permission(ctx, &tool_id,
        tool,
        &title,
        Some(args.clone()),
    )
    .await
    {
        let _ = ctx.cx.send_notification(tool_update(
            ctx.session_id,
            &tool_id,
            &title,
            ToolCallStatus::Failed,
        ));
        return done("用户拒绝了这次工具调用。".to_string());
    }
    let _ = ctx.cx.send_notification(tool_update(
        ctx.session_id,
        &tool_id,
        &title,
        ToolCallStatus::InProgress,
    ));
    let cancel: &mut watch::Receiver<bool> = &mut *ctx.cancel;
    let cwd = session_cwd(ctx.sessions, ctx.session_key);
    // Each tool reports rich results: inline content for Zed to render,
    // file locations where applicable, and the raw output payload.
    // `output` (fed back to the model) stays the full report.
    let (output, content, locations, raw_output) = match tool {
        TOOL_RUN => {
            let (report, was_cancelled) =
                exec_shell(ctx.session_key, get("command"), &cwd, cancel).await;
            if was_cancelled {
                let _ = ctx.cx.send_notification(tool_update(
                    ctx.session_id,
                    &tool_id,
                    &title,
                    ToolCallStatus::Failed,
                ));
                return ToolOutcome { output: report, cancelled: true };
            }
            let content = vec![ToolCallContent::from(clip(&report, 2000))];
            let raw = serde_json::Value::String(clip(&report, 4000));
            (report, content, Vec::new(), Some(raw))
        }
        TOOL_READ => {
            let report = exec_read(ctx.session_key, &cwd, get("path")).await;
            let locations = resolve_location(&cwd, get("path"));
            let content = vec![ToolCallContent::from(clip(&report, 2000))];
            (report, content, locations, None)
        }
        TOOL_WRITE => {
            let path = get("path");
            let (report, old_text) =
                exec_write(ctx.session_key, &cwd, path, get("content")).await;
            let locations = resolve_location(&cwd, path);
            let new_text = clip(get("content"), 2000);
            let abs = locations
                .first()
                .map(|l| l.path.clone())
                .unwrap_or_else(|| cwd.join(path));
            let mut diff = Diff::new(abs, new_text);
            if let Some(old) = old_text {
                diff.old_text = Some(clip(&old, 2000));
            }
            let content = vec![ToolCallContent::Diff(diff)];
            let raw = serde_json::Value::String(report.clone());
            (report, content, locations, Some(raw))
        }
        _ => unreachable!(),
    };
    let _ = ctx.cx.send_notification(tool_result_update(
        ctx.session_id,
        &tool_id,
        &title,
        ToolCallStatus::Completed,
        content,
        locations,
        raw_output,
    ));
    done(output)
}

/// Absolute location for a tool path, for Zed file links.
/// Returns empty when the path was rejected (never leaks guarded paths).
fn resolve_location(cwd: &std::path::Path, path: &str) -> Vec<ToolCallLocation> {
    match check_path(cwd, path) {
        Ok(target) => vec![ToolCallLocation::new(target)],
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn lexical_clean_collapses_dots() {
        assert_eq!(
            lexical_clean(std::path::Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(
            lexical_clean(std::path::Path::new("a/../../b")),
            PathBuf::from("../b")
        );
    }

    #[test]
    fn check_path_blocks_escape_and_sensitive() {
        let cwd = PathBuf::from("/tmp/nail-sandbox");
        assert!(check_path(&cwd, "sub/file.txt").is_ok());
        assert!(check_path(&cwd, "/etc/hostname").is_err());
        assert!(check_path(&cwd, "../../etc/hostname").is_err());
        assert!(check_path(&cwd, "").is_err());
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        assert!(check_path(&cwd, &format!("{home}/.ssh/id_rsa")).is_err());
    }

    #[test]
    fn screen_blocks_destructive_patterns() {
        assert!(screen_command("rm -rf / tmp").is_some());
        assert!(screen_command("mkfs.ext4 /dev/sda1").is_some());
        assert!(screen_command("echo hi").is_none());
        assert!(screen_command("cargo build 2>&1 | head").is_none());
    }
}
