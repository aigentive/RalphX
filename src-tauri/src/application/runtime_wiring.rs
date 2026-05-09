use std::sync::Arc;

use tauri::Manager;

use crate::AppState;
use crate::infrastructure::ExternalMcpHandle;

/// Visual height of the app's top navbar in points. Must match the frontend
/// header (`h-12` → 48 in `frontend/src/App.tsx`). Traffic-light centering
/// targets this value.
#[cfg(target_os = "macos")]
const NAVBAR_HEIGHT_PT: f64 = 48.0;

#[cfg(target_os = "macos")]
const TRAFFIC_LIGHT_INSET_X_PT: f64 = 20.0;

#[cfg(target_os = "macos")]
const TRAFFIC_LIGHT_TITLEBAR_INSET_Y_PT: f64 = 20.0;

pub fn create_main_window<R: tauri::Runtime + 'static, M: tauri::Manager<R>>(
    app: &M,
) -> tauri::Result<()> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("")
        .inner_size(1200.0, 800.0)
        .decorations(true)
        .visible(false);

    #[cfg(target_os = "macos")]
    let builder = {
        use tauri::{LogicalPosition, Position, TitleBarStyle};

        builder
            .hidden_title(true)
            .title_bar_style(TitleBarStyle::Overlay)
            // x: leave room for OS chrome. y is overridden vertically by
            // `center_traffic_lights_macos` below — tao only uses y to size
            // the draggable title bar; AppKit's auto-layout does not place
            // the buttons at the geometric center of an arbitrary navbar.
            .traffic_light_position(Position::Logical(LogicalPosition {
                x: TRAFFIC_LIGHT_INSET_X_PT,
                y: TRAFFIC_LIGHT_TITLEBAR_INSET_Y_PT,
            }))
    };

    let webview_window = builder.build()?;

    let _ = webview_window.show();

    #[cfg(target_os = "macos")]
    install_macos_traffic_light_centering(&webview_window);

    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos_traffic_light_centering<R: tauri::Runtime + 'static>(
    window: &tauri::WebviewWindow<R>,
) {
    request_macos_traffic_light_recenter(window);

    let event_window = window.clone();
    window.on_window_event(move |event| {
        if should_recenter_macos_traffic_lights(event) {
            request_macos_traffic_light_recenter(&event_window);
        }
    });
}

#[cfg(target_os = "macos")]
fn request_macos_traffic_light_recenter<R: tauri::Runtime + 'static>(
    window: &tauri::WebviewWindow<R>,
) {
    let recenter_window = window.clone();
    let _ = window.run_on_main_thread(move || {
        center_traffic_lights_macos(&recenter_window);
    });
}

#[cfg(target_os = "macos")]
pub(crate) fn should_recenter_macos_traffic_lights(event: &tauri::WindowEvent) -> bool {
    matches!(
        event,
        tauri::WindowEvent::Resized(_)
            | tauri::WindowEvent::ScaleFactorChanged { .. }
            | tauri::WindowEvent::Focused(true)
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_traffic_light_target_center_y(title_bar_height: f64) -> f64 {
    title_bar_height - NAVBAR_HEIGHT_PT / 2.0
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_traffic_light_origin_y(
    target_center_y_in_button_parent: f64,
    button_height: f64,
) -> f64 {
    target_center_y_in_button_parent - button_height / 2.0
}

/// Manually center the macOS standard window buttons (close / minimize / zoom)
/// on the visual midline of our 48pt navbar.
///
/// `traffic_light_position` only sizes the draggable title-bar container;
/// AppKit's auto-resize then leaves the buttons anchored near the top, so we
/// override each button's `frame.origin.y` directly.
#[cfg(target_os = "macos")]
fn center_traffic_lights_macos<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use objc2_foundation::NSPoint;

    let Ok(ns_window_ptr) = window.ns_window() else {
        return;
    };
    if ns_window_ptr.is_null() {
        return;
    }

    // SAFETY: Tauri returns a non-null `NSWindow *` for the Cocoa window
    // backing this WebviewWindow on macOS. Tauri's setup hook runs this on
    // the main thread, where AppKit reads/setters are safe.
    unsafe {
        let ns_window: &NSWindow = &*(ns_window_ptr.cast::<NSWindow>());

        for kind in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            let Some(button) = ns_window.standardWindowButton(kind) else {
                continue;
            };
            let frame = button.frame();
            let Some(button_parent) = button.superview() else {
                continue;
            };
            let Some(title_bar_container) = button_parent.superview() else {
                continue;
            };

            // Tao positions the controls inside the title-bar container
            // (`button.superview().superview()`). Convert the target center
            // from that container into the button parent's coordinates before
            // setting the button frame.
            let title_bar_height = title_bar_container.frame().size.height;
            let button_height = frame.size.height;
            if title_bar_height <= 0.0 || button_height <= 0.0 {
                continue;
            }

            let target_center_y = macos_traffic_light_target_center_y(title_bar_height);
            let target_center_in_parent = button_parent.convertPoint_fromView(
                NSPoint {
                    x: frame.origin.x,
                    y: target_center_y,
                },
                Some(&title_bar_container),
            );
            let desired_origin_y =
                macos_traffic_light_origin_y(target_center_in_parent.y, button_height);

            button.setFrameOrigin(NSPoint {
                x: frame.origin.x,
                y: desired_origin_y,
            });
        }
    }
}

pub fn build_http_app_state(
    app_state: &AppState,
    app_handle: tauri::AppHandle,
) -> crate::AppResult<Arc<AppState>> {
    let shared_db_conn = Arc::clone(app_state.db.inner());
    let shared_question_state = Arc::clone(&app_state.question_state);
    let shared_permission_state = Arc::clone(&app_state.permission_state);
    let shared_message_queue = Arc::clone(&app_state.message_queue);
    let shared_interactive_process_registry = Arc::clone(&app_state.interactive_process_registry);
    let shared_github_service = app_state.github_service.clone();
    let shared_pr_poller_registry = Arc::clone(&app_state.pr_poller_registry);
    let mut http_app_state_inner = AppState::new_production_shared(app_handle, shared_db_conn)?;
    http_app_state_inner.question_state = shared_question_state;
    http_app_state_inner.permission_state = shared_permission_state;
    http_app_state_inner.message_queue = shared_message_queue;
    http_app_state_inner.interactive_process_registry = shared_interactive_process_registry;
    http_app_state_inner.github_service = shared_github_service;
    http_app_state_inner.pr_poller_registry = shared_pr_poller_registry;
    // INVARIANT: streaming_state_cache uses Arc internally; clone shares the same cache.
    http_app_state_inner.streaming_state_cache = app_state.streaming_state_cache.clone();
    http_app_state_inner.webhook_publisher = app_state.webhook_publisher.clone();
    http_app_state_inner.session_merge_locks = Arc::clone(&app_state.session_merge_locks);
    Ok(Arc::new(http_app_state_inner))
}

pub fn register_managed_state(
    app: &mut tauri::App<tauri::Wry>,
    app_state: AppState,
    service_team_tracker: crate::application::TeamStateTracker,
) {
    let team_session_repo = Arc::clone(&app_state.team_session_repo);
    let team_message_repo = Arc::clone(&app_state.team_message_repo);

    let throttled_emitter = crate::application::ThrottledEmitter::new(app.handle().clone());
    app.manage(throttled_emitter);
    app.manage(app_state);

    let team_service = Arc::new(crate::application::TeamService::new_with_repos(
        Arc::new(service_team_tracker),
        app.handle().clone(),
        team_session_repo,
        team_message_repo,
    ));
    app.manage(team_service);
    app.manage(ExternalMcpHandle::new());
}
