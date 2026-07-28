use crate::domain::entities::{ChatConversationId, CoordinationMode, TeamSession, TeamSessionId};
use crate::domain::repositories::{TeamCoordinationTransitionRepository, TeamExitMarker};
use crate::error::AppResult;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;
pub struct MemoryTeamCoordinationTransitionRepository {
    sessions: RwLock<HashMap<TeamSessionId, TeamSession>>,
}
impl MemoryTeamCoordinationTransitionRepository {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }
}
impl Default for MemoryTeamCoordinationTransitionRepository {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl TeamCoordinationTransitionRepository for MemoryTeamCoordinationTransitionRepository {
    async fn enter_team(
        &self,
        conversation_id: &ChatConversationId,
        session: TeamSession,
    ) -> AppResult<TeamSession> {
        let mut values = self.sessions.write().await;
        if let Some(existing) = values.values().find(|value| {
            value.coordinator_conversation_id == *conversation_id && !value.status.is_closed()
        }) {
            return Ok(existing.clone());
        };
        values.insert(session.id.clone(), session.clone());
        Ok(session)
    }
    async fn mark_pending_exit(
        &self,
        team_id: &TeamSessionId,
        expected_version: i64,
        marker: TeamExitMarker,
    ) -> AppResult<bool> {
        let mut values = self.sessions.write().await;
        let Some(session) = values.get_mut(team_id) else {
            return Ok(false);
        };
        if session.version != expected_version {
            return Ok(false);
        };
        session.pending_coordination_mode = Some(marker.coordination_mode.to_string());
        session.pending_exit_action = Some(marker.exit_action);
        session.version += 1;
        Ok(true)
    }
    async fn commit_exit(
        &self,
        conversation_id: &ChatConversationId,
        team_id: &TeamSessionId,
        expected_version: i64,
    ) -> AppResult<bool> {
        let mut values = self.sessions.write().await;
        let Some(session) = values.get_mut(team_id) else {
            return Ok(false);
        };
        if session.coordinator_conversation_id != *conversation_id
            || session.version != expected_version
        {
            return Ok(false);
        };
        session.pending_coordination_mode = Some(CoordinationMode::Solo.to_string());
        session.version += 1;
        Ok(true)
    }
}
