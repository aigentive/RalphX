use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::Notification;
use crate::error::AppResult;

/// Stable newest-first notification-log page. `cursor` is `created_at|id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPage {
    pub notifications: Vec<Notification>,
    pub cursor: Option<String>,
    pub has_more: bool,
}

/// Durable notification persistence.
///
/// SQLite implementations skip malformed individual rows while logging them, so a corrupted
/// historic row cannot make the notification center unusable. All other repository failures are
/// returned to callers.
#[async_trait]
pub trait NotificationRepository: Send + Sync {
    /// Inserts a row unless its non-null dedupe key already exists.
    async fn create_with_dedupe(&self, notification: Notification) -> AppResult<bool>;
    async fn list(
        &self,
        project_id: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> AppResult<NotificationPage>;
    async fn unread_count(&self, project_id: Option<&str>) -> AppResult<u64>;
    /// Marks a row read once and returns the changed row, if any.
    async fn mark_read(&self, id: &str, read_at: DateTime<Utc>) -> AppResult<Option<Notification>>;
    /// Marks the exact workflow-correlated row read once and returns the changed row, if any.
    async fn mark_read_by_dedupe_key(
        &self,
        dedupe_key: &str,
        read_at: DateTime<Utc>,
    ) -> AppResult<Option<Notification>>;
    /// Marks every unread row read and returns the number changed.
    async fn mark_all_read(
        &self,
        project_id: Option<&str>,
        read_at: DateTime<Utc>,
    ) -> AppResult<u64>;
    /// Removes old read rows, then enforces `max_rows` while retaining newest rows.
    async fn prune(&self, read_before: DateTime<Utc>, max_rows: u32) -> AppResult<()>;
}
