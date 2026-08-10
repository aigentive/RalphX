use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::domain::entities::{
    ProjectId, RemoteExecutionResumeRequest, RemoteResumeRequestStatus, RemoteTaskActionRequest,
    TaskId,
};
use crate::domain::repositories::{
    RemoteExecutionResumeRequestRepository, RemoteTaskActionRequestRepository,
};
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryRemoteExecutionResumeRequestRepository {
    requests: Mutex<Vec<RemoteExecutionResumeRequest>>,
}

#[async_trait]
impl RemoteExecutionResumeRequestRepository for MemoryRemoteExecutionResumeRequestRepository {
    async fn create_execution_resume_request(
        &self,
        request: RemoteExecutionResumeRequest,
    ) -> AppResult<RemoteExecutionResumeRequest> {
        self.requests.lock().await.push(request.clone());
        Ok(request)
    }

    async fn get(&self, id: &str) -> AppResult<Option<RemoteExecutionResumeRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|row| row.id == id)
            .cloned())
    }

    async fn find_unsettled(
        &self,
        project_id: Option<&ProjectId>,
    ) -> AppResult<Option<RemoteExecutionResumeRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|row| !row.status.is_settled() && row.project_id.as_ref() == project_id)
            .cloned())
    }

    async fn claim_pending(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteExecutionResumeRequest>> {
        let mut rows = self.requests.lock().await;
        let Some(row) = rows
            .iter_mut()
            .find(|row| row.status == RemoteResumeRequestStatus::Pending)
        else {
            return Ok(None);
        };
        row.status = RemoteResumeRequestStatus::Starting;
        row.claimed_at = Some(claimed_at);
        row.updated_at = claimed_at;
        Ok(Some(row.clone()))
    }

    async fn complete(
        &self,
        id: &str,
        result: serde_json::Value,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        settle_execution(
            &mut self.requests.lock().await,
            id,
            RemoteResumeRequestStatus::Completed,
            Some(result),
            None,
            updated_at,
        );
        Ok(())
    }

    async fn fail(&self, id: &str, error_code: &str, updated_at: DateTime<Utc>) -> AppResult<()> {
        settle_execution(
            &mut self.requests.lock().await,
            id,
            RemoteResumeRequestStatus::Failed,
            None,
            Some(error_code.to_string()),
            updated_at,
        );
        Ok(())
    }

    async fn fail_stale(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut rows = self.requests.lock().await;
        let mut count = 0;
        for row in rows.iter_mut() {
            if row.status == RemoteResumeRequestStatus::Starting
                && row.claimed_at.is_some_and(|at| at < claimed_before)
            {
                row.status = RemoteResumeRequestStatus::FailedStale;
                row.updated_at = updated_at;
                count += 1;
            }
        }
        Ok(count)
    }
}

fn settle_execution(
    rows: &mut [RemoteExecutionResumeRequest],
    id: &str,
    status: RemoteResumeRequestStatus,
    result: Option<serde_json::Value>,
    error_code: Option<String>,
    updated_at: DateTime<Utc>,
) {
    if let Some(row) = rows
        .iter_mut()
        .find(|row| row.id == id && row.status == RemoteResumeRequestStatus::Starting)
    {
        row.status = status;
        row.result = result;
        row.error_code = error_code;
        row.updated_at = updated_at;
    }
}

#[derive(Default)]
pub struct MemoryRemoteTaskActionRequestRepository {
    requests: Mutex<Vec<RemoteTaskActionRequest>>,
}

#[async_trait]
impl RemoteTaskActionRequestRepository for MemoryRemoteTaskActionRequestRepository {
    async fn create_task_action_request(
        &self,
        request: RemoteTaskActionRequest,
    ) -> AppResult<RemoteTaskActionRequest> {
        self.requests.lock().await.push(request.clone());
        Ok(request)
    }

    async fn get(&self, id: &str) -> AppResult<Option<RemoteTaskActionRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|row| row.id == id)
            .cloned())
    }

    async fn find_unsettled_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<RemoteTaskActionRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|row| !row.status.is_settled() && row.task_id.as_ref() == Some(task_id))
            .cloned())
    }

    async fn find_unsettled_for_group(
        &self,
        project_id: &ProjectId,
        group_kind: &str,
        group_id: &str,
    ) -> AppResult<Option<RemoteTaskActionRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|row| {
                !row.status.is_settled()
                    && &row.project_id == project_id
                    && row.group_kind.as_deref() == Some(group_kind)
                    && row.group_id.as_deref() == Some(group_id)
            })
            .cloned())
    }

    async fn claim_pending(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteTaskActionRequest>> {
        let mut rows = self.requests.lock().await;
        let Some(row) = rows
            .iter_mut()
            .find(|row| row.status == RemoteResumeRequestStatus::Pending)
        else {
            return Ok(None);
        };
        row.status = RemoteResumeRequestStatus::Starting;
        row.claimed_at = Some(claimed_at);
        row.updated_at = claimed_at;
        Ok(Some(row.clone()))
    }

    async fn complete(
        &self,
        id: &str,
        result: serde_json::Value,
        updated_at: DateTime<Utc>,
    ) -> AppResult<()> {
        settle_task(
            &mut self.requests.lock().await,
            id,
            RemoteResumeRequestStatus::Completed,
            Some(result),
            None,
            updated_at,
        );
        Ok(())
    }

    async fn fail(&self, id: &str, error_code: &str, updated_at: DateTime<Utc>) -> AppResult<()> {
        settle_task(
            &mut self.requests.lock().await,
            id,
            RemoteResumeRequestStatus::Failed,
            None,
            Some(error_code.to_string()),
            updated_at,
        );
        Ok(())
    }

    async fn fail_stale(
        &self,
        claimed_before: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut rows = self.requests.lock().await;
        let mut count = 0;
        for row in rows.iter_mut() {
            if row.status == RemoteResumeRequestStatus::Starting
                && row.claimed_at.is_some_and(|at| at < claimed_before)
            {
                row.status = RemoteResumeRequestStatus::FailedStale;
                row.updated_at = updated_at;
                count += 1;
            }
        }
        Ok(count)
    }
}

fn settle_task(
    rows: &mut [RemoteTaskActionRequest],
    id: &str,
    status: RemoteResumeRequestStatus,
    result: Option<serde_json::Value>,
    error_code: Option<String>,
    updated_at: DateTime<Utc>,
) {
    if let Some(row) = rows
        .iter_mut()
        .find(|row| row.id == id && row.status == RemoteResumeRequestStatus::Starting)
    {
        row.status = status;
        row.result = result;
        row.error_code = error_code;
        row.updated_at = updated_at;
    }
}
