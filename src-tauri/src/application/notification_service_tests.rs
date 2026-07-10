use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{Duration, Utc};

use super::notification_service::{
    DesktopNotifier, NotificationEventEmitter, NotificationService, WindowFocusState,
};
use crate::domain::entities::{
    NewNotification, Notification, NotificationCategory, NotificationSettings,
    NotificationSeverity, NotificationTarget,
};
use crate::domain::repositories::{
    NotificationPage, NotificationRepository, NotificationSettingsRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryNotificationRepository, MemoryNotificationSettingsRepository,
};

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

#[derive(Default)]
struct RecordingDesktopNotifier(Mutex<Vec<(String, Option<String>)>>);

impl DesktopNotifier for RecordingDesktopNotifier {
    fn send(&self, title: &str, body: Option<&str>) -> AppResult<()> {
        self.0
            .lock()
            .unwrap()
            .push((title.to_string(), body.map(str::to_string)));
        Ok(())
    }
}

struct FailingDesktopNotifier;

impl DesktopNotifier for FailingDesktopNotifier {
    fn send(&self, _title: &str, _body: Option<&str>) -> AppResult<()> {
        Err(AppError::Infrastructure("injected desktop failure".into()))
    }
}

async fn desktop_service(
    settings: NotificationSettings,
    focus_state: Arc<WindowFocusState>,
    notifier: Arc<dyn DesktopNotifier>,
    window: StdDuration,
) -> (
    NotificationService,
    Arc<MemoryNotificationSettingsRepository>,
    Arc<dyn NotificationRepository>,
) {
    let repo: Arc<dyn NotificationRepository> = Arc::new(MemoryNotificationRepository::new());
    let settings_repo = Arc::new(MemoryNotificationSettingsRepository::new());
    let settings_repo_dyn: Arc<dyn NotificationSettingsRepository> = settings_repo.clone();
    let emitter: Arc<dyn NotificationEventEmitter> = Arc::new(RecordingEmitter::default());
    let service = NotificationService::new_with_desktop_dispatch(
        Arc::clone(&repo),
        emitter,
        settings_repo_dyn,
        focus_state,
        notifier,
        window,
    );
    settings_repo.update_settings(&settings).await.unwrap();
    (service, settings_repo, repo)
}

async fn settle_desktop_dispatch() {
    tokio::time::sleep(StdDuration::from_millis(25)).await;
}

fn notification_for(
    category: NotificationCategory,
    severity: NotificationSeverity,
    key: Option<&str>,
) -> NewNotification {
    NewNotification {
        category,
        severity,
        ..new_notification(key)
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

#[test]
fn window_focus_state_starts_unfocused_until_native_event() {
    assert!(!WindowFocusState::default().is_focused());
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
async fn desktop_gate_matrix_honors_master_focus_category_and_all_severities() {
    for desktop_enabled in [false, true] {
        for focused in [false, true] {
            for category_enabled in [false, true] {
                for severity in [
                    NotificationSeverity::ActionRequired,
                    NotificationSeverity::Warning,
                    NotificationSeverity::Info,
                ] {
                    let mut settings = NotificationSettings {
                        desktop_enabled,
                        ..NotificationSettings::default()
                    };
                    settings.desktop_reviews_enabled = category_enabled;
                    let focus_state = Arc::new(WindowFocusState::default());
                    focus_state.set_focused(focused);
                    let notifier = Arc::new(RecordingDesktopNotifier::default());
                    let (service, _, _) = desktop_service(
                        settings,
                        focus_state,
                        notifier.clone(),
                        StdDuration::from_millis(1),
                    )
                    .await;

                    service
                        .record_ephemeral(notification_for(
                            NotificationCategory::ReviewNeeded,
                            severity,
                            None,
                        ))
                        .await;
                    settle_desktop_dispatch().await;

                    let expected = desktop_enabled && category_enabled && !focused;
                    assert_eq!(
                        notifier.0.lock().unwrap().len(),
                        usize::from(expected),
                        "desktop_enabled={desktop_enabled}, focused={focused}, category_enabled={category_enabled}, severity={severity:?}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn desktop_coalescer_sends_one_summary_for_three_items_with_group_counts() {
    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        Arc::new(WindowFocusState::default()),
        notifier.clone(),
        StdDuration::from_millis(10),
    )
    .await;
    for category in [
        NotificationCategory::ReviewNeeded,
        NotificationCategory::ReviewEscalated,
        NotificationCategory::PermissionRequest,
        NotificationCategory::MergeConflict,
    ] {
        service
            .record_ephemeral(notification_for(
                category,
                NotificationSeverity::ActionRequired,
                None,
            ))
            .await;
    }
    settle_desktop_dispatch().await;

    let sent = notifier.0.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "4 items need your attention");
    assert_eq!(
        sent[0].1.as_deref(),
        Some("2 reviews, 1 permission request, 1 merge conflict — project-1")
    );
}

#[tokio::test]
async fn desktop_coalescer_sends_individual_notifications_for_two_items_and_resets_after_expiry() {
    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        Arc::new(WindowFocusState::default()),
        notifier.clone(),
        StdDuration::from_millis(10),
    )
    .await;
    for title in ["one", "two"] {
        let mut notification = notification_for(
            NotificationCategory::ReviewNeeded,
            NotificationSeverity::ActionRequired,
            None,
        );
        notification.title = title.to_string();
        service.record_ephemeral(notification).await;
    }
    settle_desktop_dispatch().await;
    assert_eq!(notifier.0.lock().unwrap().len(), 2);

    for _ in 0..3 {
        service
            .record_ephemeral(notification_for(
                NotificationCategory::ReviewNeeded,
                NotificationSeverity::ActionRequired,
                None,
            ))
            .await;
    }
    settle_desktop_dispatch().await;
    assert_eq!(notifier.0.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn duplicate_record_dispatches_one_desktop_ping() {
    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        Arc::new(WindowFocusState::default()),
        notifier.clone(),
        StdDuration::from_millis(1),
    )
    .await;
    let notification = notification_for(
        NotificationCategory::ReviewNeeded,
        NotificationSeverity::ActionRequired,
        Some("dedupe-desktop-ping"),
    );
    service.record(notification.clone()).await;
    service.record(notification).await;
    settle_desktop_dispatch().await;
    assert_eq!(notifier.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn agent_waiting_respects_unfocused_and_focused_desktop_gates() {
    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let focus_state = Arc::new(WindowFocusState::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        focus_state.clone(),
        notifier.clone(),
        StdDuration::from_millis(1),
    )
    .await;
    service
        .record_ephemeral(notification_for(
            NotificationCategory::AgentWaiting,
            NotificationSeverity::Info,
            None,
        ))
        .await;
    settle_desktop_dispatch().await;
    focus_state.set_focused(true);
    service
        .record_ephemeral(notification_for(
            NotificationCategory::AgentWaiting,
            NotificationSeverity::Info,
            None,
        ))
        .await;
    settle_desktop_dispatch().await;
    assert_eq!(notifier.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn focus_state_transitions_control_desktop_delivery_mid_sequence() {
    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let focus_state = Arc::new(WindowFocusState::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        focus_state.clone(),
        notifier.clone(),
        StdDuration::from_millis(1),
    )
    .await;
    service.record_ephemeral(new_notification(None)).await;
    settle_desktop_dispatch().await;
    focus_state.set_focused(true);
    service.record_ephemeral(new_notification(None)).await;
    settle_desktop_dispatch().await;
    focus_state.set_focused(false);
    service.record_ephemeral(new_notification(None)).await;
    settle_desktop_dispatch().await;
    assert_eq!(notifier.0.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn desktop_notifier_failure_does_not_prevent_persisting_the_row() {
    let focus_state = Arc::new(WindowFocusState::default());
    let (service, _, repo) = desktop_service(
        NotificationSettings::default(),
        focus_state,
        Arc::new(FailingDesktopNotifier),
        StdDuration::from_millis(1),
    )
    .await;
    service
        .record(new_notification(Some("failing-desktop")))
        .await;
    settle_desktop_dispatch().await;
    assert_eq!(
        repo.list(None, None, 10).await.unwrap().notifications.len(),
        1
    );
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
