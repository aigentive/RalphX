use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{AgentConversationMute, ChatConversationId};
use crate::domain::repositories::AgentConversationMuteRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

#[cfg(test)]
#[path = "sqlite_agent_conversation_mute_repo_tests.rs"]
mod tests;

const SQLITE_BIND_PARAMETER_LIMIT: usize = 900;

fn row_to_mute(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentConversationMute> {
    let muted_at: String = row.get("muted_at")?;
    let muted_at = DateTime::parse_from_rfc3339(&muted_at)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;

    Ok(AgentConversationMute {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        muted_at,
        state_fingerprint: row.get("state_fingerprint")?,
    })
}

pub struct SqliteAgentConversationMuteRepository {
    db: DbConnection,
}

impl SqliteAgentConversationMuteRepository {
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

#[async_trait]
impl AgentConversationMuteRepository for SqliteAgentConversationMuteRepository {
    async fn set_muted(&self, mute: AgentConversationMute) -> AppResult<()> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_conversation_mutes (
                        conversation_id, muted_at, state_fingerprint
                    ) VALUES (?1, ?2, ?3)
                    ON CONFLICT(conversation_id) DO UPDATE SET
                        muted_at = excluded.muted_at,
                        state_fingerprint = excluded.state_fingerprint",
                    params![
                        mute.conversation_id.as_str(),
                        mute.muted_at.to_rfc3339(),
                        mute.state_fingerprint,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn clear(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM agent_conversation_mutes WHERE conversation_id = ?1",
                    params![conversation_id],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationMute>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT conversation_id, muted_at, state_fingerprint
                     FROM agent_conversation_mutes WHERE conversation_id = ?1",
                    params![conversation_id],
                    row_to_mute,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn list_by_conversation_ids(
        &self,
        conversation_ids: &[ChatConversationId],
    ) -> AppResult<Vec<AgentConversationMute>> {
        if conversation_ids.is_empty() {
            return Ok(Vec::new());
        }

        let conversation_ids = conversation_ids
            .iter()
            .map(|conversation_id| conversation_id.as_str().to_string())
            .collect::<Vec<_>>();
        self.db
            .run(move |conn| {
                let mut mutes = Vec::new();
                for chunk in conversation_ids.chunks(SQLITE_BIND_PARAMETER_LIMIT) {
                    let placeholders = std::iter::repeat("?")
                        .take(chunk.len())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let query = format!(
                        "SELECT conversation_id, muted_at, state_fingerprint
                         FROM agent_conversation_mutes WHERE conversation_id IN ({placeholders})"
                    );
                    let mut statement = conn.prepare(&query)?;
                    let rows = statement
                        .query_map(params_from_iter(chunk.iter()), row_to_mute)?
                        .collect::<Result<Vec<_>, _>>()?;
                    mutes.extend(rows);
                }
                Ok(mutes)
            })
            .await
    }
}
