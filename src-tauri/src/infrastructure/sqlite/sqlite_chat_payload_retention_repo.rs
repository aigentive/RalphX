use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::error::AppResult;

/// Deletes payload-only rows. Timeline hydration treats a missing row as absent payload data.
pub struct SqliteChatPayloadRetentionRepository {
    db: DbConnection,
}

impl SqliteChatPayloadRetentionRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }

    pub fn from_db(db: DbConnection) -> Self {
        Self { db }
    }

    pub async fn prune_batch(
        &self,
        before: DateTime<Utc>,
        archived_before: DateTime<Utc>,
        batch_rows: u32,
    ) -> AppResult<usize> {
        let before = before.to_rfc3339();
        let archived_before = archived_before.to_rfc3339();
        self.db
            .run(move |conn| {
                Ok(conn.execute(
                    r#"
                    DELETE FROM chat_message_block_payloads
                    WHERE block_id IN (
                        SELECT payload.block_id
                        FROM chat_message_block_payloads AS payload
                        INNER JOIN chat_message_blocks AS block ON block.id = payload.block_id
                        INNER JOIN chat_conversations AS conversation ON conversation.id = block.conversation_id
                        WHERE (conversation.archived_at IS NULL AND block.created_at < ?1)
                           OR (conversation.archived_at IS NOT NULL AND block.created_at < ?2)
                        ORDER BY block.created_at ASC, payload.block_id ASC
                        LIMIT ?3
                    )
                    "#,
                    params![before, archived_before, batch_rows],
                )?)
            })
            .await
    }
}
