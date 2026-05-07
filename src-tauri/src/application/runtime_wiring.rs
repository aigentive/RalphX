use std::sync::Arc;

use tauri::Manager;

use crate::AppState;
use crate::infrastructure::ExternalMcpHandle;

/// Visual height of the app's top navbar in points. Must match the frontend
/// header (`h-12` → 48 in `frontend/src/App.tsx`). Traffic-light centering
/// targets this value.
#[cfg(target_os = "macos")]
const NAVBAR_HEIGHT_PT: f64 = 48.0;

pub fn create_main_window<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> tauri::Result<()> {
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
            .traffic_light_position(Position::Logical(LogicalPosition { x: 20.0, y: 20.0 }))
    };

    let webview_window = builder.build()?;

    let _ = webview_window.show();

    #[cfg(target_os = "macos")]
    center_traffic_lights_macos(&webview_window);

    Ok(())
}

/// Manually center the macOS standard window buttons (close / minimize / zoom)
/// on the visual midline of our 48pt navbar.
///
/// `traffic_light_position` only sizes the draggable title-bar container;
/// AppKit's auto-resize then leaves the buttons anchored near the top, so we
/// override each button's `frame.origin.y` directly. Coords are bottom-left
/// because the buttons live inside the title-bar container view.
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
            let Some(parent) = button.superview() else {
                continue;
            };

            // Title-bar container's top edge equals the window top. In its
            // bottom-left local coords, a y of `title_bar_h - NAVBAR/2` is
            // the navbar's vertical center; subtract half the button height
            // so the button *center* lands there.
            let title_bar_height = parent.frame().size.height;
            let button_height = frame.size.height;
            let desired_origin_y =
                title_bar_height - NAVBAR_HEIGHT_PT / 2.0 - button_height / 2.0;

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
