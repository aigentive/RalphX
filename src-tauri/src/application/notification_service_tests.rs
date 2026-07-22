use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{Duration, Utc};

use super::notification_service::{
    DesktopNotifier, NotificationEventEmitter, NotificationService, WindowFocusState,
};
use super::AppState;
use crate::domain::entities::{
    NewNotification, Notification, NotificationCategory, NotificationSettings,
    NotificationSeverity, NotificationTarget, NotificationTargetKind, Project, ProjectId,
};
use crate::domain::repositories::{
    NotificationPage, NotificationRepository, NotificationSettingsRepository, ProjectRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryNotificationRepository, MemoryNotificationSettingsRepository, MemoryProjectRepository,
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
    async fn mark_read_by_dedupe_key(
        &self,
        _dedupe_key: &str,
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

#[derive(Default)]
struct RecordingActionableDesktopNotifier(Mutex<Vec<Notification>>);

impl DesktopNotifier for RecordingActionableDesktopNotifier {
    fn send(&self, _title: &str, _body: Option<&str>) -> AppResult<()> {
        Ok(())
    }

    fn send_notification(&self, notification: &Notification) -> AppResult<()> {
        self.0.lock().unwrap().push(notification.clone());
        Ok(())
    }
}

#[derive(Default)]
struct RecordingUpdateEmitter(Mutex<Vec<Option<String>>>);

impl NotificationEventEmitter for RecordingUpdateEmitter {
    fn emit_created(&self, _notification: &Notification) -> AppResult<()> {
        Ok(())
    }

    fn emit_updated(&self, notification: Option<&Notification>) -> AppResult<()> {
        self.0
            .lock()
            .unwrap()
            .push(notification.map(|row| row.id.clone()));
        Ok(())
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
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let mut project = Project::new("acme-app".into(), "/tmp/acme-app".into());
    project.id = ProjectId::from_string("project-1".into());
    project_repo.create(project).await.unwrap();
    let settings_repo_dyn: Arc<dyn NotificationSettingsRepository> = settings_repo.clone();
    let emitter: Arc<dyn NotificationEventEmitter> = Arc::new(RecordingEmitter::default());
    let service = NotificationService::new_with_desktop_dispatch(
        Arc::clone(&repo),
        emitter,
        settings_repo_dyn,
        focus_state,
        notifier,
        window,
        Some(project_repo),
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
async fn workflow_resolution_emits_once_and_isolates_unrelated_rows() {
    let repo: Arc<dyn NotificationRepository> = Arc::new(MemoryNotificationRepository::new());
    let emitter = Arc::new(RecordingUpdateEmitter::default());
    let service = NotificationService::new(Arc::clone(&repo), emitter.clone());
    service
        .record(new_notification(Some("question:target")))
        .await;
    service
        .record(new_notification(Some("question:other")))
        .await;

    service
        .resolve_workflow_notification("question:target")
        .await;
    service
        .resolve_workflow_notification("question:target")
        .await;

    assert_eq!(emitter.0.lock().unwrap().len(), 1);
    let rows = repo.list(None, None, 50).await.unwrap().notifications;
    assert!(rows
        .iter()
        .any(|row| row.dedupe_key.as_deref() == Some("question:target") && row.read_at.is_some()));
    assert!(rows
        .iter()
        .any(|row| row.dedupe_key.as_deref() == Some("question:other") && row.read_at.is_none()));
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
async fn desktop_dispatch_skips_muted_project_but_keeps_unmuted_and_global_notifications() {
    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let settings = NotificationSettings {
        muted_project_ids: vec!["project-1".to_string()],
        ..NotificationSettings::default()
    };
    let (service, _, _) = desktop_service(
        settings,
        Arc::new(WindowFocusState::default()),
        notifier.clone(),
        StdDuration::from_millis(1),
    )
    .await;

    service
        .record_ephemeral(notification_for(
            NotificationCategory::ReviewNeeded,
            NotificationSeverity::ActionRequired,
            None,
        ))
        .await;
    let mut unmuted = notification_for(
        NotificationCategory::ReviewNeeded,
        NotificationSeverity::ActionRequired,
        None,
    );
    unmuted.project_id = Some("project-2".to_string());
    service.record_ephemeral(unmuted).await;
    let mut global = notification_for(
        NotificationCategory::ReviewNeeded,
        NotificationSeverity::ActionRequired,
        None,
    );
    global.project_id = None;
    service.record_ephemeral(global).await;
    settle_desktop_dispatch().await;

    assert_eq!(notifier.0.lock().unwrap().len(), 2);
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
        Some("2 reviews, 1 permission request, 1 merge conflict — acme-app")
    );
}

#[tokio::test]
async fn desktop_summary_omits_project_suffix_for_mixed_or_global_projects() {
    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        Arc::new(WindowFocusState::default()),
        notifier.clone(),
        StdDuration::from_millis(1),
    )
    .await;

    for project_id in [Some("project-1"), Some("project-2"), None] {
        let mut notification = notification_for(
            NotificationCategory::ReviewNeeded,
            NotificationSeverity::ActionRequired,
            None,
        );
        notification.project_id = project_id.map(str::to_string);
        service.record_ephemeral(notification).await;
    }
    settle_desktop_dispatch().await;

    {
        let sent = notifier.0.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].1.as_deref(), Some("3 reviews"));
    }

    for _ in 0..3 {
        let mut notification = notification_for(
            NotificationCategory::ReviewNeeded,
            NotificationSeverity::ActionRequired,
            None,
        );
        notification.project_id = Some("unresolved-project".to_string());
        service.record_ephemeral(notification).await;
    }
    settle_desktop_dispatch().await;

    let sent = notifier.0.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].1.as_deref(), Some("3 reviews"));
}

#[tokio::test]
async fn app_state_notification_service_coalesces_records_across_separate_callers() {
    let state = AppState::new_test();
    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        Arc::clone(&state.window_focus_state),
        notifier.clone(),
        StdDuration::from_millis(10),
    )
    .await;
    state.install_notification_service_for_test(Arc::new(service));

    let first_caller = state.notification_service();
    let second_caller = state.notification_service();
    assert!(Arc::ptr_eq(&first_caller, &second_caller));

    first_caller
        .record_ephemeral(notification_for(
            NotificationCategory::ReviewNeeded,
            NotificationSeverity::ActionRequired,
            None,
        ))
        .await;
    second_caller
        .record_ephemeral(notification_for(
            NotificationCategory::PermissionRequest,
            NotificationSeverity::ActionRequired,
            None,
        ))
        .await;
    first_caller
        .record_ephemeral(notification_for(
            NotificationCategory::MergeConflict,
            NotificationSeverity::ActionRequired,
            None,
        ))
        .await;
    settle_desktop_dispatch().await;

    let sent = notifier.0.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "3 items need your attention");
}

#[tokio::test]
async fn notification_service_cache_is_shared_across_tauri_and_http_app_states() {
    let tauri_state = AppState::new_test();
    let mut http_state = AppState::new_test();
    http_state.notification_service_cache = Arc::clone(&tauri_state.notification_service_cache);

    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        Arc::clone(&tauri_state.window_focus_state),
        notifier.clone(),
        StdDuration::from_millis(10),
    )
    .await;
    tauri_state.install_notification_service_for_test(Arc::new(service));

    let tauri_service = tauri_state.notification_service();
    let http_service = http_state.notification_service();
    assert!(Arc::ptr_eq(&tauri_service, &http_service));

    for (index, service) in [&tauri_service, &http_service, &tauri_service]
        .into_iter()
        .enumerate()
    {
        service
            .record_ephemeral(notification_for(
                [
                    NotificationCategory::ReviewNeeded,
                    NotificationCategory::PermissionRequest,
                    NotificationCategory::MergeConflict,
                ][index],
                NotificationSeverity::ActionRequired,
                None,
            ))
            .await;
    }
    settle_desktop_dispatch().await;

    let sent = notifier.0.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "3 items need your attention");
}

#[tokio::test]
async fn app_state_pre_handle_notification_service_does_not_block_later_dispatch_installation() {
    let state = AppState::new_test();
    let early_service = state.notification_service();
    assert!(!state.has_cached_notification_service_for_test());

    let notifier = Arc::new(RecordingDesktopNotifier::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        Arc::clone(&state.window_focus_state),
        notifier.clone(),
        StdDuration::from_millis(1),
    )
    .await;
    state.install_notification_service_for_test(Arc::new(service));

    let upgraded_service = state.notification_service();
    assert!(!Arc::ptr_eq(&early_service, &upgraded_service));
    upgraded_service
        .record_ephemeral(notification_for(
            NotificationCategory::ReviewNeeded,
            NotificationSeverity::ActionRequired,
            None,
        ))
        .await;
    settle_desktop_dispatch().await;

    assert_eq!(notifier.0.lock().unwrap().len(), 1);
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
async fn desktop_dispatch_preserves_agent_conversation_activation_target() {
    let notifier = Arc::new(RecordingActionableDesktopNotifier::default());
    let (service, _, _) = desktop_service(
        NotificationSettings::default(),
        Arc::new(WindowFocusState::default()),
        notifier.clone(),
        StdDuration::from_millis(1),
    )
    .await;
    let mut notification = notification_for(
        NotificationCategory::AgentQuestion,
        NotificationSeverity::ActionRequired,
        None,
    );
    notification.target = NotificationTarget {
        kind: NotificationTargetKind::AgentConversation,
        project_id: Some("project-2".to_string()),
        task_id: None,
        conversation_id: Some("conversation-2".to_string()),
        setup_conversation_id: None,
        automation_id: None,
        run_id: None,
    };

    service.record_ephemeral(notification).await;
    settle_desktop_dispatch().await;

    let dispatched = notifier.0.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(
        dispatched[0].target.kind,
        NotificationTargetKind::AgentConversation
    );
    assert_eq!(
        dispatched[0].target.project_id.as_deref(),
        Some("project-2")
    );
    assert_eq!(
        dispatched[0].target.conversation_id.as_deref(),
        Some("conversation-2")
    );
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
async fn mark_read_emits_only_for_a_changed_row_and_returns_repository_failures() {
    let repo: Arc<dyn NotificationRepository> = Arc::new(MemoryNotificationRepository::new());
    let emitter = Arc::new(RecordingUpdateEmitter::default());
    let service = NotificationService::new(Arc::clone(&repo), emitter.clone());
    let row = new_notification(Some("mark-read")).into_notification(Utc::now());
    repo.create_with_dedupe(row.clone()).await.unwrap();

    service.mark_read(&row.id).await.unwrap();
    service.mark_read(&row.id).await.unwrap();
    service.mark_read("missing").await.unwrap();
    assert_eq!(emitter.0.lock().unwrap().as_slice(), [Some(row.id.clone())]);

    let failing_emitter: Arc<dyn NotificationEventEmitter> = emitter.clone();
    let failing_service =
        NotificationService::new(Arc::new(FailingNotificationRepository), failing_emitter);
    assert!(failing_service.mark_read("fails").await.is_err());
    assert_eq!(emitter.0.lock().unwrap().as_slice(), [Some(row.id)]);
}

#[tokio::test]
async fn mark_all_read_emits_only_when_rows_changed_and_keeps_project_scope() {
    let repo: Arc<dyn NotificationRepository> = Arc::new(MemoryNotificationRepository::new());
    let emitter = Arc::new(RecordingUpdateEmitter::default());
    let service = NotificationService::new(Arc::clone(&repo), emitter.clone());
    let project_a = new_notification(Some("mark-all-a")).into_notification(Utc::now());
    let mut project_b_input = new_notification(Some("mark-all-b"));
    project_b_input.project_id = Some("project-2".to_string());
    let project_b = project_b_input.into_notification(Utc::now());
    repo.create_with_dedupe(project_a).await.unwrap();
    repo.create_with_dedupe(project_b).await.unwrap();

    service.mark_all_read(Some("project-1")).await.unwrap();
    service.mark_all_read(Some("project-1")).await.unwrap();
    assert_eq!(emitter.0.lock().unwrap().as_slice(), [None]);
    assert_eq!(repo.unread_count(Some("project-2")).await.unwrap(), 1);
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

#[tokio::test]
async fn memory_prune_keeps_actionable_plan_approval_but_removes_it_after_settlement() {
    let repo: Arc<dyn NotificationRepository> = Arc::new(MemoryNotificationRepository::new());
    let now = Utc::now();
    let mut approval = new_notification(Some("plan:session-1:artifact-1"))
        .into_notification(now - Duration::days(40));
    approval.category = NotificationCategory::PlanApproval;
    let newest = new_notification(Some("newest-task")).into_notification(now);
    repo.create_with_dedupe(approval.clone()).await.unwrap();
    repo.create_with_dedupe(newest.clone()).await.unwrap();

    repo.prune(now - Duration::days(30), 1).await.unwrap();
    let retained = repo.list(None, None, 50).await.unwrap().notifications;
    assert!(retained.iter().any(|row| row.id == approval.id));
    assert!(retained.iter().any(|row| row.id == newest.id));

    repo.mark_read(&approval.id, now - Duration::days(35))
        .await
        .unwrap();
    repo.prune(now - Duration::days(30), 1).await.unwrap();
    let retained = repo.list(None, None, 50).await.unwrap().notifications;
    assert!(retained.iter().all(|row| row.id != approval.id));
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].id, newest.id);
}
