use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::application::desktop_notification_budget::{click_wait_budget, SendMode};
use crate::application::desktop_notification_reaper;
use crate::domain::entities::Notification;
use crate::error::{AppError, AppResult};

const DESKTOP_NOTIFICATION_ACTIVATED_EVENT: &str = "notification:desktop_activated";

/// The native payload for one notification, resolved from the click-wait budget.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NativeSendSpec {
    pub(crate) title: String,
    pub(crate) message: String,
    /// When false the notification is also sent `asynchronous(true)`, so the crate registers no
    /// main-run-loop poll timer for it.
    pub(crate) wait_for_click: bool,
}

/// Shapes the native payload. Pure, so the production path's budget compliance is testable.
pub(crate) fn build_send_spec(notification: &Notification, mode: &SendMode) -> NativeSendSpec {
    NativeSendSpec {
        title: notification.title.clone(),
        message: notification.body.as_deref().unwrap_or("").to_string(),
        wait_for_click: matches!(mode, SendMode::WaitForClick(_)),
    }
}

pub(super) fn send_actionable<R: Runtime>(
    app_handle: &AppHandle<R>,
    notification: &Notification,
) -> AppResult<()> {
    let mode = click_wait_budget().plan_send();
    let spec = build_send_spec(notification, &mode);
    if !spec.wait_for_click {
        tracing::debug!(
            notification_id = %notification.id,
            "Click-wait budget saturated; delivering notification without in-app navigation"
        );
    }

    desktop_notification_reaper::ensure_started(app_handle.clone());

    let app_handle = app_handle.clone();
    let notification = notification.clone();
    let application_id = if tauri::is_dev() {
        "com.apple.Terminal".to_string()
    } else {
        app_handle.config().identifier.clone()
    };

    std::thread::Builder::new()
        .name("ralphx-desktop-notification".to_string())
        .spawn(move || {
            // Holds the click-wait slot until the native wait resolves — by click, by dismissal, or
            // by the reaper removing the notification from Notification Center. Dropped on panic and
            // on spawn failure too, so a slot can never leak.
            let _permit = mode;

            if let Err(error) = mac_notification_sys::set_application(&application_id) {
                tracing::debug!(error = %error, "macOS notification application identity was already initialized");
            }

            let mut native = mac_notification_sys::Notification::new();
            native.title(&spec.title).message(&spec.message);
            if spec.wait_for_click {
                native.wait_for_click(true);
            } else {
                native.asynchronous(true);
            }

            match native.send() {
                Ok(mac_notification_sys::NotificationResponse::Click) => {
                    reveal_main_window(&app_handle);
                    if let Err(error) =
                        app_handle.emit(DESKTOP_NOTIFICATION_ACTIVATED_EVENT, &notification)
                    {
                        tracing::warn!(
                            error = %error,
                            notification_id = %notification.id,
                            "Failed to emit desktop notification activation"
                        );
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        notification_id = %notification.id,
                        "Failed to dispatch actionable macOS notification"
                    );
                }
            }
        })
        .map(|_| ())
        .map_err(|error| AppError::Infrastructure(error.to_string()))
}

fn reveal_main_window<R: Runtime>(app_handle: &AppHandle<R>) {
    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };
    if let Err(error) = window.show() {
        tracing::warn!(error = %error, "Failed to show RalphX after notification activation");
    }
    if let Err(error) = window.set_focus() {
        tracing::warn!(error = %error, "Failed to focus RalphX after notification activation");
    }
}
