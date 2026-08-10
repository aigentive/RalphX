use crate::domain::{
    entities::{RemoteQueuedSendRequest, RemoteQueuedSendRequestStatus},
    repositories::RemoteQueuedSendRequestRepository,
};
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct MemoryRemoteQueuedSendRequestRepository {
    requests: Mutex<Vec<RemoteQueuedSendRequest>>,
}

#[async_trait]
impl RemoteQueuedSendRequestRepository for MemoryRemoteQueuedSendRequestRepository {
    async fn create_remote_queued_send_request(
        &self,
        request: RemoteQueuedSendRequest,
    ) -> AppResult<RemoteQueuedSendRequest> {
        self.requests.lock().await.push(request.clone());
        Ok(request)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemoteQueuedSendRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|r| r.id == id)
            .cloned())
    }
    async fn find_unsettled(
        &self,
        conversation_id: &str,
        queued_message_id: &str,
    ) -> AppResult<Option<RemoteQueuedSendRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|r| {
                r.conversation_id == conversation_id
                    && r.queued_message_id == queued_message_id
                    && !r.status.is_settled()
            })
            .cloned())
    }
    async fn claim_pending(&self, at: DateTime<Utc>) -> AppResult<Option<RemoteQueuedSendRequest>> {
        let mut rows = self.requests.lock().await;
        let Some(row) = rows
            .iter_mut()
            .find(|r| r.status == RemoteQueuedSendRequestStatus::Pending)
        else {
            return Ok(None);
        };
        row.status = RemoteQueuedSendRequestStatus::Starting;
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
            RemoteQueuedSendRequestStatus::Completed,
            None,
            Some(result),
            at,
        )
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
            RemoteQueuedSendRequestStatus::Failed,
            Some(code.into()),
            result,
            at,
        )
    }
    async fn fail_stale(&self, before: DateTime<Utc>, at: DateTime<Utc>) -> AppResult<u64> {
        let mut rows = self.requests.lock().await;
        let mut count = 0;
        for row in rows.iter_mut() {
            if row.status == RemoteQueuedSendRequestStatus::Starting
                && row.claimed_at.is_some_and(|v| v < before)
            {
                row.status = RemoteQueuedSendRequestStatus::FailedStale;
                row.updated_at = at;
                count += 1
            }
        }
        Ok(count)
    }
}
fn settle(
    rows: &mut [RemoteQueuedSendRequest],
    id: &str,
    status: RemoteQueuedSendRequestStatus,
    error: Option<String>,
    result: Option<serde_json::Value>,
    at: DateTime<Utc>,
) -> AppResult<()> {
    let row = rows
        .iter_mut()
        .find(|r| r.id == id && r.status == RemoteQueuedSendRequestStatus::Starting)
        .ok_or_else(|| AppError::Conflict("remote queued send request is not starting".into()))?;
    row.status = status;
    row.error_code = error;
    row.result = result;
    row.updated_at = at;
    Ok(())
}
