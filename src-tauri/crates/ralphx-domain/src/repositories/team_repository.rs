use crate::{
    entities::{ChatConversationId, TeamMember, TeamMemberId, TeamSession, TeamSessionId},
    error::AppResult,
};
use async_trait::async_trait;

/// Durable Team session and roster contract. Uniqueness of one open Team and member names is enforced by implementations.
#[async_trait]
pub trait TeamRepository: Send + Sync {
    async fn ensure_session(&self, session: TeamSession) -> AppResult<TeamSession>;
    async fn get_session(&self, id: &TeamSessionId) -> AppResult<Option<TeamSession>>;
    async fn get_open_session_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<TeamSession>>;
    async fn update_session(&self, session: TeamSession, expected_version: i64) -> AppResult<bool>;
    async fn create_member(&self, member: TeamMember) -> AppResult<TeamMember>;
    async fn get_member(&self, id: &TeamMemberId) -> AppResult<Option<TeamMember>>;
    async fn list_members(&self, team_id: &TeamSessionId) -> AppResult<Vec<TeamMember>>;
    async fn update_member(&self, member: TeamMember, expected_generation: i64) -> AppResult<bool>;
}
