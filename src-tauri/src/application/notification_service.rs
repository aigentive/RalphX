use std::sync::Arc;

use chrono::Utc;
use tauri::{AppHandle, Emitter};

use crate::domain::entities::{NewNotification, Notification};
use crate::domain::repositories::NotificationRepository;
use crate::error::{AppError, AppResult};

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

pub struct TauriNotificationEventEmitter {
    app_handle: AppHandle,
}
impl TauriNotificationEventEmitter {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}
impl NotificationEventEmitter for TauriNotificationEventEmitter {
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
pub struct NotificationService {
    repo: Arc<dyn NotificationRepository>,
    emitter: Arc<dyn NotificationEventEmitter>,
}
impl NotificationService {
    pub fn new(
        repo: Arc<dyn NotificationRepository>,
        emitter: Arc<dyn NotificationEventEmitter>,
    ) -> Self {
        Self { repo, emitter }
    }
    pub fn repository(&self) -> Arc<dyn NotificationRepository> {
        Arc::clone(&self.repo)
    }
    pub async fn record(&self, input: NewNotification) {
        let notification = input.into_notification(Utc::now());
        match self.repo.create_with_dedupe(notification.clone()).await {
            Ok(true) => {
                if let Err(error) = self.emitter.emit_created(&notification) {
                    tracing::warn!(error = %error, notification_id = %notification.id, "Failed to emit notification:created");
                }
                self.dispatch_desktop(&notification).await;
            }
            Ok(false) => {
                tracing::debug!(dedupe_key = ?notification.dedupe_key, "Notification deduplicated")
            }
            Err(error) => tracing::warn!(error = %error, "Failed to record notification"),
        }
    }
    pub async fn record_ephemeral(&self, input: NewNotification) {
        tracing::debug!(category = ?input.category, severity = ?input.severity, "Recording ephemeral notification dispatch hook");
        self.dispatch_desktop(&input.into_notification(Utc::now()))
            .await;
    }
    pub async fn mark_read(&self, id: &str) {
        match self.repo.mark_read(id, Utc::now()).await {
            Ok(Some(notification)) => {
                if let Err(error) = self.emitter.emit_updated(Some(&notification)) {
                    tracing::warn!(error = %error, notification_id = %notification.id, "Failed to emit notification:updated");
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(error = %error, notification_id = id, "Failed to mark notification read")
            }
        }
    }
    pub async fn mark_all_read(&self, project_id: Option<&str>) {
        match self.repo.mark_all_read(project_id, Utc::now()).await {
            Ok(changed) if changed > 0 => {
                if let Err(error) = self.emitter.emit_updated(None) {
                    tracing::warn!(error = %error, "Failed to emit notification:updated");
                }
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(error = %error, "Failed to mark all notifications read"),
        }
    }
    async fn dispatch_desktop(&self, notification: &Notification) {
        tracing::debug!(notification_id = %notification.id, "Desktop notification dispatch seam is not installed yet");
    }
}
