use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{ProjectId, RemoteExecutionResumeRequest, RemoteTaskActionRequest, TaskId};
use crate::error::AppResult;

#[async_trait]
pub trait RemoteExecutionResumeRequestRepository: Send + Sync {
    async fn create_execution_resume_request(
        &self,
        request: RemoteExecutionResumeRequest,
    ) -> AppResult<RemoteExecutionResumeRequest>;
    async fn get(&self, id: &str) -> AppResult<Option<RemoteExecutionResumeRequest>>;
    async fn find_unsettled(
        &self,
        project_id: Option<&ProjectId>,
    ) -> AppResult<Option<RemoteExecutionResumeRequest>>;
    async fn claim_pending(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteExecutionResumeRequest>>;
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

#[async_trait]
pub trait RemoteTaskActionRequestRepository: Send + Sync {
    async fn create_task_action_request(
        &self,
        request: RemoteTaskActionRequest,
    ) -> AppResult<RemoteTaskActionRequest>;
    async fn get(&self, id: &str) -> AppResult<Option<RemoteTaskActionRequest>>;
    async fn find_unsettled_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<RemoteTaskActionRequest>>;
    async fn find_unsettled_for_group(
        &self,
        project_id: &ProjectId,
        group_kind: &str,
        group_id: &str,
    ) -> AppResult<Option<RemoteTaskActionRequest>>;
    async fn claim_pending(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteTaskActionRequest>>;
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
