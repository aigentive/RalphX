use crate::domain::entities::{
    IdeationSessionId, RemoteFinalizeDecisionRequest, RemoteFinalizeDecisionRequestStatus,
};
use crate::domain::repositories::RemoteFinalizeDecisionRequestRepository;
use crate::error::AppResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct MemoryRemoteFinalizeDecisionRequestRepository {
    requests: Mutex<Vec<RemoteFinalizeDecisionRequest>>,
}

#[async_trait]
impl RemoteFinalizeDecisionRequestRepository for MemoryRemoteFinalizeDecisionRequestRepository {
    async fn create_remote_finalize_decision_request(
        &self,
        request: RemoteFinalizeDecisionRequest,
    ) -> AppResult<RemoteFinalizeDecisionRequest> {
        self.requests.lock().await.push(request.clone());
        Ok(request)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemoteFinalizeDecisionRequest>> {
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
    ) -> AppResult<Option<RemoteFinalizeDecisionRequest>> {
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
    ) -> AppResult<Option<RemoteFinalizeDecisionRequest>> {
        let mut rows = self.requests.lock().await;
        let Some(row) = rows
            .iter_mut()
            .find(|row| row.status == RemoteFinalizeDecisionRequestStatus::Pending)
        else {
            return Ok(None);
        };
        row.status = RemoteFinalizeDecisionRequestStatus::Starting;
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
            RemoteFinalizeDecisionRequestStatus::Completed,
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
            RemoteFinalizeDecisionRequestStatus::Failed,
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
            if row.status == RemoteFinalizeDecisionRequestStatus::Starting
                && row.claimed_at.is_some_and(|v| v < before)
            {
                row.status = RemoteFinalizeDecisionRequestStatus::FailedStale;
                row.updated_at = at;
                count += 1;
            }
        }
        Ok(count)
    }
}
fn settle(
    rows: &mut [RemoteFinalizeDecisionRequest],
    id: &str,
    status: RemoteFinalizeDecisionRequestStatus,
    result: Option<serde_json::Value>,
    error_code: Option<String>,
    at: DateTime<Utc>,
) {
    if let Some(row) = rows
        .iter_mut()
        .find(|row| row.id == id && row.status == RemoteFinalizeDecisionRequestStatus::Starting)
    {
        row.status = status;
        row.result = result;
        row.error_code = error_code;
        row.updated_at = at;
    }
}
