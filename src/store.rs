//! AgDb persistence for sessions.
//!
//! Deliberately KV-style (one node per session, transcript as a JSON blob) —
//! the graph stays available for later (tool lineage, memory) without
//! taxing the current design. File lives at `<data_dir>/agdb/session.db`;
//! `ZACP_DATA_DIR`/`NAIL_DATA_DIR` redirect it in tests.
//!
//! Idiom follows nail's `code/database` wrapper (aliases, upsert by
//! remove+insert, not-found tolerant reads).

use std::sync::Mutex;

use agdb::{DbAny, DbErrorType, QueryBuilder};
use async_openai::types::chat::ChatCompletionRequestMessage;

fn is_not_found(error: &agdb::DbError) -> bool {
    error.ty == DbErrorType::NotFound
}

/// The persisted shape of one session.
#[derive(Debug, Clone)]
pub struct StoredSession {
    pub cwd: std::path::PathBuf,
    pub mode: String,
    pub transcript: Vec<ChatCompletionRequestMessage>,
    /// Tools approved with "allow always" (permission memory survives restarts).
    pub always_allowed: Vec<String>,
}

pub struct Store {
    /// `None` when this process lost the admission race and runs degraded:
    /// sessions work for the process lifetime but are not persisted.
    db: Mutex<Option<DbAny>>,
    // Process-exclusive lock file, held for the whole lifetime: AgDb memory
    // maps have no inter-process exclusion, and a second writer tears the
    // file (seen twice in production). The guard lives as long as the Store.
    _lock: Option<std::fs::File>,
}

impl Store {
    /// Open (creating parents as needed) the database file.
    ///
    /// A corrupted file (e.g. torn by a previous concurrent writer) is moved
    /// aside to `<name>.corrupt-<unix_ts>` and replaced with a fresh database
    /// instead of refusing to start — an agent that won't boot helps nobody.
    /// A second *live* writer is refused loudly instead of corrupting silently.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| format!("store dir: {e}"))?;
        }
        // Admission lock first: fail fast when another server is alive.
        let lock_path = path.with_extension("lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| format!("store lock file: {e}"))?;
        {
            use fs2::FileExt;
            lock_file
                .try_lock_exclusive()
                .map_err(|_| {
                    "another nail-agent server holds the session store; \
                     refusing to start a second writer (would corrupt AgDb). \
                     Stop the other process or point NAIL_DATA_DIR elsewhere."
                        .to_string()
                })?;
        }
        let filename = path
            .to_str()
            .ok_or("non-utf8 store path")?
            .to_string();
        match DbAny::new_mapped(filename.as_str()) {
            Ok(db) => Ok(Self {
                db: Mutex::new(Some(db)),
                _lock: Some(lock_file),
            }),
            Err(first) => {
                if !std::path::Path::new(&filename).exists() {
                    return Err(format!("open store: {first}"));
                }
                let backup = format!(
                    "{filename}.corrupt-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                );
                tracing::warn!("session store corrupted, moving aside to {backup}: {first}");
                std::fs::rename(&filename, &backup).map_err(|e| {
                    format!("open store: {first}; backup also failed: {e}")
                })?;
                let db =
                    DbAny::new_mapped(filename.as_str()).map_err(|e| format!("recreate store: {e}"))?;
                Ok(Self { db: Mutex::new(Some(db)), _lock: Some(lock_file) })
            }
        }
    }

    /// Degraded in-memory store: used when another live process holds the
    /// admission lock (e.g. a second Zed window). Sessions work normally
    /// within this process but are not persisted across restarts.
    pub fn ephemeral() -> Self {
        Self {
            db: Mutex::new(None),
            _lock: None,
        }
    }

    /// `true` when sessions actually persist (vs. degraded in-memory mode).
    pub fn is_persistent(&self) -> bool {
        self.db.lock().map(|db| db.is_some()).unwrap_or(false)
    }

    fn alias(id: &str) -> String {
        let safe: String = id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        format!("session:{safe}")
    }

    /// Insert-or-replace the whole session record.
    pub fn save(&self, id: &str, stored: &StoredSession) -> Result<(), String> {
        let transcript =
            serde_json::to_string(&stored.transcript).map_err(|e| format!("encode: {e}"))?;
        let allow = serde_json::to_string(&stored.always_allowed).map_err(|e| format!("encode: {e}"))?;
        let values: Vec<agdb::DbKeyValue> = vec![
            ("cwd", stored.cwd.to_string_lossy().as_ref()).into(),
            ("mode", stored.mode.as_str()).into(),
            ("transcript", transcript.as_str()).into(),
            ("allow", allow.as_str()).into(),
        ];
        let mut guard = self.db.lock().map_err(|_| "store lock poisoned")?;
        let Some(db) = guard.as_mut() else {
            // Degraded mode: persistence is a no-op, sessions stay in memory.
            return Ok(());
        };
        // Remove-then-insert: simple, atomic inside one lock hold.
        let alias = Self::alias(id);
        match db.exec_mut(QueryBuilder::remove().ids([alias.clone()]).query()) {
            Ok(_) => {}
            Err(e) if is_not_found(&e) => {}
            Err(e) => return Err(format!("store remove: {e}")),
        }
        db.exec_mut(
            QueryBuilder::insert()
                .nodes()
                .aliases([alias])
                .values([values])
                .query(),
        )
        .map_err(|e| format!("store insert: {e}"))?;
        Ok(())
    }

    /// Load a session record, or `None` when unknown.
    pub fn load(&self, id: &str) -> Result<Option<StoredSession>, String> {
        let guard = self.db.lock().map_err(|_| "store lock poisoned")?;
        let Some(db) = guard.as_ref() else {
            return Ok(None);
        };
        let result = match db.exec(QueryBuilder::select().ids([Self::alias(id)]).query()) {
            Ok(result) => result,
            Err(e) if is_not_found(&e) => return Ok(None),
            Err(e) => return Err(format!("store select: {e}")),
        };
        let element = match result.elements.first() {
            Some(element) => element,
            None => return Ok(None),
        };
        let get = |key: &str| {
            element.values.iter().find_map(|pair| match pair.key.string() {
                Ok(k) if k.as_str() == key => pair.value.string().ok().cloned(),
                _ => None,
            })
        };
        let (Some(cwd), Some(mode), Some(transcript)) =
            (get("cwd"), get("mode"), get("transcript"))
        else {
            return Ok(None);
        };
        // Records written before permission memory existed lack "allow".
        let always_allowed: Vec<String> = get("allow")
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let transcript: Vec<ChatCompletionRequestMessage> =
            serde_json::from_str(&transcript).map_err(|e| format!("decode: {e}"))?;
        Ok(Some(StoredSession {
            cwd: std::path::PathBuf::from(cwd),
            mode,
            transcript,
            always_allowed,
        }))
    }

    /// Delete a session record. Missing records are fine.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut guard = self.db.lock().map_err(|_| "store lock poisoned")?;
        let Some(db) = guard.as_mut() else {
            return Ok(());
        };
        match db.exec_mut(QueryBuilder::remove().ids([Self::alias(id)]).query()) {
            Ok(_) => Ok(()),
            Err(e) if is_not_found(&e) => Ok(()),
            Err(e) => Err(format!("store delete: {e}")),
        }
    }
}

/// Base directory for agent data. `NAIL_DATA_DIR` wins (tests), then the
/// legacy `ZACP_DATA_DIR`, then `~/.config/nail-agent`.
pub fn data_dir() -> std::path::PathBuf {
    for var in ["NAIL_DATA_DIR", "ZACP_DATA_DIR"] {
        if let Ok(dir) = std::env::var(var)
            && !dir.is_empty()
        {
            return std::path::PathBuf::from(dir);
        }
    }
    for var in ["HOME", "USERPROFILE"] {
        if let Ok(home) = std::env::var(var)
            && !home.trim().is_empty()
        {
            return std::path::PathBuf::from(home).join(".config").join("nail-agent");
        }
    }
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA")
        && !appdata.trim().is_empty()
    {
        return std::path::PathBuf::from(appdata).join("nail-agent");
    }
    std::path::PathBuf::from("/tmp").join(".config").join("nail-agent")
}

pub fn store_path() -> std::path::PathBuf {
    data_dir().join("agdb").join("session.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = Store::open(&dir.path().join("s.db")).expect("open");
        (store, dir)
    }

    #[test]
    fn roundtrip() {
        let (store, _dir) = test_store();
        assert!(store.load("nope").expect("load").is_none());
        store
            .save(
                "s1",
                &StoredSession {
                    cwd: std::path::PathBuf::from("/tmp"),
                    mode: "m".to_string(),
                    transcript: vec![crate::llm::Llm::user("hi")],
                    always_allowed: vec![],
                },
            )
            .expect("save");
        let back = store.load("s1").expect("load").expect("found");
        assert_eq!(back.cwd, std::path::PathBuf::from("/tmp"));
        assert_eq!(back.mode, "m");
        assert_eq!(back.transcript.len(), 1);
        store.delete("s1").expect("delete");
        assert!(store.load("s1").expect("load").is_none());
        store.delete("s1").expect("delete missing ok");
    }

    #[test]
    fn corrupt_file_is_moved_aside_and_recreated() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("s.db");
        std::fs::write(&path, b"garbage bytes, not a database").expect("write");
        let store = Store::open(&path).expect("open recovers");
        store
            .save(
                "s1",
                &StoredSession {
                    cwd: std::path::PathBuf::from("/tmp"),
                    mode: "m".to_string(),
                    transcript: vec![],
                    always_allowed: vec![],
                },
            )
            .expect("save after recovery");
        assert!(store.load("s1").expect("load").is_some());
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .expect("readdir")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("s.db.corrupt-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn rejects_path_games_in_id() {        let (store, _dir) = test_store();
        store
            .save(
                "../evil",
                &StoredSession {
                    cwd: std::path::PathBuf::from("/tmp"),
                    mode: "m".to_string(),
                    transcript: vec![],
                    always_allowed: vec![],
                },
            )
            .expect("save");
        // Sanitized to a flat alias: no directory traversal happened.
        assert!(store.load("../evil").expect("load").is_some());
    }
}
