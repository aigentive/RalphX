use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::{
    ChatConversationId, TeamMember, TeamMemberId, TeamSession, TeamSessionId,
};
use crate::domain::repositories::TeamRepository;
use crate::error::{AppError, AppResult};

pub struct MemoryTeamRepository {
    sessions: RwLock<HashMap<TeamSessionId, TeamSession>>,
    members: RwLock<HashMap<TeamMemberId, TeamMember>>,
}

impl MemoryTeamRepository {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            members: RwLock::new(HashMap::new()),
        }
    }
}
impl Default for MemoryTeamRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TeamRepository for MemoryTeamRepository {
    async fn ensure_session(&self, session: TeamSession) -> AppResult<TeamSession> {
        let mut sessions = self.sessions.write().await;
        if let Some(existing) = sessions.values().find(|value| {
            value.coordinator_conversation_id == session.coordinator_conversation_id
                && !value.status.is_closed()
        }) {
            return Ok(existing.clone());
        }
        sessions.insert(session.id.clone(), session.clone());
        Ok(session)
    }
    async fn get_session(&self, id: &TeamSessionId) -> AppResult<Option<TeamSession>> {
        Ok(self.sessions.read().await.get(id).cloned())
    }
    async fn get_open_session_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<TeamSession>> {
        Ok(self
            .sessions
            .read()
            .await
            .values()
            .find(|value| {
                value.coordinator_conversation_id == *conversation_id && !value.status.is_closed()
            })
            .cloned())
    }
    async fn update_session(&self, session: TeamSession, expected_version: i64) -> AppResult<bool> {
        let mut sessions = self.sessions.write().await;
        let Some(current) = sessions.get(&session.id) else {
            return Ok(false);
        };
        if current.version != expected_version {
            return Ok(false);
        };
        sessions.insert(session.id.clone(), session);
        Ok(true)
    }
    async fn create_member(&self, member: TeamMember) -> AppResult<TeamMember> {
        member.validate_name().map_err(AppError::Validation)?;
        let mut members = self.members.write().await;
        if members.values().any(|value| {
            value.team_id == member.team_id && value.normalized_name == member.normalized_name
        }) {
            return Err(AppError::Validation(
                "team member name already exists".to_string(),
            ));
        }
        members.insert(member.id.clone(), member.clone());
        Ok(member)
    }
    async fn get_member(&self, id: &TeamMemberId) -> AppResult<Option<TeamMember>> {
        Ok(self.members.read().await.get(id).cloned())
    }
    async fn list_members(&self, team_id: &TeamSessionId) -> AppResult<Vec<TeamMember>> {
        Ok(self
            .members
            .read()
            .await
            .values()
            .filter(|value| value.team_id == *team_id)
            .cloned()
            .collect())
    }
    async fn update_member(&self, member: TeamMember, expected_generation: i64) -> AppResult<bool> {
        member.validate_name().map_err(AppError::Validation)?;
        let mut members = self.members.write().await;
        let Some(current) = members.get(&member.id) else {
            return Ok(false);
        };
        if current.generation != expected_generation {
            return Ok(false);
        };
        members.insert(member.id.clone(), member);
        Ok(true)
    }
}
