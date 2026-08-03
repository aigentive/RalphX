use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;

use crate::application::startup_status::StartupCoordinator;
use crate::application::HttpShutdownHandle;
use crate::commands;
use crate::domain::services::RunningAgentRegistry;
use crate::infrastructure::sqlite::DbConnection;
use crate::infrastructure::ExternalMcpHandle;
use crate::AppState;

pub(crate) struct ExitWatchdog {
    disarmed: Arc<AtomicBool>,
}

impl ExitWatchdog {
    fn arm(deadline: Duration) -> Self {
        Self::arm_with(deadline, || {
            const MESSAGE: &[u8] =
                b"RalphX exit cleanup exceeded its deadline; forcing process exit\n";
            // SAFETY: write(2) and _exit(2) avoid allocator, logging, and atexit
            // locks that may themselves be wedged during process teardown.
            unsafe {
                let _ = libc::write(
                    libc::STDERR_FILENO,
                    MESSAGE.as_ptr().cast::<libc::c_void>(),
                    MESSAGE.len(),
                );
                libc::_exit(1);
            }
        })
    }

    pub(crate) fn arm_with(deadline: Duration, on_fire: impl FnOnce() + Send + 'static) -> Self {
        let disarmed = Arc::new(AtomicBool::new(false));
        let thread_flag = Arc::clone(&disarmed);
        std::thread::spawn(move || {
            std::thread::sleep(deadline);
            if !thread_flag.load(Ordering::SeqCst) {
                on_fire();
            }
        });
        Self { disarmed }
    }

    pub(crate) fn disarm(&self) {
        self.disarmed.store(true, Ordering::SeqCst);
    }
}

pub fn handle_run_event<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    event: &tauri::RunEvent,
) {
    match event {
        // Fires first — before the window actually closes. Kick off HTTP
        // server drain immediately so axum has the maximum window to close
        // idle keep-alive sockets and finish in-flight requests before the
        // process is reaped. Do NOT prevent the exit — we want it to proceed.
        tauri::RunEvent::ExitRequested { .. } => {
            trigger_startup_cancellation(app_handle);
            trigger_http_shutdown(app_handle);
            release_remote_serve_mapping(app_handle);
        }
        // Final exit. Re-fire the HTTP shutdown trigger as a safety net in
        // case ExitRequested didn't fire on this code path (idempotent), then
        // do the existing child-process / WAL cleanup.
        tauri::RunEvent::Exit => {
            trigger_startup_cancellation(app_handle);
            trigger_http_shutdown(app_handle);
            run_exit_cleanup(app_handle);
        }
        _ => {}
    }
}

/// A clean quit must not leave the tailnet forward pointing at a port this process no longer
/// owns (§5.3 item 2). Bounded and best-effort; the crash path stays covered by the
/// start-time sweep.
fn release_remote_serve_mapping<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    if let Some(handle) = app_handle.try_state::<crate::remote_server::RemoteListenerHandle>() {
        crate::remote_server::release_serve_mapping_on_exit(handle.inner());
    }
}

pub(crate) fn trigger_startup_cancellation<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    if let Some(coordinator) = app_handle.try_state::<Arc<StartupCoordinator>>() {
        coordinator.cancel();
        tracing::info!("Cancelled active startup attempt");
    }
}

/// `pub(crate)` so the sidecar test in `shutdown_tests.rs` can exercise the
/// handle-present vs handle-missing branches directly without having to
/// construct a `RunEvent::ExitRequested` (whose `api` field has no public
/// constructor in Tauri 2.x).
pub(crate) fn trigger_http_shutdown<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    if let Some(handle) = app_handle.try_state::<HttpShutdownHandle>() {
        handle.trigger();
        tracing::info!("Triggered HTTP server graceful shutdown");
    } else {
        // Tests or early-exit paths may run without the HTTP server registered.
        tracing::debug!("HttpShutdownHandle not registered; skipping HTTP drain");
    }
}

fn run_exit_cleanup<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    // Arm a fixed fallback before the lazy config accessor can perform any I/O.
    let bootstrap_watchdog = ExitWatchdog::arm(Duration::from_secs(
        crate::infrastructure::agents::claude::ShutdownConfig::default().watchdog_deadline_secs,
    ));
    let configured_deadline =
        crate::infrastructure::agents::claude::shutdown_config().watchdog_deadline_secs;
    let deadline_secs =
        crate::infrastructure::agents::claude::bounded_shutdown_watchdog_deadline_secs(
            configured_deadline,
        );
    let watchdog = ExitWatchdog::arm(Duration::from_secs(deadline_secs));
    bootstrap_watchdog.disarm();

    // Set shutdown flag before killing agents so stream handlers can skip escalation.
    if let Some(exec_state) = app_handle.try_state::<Arc<commands::ExecutionState>>() {
        exec_state.is_shutting_down.store(true, Ordering::SeqCst);
    }

    let Some(app_state) = app_handle.try_state::<AppState>() else {
        tracing::debug!("AppState not registered; skipping AppState exit cleanup");
        shutdown_external_mcp(app_handle);
        watchdog.disarm();
        return;
    };

    let registry = Arc::clone(&app_state.running_agent_registry);
    let interactive = Arc::clone(&app_state.interactive_process_registry);
    let terminal_service = Arc::clone(&app_state.agent_terminal_service);
    let db = app_state.db.clone();

    run_exit_steps(
        move || shutdown_agent_terminals(terminal_service),
        move || shutdown_agents(registry, interactive),
        || shutdown_external_mcp(app_handle),
        move || checkpoint_wal(db),
    );
    watchdog.disarm();
}

pub(crate) fn run_exit_steps(
    shutdown_terminals: impl FnOnce(),
    shutdown_agents: impl FnOnce(),
    shutdown_external_mcp: impl FnOnce(),
    checkpoint_wal: impl FnOnce(),
) {
    shutdown_terminals();
    shutdown_agents();
    shutdown_external_mcp();
    checkpoint_wal();
}

fn shutdown_agent_terminals(terminal_service: Arc<crate::application::AgentTerminalService>) {
    tauri::async_runtime::block_on(async {
        terminal_service.close_all().await;
    });
}

fn shutdown_agents(
    registry: Arc<dyn RunningAgentRegistry>,
    interactive: Arc<crate::application::InteractiveProcessRegistry>,
) {
    tauri::async_runtime::block_on(async {
        let _ = tokio::time::timeout(Duration::from_millis(2500), async move {
            let ipr_dump = interactive.dump_state().await;
            tracing::info!(
                count = ipr_dump.len(),
                "[IPR_EXIT_DUMP] IPR entries at shutdown: {:?}",
                ipr_dump
            );
            interactive.clear().await;
            let stopped = registry.stop_all().await;
            // Reap tracked head-process maps for both harnesses. Each helper
            // sends SIGTERM to the spawn's process group (setsid-isolated),
            // gives it a short grace window, then SIGKILLs the group — so
            // the stdio MCP server gets a chance to close keep-alive sockets
            // cleanly instead of being orphaned mid-burst.
            crate::infrastructure::agents::claude::kill_all_tracked_processes().await;
            crate::infrastructure::agents::codex::kill_all_tracked_processes().await;
            if !stopped.is_empty() {
                tracing::info!(count = stopped.len(), "Killed running agents on app exit");
            }
        })
        .await;
    });
}

fn shutdown_external_mcp<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    let Some(handle) = app_handle.try_state::<ExternalMcpHandle>() else {
        return;
    };
    if let Some(supervisor) = handle.get() {
        supervisor.shutdown_blocking();
    }
}

fn checkpoint_wal(db: DbConnection) {
    tauri::async_runtime::block_on(async {
        let checkpoint_result = db
            .run(|conn| {
                conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                    .map_err(|e| {
                        crate::error::AppError::Database(format!("WAL checkpoint failed: {e}"))
                    })
            })
            .await;
        if let Err(e) = checkpoint_result {
            tracing::warn!(error = %e, "WAL checkpoint on exit failed");
        }
    });
}
