use crate::domain::{
    entities::{RemoteConversationLifecycleRequest, RemoteConversationLifecycleStatus},
    repositories::RemoteConversationLifecycleRequestRepository,
};
use crate::error::AppResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct MemoryRemoteConversationLifecycleRequestRepository {
    requests: Mutex<Vec<RemoteConversationLifecycleRequest>>,
}
#[async_trait]
impl RemoteConversationLifecycleRequestRepository
    for MemoryRemoteConversationLifecycleRequestRepository
{
    async fn create_remote_conversation_lifecycle_request(
        &self,
        row: RemoteConversationLifecycleRequest,
    ) -> AppResult<RemoteConversationLifecycleRequest> {
        self.requests.lock().await.push(row.clone());
        Ok(row)
    }
    async fn get(&self, id: &str) -> AppResult<Option<RemoteConversationLifecycleRequest>> {
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
        c: &str,
    ) -> AppResult<Option<RemoteConversationLifecycleRequest>> {
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .find(|r| r.conversation_id == c && !r.status.is_settled())
            .cloned())
    }
    async fn claim_pending(
        &self,
        at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationLifecycleRequest>> {
        let mut rows = self.requests.lock().await;
        let Some(r) = rows
            .iter_mut()
            .find(|r| r.status == RemoteConversationLifecycleStatus::Pending)
        else {
            return Ok(None);
        };
        r.status = RemoteConversationLifecycleStatus::Starting;
        r.claimed_at = Some(at);
        r.updated_at = at;
        Ok(Some(r.clone()))
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
            RemoteConversationLifecycleStatus::Completed,
            None,
            Some(result),
            at,
        );
        Ok(())
    }
    async fn fail(&self, id: &str, code: &str, at: DateTime<Utc>) -> AppResult<()> {
        settle(
            &mut self.requests.lock().await,
            id,
            RemoteConversationLifecycleStatus::Failed,
            Some(code.into()),
            None,
            at,
        );
        Ok(())
    }
    async fn fail_stale(&self, before: DateTime<Utc>, at: DateTime<Utc>) -> AppResult<u64> {
        let mut rows = self.requests.lock().await;
        let mut n = 0;
        for r in rows.iter_mut() {
            if r.status == RemoteConversationLifecycleStatus::Starting
                && r.claimed_at.is_some_and(|v| v < before)
            {
                r.status = RemoteConversationLifecycleStatus::FailedStale;
                r.updated_at = at;
                n += 1
            }
        }
        Ok(n)
    }
}
fn settle(
    rows: &mut [RemoteConversationLifecycleRequest],
    id: &str,
    status: RemoteConversationLifecycleStatus,
    error: Option<String>,
    result: Option<serde_json::Value>,
    at: DateTime<Utc>,
) {
    if let Some(r) = rows
        .iter_mut()
        .find(|r| r.id == id && r.status == RemoteConversationLifecycleStatus::Starting)
    {
        r.status = status;
        r.error_code = error;
        r.result = result;
        r.updated_at = at
    }
}
