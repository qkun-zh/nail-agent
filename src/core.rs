//! Session registry: lifecycle states plus per-session live data.
//!
//! A session is `Created` by `session/new`, `Active` while a turn runs, and
//! ends the turn `Completed`, `Cancelled` or `Failed`. Later phases add the
//! transcript (history), mode (model selection) and persistence hooks here —
//! the registry stays the single place that owns session state.

use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

use async_openai::types::chat::ChatCompletionRequestMessage;
use tokio::sync::watch;

use agent_client_protocol::schema::v1::McpServer;

use crate::llm::DEFAULT_MODEL;
use crate::store::{Store, StoredSession, store_path};

/// Max messages kept per transcript; older ones are dropped.
pub const MAX_TRANSCRIPT: usize = 100;

/// Lifecycle states of one ACP session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionState {
    /// Created via `session/new`, no turn has run yet.
    #[default]
    Created,
    /// A prompt turn is currently running.
    Active,
    /// Last turn finished successfully.
    Completed,
    /// Last turn was cancelled via `session/cancel`.
    Cancelled,
    /// Last turn failed with an error.
    Failed,
}

/// Live data for one session.
#[derive(Debug)]
pub struct Session {
    pub state: SessionState,
    /// Working directory from `session/new`; tools resolve against it.
    /// Phase 3 (tools layer) starts reading this.
    #[allow(dead_code)]
    pub cwd: std::path::PathBuf,
    /// Active mode id (= model id, see `llm::available_modes`).
    pub mode: String,
    /// Conversation history, sent back on every turn.
    pub transcript: Vec<ChatCompletionRequestMessage>,
    /// MCP servers forwarded by the client (`session/new`); connected lazily.
    /// Not persisted: the client resends them on `session/resume`.
    pub mcp_servers: Vec<McpServer>,
    /// Tool names the user approved with "allow always" in this session.
    pub always_allowed: HashSet<String>,
    pub turn_count: u32,
    pub last_activity_at: u64,
    /// Set to `true` by the `session/cancel` handler; the running turn
    /// observes it between streamed chunks and stops early.
    cancel_tx: watch::Sender<bool>,
    cancel_rx: watch::Receiver<bool>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Thread-safe registry of all live sessions.
pub struct Sessions {
    inner: Mutex<HashMap<String, Session>>,
    next_id: AtomicU64,
    store: Store,
}

impl Sessions {
    pub fn new() -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            store: Store::open(&store_path())?,
        }))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Session>> {
        self.inner.lock().expect("sessions mutex poisoned")
    }

    /// Create a session in [`SessionState::Created`] and return its id.
    pub fn create(&self, cwd: std::path::PathBuf, mcp_servers: Vec<McpServer>) -> String {
        let id = format!("nail-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let now = now_unix();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        self.lock().insert(
            id.clone(),
            Session {
                state: SessionState::Created,
                cwd,
                mode: DEFAULT_MODEL.to_string(),
                transcript: Vec::new(),
                mcp_servers,
                always_allowed: HashSet::new(),
                turn_count: 0,
                last_activity_at: now,
                cancel_tx,
                cancel_rx,
            },
        );
        id
    }

    /// Drop a session (`session/close`). Returns `false` when unknown.
    pub fn remove(&self, id: &str) -> bool {
        let existed = self.lock().remove(id).is_some();
        if existed
            && let Err(err) = self.store.delete(id)
        {
            tracing::warn!(session = id, error = %err, "persisted session not deleted");
        }
        existed
    }

    pub fn exists(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    /// Working directory of a session, if it exists.
    /// Phase 3 (tools layer) starts calling this.
    #[allow(dead_code)]
    pub fn cwd_of(&self, id: &str) -> Option<std::path::PathBuf> {
        self.lock().get(id).map(|s| s.cwd.clone())
    }

    /// Current mode id of a session, if it exists.
    pub fn mode_of(&self, id: &str) -> Option<String> {
        self.lock().get(id).map(|s| s.mode.clone())
    }

    /// Switch a session's mode. Returns `false` when the session is unknown.
    pub fn set_mode(&self, id: &str, mode: &str) -> bool {
        if let Some(session) = self.lock().get_mut(id) {
            session.mode = mode.to_string();
            session.last_activity_at = now_unix();
            true
        } else {
            false
        }
    }

    /// A copy of the session transcript for building the next request.
    pub fn transcript_of(&self, id: &str) -> Option<Vec<ChatCompletionRequestMessage>> {
        self.lock().get(id).map(|s| s.transcript.clone())
    }

    /// The MCP servers forwarded for a session, if it exists.
    pub fn mcp_servers_of(&self, id: &str) -> Option<Vec<McpServer>> {
        self.lock().get(id).map(|s| s.mcp_servers.clone())
    }

    /// Replace the forwarded MCP servers (used on `session/resume`, where
    /// the client resends them).
    pub fn set_mcp_servers(&self, id: &str, servers: Vec<McpServer>) -> bool {
        if let Some(session) = self.lock().get_mut(id) {
            session.mcp_servers = servers;
            true
        } else {
            false
        }
    }

    pub fn remember_allow_always(&self, id: &str, tool: &str) {
        if let Some(session) = self.lock().get_mut(id) {
            session.always_allowed.insert(tool.to_string());
        }
    }

    pub fn is_always_allowed(&self, id: &str, tool: &str) -> bool {
        self.lock()
            .get(id)
            .map(|s| s.always_allowed.contains(tool))
            .unwrap_or(false)
    }

    /// Replace the transcript (capped) and persist the session.
    pub fn save_transcript(&self, id: &str, mut transcript: Vec<ChatCompletionRequestMessage>) {
        if transcript.len() > MAX_TRANSCRIPT {
            transcript.drain(..transcript.len() - MAX_TRANSCRIPT);
        }
        let stored = self.lock().get_mut(id).map(|s| {
            s.transcript = transcript.clone();
            s.last_activity_at = now_unix();
            StoredSession {
                cwd: s.cwd.clone(),
                mode: s.mode.clone(),
                transcript,
            }
        });
        if let Some(stored) = stored
            && let Err(err) = self.store.save(id, &stored)
        {
            tracing::warn!(session = id, error = %err, "session not persisted");
        }
    }

    /// Recreate a session from the store (`session/resume`). A live session
    /// resumes trivially; returns `None` when unknown everywhere.
    pub fn restore(&self, id: &str) -> bool {
        if self.lock().contains_key(id) {
            return true;
        }
        let stored = match self.store.load(id) {
            Ok(stored) => stored,
            Err(err) => {
                tracing::warn!(session = id, error = %err, "resume load failed");
                return false;
            }
        };
        let Some(stored) = stored else {
            return false;
        };
        let now = now_unix();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        self.lock().insert(
            id.to_string(),
            Session {
                state: SessionState::Created,
                cwd: stored.cwd,
                mode: stored.mode,
                transcript: stored.transcript,
                // Fresh server list arrives with the resume request.
                mcp_servers: Vec::new(),
                always_allowed: HashSet::new(),
                turn_count: 0,
                last_activity_at: now,
                cancel_tx,
                cancel_rx,
            },
        );
        true
    }

    pub fn set_state(&self, id: &str, state: SessionState) -> bool {
        if let Some(session) = self.lock().get_mut(id) {
            session.state = state;
            session.last_activity_at = now_unix();
            true
        } else {
            false
        }
    }

    pub fn bump_turn(&self, id: &str) -> bool {
        if let Some(session) = self.lock().get_mut(id) {
            session.turn_count += 1;
            session.last_activity_at = now_unix();
            // Fresh turn, fresh cancellation flag.
            let _ = session.cancel_tx.send(false);
            true
        } else {
            false
        }
    }

    pub fn cancel_watcher(&self, id: &str) -> Option<watch::Receiver<bool>> {
        self.lock().get(id).map(|s| s.cancel_rx.clone())
    }

    /// Mark the session cancelled. Returns `false` when unknown.
    pub fn cancel(&self, id: &str) -> bool {
        if let Some(session) = self.lock().get_mut(id) {
            session.state = SessionState::Cancelled;
            session.last_activity_at = now_unix();
            let _ = session.cancel_tx.send(true);
            true
        } else {
            false
        }
    }
}
