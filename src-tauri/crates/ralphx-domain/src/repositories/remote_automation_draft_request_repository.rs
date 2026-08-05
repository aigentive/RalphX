use crate::{entities::RemoteAutomationDraftRequest, error::AppResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait RemoteAutomationDraftRequestRepository: Send + Sync {
    async fn create_remote_automation_draft_request(
        &self,
        request: RemoteAutomationDraftRequest,
    ) -> AppResult<RemoteAutomationDraftRequest>;
    async fn get(&self, id: &str) -> AppResult<Option<RemoteAutomationDraftRequest>>;
    async fn find_unsettled(
        &self,
        project_id: &str,
        name: &str,
    ) -> AppResult<Option<RemoteAutomationDraftRequest>>;
    async fn claim_pending(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteAutomationDraftRequest>>;
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
