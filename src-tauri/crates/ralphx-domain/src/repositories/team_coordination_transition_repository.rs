use crate::{
    entities::{ChatConversationId, CoordinationMode, TeamSession, TeamSessionId},
    error::AppResult,
};
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct TeamExitMarker {
    pub coordination_mode: CoordinationMode,
    pub exit_action: String,
}
/// Atomic conversation-mode and Team lifecycle CAS operations; reads retain backend errors.
#[async_trait]
pub trait TeamCoordinationTransitionRepository: Send + Sync {
    async fn enter_team(
        &self,
        conversation_id: &ChatConversationId,
        session: TeamSession,
    ) -> AppResult<TeamSession>;
    async fn mark_pending_exit(
        &self,
        team_id: &TeamSessionId,
        expected_version: i64,
        marker: TeamExitMarker,
    ) -> AppResult<bool>;
    async fn commit_exit(
        &self,
        conversation_id: &ChatConversationId,
        team_id: &TeamSessionId,
        expected_version: i64,
    ) -> AppResult<bool>;
}
