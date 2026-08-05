use crate::{entities::RemoteConversationLifecycleRequest, error::AppResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
#[async_trait]
pub trait RemoteConversationLifecycleRequestRepository: Send + Sync {
    async fn create_remote_conversation_lifecycle_request(
        &self,
        row: RemoteConversationLifecycleRequest,
    ) -> AppResult<RemoteConversationLifecycleRequest>;
    async fn get(&self, id: &str) -> AppResult<Option<RemoteConversationLifecycleRequest>>;
    async fn find_unsettled(
        &self,
        conversation_id: &str,
    ) -> AppResult<Option<RemoteConversationLifecycleRequest>>;
    async fn claim_pending(
        &self,
        at: DateTime<Utc>,
    ) -> AppResult<Option<RemoteConversationLifecycleRequest>>;
    async fn complete(
        &self,
        id: &str,
        result: serde_json::Value,
        at: DateTime<Utc>,
    ) -> AppResult<()>;
    async fn fail(&self, id: &str, code: &str, at: DateTime<Utc>) -> AppResult<()>;
    async fn fail_stale(&self, before: DateTime<Utc>, at: DateTime<Utc>) -> AppResult<u64>;
}
