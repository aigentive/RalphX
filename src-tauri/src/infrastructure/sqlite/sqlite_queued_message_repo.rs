use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::domain::entities::ChatContextType;
use crate::domain::repositories::QueuedMessageRepository;
use crate::domain::services::{QueueKey, QueuedMessage};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

pub struct SqliteQueuedMessageRepository {
    db: DbConnection,
}

impl SqliteQueuedMessageRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

fn serialize_message(message: &QueuedMessage) -> AppResult<String> {
    serde_json::to_string(message).map_err(|error| AppError::Database(error.to_string()))
}

fn deserialize_message(payload_json: &str) -> AppResult<QueuedMessage> {
    serde_json::from_str(payload_json).map_err(|error| AppError::Database(error.to_string()))
}

fn parse_context_type(raw: String) -> AppResult<ChatContextType> {
    ChatContextType::from_str(&raw).map_err(AppError::Database)
}

fn sequence_for_insert(conn: &Connection, key: &QueueKey, insert_front: bool) -> AppResult<i64> {
    let sql = if insert_front {
        "SELECT COALESCE(MIN(sequence), 0) - 1
         FROM queued_messages
         WHERE context_type = ?1 AND context_id = ?2"
    } else {
        "SELECT COALESCE(MAX(sequence), 0) + 1
         FROM queued_messages
         WHERE context_type = ?1 AND context_id = ?2"
    };
    let sequence = conn.query_row(
        sql,
        params![key.context_type.to_string(), key.context_id],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(sequence)
}

fn enqueue_with_position(
    conn: &Connection,
    key: &QueueKey,
    message: &QueuedMessage,
    insert_front: bool,
) -> AppResult<()> {
    let payload_json = serialize_message(message)?;
    let sequence = sequence_for_insert(conn, key, insert_front)?;
    conn.execute(
        "INSERT INTO queued_messages (
            id, context_type, context_id, content, created_at, is_editing,
            sequence, payload_json, inserted_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8,
            strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'),
            strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
         )
         ON CONFLICT(id) DO UPDATE SET
            context_type = excluded.context_type,
            context_id = excluded.context_id,
            content = excluded.content,
            created_at = excluded.created_at,
            is_editing = excluded.is_editing,
            sequence = excluded.sequence,
            payload_json = excluded.payload_json,
            updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')",
        params![
            message.id,
            key.context_type.to_string(),
            key.context_id,
            message.content,
            message.created_at,
            message.is_editing as i64,
            sequence,
            payload_json,
        ],
    )?;
    Ok(())
}

#[async_trait]
impl QueuedMessageRepository for SqliteQueuedMessageRepository {
    async fn enqueue_back(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
        let key = key.clone();
        let message = message.clone();
        self.db
            .run_transaction(move |conn| enqueue_with_position(conn, &key, &message, false))
            .await
    }

    async fn enqueue_front(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()> {
        let key = key.clone();
        let message = message.clone();
        self.db
            .run_transaction(move |conn| enqueue_with_position(conn, &key, &message, true))
            .await
    }

    async fn list(&self, key: &QueueKey) -> AppResult<Vec<QueuedMessage>> {
        let key = key.clone();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT payload_json
                     FROM queued_messages
                     WHERE context_type = ?1 AND context_id = ?2
                     ORDER BY sequence ASC, created_at ASC, id ASC",
                )?;
                let rows = stmt
                    .query_map(
                        params![key.context_type.to_string(), key.context_id],
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.iter()
                    .map(|payload| deserialize_message(payload))
                    .collect()
            })
            .await
    }

    async fn list_keys(&self) -> AppResult<Vec<QueueKey>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT context_type, context_id
                     FROM queued_messages
                     GROUP BY context_type, context_id
                     HAVING COUNT(*) > 0
                     ORDER BY MIN(sequence) ASC, context_type ASC, context_id ASC",
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.into_iter()
                    .map(|(context_type, context_id)| {
                        Ok(QueueKey::new(parse_context_type(context_type)?, context_id))
                    })
                    .collect()
            })
            .await
    }

    async fn delete(&self, key: &QueueKey, message_id: &str) -> AppResult<bool> {
        let key = key.clone();
        let message_id = message_id.to_string();
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "DELETE FROM queued_messages
                     WHERE context_type = ?1 AND context_id = ?2 AND id = ?3",
                    params![key.context_type.to_string(), key.context_id, message_id],
                )?;
                Ok(rows > 0)
            })
            .await
    }

    async fn delete_by_id(&self, message_id: &str) -> AppResult<bool> {
        let message_id = message_id.to_string();
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "DELETE FROM queued_messages WHERE id = ?1",
                    params![message_id],
                )?;
                Ok(rows > 0)
            })
            .await
    }

    async fn clear(&self, key: &QueueKey) -> AppResult<()> {
        let key = key.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM queued_messages
                     WHERE context_type = ?1 AND context_id = ?2",
                    params![key.context_type.to_string(), key.context_id],
                )?;
                Ok(())
            })
            .await
    }

    async fn pop_front(&self, key: &QueueKey) -> AppResult<Option<QueuedMessage>> {
        let key = key.clone();
        self.db
            .run_transaction(move |conn| {
                let selected = conn.query_row(
                    "SELECT id, payload_json
                     FROM queued_messages
                     WHERE context_type = ?1 AND context_id = ?2
                     ORDER BY sequence ASC, created_at ASC, id ASC
                     LIMIT 1",
                    params![key.context_type.to_string(), key.context_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                );
                let (id, payload_json) = match selected {
                    Ok(row) => row,
                    Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                    Err(error) => return Err(AppError::from(error)),
                };
                conn.execute("DELETE FROM queued_messages WHERE id = ?1", params![id])?;
                Ok(Some(deserialize_message(&payload_json)?))
            })
            .await
    }

    async fn remove_stale(
        &self,
        key: &QueueKey,
        threshold_secs: u64,
    ) -> AppResult<Vec<QueuedMessage>> {
        let key = key.clone();
        self.db
            .run_transaction(move |conn| {
                let rows = {
                    let mut stmt = conn.prepare(
                        "SELECT id, payload_json
                         FROM queued_messages
                         WHERE context_type = ?1 AND context_id = ?2
                         ORDER BY sequence ASC, created_at ASC, id ASC",
                    )?;
                    let rows = stmt.query_map(
                        params![key.context_type.to_string(), key.context_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )?;
                    rows.collect::<rusqlite::Result<Vec<_>>>()?
                };

                let now = chrono::Utc::now();
                let mut stale = Vec::new();
                for (id, payload_json) in rows {
                    let message = deserialize_message(&payload_json)?;
                    let is_stale = chrono::DateTime::parse_from_rfc3339(&message.created_at)
                        .map(|timestamp| {
                            let age =
                                now.signed_duration_since(timestamp.with_timezone(&chrono::Utc));
                            age.num_seconds() > threshold_secs as i64
                        })
                        .unwrap_or(false);
                    if is_stale && message.is_hidden_recovery() {
                        conn.execute("DELETE FROM queued_messages WHERE id = ?1", params![id])?;
                        stale.push(message);
                    }
                }
                Ok(stale)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_queued_message_repo_tests.rs"]
mod tests;
