use crate::{
    entities::{
        AgentRunId, TeamMemberId, TeamRunBinding, TeamRunBindingId, TeamRunBindingStatus,
        TeamSessionId,
    },
    error::AppResult,
};
use async_trait::async_trait;
#[async_trait]
pub trait TeamRunBindingRepository: Send + Sync {
    async fn create(&self, binding: TeamRunBinding) -> AppResult<TeamRunBinding>;
    async fn get_by_id(&self, id: &TeamRunBindingId) -> AppResult<Option<TeamRunBinding>>;
    async fn get_by_agent_run_id(
        &self,
        agent_run_id: &AgentRunId,
    ) -> AppResult<Option<TeamRunBinding>>;
    async fn list_for_team(&self, team_id: &TeamSessionId) -> AppResult<Vec<TeamRunBinding>>;
    async fn get_current_member_binding(
        &self,
        member_id: &TeamMemberId,
        generation: i64,
    ) -> AppResult<Option<TeamRunBinding>>;
    async fn transition(
        &self,
        id: &TeamRunBindingId,
        expected_version: i64,
        binding: TeamRunBinding,
    ) -> AppResult<bool>;

    async fn count_active_dispatches(&self, team_id: &TeamSessionId) -> AppResult<u32> {
        let count = self
            .list_for_team(team_id)
            .await?
            .into_iter()
            .filter(|b| {
                b.team_member_id.is_some()
                    && matches!(
                        b.status,
                        TeamRunBindingStatus::Launching | TeamRunBindingStatus::Running
                    )
            })
            .count();
        u32::try_from(count).map_err(|_| {
            crate::error::AppError::Conflict(
                "managed Team dispatch count overflowed".to_string(),
            )
        })
    }
}
