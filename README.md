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

All configuration is environment-driven (no recompile needed to switch providers):

| Variable | Default | Meaning |
|---|---|---|
| `NAIL_API_KEY` | — (required) | Model API key |
| `NAIL_BASE_URL` | built-in workspace endpoint | OpenAI-compatible base URL |
| `NAIL_MODEL` | first of `NAIL_MODELS` | Default model |
| `NAIL_MODELS` | 3 built-in modes | `id\|name\|description\|context_window,...` |
| `NAIL_KEY_FILE` | `~/.config/nail-agent/api_key` | Key file redirect |
| `NAIL_DATA_DIR` | `~/.config/nail-agent` | Session store + audit log |
| `NAIL_MAX_TOOL_TURNS` | `8` | Model→tool→model rounds per turn |
| `NAIL_MAX_TRANSCRIPT` | `100` | Max messages kept per transcript |
| `NAIL_OCTOFS` | enabled | Set to `0`/`off`/`false`/`no` to skip the embedded octofs server |
| `OCTOFS_BIN` | `PATH` lookup | Explicit octofs binary path |

Example (OpenAI):

```sh
export NAIL_API_KEY=sk-...
export NAIL_BASE_URL=https://api.openai.com/v1
export NAIL_MODELS='gpt-4o|GPT-4o|Default|128000,gpt-4o-mini|GPT-4o mini|Cheap|128000'
```

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

Notes for daily use:

- Running two Zed windows at once: the second process detects the first
  and keeps working with in-memory sessions (no resume across restarts
  there) instead of refusing to start.
- `allow always` permission memory persists across restarts per session.
- Errors and setup problems stream back prefixed (`[nail-agent error]` /
  `[nail-agent setup]`), never disguised as answers.
- Only MCP stdio servers are supported; others are skipped with a visible
  notice once per session.
- History replay shows text only; details live under `<data_dir>/audit.log`
  (rotated at ~1 MiB).

## What's implemented

- initialize (+ `agent`-type `api-key` auth method), session/new/load-guard,
  prompt (spawned turns), cancel (interrupts streams, kills children),
  close, resume + load (AgDb-backed, history replay), set_mode, authenticate
- streamed replies with per-turn token usage (`usage_update`) and visible
  thinking (`agent_thought_chunk`, via `byot` chunk types)
- tools run through **client capabilities** (the official surfaces):
  `fs/read_text_file`, `fs/write_text_file`, and `terminal/*` (live terminal
  embed + wait/kill/release) — so they work remotely and render natively
- **octofs embedded**: the [octofs](https://github.com/muvon/octofs) MCP filesystem
  server auto-attaches per session (`--path <cwd>`) when its binary is found —
  lookup order: `OCTOFS_BIN` env, then next to the agent executable itself
  (release archives ship both binaries together), then `PATH`.
  Its tools (`octofs__view`, `octofs__shell`, …) shadow the overlapping
  built-ins so the model sees one toolbox.
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
