// RalphX - Autonomous AI-driven development system
// Tauri 2.0 backend with clean architecture

// Allow clippy lints for patterns used throughout the codebase
#![allow(clippy::derivable_impls)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::single_match)]
#![allow(clippy::type_complexity)]
#![allow(clippy::identity_op)]
#![allow(clippy::comparison_to_empty)]
#![allow(clippy::needless_lifetimes)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::unnecessary_literal_unwrap)]
#![allow(clippy::bool_comparison)]
#![allow(clippy::while_let_loop)]
#![allow(clippy::const_is_empty)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unwrap_or_default)]
#![allow(clippy::unnecessary_map_or)]

// Core modules
pub mod application;
pub mod commands;
pub mod domain;
pub mod error;
pub mod http_server;
pub mod infrastructure;
pub mod shell;
pub mod testing;
pub mod utils;

// Re-export common types
pub use application::AppState;
pub use error::{AppError, AppResult};

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::{Duration, Instant};

use shell::app_setup::run_app_setup;
use shell::startup_bootstrap::initialize_process_bootstrap;
use application::startup_status::StartupCoordinator;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// Wait for the local HTTP backend at `port` to respond with HTTP 200 on `/health`.
/// Retries every 200ms until `timeout` elapses (2s per-request timeout).
pub(crate) async fn wait_for_backend_ready(port: u16, timeout: Duration) -> Result<(), String> {
    wait_for_backend_ready_with_probe(port, timeout, probe_backend_health).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendReadyProbeResult {
    Ready,
    HttpStatus(u16),
    Unreachable,
}

async fn wait_for_backend_ready_with_probe<F, Fut>(
    port: u16,
    timeout: Duration,
    mut probe: F,
) -> Result<(), String>
where
    F: FnMut(u16) -> Fut,
    Fut: std::future::Future<Output = BackendReadyProbeResult>,
{
    let start = Instant::now();
    let mut logged_non_200 = false;
    let mut logged_conn_refused = false;
    loop {
        if start.elapsed() > timeout {
            return Err(format!("Backend :{port} not ready after {timeout:?}"));
        }

        match probe(port).await {
            BackendReadyProbeResult::Ready => return Ok(()),
            BackendReadyProbeResult::HttpStatus(status) => {
                logged_conn_refused = false;
                if !logged_non_200 {
                    tracing::debug!(
                        "Backend :{port} /health returned status {status} (expected 200), retrying"
                    );
                    logged_non_200 = true;
                }
            }
            BackendReadyProbeResult::Unreachable => {
                if !logged_conn_refused {
                    tracing::debug!("Backend :{port} not yet accepting connections, retrying");
                    logged_conn_refused = true;
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn probe_backend_health(port: u16) -> BackendReadyProbeResult {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = format!("127.0.0.1:{port}");
    let conn = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect(&addr),
    )
    .await;
    match conn {
        Ok(Ok(mut stream)) => {
            let req = "GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
            if tokio::time::timeout(Duration::from_secs(2), stream.write_all(req.as_bytes()))
                .await
                .is_ok()
            {
                let mut buf = [0u8; 1024];
                if let Ok(Ok(n)) =
                    tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf)).await
                {
                    let response = std::str::from_utf8(&buf[..n]).unwrap_or("");
                    let status = response
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u16>().ok())
                        .unwrap_or(0);
                    if status == 200 {
                        return BackendReadyProbeResult::Ready;
                    }
                    return BackendReadyProbeResult::HttpStatus(status);
                }
            }

            BackendReadyProbeResult::Unreachable
        }
        _ => BackendReadyProbeResult::Unreachable,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _log_guard = initialize_process_bootstrap();

    // Create execution state for global execution control
    let execution_state = Arc::new(commands::ExecutionState::new());
    // Create active project state for per-project execution scoping (Phase 82)
    let active_project_state = Arc::new(commands::ActiveProjectState::new());
    // This process-local authority must exist before Tauri setup so the
    // lightweight bootstrap surface can always poll a truthful status.
    let startup_coordinator = Arc::new(StartupCoordinator::new());
    crate::infrastructure::sqlite::db_connection::register_startup_boot_id(
        &startup_coordinator.snapshot().boot_id,
    );
    // Clone for usage inside setup closure before closure borrows them
    let init_execution_state = Arc::clone(&execution_state);
    let startup_execution_state = Arc::clone(&execution_state);
    let startup_active_project_state = Arc::clone(&active_project_state);
    let http_execution_state = Arc::clone(&execution_state);
    let init_startup_coordinator = Arc::clone(&startup_coordinator);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(tauri_plugin_window_state::StateFlags::all())
                .build()
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .menu(shell::native_menu::build_app_menu)
        .on_menu_event(shell::native_menu::handle_menu_event)
        .manage(startup_coordinator)
        .manage(execution_state)
        .manage(active_project_state)
        .setup(move |app| {
            run_app_setup(
                app,
                Arc::clone(&init_execution_state),
                Arc::clone(&startup_execution_state),
                Arc::clone(&startup_active_project_state),
                Arc::new(|app_state, execution_state| {
                    Some(Arc::new(
                        commands::unified_chat_commands::AgentWorkspacePrFixReviewPublishCommandResumer {
                            app_state: app_state.clone(),
                            execution_state,
                        },
                    )
                        as Arc<
                            dyn application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
                        >)
                }),
                Arc::clone(&http_execution_state),
                Arc::clone(&init_startup_coordinator),
            )
        })
        .invoke_handler(crate::register_tauri_commands!())
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            #[cfg(all(dev, target_os = "macos"))]
            if matches!(&event, tauri::RunEvent::Ready) {
                shell::dev_dock_icon::set_light_dev_dock_icon();
            }

            shell::shutdown::handle_run_event(app_handle, &event);
        });
}
