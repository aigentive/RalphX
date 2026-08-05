use crate::{
    entities::{RemoteAutomationRunKind, RemoteAutomationRunRequest},
    error::AppResult,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait RemoteAutomationRunRequestRepository: Send + Sync {
    async fn create_remote_automation_run_request(
        &self,
        request: RemoteAutomationRunRequest,
    ) -> AppResult<RemoteAutomationRunRequest>;
    async fn get(&self, id: &str) -> AppResult<Option<RemoteAutomationRunRequest>>;
    async fn find_unsettled(
        &self,
        automation_id: &str,
        kind: RemoteAutomationRunKind,
    ) -> AppResult<Option<RemoteAutomationRunRequest>>;
    async fn claim_pending(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteAutomationRunRequest>>;
    async fn complete(
        &self,
        id: &str,
        result: serde_json::Value,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;
    async fn fail(
        &self,
        id: &str,
        error_code: &str,
        result: Option<serde_json::Value>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()>;
    async fn fail_stale(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64>;
}
