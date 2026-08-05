use crate::domain::{
    entities::{
        RemoteAutomationRunKind, RemoteAutomationRunRequest, RemoteAutomationRunRequestStatus,
    },
    repositories::RemoteAutomationRunRequestRepository,
};
use crate::error::AppResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct MemoryRemoteAutomationRunRequestRepository {
    requests: Mutex<Vec<RemoteAutomationRunRequest>>,
}

#[async_trait]
impl RemoteAutomationRunRequestRepository for MemoryRemoteAutomationRunRequestRepository {
    async fn create_remote_automation_run_request(
        &self,
        request: RemoteAutomationRunRequest,
    ) -> AppResult<RemoteAutomationRunRequest> {
        self.requests.lock().await.push(request.clone());
        Ok(request)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemoteAutomationRunRequest>> {
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
        automation_id: &str,
        kind: RemoteAutomationRunKind,
    ) -> AppResult<Option<RemoteAutomationRunRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|row| {
                !row.status.is_settled() && row.automation_id == automation_id && row.kind == kind
            })
            .cloned())
    }
    async fn claim_pending(
        &self,
        at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteAutomationRunRequest>> {
        let mut rows = self.requests.lock().await;
        let Some(row) = rows
            .iter_mut()
            .find(|row| row.status == RemoteAutomationRunRequestStatus::Pending)
        else {
            return Ok(None);
        };
        row.status = RemoteAutomationRunRequestStatus::Starting;
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
            RemoteAutomationRunRequestStatus::Completed,
            Some(result),
            None,
            at,
        );
        Ok(())
    }
    async fn fail(
        &self,
        id: &str,
        code: &str,
        result: Option<serde_json::Value>,
        at: DateTime<Utc>,
    ) -> AppResult<()> {
        settle(
            &mut self.requests.lock().await,
            id,
            RemoteAutomationRunRequestStatus::Failed,
            result,
            Some(code.to_string()),
            at,
        );
        Ok(())
    }
    async fn fail_stale(&self, before: DateTime<Utc>, at: DateTime<Utc>) -> AppResult<u64> {
        let mut rows = self.requests.lock().await;
        let mut count = 0;
        for row in rows.iter_mut() {
            if row.status == RemoteAutomationRunRequestStatus::Starting
                && row.claimed_at.is_some_and(|v| v < before)
            {
                row.status = RemoteAutomationRunRequestStatus::FailedStale;
                row.updated_at = at;
                count += 1;
            }
        }
        Ok(count)
    }
}
fn settle(
    rows: &mut [RemoteAutomationRunRequest],
    id: &str,
    status: RemoteAutomationRunRequestStatus,
    result: Option<serde_json::Value>,
    error_code: Option<String>,
    at: DateTime<Utc>,
) {
    if let Some(row) = rows
        .iter_mut()
        .find(|row| row.id == id && row.status == RemoteAutomationRunRequestStatus::Starting)
    {
        row.status = status;
        row.result = result;
        row.error_code = error_code;
        row.updated_at = at;
    }
}
