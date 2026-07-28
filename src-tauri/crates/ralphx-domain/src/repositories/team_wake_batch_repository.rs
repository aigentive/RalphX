use crate::{
    entities::{TeamWakeBatch, TeamWakeBatchId, TeamWakeBatchStatus},
    error::AppResult,
};
use async_trait::async_trait;
#[async_trait]
pub trait TeamWakeBatchRepository: Send + Sync {
    async fn create_or_extend_active(&self, batch: TeamWakeBatch) -> AppResult<TeamWakeBatch>;
    async fn get_by_id(&self, id: &TeamWakeBatchId) -> AppResult<Option<TeamWakeBatch>>;
    async fn transition(
        &self,
        id: &TeamWakeBatchId,
        expected_version: i64,
        expected: TeamWakeBatchStatus,
        batch: TeamWakeBatch,
    ) -> AppResult<bool>;
}
