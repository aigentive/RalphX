use crate::{
    entities::{TeamWakeBatch, TeamWakeBatchId, TeamWakeBatchStatus},
    error::AppResult,
};
use async_trait::async_trait;
#[async_trait]
pub trait TeamWakeBatchRepository: Send + Sync {
    async fn create_or_extend_active(&self, batch: TeamWakeBatch) -> AppResult<TeamWakeBatch>;
    async fn get_by_id(&self, id: &TeamWakeBatchId) -> AppResult<Option<TeamWakeBatch>>;
    /// Lists queued batches in deterministic creation order for the Team-owned
    /// wake dispatcher. Claiming remains a compare-and-swap transition.
    async fn list_queued_for_team(
        &self,
        team_id: &crate::entities::TeamSessionId,
        limit: u32,
    ) -> AppResult<Vec<TeamWakeBatch>>;
    async fn transition(
        &self,
        id: &TeamWakeBatchId,
        expected_version: i64,
        expected: TeamWakeBatchStatus,
        batch: TeamWakeBatch,
    ) -> AppResult<bool>;
}
