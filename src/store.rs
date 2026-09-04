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
}

pub struct Store {
    db: Mutex<DbAny>,
}

impl Store {
    /// Open (creating parents as needed) the database file.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| format!("store dir: {e}"))?;
        }
        let filename = path.to_str().ok_or("non-utf8 store path")?;
        let db = DbAny::new_mapped(filename).map_err(|e| format!("open store: {e}"))?;
        Ok(Self {
            db: Mutex::new(db),
        })
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
        let values: Vec<agdb::DbKeyValue> = vec![
            ("cwd", stored.cwd.to_string_lossy().as_ref()).into(),
            ("mode", stored.mode.as_str()).into(),
            ("transcript", transcript.as_str()).into(),
        ];
        let mut db = self.db.lock().map_err(|_| "store lock poisoned")?;
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
        let db = self.db.lock().map_err(|_| "store lock poisoned")?;
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
        let transcript: Vec<ChatCompletionRequestMessage> =
            serde_json::from_str(&transcript).map_err(|e| format!("decode: {e}"))?;
        Ok(Some(StoredSession {
            cwd: std::path::PathBuf::from(cwd),
            mode,
            transcript,
        }))
    }

    /// Delete a session record. Missing records are fine.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut db = self.db.lock().map_err(|_| "store lock poisoned")?;
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
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".config").join("nail-agent")
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
    fn rejects_path_games_in_id() {
        let (store, _dir) = test_store();
        store
            .save(
                "../evil",
                &StoredSession {
                    cwd: std::path::PathBuf::from("/tmp"),
                    mode: "m".to_string(),
                    transcript: vec![],
                },
            )
            .expect("save");
        // Sanitized to a flat alias: no directory traversal happened.
        assert!(store.load("../evil").expect("load").is_some());
    }
}
