//! SQLite implementation of atomic Team coordination-mode transition operations.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::sqlite_team_repo::{get_open_session, insert_session};
use super::DbConnection;
use crate::domain::entities::{ChatConversationId, CoordinationMode, TeamSession, TeamSessionId};
use crate::domain::repositories::{TeamCoordinationTransitionRepository, TeamExitMarker};
use crate::error::AppResult;

pub struct SqliteTeamCoordinationTransitionRepository {
    db: DbConnection,
}

impl SqliteTeamCoordinationTransitionRepository {
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
impl TeamCoordinationTransitionRepository for SqliteTeamCoordinationTransitionRepository {
    async fn enter_team(
        &self,
        conversation_id: &ChatConversationId,
        session: TeamSession,
    ) -> AppResult<TeamSession> {
        let conversation_id = conversation_id.as_str();
        self.db
            .run_transaction(move |conn| {
                if let Some(existing) = get_open_session(conn, &conversation_id)? {
                    return Ok(existing);
                }
                insert_session(conn, &session)?;
                Ok(session)
            })
            .await
    }

    async fn mark_pending_exit(
        &self,
        team_id: &TeamSessionId,
        expected_version: i64,
        marker: TeamExitMarker,
    ) -> AppResult<bool> {
        let team_id = team_id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                let count = conn.execute(
                    "UPDATE managed_team_sessions SET
                        pending_coordination_mode = ?1, pending_exit_action = ?2,
                        version = version + 1
                     WHERE id = ?3 AND version = ?4",
                    rusqlite::params![
                        marker.coordination_mode.to_string(),
                        marker.exit_action,
                        team_id,
                        expected_version,
                    ],
                )?;
                Ok(count == 1)
            })
            .await
    }

    async fn commit_exit(
        &self,
        conversation_id: &ChatConversationId,
        team_id: &TeamSessionId,
        expected_version: i64,
    ) -> AppResult<bool> {
        let conversation_id = conversation_id.as_str();
        let team_id = team_id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                let count = conn.execute(
                    "UPDATE managed_team_sessions SET
                        pending_coordination_mode = ?1, version = version + 1
                     WHERE id = ?2 AND coordinator_conversation_id = ?3 AND version = ?4",
                    rusqlite::params![
                        CoordinationMode::Solo.to_string(),
                        team_id,
                        conversation_id,
                        expected_version,
                    ],
                )?;
                Ok(count == 1)
            })
            .await
    }
}
