use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, TransactionBehavior};
use tokio::sync::Mutex;

use crate::domain::entities::{
    ChatConversationId, ConversationFolderReference, ConversationFolderReferenceId,
};
use crate::domain::repositories::ConversationFolderReferenceRepository;
use crate::error::{AppError, AppResult};

use super::DbConnection;

pub struct SqliteConversationFolderReferenceRepository {
    db: DbConnection,
}

const LIVE_PATH_UNIQUE_INDEX: &str = "idx_conversation_folder_references_live_path";

fn map_live_path_unique_error(
    error: AppError,
    reference: &ConversationFolderReference,
) -> AppError {
    match error {
        AppError::Database(message)
            if message.contains(
                "UNIQUE constraint failed: conversation_folder_references.conversation_id, conversation_folder_references.folder_path",
            ) || message.contains(LIVE_PATH_UNIQUE_INDEX) =>
        {
            AppError::ConversationFolderReferenceDuplicate {
                conversation_id: reference.conversation_id.as_str(),
                folder_path: reference.folder_path.clone(),
            }
        }
        other => other,
    }
}

impl SqliteConversationFolderReferenceRepository {
    pub fn new(connection: Connection) -> Self {
        Self {
            db: DbConnection::new(connection),
        }
    }

    pub fn from_shared(connection: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(connection),
        }
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationFolderReference> {
    let created_at: String = row.get("created_at")?;
    let removed_at: Option<String> = row.get("removed_at")?;
    Ok(ConversationFolderReference {
        id: ConversationFolderReferenceId::from_string(row.get::<_, String>("id")?),
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        folder_path: row.get("folder_path")?,
        display_name: row.get("display_name")?,
        created_at: DateTime::parse_from_rfc3339(&created_at)
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    created_at.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?
            .with_timezone(&Utc),
        removed_at: removed_at
            .map(|value| DateTime::parse_from_rfc3339(&value).map(|time| time.with_timezone(&Utc)))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
    })
}

#[async_trait]
impl ConversationFolderReferenceRepository for SqliteConversationFolderReferenceRepository {
    async fn create_if_below_live_cap(
        &self,
        reference: ConversationFolderReference,
        max_live_references: usize,
    ) -> AppResult<ConversationFolderReference> {
        let duplicate_context = reference.clone();
        self.db
            .run(move |connection| {
                let transaction = rusqlite::Transaction::new_unchecked(
                    connection,
                    TransactionBehavior::Immediate,
                )?;
                let duplicate_exists: bool = transaction.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM conversation_folder_references
                        WHERE conversation_id = ?1 AND folder_path = ?2 AND removed_at IS NULL
                     )",
                    rusqlite::params![reference.conversation_id.as_str(), &reference.folder_path,],
                    |row| row.get(0),
                )?;
                if duplicate_exists {
                    return Err(AppError::ConversationFolderReferenceDuplicate {
                        conversation_id: reference.conversation_id.as_str(),
                        folder_path: reference.folder_path.clone(),
                    });
                }
                let live_count: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM conversation_folder_references
                     WHERE conversation_id = ?1 AND removed_at IS NULL",
                    [reference.conversation_id.as_str()],
                    |row| row.get(0),
                )?;
                if live_count >= max_live_references as i64 {
                    return Err(AppError::ConversationFolderReferenceLimit {
                        conversation_id: reference.conversation_id.as_str(),
                        limit: max_live_references,
                    });
                }
                transaction.execute(
                    "INSERT INTO conversation_folder_references
                     (id, conversation_id, folder_path, display_name, created_at, removed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                    rusqlite::params![
                        reference.id.as_str(),
                        reference.conversation_id.as_str(),
                        reference.folder_path,
                        reference.display_name,
                        reference.created_at.to_rfc3339(),
                    ],
                )?;
                transaction.commit()?;
                Ok(reference)
            })
            .await
            .map_err(|error| map_live_path_unique_error(error, &duplicate_context))
    }

    async fn list_live(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ConversationFolderReference>> {
        let conversation_id = conversation_id.as_str();
        self.db
            .run(move |connection| {
                let mut statement = connection.prepare(
                    "SELECT id, conversation_id, folder_path, display_name, created_at, removed_at
                     FROM conversation_folder_references
                     WHERE conversation_id = ?1 AND removed_at IS NULL
                     ORDER BY created_at ASC, id ASC",
                )?;
                let references = statement
                    .query_map([conversation_id], map_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(references)
            })
            .await
    }

    async fn soft_remove(
        &self,
        id: &ConversationFolderReferenceId,
        conversation_id: &ChatConversationId,
    ) -> AppResult<bool> {
        let id = id.as_str();
        let conversation_id = conversation_id.as_str();
        self.db
            .run(move |connection| {
                Ok(connection.execute(
                    "UPDATE conversation_folder_references
                     SET removed_at = ?1
                     WHERE id = ?2 AND conversation_id = ?3 AND removed_at IS NULL",
                    rusqlite::params![Utc::now().to_rfc3339(), id, conversation_id],
                )? == 1)
            })
            .await
    }

    async fn delete_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str();
        self.db
            .run(move |connection| {
                connection.execute(
                    "DELETE FROM conversation_folder_references WHERE conversation_id = ?1",
                    [conversation_id],
                )?;
                Ok(())
            })
            .await
    }
}
