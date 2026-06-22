use anyhow::Context;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::types::SurrealValue;
use uuid::Uuid;

pub static DB: OnceCell<Arc<Surreal<Db>>> = OnceCell::new();

#[derive(Debug, Serialize, Deserialize, Clone, SurrealValue)]
pub struct SingleMessage {
    pub id: String,
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, SurrealValue)]
struct SessionHistory {
    session_id: String,
    messages: Vec<SingleMessage>,
}

pub async fn init_db(db_path: &str) -> anyhow::Result<()> {
    let timer = crate::logger::Timer::start("DB.init");
    log::info!("[DB] initializing database, path: {}", db_path);

    let db = Surreal::new::<SurrealKv>(db_path)
        .await
        .context("failed to create Surreal database instance")?;
    timer.lap("connection established");

    db.use_ns("agent")
        .use_db("sessions")
        .await
        .context("failed to select namespace and database")?;
    timer.lap("namespace/database selected");

    db.query("DEFINE TABLE IF NOT EXISTS session_history SCHEMALESS;")
        .await?
        .check()
        .context("failed to define session_history table")?;
    timer.lap("table created/confirmed");

    DB.set(Arc::new(db))
        .map_err(|_| anyhow::anyhow!("database already initialized"))?;

    timer.stop();
    log::info!("[DB] database initialization complete");
    Ok(())
}

fn get_db() -> anyhow::Result<Arc<Surreal<Db>>> {
    DB.get()
        .map(Clone::clone)
        .ok_or_else(|| anyhow::anyhow!("database not initialized"))
}

pub async fn load_session_history(session_id: &str) -> anyhow::Result<Vec<SingleMessage>> {
    let timer = crate::logger::Timer::start(format!("DB.load_session_history({})", session_id));
    let db = get_db()?;
    let result: Option<SessionHistory> = db.select(("session_history", session_id)).await?;
    let messages = result.map(|h| h.messages).unwrap_or_default();
    let elapsed = timer.stop();
    log::info!(
        "[DB] loaded session history: session={}, messages={}, elapsed={}ms",
        session_id,
        messages.len(),
        elapsed
    );
    Ok(messages)
}

pub async fn append_message(session_id: &str, role: &str, content: &str) -> anyhow::Result<()> {
    let content_preview = if content.len() > 200 {
        let truncated: String = content.chars().take(200).collect();
        format!("{}... ({} bytes)", truncated, content.len())
    } else {
        content.to_string()
    };

    log::info!(
        "[DB] appending message: session={}, role={}, content_len={}",
        session_id,
        role,
        content.len()
    );
    log::debug!(
        "[DB] message preview: role={}, content={}",
        role,
        content_preview
    );

    let timer = crate::logger::Timer::start(format!("DB.append_message({}, {})", session_id, role));

    let id = Uuid::now_v7().to_string();
    let new_msg = SingleMessage {
        id,
        role: role.to_string(),
        content: content.to_string(),
    };

    let sql = r#"
        UPSERT type::record("session_history", $sid)
        SET
            messages = array::append(messages ?? [], $msg),
            session_id = $sid
    "#;

    let db = get_db()?;
    db.query(sql)
        .bind(("msg", new_msg))
        .bind(("sid", session_id))
        .await?
        .check()
        .context("failed to append message to database")?;

    let elapsed = timer.stop();
    log::info!("[DB] message append complete, elapsed={}ms", elapsed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_crud_operations() {
        let temp_dir = tempdir().expect("failed to create temp dir");
        let path = temp_dir.path().to_str().expect("failed to convert path");

        init_db(path).await.expect("failed to init db");

        let sid = "test-session-001";

        append_message(sid, "user", "Hello").await.unwrap();
        append_message(sid, "assistant", "Hi there!").await.unwrap();

        let history = load_session_history(sid).await.unwrap();
        assert_eq!(history.len(), 2);
        assert!(!history[0].id.is_empty());
        assert!(!history[1].id.is_empty());
        assert_eq!(history[0].content, "Hello");
        assert_eq!(history[1].content, "Hi there!");

        let mut updated_messages = history.clone();
        updated_messages[0].content = "Hey".to_string();
        let _: Option<SessionHistory> = get_db()
            .unwrap()
            .update(("session_history", sid))
            .content(SessionHistory {
                session_id: sid.to_string(),
                messages: updated_messages,
            })
            .await
            .unwrap();

        let updated = load_session_history(sid).await.unwrap();
        assert_eq!(updated[0].content, "Hey");

        let _: Option<SessionHistory> = get_db()
            .unwrap()
            .delete(("session_history", sid))
            .await
            .unwrap();
        let empty = load_session_history(sid).await.unwrap();
        assert!(empty.is_empty());

        append_message(sid, "user", "New start").await.unwrap();
        let new_hist = load_session_history(sid).await.unwrap();
        assert_eq!(new_hist.len(), 1);
        assert_eq!(new_hist[0].content, "New start");
        assert!(!new_hist[0].id.is_empty());
    }
}
