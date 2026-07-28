use std::sync::Arc;

use tauri::Manager;

use crate::application::notification_service::WindowFocusState;
use crate::application::startup_status::StartupCoordinator;
use crate::infrastructure::ExternalMcpHandle;
use crate::{AppError, AppResult, AppState};

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
    focus_state: Arc<WindowFocusState>,
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

    install_window_focus_tracking(&webview_window, focus_state);

    let _ = webview_window.show();

    #[cfg(target_os = "macos")]
    install_macos_traffic_light_centering(&webview_window);

    #[cfg(target_os = "macos")]
    let _ = webview_window.set_focus();

    Ok(())
}

fn install_window_focus_tracking<R: tauri::Runtime + 'static>(
    window: &tauri::WebviewWindow<R>,
    focus_state: Arc<WindowFocusState>,
) {
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(focused) = event {
            focus_state.set_focused(*focused);
        }
    });
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
    let shared_queued_message_repo = Arc::clone(&app_state.queued_message_repo);
    let shared_interactive_process_registry = Arc::clone(&app_state.interactive_process_registry);
    let shared_github_service = app_state.github_service.clone();
    let shared_pr_poller_registry = Arc::clone(&app_state.pr_poller_registry);
    let shared_events = Arc::clone(&app_state.events);
    let shared_internal_event_bus = app_state.internal_event_bus.clone();
    let shared_app_paths = app_state.app_paths.clone();
    let shared_window_focus_state = Arc::clone(&app_state.window_focus_state);
    let shared_notification_service_cache = Arc::clone(&app_state.notification_service_cache);
    let shared_agent_capability_gate = Arc::clone(&app_state.agent_capability_gate);
    let mut http_app_state_inner = AppState::new_production_shared_with_paths_and_events(
        app_handle,
        shared_db_conn,
        shared_app_paths,
        shared_events,
        shared_internal_event_bus,
    )?;
    http_app_state_inner.question_state = shared_question_state;
    http_app_state_inner.permission_state = shared_permission_state;
    http_app_state_inner.message_queue = shared_message_queue;
    http_app_state_inner.queued_message_repo = shared_queued_message_repo;
    http_app_state_inner.interactive_process_registry = shared_interactive_process_registry;
    http_app_state_inner.github_service = shared_github_service;
    http_app_state_inner.pr_poller_registry = shared_pr_poller_registry;
    // INVARIANT: streaming_state_cache uses Arc internally; clone shares the same cache.
    http_app_state_inner.streaming_state_cache = app_state.streaming_state_cache.clone();
    http_app_state_inner.webhook_publisher = app_state.webhook_publisher.clone();
    http_app_state_inner.session_merge_locks = Arc::clone(&app_state.session_merge_locks);
    // INVARIANT: both AppStates share native focus transitions and notification coalescing.
    http_app_state_inner.window_focus_state = shared_window_focus_state;
    http_app_state_inner.notification_service_cache = shared_notification_service_cache;
    // INVARIANT: Tauri commands and HTTP/MCP handlers enforce the same live capability state.
    http_app_state_inner.agent_capability_gate = shared_agent_capability_gate;
    // INVARIANT: both graphs share one managed-Team authority (sessions, roster,
    // run bindings, startup barrier); separate instances would split barrier state.
    http_app_state_inner.managed_team = Arc::clone(&app_state.managed_team);
    share_startup_coordinator(app_state, &mut http_app_state_inner);
    share_plan_verification_runtime(app_state, &mut http_app_state_inner);
    // INVARIANT: notification_repo and notification_settings_repo must stay on this shared
    // connection; a per-connection refactor would silently split notification storage.
    Ok(Arc::new(http_app_state_inner))
}

pub(crate) fn share_plan_verification_runtime(source: &AppState, target: &mut AppState) {
    // INVARIANT: automatic stream finalization and manual HTTP admission serialize together.
    target.plan_verification_locks = Arc::clone(&source.plan_verification_locks);
    target.plan_verification_admissions = Arc::clone(&source.plan_verification_admissions);
}

pub(crate) fn share_startup_coordinator(source: &AppState, target: &mut AppState) {
    // INVARIANT: both managed AppState graphs consult the same startup attempt.
    target.startup_coordinator = Arc::clone(&source.startup_coordinator);
}

/// Registers the dynamically constructed AppState exactly once after the
/// blocking bootstrap worker succeeds.
pub fn register_managed_state<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    app_state: AppState,
    coordinator: &StartupCoordinator,
    attempt_id: u64,
) -> AppResult<()> {
    let throttled_emitter =
        crate::application::ThrottledEmitter::new(Arc::clone(&app_state.events));
    let window_focus_state = Arc::clone(&app_state.window_focus_state);
    let mut registration_error = None;
    coordinator
        .register_app_state(attempt_id, |effects| {
            if !app_handle.manage(throttled_emitter) {
                registration_error =
                    Some("ThrottledEmitter was already registered during startup".to_string());
                return false;
            }
            effects.record_side_effect();

            if !app_handle.manage(window_focus_state) {
                registration_error =
                    Some("WindowFocusState was already registered during startup".to_string());
                return false;
            }
            effects.record_side_effect();

            if !app_handle.manage(ExternalMcpHandle::new()) {
                registration_error =
                    Some("ExternalMcpHandle was already registered during startup".to_string());
                return false;
            }
            effects.record_side_effect();

            if !app_handle.manage(app_state) {
                registration_error =
                    Some("AppState was already registered during startup".to_string());
                return false;
            }
            effects.record_side_effect();
            true
        })
        .map_err(|error| {
            AppError::Infrastructure(registration_error.unwrap_or_else(|| error.to_string()))
        })?;

    Ok(())
}
