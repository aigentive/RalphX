use crate::domain::{
    entities::{RemotePlanEditRequest, RemotePlanEditRequestStatus},
    repositories::RemotePlanEditRequestRepository,
};
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct MemoryRemotePlanEditRequestRepository {
    requests: Mutex<Vec<RemotePlanEditRequest>>,
}

#[async_trait]
impl RemotePlanEditRequestRepository for MemoryRemotePlanEditRequestRepository {
    async fn create_remote_plan_edit_request(
        &self,
        request: RemotePlanEditRequest,
    ) -> AppResult<RemotePlanEditRequest> {
        self.requests.lock().await.push(request.clone());
        Ok(request)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemotePlanEditRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|r| r.id == id)
            .cloned())
    }
    async fn find_unsettled_for_artifact(
        &self,
        artifact_id: &str,
    ) -> AppResult<Option<RemotePlanEditRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|r| r.artifact_id == artifact_id && !r.status.is_settled())
            .cloned())
    }
    async fn claim_pending(&self, at: DateTime<Utc>) -> AppResult<Option<RemotePlanEditRequest>> {
        let mut rows = self.requests.lock().await;
        let Some(row) = rows
            .iter_mut()
            .find(|r| r.status == RemotePlanEditRequestStatus::Pending)
        else {
            return Ok(None);
        };
        row.status = RemotePlanEditRequestStatus::Starting;
        row.claimed_at = Some(at);
        row.updated_at = at;
        Ok(Some(row.clone()))
    }
    async fn complete(
        &self,
        id: &str,
        result: serde_json::Value,
        at: DateTime<Utc>,
    ) -> AppResult<()> {
        settle(
            &mut self.requests.lock().await,
            id,
            RemotePlanEditRequestStatus::Completed,
            None,
            Some(result),
            at,
        )
    }
    async fn fail(&self, id: &str, code: &str, at: DateTime<Utc>) -> AppResult<()> {
        settle(
            &mut self.requests.lock().await,
            id,
            RemotePlanEditRequestStatus::Failed,
            Some(code.to_string()),
            None,
            at,
        )
    }
    async fn fail_stale(&self, before: DateTime<Utc>, at: DateTime<Utc>) -> AppResult<u64> {
        let mut rows = self.requests.lock().await;
        let mut n = 0;
        for row in rows.iter_mut() {
            if row.status == RemotePlanEditRequestStatus::Starting
                && row.claimed_at.is_some_and(|v| v < before)
            {
                row.status = RemotePlanEditRequestStatus::FailedStale;
                row.updated_at = at;
                n += 1
            }
        }
        Ok(n)
    }
}
fn settle(
    rows: &mut [RemotePlanEditRequest],
    id: &str,
    status: RemotePlanEditRequestStatus,
    error: Option<String>,
    result: Option<serde_json::Value>,
    at: DateTime<Utc>,
) -> AppResult<()> {
    let row = rows
        .iter_mut()
        .find(|r| r.id == id && r.status == RemotePlanEditRequestStatus::Starting)
        .ok_or_else(|| AppError::Conflict("remote plan edit request is not starting".into()))?;
    row.status = status;
    row.error_code = error;
    row.result = result;
    row.updated_at = at;
    Ok(())
}
