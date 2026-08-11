use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::domain::entities::Notification;
use crate::error::{AppError, AppResult};

const DESKTOP_NOTIFICATION_ACTIVATED_EVENT: &str = "notification:desktop_activated";

pub(super) fn send_actionable<R: Runtime>(
    app_handle: &AppHandle<R>,
    notification: &Notification,
) -> AppResult<()> {
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
            if let Err(error) = mac_notification_sys::set_application(&application_id) {
                tracing::debug!(error = %error, "macOS notification application identity was already initialized");
            }

            let mut native = mac_notification_sys::Notification::new();
            native
                .title(&notification.title)
                .message(notification.body.as_deref().unwrap_or(""))
                .wait_for_click(true);

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
