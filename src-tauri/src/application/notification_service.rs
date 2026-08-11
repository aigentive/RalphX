use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_notification::{NotificationExt, PermissionState};
use tokio::sync::Mutex;

use crate::domain::entities::{
    notification_category_group, NewNotification, Notification, NotificationCategory,
    NotificationCategoryGroup, ProjectId,
};
use crate::domain::repositories::{
    NotificationRepository, NotificationSettingsRepository, ProjectRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::stream_timeouts;

pub const NOTIFICATION_CREATED_EVENT: &str = "notification:created";
pub const NOTIFICATION_UPDATED_EVENT: &str = "notification:updated";

pub trait NotificationEventEmitter: Send + Sync {
    fn emit_created(&self, notification: &Notification) -> AppResult<()>;
    fn emit_updated(&self, notification: Option<&Notification>) -> AppResult<()>;
}

#[derive(Default)]
pub struct NoopNotificationEventEmitter;
impl NotificationEventEmitter for NoopNotificationEventEmitter {
    fn emit_created(&self, _notification: &Notification) -> AppResult<()> {
        tracing::warn!("Notification created without an AppHandle; event was not delivered");
        Ok(())
    }
    fn emit_updated(&self, _notification: Option<&Notification>) -> AppResult<()> {
        tracing::warn!("Notification updated without an AppHandle; event was not delivered");
        Ok(())
    }
}

pub struct TauriNotificationEventEmitter<R: Runtime = tauri::Wry> {
    app_handle: AppHandle<R>,
}

/// Process-wide window-focus state. It begins unfocused so notifications raised before the
/// first native focus event remain deliverable instead of being silently suppressed.
#[derive(Default)]
pub struct WindowFocusState(AtomicBool);

impl WindowFocusState {
    pub fn is_focused(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub fn set_focused(&self, focused: bool) {
        self.0.store(focused, Ordering::Release);
    }
}

pub trait DesktopNotifier: Send + Sync {
    fn send(&self, title: &str, body: Option<&str>) -> AppResult<()>;

    fn send_notification(&self, notification: &Notification) -> AppResult<()> {
        self.send(&notification.title, notification.body.as_deref())
    }
}

pub struct TauriDesktopNotifier<R: Runtime = tauri::Wry> {
    app_handle: AppHandle<R>,
}

impl<R: Runtime> TauriDesktopNotifier<R> {
    pub fn new(app_handle: AppHandle<R>) -> Self {
        Self { app_handle }
    }

    fn ensure_permission(&self) -> AppResult<()> {
        let notification = self.app_handle.notification();
        if notification
            .permission_state()
            .map_err(|error| AppError::Infrastructure(error.to_string()))?
            == PermissionState::Prompt
        {
            notification
                .request_permission()
                .map_err(|error| AppError::Infrastructure(error.to_string()))?;
        }
        if notification
            .permission_state()
            .map_err(|error| AppError::Infrastructure(error.to_string()))?
            != PermissionState::Granted
        {
            return Err(AppError::Infrastructure(
                "Desktop notification permission was not granted".to_string(),
            ));
        }
        Ok(())
    }
}

impl<R: Runtime> DesktopNotifier for TauriDesktopNotifier<R> {
    fn send(&self, title: &str, body: Option<&str>) -> AppResult<()> {
        self.ensure_permission()?;
        let notification = self.app_handle.notification();
        let mut builder = notification.builder().title(title);
        if let Some(body) = body {
            builder = builder.body(body);
        }
        builder
            .show()
            .map_err(|error| AppError::Infrastructure(error.to_string()))
    }

    fn send_notification(&self, notification: &Notification) -> AppResult<()> {
        #[cfg(target_os = "macos")]
        {
            self.ensure_permission()?;
            super::desktop_notification::send_actionable(&self.app_handle, notification)
        }

        #[cfg(not(target_os = "macos"))]
        self.send(&notification.title, notification.body.as_deref())
    }
}

#[derive(Default)]
pub struct NoopDesktopNotifier;

impl DesktopNotifier for NoopDesktopNotifier {
    fn send(&self, _title: &str, _body: Option<&str>) -> AppResult<()> {
        tracing::warn!("Desktop notification requested without an AppHandle; delivery skipped");
        Ok(())
    }
}

#[derive(Default)]
struct DefaultNotificationSettingsRepository;

#[async_trait::async_trait]
impl NotificationSettingsRepository for DefaultNotificationSettingsRepository {
    async fn get_settings(&self) -> AppResult<crate::domain::entities::NotificationSettings> {
        Ok(crate::domain::entities::NotificationSettings::default())
    }

    async fn update_settings(
        &self,
        settings: &crate::domain::entities::NotificationSettings,
    ) -> AppResult<crate::domain::entities::NotificationSettings> {
        Ok(settings.clone())
    }
}

struct DesktopNotificationCoalescer {
    pending: Mutex<Vec<Notification>>,
    flush_scheduled: AtomicBool,
    window: Duration,
    notifier: Arc<dyn DesktopNotifier>,
    project_repo: Option<Arc<dyn ProjectRepository>>,
}

impl DesktopNotificationCoalescer {
    fn new(
        window: Duration,
        notifier: Arc<dyn DesktopNotifier>,
        project_repo: Option<Arc<dyn ProjectRepository>>,
    ) -> Self {
        Self {
            pending: Mutex::new(Vec::new()),
            flush_scheduled: AtomicBool::new(false),
            window,
            notifier,
            project_repo,
        }
    }

    async fn enqueue(self: &Arc<Self>, notification: Notification) {
        self.pending.lock().await.push(notification);
        if !self.flush_scheduled.swap(true, Ordering::AcqRel) {
            let coalescer = Arc::clone(self);
            tokio::spawn(async move {
                tokio::time::sleep(coalescer.window).await;
                coalescer.flush().await;
            });
        }
    }

    async fn flush(&self) {
        let notifications = std::mem::take(&mut *self.pending.lock().await);
        self.flush_scheduled.store(false, Ordering::Release);
        if notifications.len() >= 3 {
            let (title, body) = desktop_summary(&notifications, self.project_repo.as_deref()).await;
            self.send(&title, Some(&body));
            return;
        }
        for notification in notifications {
            if let Err(error) = self.notifier.send_notification(&notification) {
                tracing::warn!(
                    error = %error,
                    notification_id = %notification.id,
                    "Failed to dispatch desktop notification"
                );
            }
        }
    }

    fn send(&self, title: &str, body: Option<&str>) {
        if let Err(error) = self.notifier.send(title, body) {
            tracing::warn!(error = %error, "Failed to dispatch desktop notification");
        }
    }
}

async fn desktop_summary(
    notifications: &[Notification],
    project_repo: Option<&dyn ProjectRepository>,
) -> (String, String) {
    let mut counts = HashMap::<&'static str, usize>::new();
    let mut order = Vec::<&'static str>::new();
    for notification in notifications {
        let group = desktop_summary_group(notification.category);
        if !counts.contains_key(group) {
            order.push(group);
        }
        *counts.entry(group).or_default() += 1;
    }
    let details = order
        .into_iter()
        .map(|group| {
            let count = counts[&group];
            format!("{count} {}", desktop_summary_count_label(group, count))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let project = notifications
        .first()
        .and_then(|notification| notification.project_id.as_deref())
        .filter(|project| {
            notifications
                .iter()
                .all(|notification| notification.project_id.as_deref() == Some(*project))
        });
    let project_name = match (project, project_repo) {
        (Some(project_id), Some(project_repo)) => match project_repo
            .get_by_id(&ProjectId::from_string(project_id.to_string()))
            .await
        {
            Ok(Some(project)) => Some(project.name),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(error = %error, project_id, "Failed to resolve desktop summary project name");
                None
            }
        },
        _ => None,
    };
    let body = project_name
        .map(|project_name| format!("{details} — {project_name}"))
        .unwrap_or(details);
    (
        format!("{} items need your attention", notifications.len()),
        body,
    )
}

fn desktop_summary_group(category: NotificationCategory) -> &'static str {
    match category {
        NotificationCategory::ReviewNeeded
        | NotificationCategory::ReviewEscalated
        | NotificationCategory::PlanApproval => "reviews",
        NotificationCategory::PermissionRequest => "permission request",
        NotificationCategory::AgentQuestion => "agent question",
        NotificationCategory::MergeConflict => "merge conflict",
        _ => match notification_category_group(category) {
            NotificationCategoryGroup::AgentRequests => "agent requests",
            NotificationCategoryGroup::AgentWaiting => "agent waiting",
            NotificationCategoryGroup::Reviews => "reviews",
            NotificationCategoryGroup::TaskFailures => "task failures",
            NotificationCategoryGroup::AutomationApprovals => "automation approvals",
            NotificationCategoryGroup::AutomationRunCompletions => "automation completions",
            NotificationCategoryGroup::GitGithub => "GitHub items",
        },
    }
}

fn desktop_summary_count_label(group: &'static str, count: usize) -> &'static str {
    match (group, count == 1) {
        ("reviews", true) => "review",
        ("permission request", false) => "permission requests",
        ("agent question", false) => "agent questions",
        ("merge conflict", false) => "merge conflicts",
        ("agent requests", true) => "agent request",
        ("task failures", true) => "task failure",
        ("automation approvals", true) => "automation approval",
        ("automation completions", true) => "automation completion",
        ("GitHub items", true) => "GitHub item",
        (group, _) => group,
    }
}

fn desktop_category_is_always(_category: NotificationCategory) -> bool {
    false
}
impl<R: Runtime> TauriNotificationEventEmitter<R> {
    pub fn new(app_handle: AppHandle<R>) -> Self {
        Self { app_handle }
    }
}
impl<R: Runtime> NotificationEventEmitter for TauriNotificationEventEmitter<R> {
    fn emit_created(&self, notification: &Notification) -> AppResult<()> {
        self.app_handle
            .emit(NOTIFICATION_CREATED_EVENT, notification)
            .map_err(|error| AppError::Infrastructure(error.to_string()))
    }
    fn emit_updated(&self, notification: Option<&Notification>) -> AppResult<()> {
        self.app_handle
            .emit(NOTIFICATION_UPDATED_EVENT, notification)
            .map_err(|error| AppError::Infrastructure(error.to_string()))
    }
}

/// Best-effort notification effects: storage or event failures never fail workflow authority.
#[derive(Clone)]
pub struct NotificationService {
    repo: Arc<dyn NotificationRepository>,
    emitter: Arc<dyn NotificationEventEmitter>,
    settings_repo: Arc<dyn NotificationSettingsRepository>,
    focus_state: Arc<WindowFocusState>,
    coalescer: Arc<DesktopNotificationCoalescer>,
}
impl NotificationService {
    pub fn new(
        repo: Arc<dyn NotificationRepository>,
        emitter: Arc<dyn NotificationEventEmitter>,
    ) -> Self {
        Self::new_with_desktop_dispatch(
            repo,
            emitter,
            Arc::new(DefaultNotificationSettingsRepository),
            Arc::new(WindowFocusState::default()),
            Arc::new(NoopDesktopNotifier),
            Duration::from_secs(stream_timeouts().desktop_notification_coalesce_window_secs),
            None,
        )
    }

    pub(crate) fn new_with_desktop_dispatch(
        repo: Arc<dyn NotificationRepository>,
        emitter: Arc<dyn NotificationEventEmitter>,
        settings_repo: Arc<dyn NotificationSettingsRepository>,
        focus_state: Arc<WindowFocusState>,
        notifier: Arc<dyn DesktopNotifier>,
        coalesce_window: Duration,
        project_repo: Option<Arc<dyn ProjectRepository>>,
    ) -> Self {
        Self {
            repo,
            emitter,
            settings_repo,
            focus_state,
            coalescer: Arc::new(DesktopNotificationCoalescer::new(
                coalesce_window,
                notifier,
                project_repo,
            )),
        }
    }
    pub fn repository(&self) -> Arc<dyn NotificationRepository> {
        Arc::clone(&self.repo)
    }
    pub async fn record(&self, input: NewNotification) {
        if let Err(error) = self.record_result(input).await {
            tracing::warn!(error = %error, "Failed to record notification");
        }
    }
    /// Records a durable notification and reports whether a new row was inserted.
    /// A deduplicated row is still a successful durable outcome.
    pub async fn record_result(&self, input: NewNotification) -> AppResult<bool> {
        let notification = input.into_notification(Utc::now());
        match self.repo.create_with_dedupe(notification.clone()).await {
            Ok(true) => {
                if let Err(error) = self.emitter.emit_created(&notification) {
                    tracing::warn!(error = %error, notification_id = %notification.id, "Failed to emit notification:created");
                }
                self.dispatch_desktop(&notification).await;
                Ok(true)
            }
            Ok(false) => {
                tracing::debug!(dedupe_key = ?notification.dedupe_key, "Notification deduplicated");
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
    pub async fn record_ephemeral(&self, input: NewNotification) {
        tracing::debug!(category = ?input.category, severity = ?input.severity, "Recording ephemeral notification dispatch hook");
        self.dispatch_desktop(&input.into_notification(Utc::now()))
            .await;
    }
    pub async fn mark_read(&self, id: &str) -> AppResult<()> {
        match self.repo.mark_read(id, Utc::now()).await {
            Ok(Some(notification)) => {
                if let Err(error) = self.emitter.emit_updated(Some(&notification)) {
                    tracing::warn!(error = %error, notification_id = %notification.id, "Failed to emit notification:updated");
                }
            }
            Ok(None) => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }
    /// Best-effort settlement after workflow authority commits.
    pub async fn resolve_workflow_notification(&self, dedupe_key: &str) {
        match self
            .repo
            .mark_read_by_dedupe_key(dedupe_key, Utc::now())
            .await
        {
            Ok(Some(notification)) => {
                if let Err(error) = self.emitter.emit_updated(Some(&notification)) {
                    tracing::warn!(error = %error, notification_id = %notification.id, dedupe_key, "Failed to emit workflow notification settlement");
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(error = %error, dedupe_key, "Failed to settle workflow notification")
            }
        }
    }
    pub async fn mark_all_read(&self, project_id: Option<&str>) -> AppResult<()> {
        match self.repo.mark_all_read(project_id, Utc::now()).await {
            Ok(changed) if changed > 0 => {
                if let Err(error) = self.emitter.emit_updated(None) {
                    tracing::warn!(error = %error, "Failed to emit notification:updated");
                }
            }
            Ok(_) => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }
    async fn dispatch_desktop(&self, notification: &Notification) {
        let settings = match self.settings_repo.get_settings().await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(error = %error, "Failed to load desktop notification settings");
                return;
            }
        };
        if !settings.desktop_enabled {
            return;
        }
        if !settings.desktop_category_enabled(notification.category) {
            return;
        }
        if notification
            .project_id
            .as_deref()
            .is_some_and(|project_id| settings.muted_project_ids.iter().any(|id| id == project_id))
        {
            return;
        }
        if settings.desktop_only_when_unfocused
            && self.focus_state.is_focused()
            && !desktop_category_is_always(notification.category)
        {
            return;
        }
        self.coalescer.enqueue(notification.clone()).await;
    }
}
