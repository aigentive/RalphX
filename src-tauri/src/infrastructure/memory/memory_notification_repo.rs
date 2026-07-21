use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::domain::entities::{Notification, NotificationCategory};
use crate::domain::repositories::{NotificationPage, NotificationRepository};
use crate::error::AppResult;

pub struct MemoryNotificationRepository {
    notifications: RwLock<Vec<Notification>>,
}

impl MemoryNotificationRepository {
    pub fn new() -> Self {
        Self {
            notifications: RwLock::new(Vec::new()),
        }
    }
}
impl Default for MemoryNotificationRepository {
    fn default() -> Self {
        Self::new()
    }
}

fn is_retention_protected(row: &Notification) -> bool {
    row.category == NotificationCategory::PlanApproval && row.read_at.is_none()
}

#[async_trait]
impl NotificationRepository for MemoryNotificationRepository {
    async fn create_with_dedupe(&self, notification: Notification) -> AppResult<bool> {
        let mut notifications = self.notifications.write().await;
        if notification.dedupe_key.as_ref().is_some_and(|key| {
            notifications
                .iter()
                .any(|row| row.dedupe_key.as_ref() == Some(key))
        }) {
            return Ok(false);
        }
        notifications.push(notification);
        Ok(true)
    }
    async fn list(
        &self,
        project_id: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> AppResult<NotificationPage> {
        let mut rows: Vec<_> = self
            .notifications
            .read()
            .await
            .iter()
            .filter(|row| project_id.is_none_or(|id| row.project_id.as_deref() == Some(id)))
            .cloned()
            .collect();
        rows.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        if let Some((created_at, id)) = cursor.and_then(|value| value.split_once('|')) {
            rows.retain(|row| {
                row.created_at.to_rfc3339().as_str() < created_at
                    || (row.created_at.to_rfc3339() == created_at && row.id.as_str() < id)
            });
        }
        let limit = limit.clamp(1, 100) as usize;
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let cursor = has_more
            .then(|| {
                rows.last()
                    .map(|row| format!("{}|{}", row.created_at.to_rfc3339(), row.id))
            })
            .flatten();
        Ok(NotificationPage {
            notifications: rows,
            cursor,
            has_more,
        })
    }
    async fn unread_count(&self, project_id: Option<&str>) -> AppResult<u64> {
        Ok(self
            .notifications
            .read()
            .await
            .iter()
            .filter(|row| {
                row.read_at.is_none()
                    && project_id.is_none_or(|id| row.project_id.as_deref() == Some(id))
            })
            .count() as u64)
    }
    async fn mark_read(&self, id: &str, read_at: DateTime<Utc>) -> AppResult<Option<Notification>> {
        let mut rows = self.notifications.write().await;
        Ok(rows
            .iter_mut()
            .find(|row| row.id == id && row.read_at.is_none())
            .map(|row| {
                row.read_at = Some(read_at);
                row.clone()
            }))
    }
    async fn mark_read_by_dedupe_key(
        &self,
        dedupe_key: &str,
        read_at: DateTime<Utc>,
    ) -> AppResult<Option<Notification>> {
        let mut rows = self.notifications.write().await;
        Ok(rows
            .iter_mut()
            .find(|row| row.dedupe_key.as_deref() == Some(dedupe_key) && row.read_at.is_none())
            .map(|row| {
                row.read_at = Some(read_at);
                row.clone()
            }))
    }
    async fn mark_all_read(
        &self,
        project_id: Option<&str>,
        read_at: DateTime<Utc>,
    ) -> AppResult<u64> {
        let mut count = 0;
        for row in self.notifications.write().await.iter_mut().filter(|row| {
            row.read_at.is_none()
                && project_id.is_none_or(|id| row.project_id.as_deref() == Some(id))
        }) {
            row.read_at = Some(read_at);
            count += 1;
        }
        Ok(count)
    }
    async fn prune(&self, read_before: DateTime<Utc>, max_rows: u32) -> AppResult<()> {
        let mut rows = self.notifications.write().await;
        rows.retain(|row| {
            is_retention_protected(row) || row.read_at.is_none_or(|read_at| read_at >= read_before)
        });
        rows.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        let mut unprotected_retained = 0usize;
        rows.retain(|row| {
            if is_retention_protected(row) {
                return true;
            }
            if unprotected_retained >= max_rows as usize {
                return false;
            }
            unprotected_retained += 1;
            true
        });
        Ok(())
    }
}
