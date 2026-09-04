# nail-agent — registry-grade ACP agent for Zed

Speaks [Agent Client Protocol v1](https://agentclientprotocol.com) over stdio.
Plain prompts are answered by a real model (streamed); the model gets local
tools (`run` / `read` / `write`) plus Zed-forwarded MCP servers through
standard function calling. Sessions persist in AgDb and survive restarts.

## Build & test

```sh
cargo build
cargo test   # unit + stdio e2e (some tests call the live model)
cargo clippy --all-targets  # zero warnings required
```

## Model key setup

Reads, in order: `NAIL_API_KEY` env → `~/.config/nail-agent/api_key` (`chmod 600`).
Optional: `NAIL_BASE_URL` (default: the project's DashScope workspace),
`NAIL_MODEL` (default: `qwen3.7-flash`), `NAIL_DATA_DIR` (data redirect),
`NAIL_KEY_FILE` (key file redirect).

## Use from Zed

```jsonc
{ "agent_servers": { "nail-agent": {
  "type": "custom",
  "command": "/path/to/nail-agent/target/release/nail-agent",
  "args": [], "env": {}
} } }
```

For a WSL project opened from Windows Zed, point `command` at the Linux
binary path (threads spawn inside the distro — no `wsl` wrapper).

Then: ask anything, switch models in the mode picker (Qwen3.7 Flash,
DeepSeek V4 Flash, Qwen Coder Flash), approve tool calls in the permission
prompt. Threads reopen from history via `session/resume`.

## What's implemented

- initialize (+ `agent`-type `api-key` auth method), session/new/load-guard,
  prompt (spawned turns), cancel (interrupts streams, kills children),
  close, resume + load (AgDb-backed, history replay), set_mode, authenticate
- streamed replies with per-turn token usage (`usage_update`) and visible
  thinking (`agent_thought_chunk`, via `byot` chunk types)
- **octofs embedded**: the [octofs](https://github.com/muvon/octofs) MCP filesystem
  server auto-attaches per session (`--path <cwd>`) when its binary is found
  (`OCTOFS_BIN` env or `PATH`); its tools (`octofs__view`, `octofs__shell`, …)
  shadow the overlapping built-ins so the model sees one toolbox.
  Without it, the three built-in tools cover run/read/write.
- permission-gated tools (allow-once/always/reject) with rich reports:
  inline content, file locations, raw I/O, and real diffs for writes
- safety: cwd confinement, sensitive-path refusal, destructive-command
  screen — but **not a sandbox**: the process runs as your user, the
  permission dialog is the real gate

## Layout

- `main.rs` — bootstrap (tracing to stderr)
- `proto.rs` — ACP handlers + turn loop
- `core.rs` — session registry and lifecycle
- `llm.rs` — model backends, modes, streaming
- `tools/` — local toolbox + permission + safety (`mcp.rs`: stdio client)
- `store.rs` — AgDb session persistence
