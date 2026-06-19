use async_trait::async_trait;

use crate::domain::services::{QueueKey, QueuedMessage};
use crate::error::AppResult;

/// Repository trait for durable queued agent messages.
///
/// The in-memory queue remains the live process buffer; this repository is the
/// restart-safe source for pending rows and queue UI hydration.
#[async_trait]
pub trait QueuedMessageRepository: Send + Sync {
    async fn enqueue_back(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()>;
    async fn enqueue_front(&self, key: &QueueKey, message: &QueuedMessage) -> AppResult<()>;
    async fn list(&self, key: &QueueKey) -> AppResult<Vec<QueuedMessage>>;
    async fn list_keys(&self) -> AppResult<Vec<QueueKey>>;
    async fn delete(&self, key: &QueueKey, message_id: &str) -> AppResult<bool>;
    async fn delete_by_id(&self, message_id: &str) -> AppResult<bool>;
    async fn clear(&self, key: &QueueKey) -> AppResult<()>;
    async fn pop_front(&self, key: &QueueKey) -> AppResult<Option<QueuedMessage>>;
    async fn remove_stale(
        &self,
        key: &QueueKey,
        threshold_secs: u64,
    ) -> AppResult<Vec<QueuedMessage>>;
}
