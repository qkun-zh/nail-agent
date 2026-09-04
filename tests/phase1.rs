//! Phase-1 tests: live chat, session modes, cross-turn memory.

use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};

/// At most one live-model turn at a time.
static LIVE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Peer {
    stdin: ChildStdin,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    next_id: i64,
    _child: tokio::process::Child,
}

impl Peer {
    async fn spawn() -> Self {
        // Isolate on-disk sessions per spawned agent.
        static PEER_SEQ: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        Self::spawn_in(std::env::temp_dir().join(format!(
            "nail-test-{}-{}",
            std::process::id(),
            PEER_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )))
        .await
    }

    /// Spawn sharing one data dir (for cross-process resume tests).
    async fn spawn_in(dir: std::path::PathBuf) -> Self {
        Self::spawn_in_with_env(dir, &[]).await
    }

    async fn spawn_in_with_env(dir: std::path::PathBuf, envs: &[(&str, &str)]) -> Self {
        let exe = env!("CARGO_BIN_EXE_nail-agent");
        let mut cmd = Command::new(exe);
        cmd.env("NAIL_DATA_DIR", &dir);
        for (key, value) in envs {
            cmd.env(key, value);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nail-agent");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        Self {
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 1,
            _child: child,
        }
    }

    async fn send(&mut self, frame: &Value) {
        let mut line = serde_json::to_string(frame).unwrap();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn next_frame(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(90), self.lines.next_line())
            .await
            .expect("timed out waiting for agent frame")
            .expect("io")
            .expect("agent closed stdout");
        serde_json::from_str(&line).expect("agent frame is JSON")
    }

    async fn request(&mut self, method: &str, params: Value) -> (Value, Vec<Value>) {        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        let mut notifs = Vec::new();
        loop {
            let frame = self.next_frame().await;
            if frame.get("id") == Some(&json!(id)) {
                assert!(
                    frame.get("error").is_none(),
                    "request {method} failed: {}",
                    frame["error"]
                );
                return (frame["result"].clone(), notifs);
            }
            notifs.push(frame);
        }
    }

    fn agent_text(notifs: &[Value]) -> String {
        let mut out = String::new();
        for n in notifs {
            if n.get("method") == Some(&json!("session/update"))
                && n["params"]["update"].get("sessionUpdate")
                    == Some(&json!("agent_message_chunk"))
                && let Some(t) = n["params"]["update"]["content"]["text"].as_str()
            {
                out.push_str(t);
            }
        }
        out
    }

    async fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc":"2.0","method":method,"params":params}))
            .await;
    }
}

async fn new_session(peer: &mut Peer) -> String {
    peer.request("initialize", json!({"protocolVersion": 1}))
        .await;
    let (result, _) = peer
        .request("session/new", json!({"cwd": "/tmp", "mcpServers": []}))
        .await;
    result["sessionId"].as_str().unwrap().to_string()
}

async fn prompt(peer: &mut Peer, session: &str, text: &str) -> (Value, String) {
    let (result, notifs) = peer
        .request(
            "session/prompt",
            json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]}),
        )
        .await;
    (result, Peer::agent_text(&notifs))
}

#[tokio::test]
async fn phase1_live_chat() {
    let _live = LIVE.lock().await;
    let mut peer = Peer::spawn().await;
    let session = new_session(&mut peer).await;
    let (result, text) = prompt(&mut peer, &session, "Reply with exactly: ok").await;
    assert_eq!(result["stopReason"], json!("end_turn"));
    assert!(text.contains("ok"), "unexpected answer: {text}");
}

#[tokio::test]
async fn phase1_modes_advertised_and_switchable() {
    let _live = LIVE.lock().await;
    let mut peer = Peer::spawn().await;
    peer.request("initialize", json!({"protocolVersion": 1}))
        .await;
    let (result, _) = peer
        .request("session/new", json!({"cwd": "/tmp", "mcpServers": []}))
        .await;
    let session = result["sessionId"].as_str().unwrap().to_string();
    let modes = result["modes"]["availableModes"].as_array().unwrap();
    assert!(modes.iter().any(|m| m["id"] == json!("deepseek-v4-flash")));

    let (result, notifs) = peer
        .request(
            "session/set_mode",
            json!({"sessionId": session, "modeId": "deepseek-v4-flash"}),
        )
        .await;
    assert_eq!(result, json!({}));
    assert!(notifs.iter().any(|n| n["params"]["update"].get("sessionUpdate")
        == Some(&json!("current_mode_update"))));

    let (result, _) = prompt(&mut peer, &session, "Reply with exactly: ok").await;
    assert_eq!(result["stopReason"], json!("end_turn"));
}

#[tokio::test]
async fn phase1_remembers_across_turns() {
    let _live = LIVE.lock().await;
    let mut peer = Peer::spawn().await;
    let session = new_session(&mut peer).await;
    let (result, _) = prompt(
        &mut peer,
        &session,
        "Remember this codename: Blueberry. Reply with exactly: stored.",
    )
    .await;
    assert_eq!(result["stopReason"], json!("end_turn"));

    let (result, text) = prompt(
        &mut peer,
        &session,
        "What is my codename? Reply with only the codename.",
    )
    .await;
    assert_eq!(result["stopReason"], json!("end_turn"));
    assert!(text.contains("Blueberry"), "model forgot: {text}");
}

#[tokio::test]
async fn phase1_resume_across_restart() {
    let _live = LIVE.lock().await;
    let dir = std::env::temp_dir().join(format!("nail-resume-{}", std::process::id()));

    let mut peer = Peer::spawn_in(dir.clone()).await;
    let session = new_session(&mut peer).await;
    let (result, _) = prompt(
        &mut peer,
        &session,
        "Remember this codename: Blueberry. Reply with exactly: stored.",
    )
    .await;
    assert_eq!(result["stopReason"], json!("end_turn"));
    // Kill the process: memory is gone, only AgDb remains.
    peer._child.kill().await.expect("kill");

    let mut peer = Peer::spawn_in(dir).await;
    peer.request("initialize", json!({"protocolVersion": 1}))
        .await;
    let (result, _) = peer
        .request("session/resume", json!({"sessionId": session, "cwd": "/tmp"}))
        .await;
    assert!(result.get("modes").is_some());

    let (result, text) = prompt(
        &mut peer,
        &session,
        "What is my codename? Reply with only the codename.",
    )
    .await;
    assert_eq!(result["stopReason"], json!("end_turn"));
    assert!(text.contains("Blueberry"), "history lost: {text}");
}

#[tokio::test]
async fn phase1_model_calls_tool_by_itself() {
    let _live = LIVE.lock().await;
    let mut peer = Peer::spawn().await;
    let session = new_session(&mut peer).await;

    let id = peer.next_id;
    peer.next_id += 1;
    peer.send(&json!({"jsonrpc":"2.0","id":id,"method":"session/prompt",
        "params":{"sessionId": session, "prompt": [{
            "type": "text",
            "text": "You MUST call the run tool with command `echo final-tool-ok`, then reply with the command output and nothing else."
        }]}}))
        .await;
    let mut notifs = Vec::new();
    let stop = loop {
        let frame = peer.next_frame().await;
        if frame.get("id") == Some(&json!(id)) {
            assert!(frame.get("error").is_none(), "prompt failed: {}", frame["error"]);
            break frame["result"]["stopReason"].clone();
        }
        if frame.get("method") == Some(&json!("session/request_permission")) {
            let req_id = frame["id"].clone();
            peer.send(&json!({"jsonrpc":"2.0","id":req_id,
                "result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}))
                .await;
            continue;
        }
        notifs.push(frame);
    };
    assert_eq!(stop, json!("end_turn"));
    assert!(
        notifs.iter().any(|n| n["params"]["update"].get("sessionUpdate")
            == Some(&json!("tool_call"))),
        "expected a tool_call update"
    );
    // Zed renders these: the completion update carries inline content…
    assert!(
        notifs.iter().any(|n| {
            let update = &n["params"]["update"];
            update.get("sessionUpdate") == Some(&json!("tool_call_update"))
                && update.get("content").is_some()
        }),
        "expected a tool_call_update with content"
    );
    // …and the turn reports token usage.
    assert!(
        notifs.iter().any(|n| {
            let update = &n["params"]["update"];
            update.get("sessionUpdate") == Some(&json!("usage_update"))
                && update.get("used").and_then(|u| u.as_u64()).unwrap_or(0) > 0
        }),
        "expected a usage_update with used > 0"
    );
    assert!(Peer::agent_text(&notifs).contains("final-tool-ok"));
}

#[tokio::test]
async fn phase1_mcp_stdio_tool() {
    let _live = LIVE.lock().await;
    let mut peer = Peer::spawn().await;
    peer.request("initialize", json!({"protocolVersion": 1})).await;
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fake_mcp_server.py");
    let (result, _) = peer
        .request(
            "session/new",
            json!({"cwd": "/tmp", "mcpServers": [{
                "name": "fake",
                "command": "/usr/bin/python3",
                "args": [script.to_string_lossy()],
                "env": [],
            }]}),
        )
        .await;
    let session = result["sessionId"].as_str().unwrap().to_string();

    let id = peer.next_id;
    peer.next_id += 1;
    peer.send(&json!({"jsonrpc":"2.0","id":id,"method":"session/prompt",
        "params":{"sessionId": session, "prompt": [{
            "type": "text",
            "text": "You MUST call the fake__fake_echo tool with text `mcp-ok`, then reply with the tool result and nothing else."
        }]}}))
        .await;
    let mut notifs = Vec::new();
    let stop = loop {
        let frame = peer.next_frame().await;
        if frame.get("id") == Some(&json!(id)) {
            assert!(frame.get("error").is_none(), "prompt failed: {}", frame["error"]);
            break frame["result"]["stopReason"].clone();
        }
        if frame.get("method") == Some(&json!("session/request_permission")) {
            let req_id = frame["id"].clone();
            peer.send(&json!({"jsonrpc":"2.0","id":req_id,
                "result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}))
                .await;
            continue;
        }
        notifs.push(frame);
    };
    assert_eq!(stop, json!("end_turn"));
    assert!(
        notifs.iter().any(|n| n["params"]["update"].get("sessionUpdate")
            == Some(&json!("tool_call"))),
        "expected a tool_call update"
    );
    assert!(Peer::agent_text(&notifs).contains("fake:mcp-ok"));
}

#[tokio::test]
async fn phase1_auth_method_advertised_and_usable() {
    let mut peer = Peer::spawn().await;
    let (result, _) = peer
        .request("initialize", json!({"protocolVersion": 1}))
        .await;
    let methods = result["authMethods"].as_array().unwrap();
    assert!(
        methods.iter().any(|m| m.get("id") == Some(&json!("api-key"))),
        "api-key method missing: {methods:?}"
    );

    // The server holds a key (test env), so authenticate succeeds.
    peer.request("authenticate", json!({"methodId": "api-key"})).await;

    // Unknown methods are rejected.
    let id = peer.next_id;
    peer.next_id += 1;
    peer.send(&json!({"jsonrpc":"2.0","id":id,"method":"authenticate",
        "params":{"methodId": "nope"}}))
        .await;
    let frame = peer.next_frame().await;
    assert!(frame.get("error").is_some(), "expected error, got {frame}");
}

#[tokio::test]
async fn phase1_load_replays_history() {
    let _live = LIVE.lock().await;
    let mut peer = Peer::spawn().await;
    let session = new_session(&mut peer).await;
    let (result, _) = prompt(
        &mut peer,
        &session,
        "Remember this codename: Blueberry. Reply with exactly: stored.",
    )
    .await;
    assert_eq!(result["stopReason"], json!("end_turn"));

    let id = peer.next_id;
    peer.next_id += 1;
    peer.send(&json!({"jsonrpc":"2.0","id":id,"method":"session/load",
        "params":{"sessionId": session, "cwd": "/tmp", "mcpServers": []}}))
        .await;
    let mut replay = String::new();
    let modes = loop {
        let frame = peer.next_frame().await;
        if frame.get("id") == Some(&json!(id)) {
            assert!(frame.get("error").is_none(), "load failed: {}", frame["error"]);
            break frame["result"]["modes"].clone();
        }
        if frame.get("method") == Some(&json!("session/update")) {
            let update = &frame["params"]["update"];
            let kind = update.get("sessionUpdate");
            if (kind == Some(&json!("user_message_chunk"))
                || kind == Some(&json!("agent_message_chunk")))
                && let Some(t) = update["content"]["text"].as_str()
            {
                replay.push_str(t);
            }
        }
    };
    assert!(modes.get("availableModes").is_some());
    assert!(replay.contains("Blueberry"), "history not replayed: {replay}");

    // Unknown sessions are rejected.
    let id = peer.next_id;
    peer.next_id += 1;
    peer.send(&json!({"jsonrpc":"2.0","id":id,"method":"session/load",
        "params":{"sessionId": "nope", "cwd": "/tmp", "mcpServers": []}}))
        .await;
    let frame = peer.next_frame().await;
    assert!(frame.get("error").is_some(), "expected error, got {frame}");
}

#[tokio::test]
async fn phase1_write_reports_diff() {
    let _live = LIVE.lock().await;
    let mut peer = Peer::spawn().await;
    let session = new_session(&mut peer).await;

    let id = peer.next_id;
    peer.next_id += 1;
    peer.send(&json!({"jsonrpc":"2.0","id":id,"method":"session/prompt",
        "params":{"sessionId": session, "prompt": [{
            "type": "text",
            "text": "Write exactly `write-diff-ok` to /tmp/nail-write-probe.txt using the write tool, then reply with done and nothing else."
        }]}}))
        .await;
    let mut notifs = Vec::new();
    let stop = loop {
        let frame = peer.next_frame().await;
        if frame.get("id") == Some(&json!(id)) {
            assert!(frame.get("error").is_none(), "prompt failed: {}", frame["error"]);
            break frame["result"]["stopReason"].clone();
        }
        if frame.get("method") == Some(&json!("session/request_permission")) {
            let req_id = frame["id"].clone();
            peer.send(&json!({"jsonrpc":"2.0","id":req_id,
                "result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}))
                .await;
            continue;
        }
        notifs.push(frame);
    };
    assert_eq!(stop, json!("end_turn"));
    // The completion update embeds a real diff for Zed to render
    // (ToolCallContent is internally tagged: {"type": "diff", ...}).
    assert!(
        notifs.iter().any(|n| {
            let update = &n["params"]["update"];
            update.get("sessionUpdate") == Some(&json!("tool_call_update"))
                && update.get("content").and_then(|c| c.as_array()).map(|items| {
                    items.iter().any(|i| {
                        i.get("type") == Some(&json!("diff"))
                            && i.get("newText").and_then(|t| t.as_str())
                                == Some("write-diff-ok")
                    })
                }).unwrap_or(false)
        }),
        "expected a tool_call_update with diff"
    );
    assert!(Peer::agent_text(&notifs).contains("done"));
}

#[tokio::test]
async fn phase1_thought_chunks_stream() {
    let _live = LIVE.lock().await;
    let mut peer = Peer::spawn().await;
    let session = new_session(&mut peer).await;

    let id = peer.next_id;
    peer.next_id += 1;
    peer.send(&json!({"jsonrpc":"2.0","id":id,"method":"session/prompt",
        "params":{"sessionId": session, "prompt": [{
            "type": "text",
            "text": "Why is the sky blue? Think step by step, then answer briefly."
        }]}}))
        .await;
    let mut thought = String::new();
    let stop = loop {
        let frame = peer.next_frame().await;
        if frame.get("id") == Some(&json!(id)) {
            assert!(frame.get("error").is_none(), "prompt failed: {}", frame["error"]);
            break frame["result"]["stopReason"].clone();
        }
        if frame.get("method") == Some(&json!("session/update"))
            && frame["params"]["update"].get("sessionUpdate")
                == Some(&json!("agent_thought_chunk"))
            && let Some(t) = frame["params"]["update"]["content"]["text"].as_str()
        {
            thought.push_str(t);
        }
    };
    assert_eq!(stop, json!("end_turn"));
    assert!(!thought.is_empty(), "expected thought chunks");
}

/// octofs auto-attaches per session and shadows the built-in tools.
#[tokio::test]
async fn phase1_octofs_embedded() {
    let octofs = "/home/qkun/agent/nail-agent/tools/octofs/target/release/octofs";
    if !std::path::Path::new(octofs).is_file() {
        eprintln!("SKIP: octofs binary not built at {octofs}");
        return;
    }
    let _live = LIVE.lock().await;
    let dir = std::env::temp_dir().join(format!("nail-octofs-{}", std::process::id()));
    let mut peer = Peer::spawn_in_with_env(dir, &[("OCTOFS_BIN", octofs)]).await;
    let session = new_session(&mut peer).await;

    let id = peer.next_id;
    peer.next_id += 1;
    peer.send(&json!({"jsonrpc":"2.0","id":id,"method":"session/prompt",
        "params":{"sessionId": session, "prompt": [{
            "type": "text",
            "text": "You MUST call the octofs__shell tool with command `echo octofs-ok`, then reply with the tool result and nothing else."
        }]}}))
        .await;
    let mut notifs = Vec::new();
    let stop = loop {
        let frame = peer.next_frame().await;
        if frame.get("id") == Some(&json!(id)) {
            assert!(frame.get("error").is_none(), "prompt failed: {}", frame["error"]);
            break frame["result"]["stopReason"].clone();
        }
        if frame.get("method") == Some(&json!("session/request_permission")) {
            let req_id = frame["id"].clone();
            peer.send(&json!({"jsonrpc":"2.0","id":req_id,
                "result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}))
                .await;
            continue;
        }
        notifs.push(frame);
    };
    assert_eq!(stop, json!("end_turn"));
    // The call went through the embedded server, not the built-in run tool.
    assert!(
        notifs.iter().any(|n| {
            let update = &n["params"]["update"];
            update.get("sessionUpdate") == Some(&json!("tool_call"))
                && update.get("title").and_then(|t| t.as_str()).unwrap_or("")
                    .starts_with("octofs: shell")
        }),
        "expected an octofs shell tool_call"
    );
    assert!(Peer::agent_text(&notifs).contains("octofs-ok"));
}

/// A cancel landing on an idle session must not poison the next turn.
/// Regression: the flag used to stay set and the following prompt died
/// instantly with zero output, zero thought, zero usage.
#[tokio::test]
async fn phase1_idle_cancel_does_not_poison_next_turn() {
    let _live = LIVE.lock().await;
    let mut peer = Peer::spawn().await;
    let session = new_session(&mut peer).await;

    // Cancel with nothing running.
    peer.notify("session/cancel", json!({"sessionId": session})).await;
    // Give the notification a moment to be processed.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let (result, text) = prompt(&mut peer, &session, "Reply with exactly: ok").await;
    assert_eq!(result["stopReason"], json!("end_turn"));
    assert!(text.contains("ok"), "turn was poisoned by idle cancel: {text}");
}
