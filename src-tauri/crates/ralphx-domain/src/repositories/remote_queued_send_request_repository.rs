use crate::{entities::RemoteQueuedSendRequest, error::AppResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait RemoteQueuedSendRequestRepository: Send + Sync {
    async fn create_remote_queued_send_request(
        &self,
        request: RemoteQueuedSendRequest,
    ) -> AppResult<RemoteQueuedSendRequest>;
    async fn get(&self, id: &str) -> AppResult<Option<RemoteQueuedSendRequest>>;
    async fn find_unsettled(
        &self,
        conversation_id: &str,
        queued_message_id: &str,
    ) -> AppResult<Option<RemoteQueuedSendRequest>>;
    async fn claim_pending(
        &self,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteQueuedSendRequest>>;
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
