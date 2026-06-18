// SQLite-based ChatAttachmentRepository implementation
// All rusqlite calls go through DbConnection::run() (spawn_blocking + blocking_lock)
// to prevent blocking the tokio async runtime / timer driver.

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;

use crate::domain::entities::{
    ChatAttachment, ChatAttachmentId, ChatConversationId, ChatMessageId,
};
use crate::domain::repositories::ChatAttachmentRepository;
use crate::error::AppResult;
use crate::infrastructure::sqlite::DbConnection;

/// Parse datetime string handling both RFC3339 and SQLite's CURRENT_TIMESTAMP formats
fn parse_datetime(s: &str) -> DateTime<Utc> {
    // Try RFC3339 first (e.g., "2026-01-26T06:42:37.662598+00:00")
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&Utc);
    }

    // Try SQLite's CURRENT_TIMESTAMP format (e.g., "2026-01-26 07:06:32")
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&ndt);
    }

    // Fallback to now
    Utc::now()
}

/// SQLite implementation of ChatAttachmentRepository
pub struct SqliteChatAttachmentRepository {
    db: DbConnection,
}

impl SqliteChatAttachmentRepository {
    /// Create a new SQLite chat attachment repository with the given connection
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    /// Create from an Arc-wrapped mutex connection (for sharing)
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl ChatAttachmentRepository for SqliteChatAttachmentRepository {
    async fn create(&self, attachment: ChatAttachment) -> AppResult<ChatAttachment> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO chat_attachments (id, conversation_id, message_id, file_name, file_path, mime_type, file_size, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        attachment.id.as_str(),
                        attachment.conversation_id.as_str(),
                        attachment.message_id.as_ref().map(|id| id.as_str()),
                        attachment.file_name,
                        attachment.file_path,
                        attachment.mime_type,
                        attachment.file_size,
                        attachment.created_at.to_rfc3339(),
                    ],
                )?;
                Ok(attachment)
            })
            .await
    }

    async fn get_by_id(&self, id: &ChatAttachmentId) -> AppResult<Option<ChatAttachment>> {
        let id_str = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, message_id, file_name, file_path, mime_type, file_size, created_at
                     FROM chat_attachments WHERE id = ?1",
                    rusqlite::params![id_str],
                    |row| {
                        let created_at_str: String = row.get("created_at")?;
                        let created_at = parse_datetime(&created_at_str);
                        Ok(ChatAttachment {
                            id: ChatAttachmentId::from_string(row.get::<_, String>("id")?),
                            conversation_id: ChatConversationId::from_string(
                                row.get::<_, String>("conversation_id")?,
                            ),
                            message_id: row
                                .get::<_, Option<String>>("message_id")?
                                .map(ChatMessageId::from_string),
                            file_name: row.get("file_name")?,
                            file_path: row.get("file_path")?,
                            mime_type: row.get("mime_type")?,
                            file_size: row.get("file_size")?,
                            created_at,
                        })
                    },
                )
            })
            .await
    }

    async fn find_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatAttachment>> {
        let conversation_id_str = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, conversation_id, message_id, file_name, file_path, mime_type, file_size, created_at
                     FROM chat_attachments WHERE conversation_id = ?1 ORDER BY created_at ASC",
                )?;
                let attachments = stmt
                    .query_map(rusqlite::params![conversation_id_str], |row| {
                        let created_at_str: String = row.get("created_at")?;
                        let created_at = parse_datetime(&created_at_str);
                        Ok(ChatAttachment {
                            id: ChatAttachmentId::from_string(row.get::<_, String>("id")?),
                            conversation_id: ChatConversationId::from_string(
                                row.get::<_, String>("conversation_id")?,
                            ),
                            message_id: row
                                .get::<_, Option<String>>("message_id")?
                                .map(ChatMessageId::from_string),
                            file_name: row.get("file_name")?,
                            file_path: row.get("file_path")?,
                            mime_type: row.get("mime_type")?,
                            file_size: row.get("file_size")?,
                            created_at,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(attachments)
            })
            .await
    }

    async fn find_by_message_id(
        &self,
        message_id: &ChatMessageId,
    ) -> AppResult<Vec<ChatAttachment>> {
        let message_id_str = message_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, conversation_id, message_id, file_name, file_path, mime_type, file_size, created_at
                     FROM chat_attachments WHERE message_id = ?1 ORDER BY created_at ASC",
                )?;
                let attachments = stmt
                    .query_map(rusqlite::params![message_id_str], |row| {
                        let created_at_str: String = row.get("created_at")?;
                        let created_at = parse_datetime(&created_at_str);
                        Ok(ChatAttachment {
                            id: ChatAttachmentId::from_string(row.get::<_, String>("id")?),
                            conversation_id: ChatConversationId::from_string(
                                row.get::<_, String>("conversation_id")?,
                            ),
                            message_id: row
                                .get::<_, Option<String>>("message_id")?
                                .map(ChatMessageId::from_string),
                            file_name: row.get("file_name")?,
                            file_path: row.get("file_path")?,
                            mime_type: row.get("mime_type")?,
                            file_size: row.get("file_size")?,
                            created_at,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(attachments)
            })
            .await
    }

    async fn update_message_id(
        &self,
        id: &ChatAttachmentId,
        message_id: &ChatMessageId,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let message_id_str = message_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE chat_attachments SET message_id = ?1 WHERE id = ?2",
                    rusqlite::params![message_id_str, id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_message_ids(
        &self,
        attachment_ids: &[ChatAttachmentId],
        message_id: &ChatMessageId,
    ) -> AppResult<()> {
        let message_id_str = message_id.as_str().to_string();
        let id_strings: Vec<String> = attachment_ids
            .iter()
            .map(|id| id.as_str().to_string())
            .collect();

        self.db
            .run(move |conn| {
                // Build a list of placeholders for the IN clause
                let placeholders = id_strings
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", i + 2))
                    .collect::<Vec<_>>()
                    .join(", ");

                let query = format!(
                    "UPDATE chat_attachments SET message_id = ?1 WHERE id IN ({})",
                    placeholders
                );

                // Build params using the owned strings
                let mut params: Vec<&dyn rusqlite::ToSql> =
                    vec![&message_id_str as &dyn rusqlite::ToSql];
                for id_str in &id_strings {
                    params.push(id_str as &dyn rusqlite::ToSql);
                }

                conn.execute(&query, params.as_slice())?;
                Ok(())
            })
            .await
    }

    async fn reparent_pending_attachments(
        &self,
        from_conversation_id: &ChatConversationId,
        to_conversation_id: &ChatConversationId,
    ) -> AppResult<usize> {
        let from_str = from_conversation_id.as_str().to_string();
        let to_str = to_conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE chat_attachments SET conversation_id = ?1 \
                     WHERE conversation_id = ?2 AND message_id IS NULL",
                    rusqlite::params![to_str, from_str],
                )?)
            })
            .await
    }

    async fn delete(&self, id: &ChatAttachmentId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM chat_attachments WHERE id = ?1",
                    rusqlite::params![id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        let conversation_id_str = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM chat_attachments WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id_str],
                )?;
                Ok(())
            })
            .await
    }
}
