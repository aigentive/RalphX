use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{Duration, Utc};

use super::notification_service::{NotificationEventEmitter, NotificationService};
use crate::domain::entities::{
    NewNotification, Notification, NotificationCategory, NotificationSeverity, NotificationTarget,
};
use crate::domain::repositories::{NotificationPage, NotificationRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryNotificationRepository;

#[derive(Default)]
struct RecordingEmitter(Mutex<Vec<String>>);
impl NotificationEventEmitter for RecordingEmitter {
    fn emit_created(&self, notification: &Notification) -> AppResult<()> {
        self.0.lock().unwrap().push(notification.id.clone());
        Ok(())
    }
    fn emit_updated(&self, _notification: Option<&Notification>) -> AppResult<()> {
        Ok(())
    }
}

struct FailingNotificationRepository;
#[async_trait]
impl NotificationRepository for FailingNotificationRepository {
    async fn create_with_dedupe(&self, _notification: Notification) -> AppResult<bool> {
        Err(AppError::Database("injected failure".into()))
    }
    async fn list(
        &self,
        _project_id: Option<&str>,
        _cursor: Option<&str>,
        _limit: u32,
    ) -> AppResult<NotificationPage> {
        Err(AppError::Database("injected failure".into()))
    }
    async fn unread_count(&self, _project_id: Option<&str>) -> AppResult<u64> {
        Err(AppError::Database("injected failure".into()))
    }
    async fn mark_read(
        &self,
        _id: &str,
        _read_at: chrono::DateTime<Utc>,
    ) -> AppResult<Option<Notification>> {
        Err(AppError::Database("injected failure".into()))
    }
    async fn mark_all_read(
        &self,
        _project_id: Option<&str>,
        _read_at: chrono::DateTime<Utc>,
    ) -> AppResult<u64> {
        Err(AppError::Database("injected failure".into()))
    }
    async fn prune(&self, _read_before: chrono::DateTime<Utc>, _max_rows: u32) -> AppResult<()> {
        Err(AppError::Database("injected failure".into()))
    }
}

fn new_notification(key: Option<&str>) -> NewNotification {
    NewNotification {
        project_id: Some("project-1".into()),
        category: NotificationCategory::ReviewNeeded,
        severity: NotificationSeverity::ActionRequired,
        title: "Review needed".into(),
        body: None,
        target: NotificationTarget::none(),
        dedupe_key: key.map(str::to_owned),
    }
}

#[tokio::test]
async fn record_deduplicates_and_emits_only_the_inserted_row() {
    let repo: Arc<dyn NotificationRepository> = Arc::new(MemoryNotificationRepository::new());
    let emitter = Arc::new(RecordingEmitter::default());
    let service = NotificationService::new(Arc::clone(&repo), emitter.clone());
    service
        .record(new_notification(Some("task:1:review:history-1")))
        .await;
    service
        .record(new_notification(Some("task:1:review:history-1")))
        .await;
    assert_eq!(
        repo.list(None, None, 50).await.unwrap().notifications.len(),
        1
    );
    assert_eq!(emitter.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn record_swallows_repository_failures_without_emitting() {
    let emitter = Arc::new(RecordingEmitter::default());
    let service =
        NotificationService::new(Arc::new(FailingNotificationRepository), emitter.clone());
    service.record(new_notification(None)).await;
    assert!(emitter.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn read_transitions_and_prune_preserve_unread_rows_and_keep_newest() {
    let repo: Arc<dyn NotificationRepository> = Arc::new(MemoryNotificationRepository::new());
    let now = Utc::now();
    let old_read = new_notification(Some("old-read")).into_notification(now - Duration::days(40));
    let old_unread =
        new_notification(Some("old-unread")).into_notification(now - Duration::days(40));
    let newest = new_notification(Some("newest")).into_notification(now);
    repo.create_with_dedupe(old_read.clone()).await.unwrap();
    repo.create_with_dedupe(old_unread.clone()).await.unwrap();
    repo.create_with_dedupe(newest.clone()).await.unwrap();
    assert!(repo
        .mark_read(&old_read.id, now - Duration::days(35))
        .await
        .unwrap()
        .is_some());
    assert!(repo.mark_read(&old_read.id, now).await.unwrap().is_none());
    assert_eq!(repo.unread_count(None).await.unwrap(), 2);
    repo.prune(now - Duration::days(30), 100).await.unwrap();
    let retained = repo.list(None, None, 50).await.unwrap();
    assert!(retained
        .notifications
        .iter()
        .any(|row| row.id == old_unread.id));
    assert_eq!(repo.mark_all_read(Some("project-1"), now).await.unwrap(), 2);
    assert_eq!(repo.unread_count(None).await.unwrap(), 0);
    repo.prune(now - Duration::days(30), 1).await.unwrap();
    let page = repo.list(None, None, 50).await.unwrap();
    assert_eq!(page.notifications.len(), 1);
    assert_eq!(page.notifications[0].id, newest.id);
}
