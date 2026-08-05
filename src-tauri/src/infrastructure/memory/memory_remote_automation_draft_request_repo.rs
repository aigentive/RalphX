use crate::domain::{
    entities::{RemoteAutomationDraftRequest, RemoteAutomationDraftRequestStatus},
    repositories::RemoteAutomationDraftRequestRepository,
};
use crate::error::AppResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct MemoryRemoteAutomationDraftRequestRepository {
    requests: Mutex<Vec<RemoteAutomationDraftRequest>>,
}

#[async_trait]
impl RemoteAutomationDraftRequestRepository for MemoryRemoteAutomationDraftRequestRepository {
    async fn create_remote_automation_draft_request(
        &self,
        request: RemoteAutomationDraftRequest,
    ) -> AppResult<RemoteAutomationDraftRequest> {
        self.requests.lock().await.push(request.clone());
        Ok(request)
    }

    async fn get(&self, id: &str) -> AppResult<Option<RemoteAutomationDraftRequest>> {
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
        project_id: &str,
        name: &str,
    ) -> AppResult<Option<RemoteAutomationDraftRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|row| {
                !row.status.is_settled() && row.project_id == project_id && row.name == name
            })
            .cloned())
    }

    async fn claim_pending(
        &self,
        at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteAutomationDraftRequest>> {
        let mut rows = self.requests.lock().await;
        let Some(row) = rows
            .iter_mut()
            .find(|row| row.status == RemoteAutomationDraftRequestStatus::Pending)
        else {
            return Ok(None);
        };
        row.status = RemoteAutomationDraftRequestStatus::Starting;
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
            RemoteAutomationDraftRequestStatus::Completed,
            Some(result),
            None,
            at,
        );
        Ok(())
    }

    async fn fail(&self, id: &str, code: &str, at: DateTime<Utc>) -> AppResult<()> {
        settle(
            &mut self.requests.lock().await,
            id,
            RemoteAutomationDraftRequestStatus::Failed,
            None,
            Some(code.to_string()),
            at,
        );
        Ok(())
    }

    async fn fail_stale(&self, before: DateTime<Utc>, at: DateTime<Utc>) -> AppResult<u64> {
        let mut rows = self.requests.lock().await;
        let mut count = 0;
        for row in rows.iter_mut() {
            if row.status == RemoteAutomationDraftRequestStatus::Starting
                && row.claimed_at.is_some_and(|value| value < before)
            {
                row.status = RemoteAutomationDraftRequestStatus::FailedStale;
                row.updated_at = at;
                count += 1;
            }
        }
        Ok(count)
    }
}

fn settle(
    rows: &mut [RemoteAutomationDraftRequest],
    id: &str,
    status: RemoteAutomationDraftRequestStatus,
    result: Option<serde_json::Value>,
    error_code: Option<String>,
    at: DateTime<Utc>,
) {
    if let Some(row) = rows
        .iter_mut()
        .find(|row| row.id == id && row.status == RemoteAutomationDraftRequestStatus::Starting)
    {
        row.status = status;
        row.result = result;
        row.error_code = error_code;
        row.updated_at = at;
    }
}
