use crate::domain::entities::{
    IdeationSessionId, RemotePlanApprovalRequest, RemotePlanApprovalRequestStatus,
};
use crate::domain::repositories::RemotePlanApprovalRequestRepository;
use crate::error::AppResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct MemoryRemotePlanApprovalRequestRepository {
    requests: Mutex<Vec<RemotePlanApprovalRequest>>,
}

#[async_trait]
impl RemotePlanApprovalRequestRepository for MemoryRemotePlanApprovalRequestRepository {
    async fn create_remote_plan_approval_request(
        &self,
        request: RemotePlanApprovalRequest,
    ) -> AppResult<RemotePlanApprovalRequest> {
        self.requests.lock().await.push(request.clone());
        Ok(request)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemotePlanApprovalRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|row| row.id == id)
            .cloned())
    }
    async fn find_unsettled_for_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<RemotePlanApprovalRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|row| !row.status.is_settled() && &row.session_id == session_id)
            .cloned())
    }
    async fn claim_pending(
        &self,
        at: DateTime<Utc>,
    ) -> AppResult<Option<RemotePlanApprovalRequest>> {
        let mut rows = self.requests.lock().await;
        let Some(row) = rows
            .iter_mut()
            .find(|row| row.status == RemotePlanApprovalRequestStatus::Pending)
        else {
            return Ok(None);
        };
        row.status = RemotePlanApprovalRequestStatus::Starting;
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
            RemotePlanApprovalRequestStatus::Completed,
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
            RemotePlanApprovalRequestStatus::Failed,
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
            if row.status == RemotePlanApprovalRequestStatus::Starting
                && row.claimed_at.is_some_and(|v| v < before)
            {
                row.status = RemotePlanApprovalRequestStatus::FailedStale;
                row.updated_at = at;
                count += 1;
            }
        }
        Ok(count)
    }
}
fn settle(
    rows: &mut [RemotePlanApprovalRequest],
    id: &str,
    status: RemotePlanApprovalRequestStatus,
    result: Option<serde_json::Value>,
    error_code: Option<String>,
    at: DateTime<Utc>,
) {
    if let Some(row) = rows
        .iter_mut()
        .find(|row| row.id == id && row.status == RemotePlanApprovalRequestStatus::Starting)
    {
        row.status = status;
        row.result = result;
        row.error_code = error_code;
        row.updated_at = at;
    }
}
