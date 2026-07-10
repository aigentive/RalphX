use chrono::{Duration, Utc};

use super::SqliteNotificationRepository;
use crate::domain::entities::{
    NewNotification, NotificationCategory, NotificationSeverity, NotificationTarget,
};
use crate::domain::repositories::NotificationRepository;
use crate::testing::SqliteTestDb;

fn notification(
    key: &str,
    created_at: chrono::DateTime<Utc>,
) -> crate::domain::entities::Notification {
    NewNotification {
        project_id: Some("project-a".into()),
        category: NotificationCategory::TaskFailed,
        severity: NotificationSeverity::Warning,
        title: key.into(),
        body: None,
        target: NotificationTarget::none(),
        dedupe_key: Some(key.into()),
    }
    .into_notification(created_at)
}

#[tokio::test]
async fn sqlite_notification_repo_dedupes_and_prunes_with_shared_fixture() {
    let db = SqliteTestDb::new("sqlite-notification-repo");
    let repo = SqliteNotificationRepository::from_shared(db.shared_conn());
    let now = Utc::now();
    let old = notification("old", now - Duration::days(40));
    assert!(repo.create_with_dedupe(old.clone()).await.unwrap());
    assert!(!repo.create_with_dedupe(old.clone()).await.unwrap());
    assert!(repo
        .mark_read(&old.id, now - Duration::days(35))
        .await
        .unwrap()
        .is_some());
    let newest = notification("newest", now);
    assert!(repo.create_with_dedupe(newest.clone()).await.unwrap());
    repo.prune(now - Duration::days(30), 1).await.unwrap();
    let page = repo.list(None, None, 50).await.unwrap();
    assert_eq!(page.notifications.len(), 1);
    assert_eq!(page.notifications[0].id, newest.id);
}
