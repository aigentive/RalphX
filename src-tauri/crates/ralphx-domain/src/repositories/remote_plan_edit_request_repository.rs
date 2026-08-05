use crate::{entities::RemotePlanEditRequest, error::AppResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait RemotePlanEditRequestRepository: Send + Sync {
    async fn create_remote_plan_edit_request(
        &self,
        request: RemotePlanEditRequest,
    ) -> AppResult<RemotePlanEditRequest>;
    async fn get(&self, id: &str) -> AppResult<Option<RemotePlanEditRequest>>;
    async fn find_unsettled_for_artifact(
        &self,
        artifact_id: &str,
    ) -> AppResult<Option<RemotePlanEditRequest>>;
    async fn claim_pending(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemotePlanEditRequest>>;
    async fn complete(
        &self,
        id: &str,
        result: serde_json::Value,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;
    async fn fail(&self, id: &str, error_code: &str, updated_at: DateTime<Utc>) -> AppResult<()>;
    async fn fail_stale(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;
}
