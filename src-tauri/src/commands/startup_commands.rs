use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::application::startup_status::{
    StartupAttemptLauncher, StartupCoordinator, StartupFailureCode, StartupFrontendMilestone,
    StartupSnapshot, StartupStage,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportStartupFrontendMilestoneInput {
    pub boot_id: String,
    pub attempt_id: u64,
    pub milestone: StartupFrontendMilestone,
}

/// Failure metadata that is safe for the bootstrap UI to display or attach to
/// a support report. It intentionally excludes raw error strings, paths, and
/// the per-process boot identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct StartupDiagnostics {
    pub attempt_id: u64,
    pub stage: StartupStage,
    pub message_code: String,
    pub failure_code: Option<StartupFailureCode>,
    pub can_retry: bool,
}

pub(crate) fn build_startup_diagnostics(snapshot: StartupSnapshot) -> StartupDiagnostics {
    StartupDiagnostics {
        attempt_id: snapshot.attempt_id,
        stage: snapshot.stage,
        message_code: snapshot.message_code,
        failure_code: snapshot.failure_code,
        can_retry: snapshot.retry_allowed,
    }
}

pub(crate) fn retry_startup_with_launcher(
    coordinator: &StartupCoordinator,
    launch: impl FnOnce(u64),
) -> Result<StartupSnapshot, String> {
    let attempt_id = coordinator
        .begin_retry()
        .map_err(|error| error.to_string())?;
    launch(attempt_id);
    Ok(coordinator.snapshot())
}

pub(crate) fn startup_runtime_log_directory() -> PathBuf {
    crate::utils::runtime_log_paths::app_log_dir()
}

pub(crate) fn open_startup_log_directory(
    open_directory: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let log_directory = startup_runtime_log_directory();
    open_directory(&log_directory)
}

/// Returns the process-local startup snapshot for polling by the lightweight
/// bootstrap root. This never reads SQLite or derives readiness from the UI.
#[tauri::command]
pub fn get_startup_status(state: State<'_, Arc<StartupCoordinator>>) -> StartupSnapshot {
    state.snapshot()
}

/// Returns a deliberately redacted diagnostic view for startup recovery UI.
#[tauri::command]
pub fn get_startup_diagnostics(state: State<'_, Arc<StartupCoordinator>>) -> StartupDiagnostics {
    build_startup_diagnostics(state.snapshot())
}

/// Starts a fresh pre-registration attempt after a terminal startup failure.
#[tauri::command]
pub(crate) fn retry_startup(
    coordinator: State<'_, Arc<StartupCoordinator>>,
    launcher: State<'_, Arc<StartupAttemptLauncher>>,
) -> Result<StartupSnapshot, String> {
    retry_startup_with_launcher(coordinator.inner(), |attempt_id| {
        launcher.launch(attempt_id)
    })
}

/// Opens the RalphX-owned runtime log directory. The frontend supplies no
/// path, so the process never opens an arbitrary project or user filesystem
/// location from this startup recovery surface.
#[tauri::command]
pub fn open_startup_logs(app_handle: tauri::AppHandle) -> Result<(), String> {
    open_startup_log_directory(|log_directory| {
        app_handle
            .opener()
            .open_path(log_directory.to_string_lossy().into_owned(), None::<String>)
            .map_err(|error| format!("Failed to open RalphX runtime logs: {error}"))
    })
}

/// Accepts a typed frontend milestone only for the current runtime-ready boot.
#[tauri::command]
pub fn report_startup_frontend_milestone(
    input: ReportStartupFrontendMilestoneInput,
    state: State<'_, Arc<StartupCoordinator>>,
) -> Result<(), String> {
    state
        .accept_frontend_milestone(&input.boot_id, input.attempt_id, input.milestone)
        .map_err(|error| error.to_string())
}
