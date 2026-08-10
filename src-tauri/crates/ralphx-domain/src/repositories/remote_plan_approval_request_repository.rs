use crate::entities::{IdeationSessionId, RemotePlanApprovalRequest};
use crate::error::AppResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait RemotePlanApprovalRequestRepository: Send + Sync {
    async fn create_remote_plan_approval_request(
        &self,
        request: RemotePlanApprovalRequest,
    ) -> AppResult<RemotePlanApprovalRequest>;
    async fn get(&self, id: &str) -> AppResult<Option<RemotePlanApprovalRequest>>;
    async fn find_unsettled_for_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<RemotePlanApprovalRequest>>;
    async fn claim_pending(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemotePlanApprovalRequest>>;
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
