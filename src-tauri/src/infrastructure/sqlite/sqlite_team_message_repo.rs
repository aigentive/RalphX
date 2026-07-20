// SQLite-based TeamMessageRepository implementation
// Uses rusqlite with mutex-protected connection for thread-safe access

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::entities::team::{TeamMessageId, TeamMessageRecord, TeamSessionId};
use crate::domain::repositories::TeamMessageRepository;
use crate::error::AppResult;

fn parse_datetime(s: &str) -> DateTime<Utc> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&ndt);
    }
    Utc::now()
}

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<TeamMessageRecord> {
    let created_at_str: String = row.get("created_at")?;
    Ok(TeamMessageRecord {
        id: TeamMessageId::from_string(row.get::<_, String>("id")?),
        team_session_id: TeamSessionId::from_string(row.get::<_, String>("team_session_id")?),
        sender: row.get("sender")?,
        recipient: row.get("recipient")?,
        content: row.get("content")?,
        message_type: row.get("message_type")?,
        created_at: parse_datetime(&created_at_str),
    })
}

pub struct SqliteTeamMessageRepository {
    db: DbConnection,
}

impl SqliteTeamMessageRepository {
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
impl TeamMessageRepository for SqliteTeamMessageRepository {
    async fn create(&self, message: TeamMessageRecord) -> AppResult<TeamMessageRecord> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO team_messages (id, team_session_id, sender, recipient, content, message_type, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        message.id.as_str(),
                        message.team_session_id.as_str(),
                        message.sender,
                        message.recipient,
                        message.content,
                        message.message_type,
                        message.created_at.to_rfc3339(),
                    ],
                )?;
                Ok(message)
            })
            .await
    }

    async fn get_by_session(
        &self,
        session_id: &TeamSessionId,
    ) -> AppResult<Vec<TeamMessageRecord>> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, team_session_id, sender, recipient, content, message_type, created_at
                     FROM team_messages WHERE team_session_id = ?1 ORDER BY created_at ASC",
                )?;
                let messages = stmt
                    .query_map([session_id.as_str()], row_to_message)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(messages)
            })
            .await
    }

    async fn get_recent_by_session(
        &self,
        session_id: &TeamSessionId,
        limit: u32,
    ) -> AppResult<Vec<TeamMessageRecord>> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, team_session_id, sender, recipient, content, message_type, created_at
                     FROM team_messages WHERE team_session_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                )?;
                let mut messages: Vec<TeamMessageRecord> = stmt
                    .query_map(rusqlite::params![session_id.as_str(), limit], row_to_message)?
                    .collect::<Result<Vec<_>, _>>()?;
                messages.reverse();
                Ok(messages)
            })
            .await
    }

    async fn count_by_session(&self, session_id: &TeamSessionId) -> AppResult<u32> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM team_messages WHERE team_session_id = ?1",
                    [session_id.as_str()],
                    |row| row.get(0),
                )?;
                Ok(count as u32)
            })
            .await
    }

    async fn delete_by_session(&self, session_id: &TeamSessionId) -> AppResult<()> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM team_messages WHERE team_session_id = ?1",
                    [session_id.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &TeamMessageId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM team_messages WHERE id = ?1", [id.as_str()])?;
                Ok(())
            })
            .await
    }
}
