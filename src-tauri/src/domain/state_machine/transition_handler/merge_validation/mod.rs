// Merge validation: post-merge validation gate
//
// Extracted from side_effects.rs — runs project analysis commands to verify merge correctness.
// Decomposed into setup phase, validate phase, and orchestrator.

mod logging;
mod install;
mod metadata;
mod setup;
mod validate;

/// Delay before retrying a failed install command (ms).
/// Covers macOS filesystem lock recovery window (Spotlight indexing, npm `ENOTEMPTY` errors).
/// 500ms is sufficient for the realistic lock window while keeping latency low.
pub(super) const INSTALL_RETRY_DELAY_MS: u64 = 500;

/// Delay before retrying a failed validation command (ms).
/// Covers transient compilation timeouts and file-system lock windows.
/// 2 seconds allows caches to settle while keeping merge latency acceptable.
pub(super) const VALIDATE_RETRY_DELAY_MS: u64 = 2000;

/// Status string for failed validation/install log entries.
/// Used in ValidationLogEntry.status and compared in retry logic.
const STATUS_FAILED: &str = "failed";

use std::path::Path;

use tauri::Emitter;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::domain::entities::{
    merge_progress_event::{
        derive_phases_from_analysis, MergePhase, MergePhaseStatus, PhaseAnalysisEntry,
    },
    Project, Task,
};

use crate::infrastructure::tool_paths::resolve_shell_cli_path;
use crate::utils::truncate_str;

pub(crate) use metadata::format_validation_error_metadata;
pub(crate) use metadata::{
    extract_cached_validation, format_validation_warn_metadata, take_skip_validation_flag,
};
pub(crate) use logging::{cleanup_validation_logs, emit_merge_progress};
#[cfg(test)]
pub(crate) use logging::validation_log_dir;
#[cfg(test)]
pub(crate) use install::run_install_phase;
pub use install::run_pre_execution_setup;
#[cfg(test)]
pub(crate) use setup::{parse_symlink_command, try_handle_symlink_idempotent};
use setup::run_setup_phase;
use validate::run_validate_phase;

/// Outcome of a cancellable shell command execution.
///
/// Replaces the nested `Result<Result<Output, io::Error>, JoinError>` from
/// `spawn_blocking` + `Command::output()` with a flat enum that adds explicit
/// cancellation support via `CancellationToken`.
pub(crate) enum CancellableCommandResult {
    /// Process completed normally (may have succeeded or failed — check `output.status`).
    Completed(std::process::Output),
    /// Failed to spawn the process or read its output.
    SpawnError(std::io::Error),
    /// Process was cancelled via the cancellation token.
    /// Child process tree has been killed.
    Cancelled,
}

/// Spawn a shell command with cancellation support.
///
/// Uses `tokio::process::Command` (async, non-blocking) instead of
/// `std::process::Command` inside `spawn_blocking` (blocking, uncancellable).
/// The process can be killed cooperatively via the `CancellationToken`, or
/// externally via PID-based kill (e.g., `kill_worktree_processes`).
///
/// `kill_on_drop(true)` provides a safety net: if the `Child` is dropped
/// without explicit cleanup, the OS sends SIGKILL to the process.
pub(crate) async fn spawn_cancellable_command(
    cmd: &str,
    cwd: &Path,
    cancel: &CancellationToken,
) -> CancellableCommandResult {
    let mut child = match tokio::process::Command::new(resolve_shell_cli_path())
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return CancellableCommandResult::SpawnError(e),
    };

    // Capture PID before taking stdout/stderr handles.
    let pid = child.id();

    // Take stdout/stderr handles so we can read them concurrently with wait().
    // This avoids the deadlock that occurs when a child writes more data than
    // the pipe buffer can hold while we're waiting for exit (child blocks on
    // write, we block on wait — neither makes progress).
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    // Read stdout/stderr concurrently with wait to avoid pipe-buffer deadlock.
    // If the child writes >64KB and nobody drains the pipe, the child blocks on
    // write and wait() never returns. By draining in parallel, both make progress.
    let stdout_fut = async {
        let mut buf = Vec::new();
        if let Some(mut out) = stdout_handle {
            let _ = out.read_to_end(&mut buf).await;
        }
        buf
    };
    let stderr_fut = async {
        let mut buf = Vec::new();
        if let Some(mut err) = stderr_handle {
            let _ = err.read_to_end(&mut buf).await;
        }
        buf
    };

    tokio::select! {
        biased; // Check cancellation first on each poll

        _ = cancel.cancelled() => {
            // Kill the process tree: children first, then the shell itself.
            if let Some(id) = pid {
                crate::domain::services::kill_process(id);
            }
            // Explicit kill + reap to avoid zombie processes.
            let _ = child.kill().await;
            let _ = child.wait().await;
            CancellableCommandResult::Cancelled
        }

        (status, stdout_bytes, stderr_bytes) = async {
            tokio::join!(child.wait(), stdout_fut, stderr_fut)
        } => {
            match status {
                Ok(exit_status) => {
                    CancellableCommandResult::Completed(std::process::Output {
                        status: exit_status,
                        stdout: stdout_bytes,
                        stderr: stderr_bytes,
                    })
                }
                Err(e) => CancellableCommandResult::SpawnError(e),
            }
        }
    }
}

/// Analysis entry for path-scoped build/validation commands.
/// Mirrors the HTTP handler's AnalysisEntry but kept local to avoid cross-module coupling.
#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct MergeAnalysisEntry {
    pub(super) path: String,
    #[allow(dead_code)]
    pub(super) label: String,
    #[serde(default)]
    pub(super) validate: Vec<String>,
    #[serde(default)]
    pub(super) worktree_setup: Vec<String>,
}

/// Analysis entry for pre-execution setup commands.
/// Includes the `install` field (unlike MergeAnalysisEntry which omits it).
#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct PreExecAnalysisEntry {
    pub(super) path: String,
    pub(super) label: String,
    #[serde(default)]
    pub(super) install: Option<String>,
    #[serde(default)]
    pub(super) worktree_setup: Vec<String>,
}

/// A single validation command execution record for streaming + storage.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ValidationLogEntry {
    pub phase: String,
    pub command: String,
    pub path: String,
    pub label: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    /// Whether this command was retried after initial failure.
    /// True = passed/failed on automatic retry (attempt 2/2).
    #[serde(default)]
    pub retried: bool,
    /// Path to full stdout log file (only set for failed commands)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_log_path: Option<String>,
    /// Path to full stderr log file (only set for failed commands)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_log_path: Option<String>,
}

impl ValidationLogEntry {
    /// Create a new ValidationLogEntry with all required fields.
    /// Sets retried=false, stdout_log_path=None, stderr_log_path=None by default.
    pub(super) fn new(
        phase: impl Into<String>,
        command: impl Into<String>,
        path: impl Into<String>,
        label: impl Into<String>,
        status: impl Into<String>,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        duration_ms: u64,
    ) -> Self {
        Self {
            phase: phase.into(),
            command: command.into(),
            path: path.into(),
            label: label.into(),
            status: status.into(),
            exit_code,
            stdout,
            stderr,
            duration_ms,
            retried: false,
            stdout_log_path: None,
            stderr_log_path: None,
        }
    }

    /// Attach full log file paths for a failed command.
    ///
    /// Writes full stdout/stderr under the app-owned log root and stores
    /// the paths on the entry so the fixer agent can read untruncated output.
    pub(crate) fn attach_failure_logs(&mut self, task_id: &str) {
        let (stdout_path, stderr_path) =
            logging::write_failure_logs(task_id, &self.command, &self.stdout, &self.stderr);
        self.stdout_log_path = stdout_path;
        self.stderr_log_path = stderr_path;
    }
}

/// Result of running post-merge validation commands.
#[derive(Debug)]
pub(crate) struct ValidationResult {
    pub(crate) all_passed: bool,
    pub(crate) failures: Vec<ValidationFailure>,
    pub(crate) log: Vec<ValidationLogEntry>,
}

#[derive(Debug)]
pub(crate) struct ValidationFailure {
    pub(super) command: String,
    pub(super) path: String,
    pub(super) exit_code: Option<i32>,
    pub(super) stderr: String,
}

/// Result of running pre-execution setup commands.
#[derive(Debug)]
pub struct PreExecSetupResult {
    pub success: bool,
    pub log: Vec<ValidationLogEntry>,
}

/// Truncate a string to `max_len` bytes, appending "... (truncated)" if needed.
fn truncate_output(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}... (truncated)", truncate_str(s, max_len))
    }
}

/// Directory for validation log files under the app-owned log root.
/// Load effective analysis, resolve template vars, and run all validate commands.
///
/// Returns `None` if no analysis entries exist (backward compatible — skip validation).
/// Returns `Some(ValidationResult)` with pass/fail details otherwise.
///
/// When `app_handle` is `Some`, emits `merge:validation_step` events for real-time UI streaming.
/// All executed commands are recorded in `ValidationResult::log` for metadata storage.
///
/// When `cached_log` is provided, validate-phase commands that previously passed (status
/// "success" or "cached") are skipped and emitted as "cached" instead of re-running.
/// Setup-phase commands always re-run. Previously-failed commands always re-run.
pub(crate) async fn run_validation_commands(
    project: &Project,
    task: &Task,
    merge_cwd: &Path,
    task_id_str: &str,
    app_handle: Option<&tauri::AppHandle>,
    cached_log: Option<&[ValidationLogEntry]>,
    validation_mode: &crate::domain::entities::MergeValidationMode,
    cancel: &CancellationToken,
) -> Option<ValidationResult> {
    let overall_start = std::time::Instant::now();
    tracing::info!(
        task_id = task_id_str,
        cwd = %merge_cwd.display(),
        "run_validation_commands: starting validation"
    );

    // Load effective analysis: custom_analysis ?? detected_analysis
    let analysis_json = project
        .custom_analysis
        .as_ref()
        .or(project.detected_analysis.as_ref())?;

    let entries: Vec<MergeAnalysisEntry> = match serde_json::from_str(analysis_json) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to parse project analysis JSON, skipping validation");
            return None;
        }
    };

    if entries.is_empty() {
        return None;
    }

    // Emit dynamic phase list to frontend for timeline rendering.
    // Convert MergeAnalysisEntry → PhaseAnalysisEntry (only validate commands needed).
    // Derive phase list and store in hydration map + emit to frontend
    {
        let phase_entries: Vec<PhaseAnalysisEntry> = entries
            .iter()
            .map(|e| PhaseAnalysisEntry {
                validate: e.validate.clone(),
            })
            .collect();
        let phases = derive_phases_from_analysis(&phase_entries);

        // Always store in hydration map (even without app_handle)
        crate::domain::entities::merge_progress_event::store_merge_phase_list(
            task_id_str,
            phases.clone(),
        );

        if let Some(handle) = app_handle {
            let _ = handle.emit(
                "task:merge_phases",
                serde_json::json!({
                    "task_id": task_id_str,
                    "phases": phases,
                }),
            );
        }
    }

    // Acquire a worktree permit so cleanup_branch_and_worktree_internal knows
    // this worktree is in active use and should not be deleted underneath us.
    // The permit is RAII — it auto-releases when this function returns.
    let _worktree_permit = crate::domain::services::acquire_worktree_permit(merge_cwd);

    // NOTE: Previously, hardening code here would merge worktree_setup from
    // detected_analysis into custom_analysis entries that had empty worktree_setup.
    // This was removed because it treated intentionally empty worktree_setup []
    // (e.g., user explicitly removing target/ symlinks) as "not configured" and
    // overrode the user's choice. If custom_analysis exists, trust it fully.

    // Build template resolver
    let project_root = &project.working_directory;
    let worktree_path = merge_cwd.to_str().unwrap_or(project_root);
    let task_branch = task.task_branch.as_deref().unwrap_or("");

    let project_root_owned = project_root.clone();
    let worktree_path_owned = worktree_path.to_string();
    let task_branch_owned = task_branch.to_string();

    let resolve = |s: &str| -> String {
        s.replace("{project_root}", &project_root_owned)
            .replace("{worktree_path}", &worktree_path_owned)
            .replace("{task_branch}", &task_branch_owned)
    };

    // Phase 1: Setup — skip when merge_cwd == project_root (no worktree created).
    // Running symlink setup with {worktree_path} == {project_root} would create
    // circular symlinks (source == target), potentially destroying node_modules/target.
    let skip_setup = worktree_path == project_root.as_str();
    let (mut log, _setup_had_failures) = if skip_setup {
        tracing::info!(
            task_id = task_id_str,
            "Skipping worktree setup phase: merge_cwd equals project_root (no worktree)"
        );
        // Emit Passed event so the frontend shows a checkmark for WorktreeSetup
        emit_merge_progress(
            app_handle,
            task_id_str,
            MergePhase::worktree_setup(),
            MergePhaseStatus::Passed,
            "Worktree setup skipped (no worktree needed)".to_string(),
        );
        (Vec::new(), false)
    } else {
        run_setup_phase(
            &entries,
            merge_cwd,
            task_id_str,
            app_handle,
            &resolve,
            None,
            cancel,
        )
        .await
    };

    // Phase 2: Validate
    let (validate_log, failures, ran_any) = run_validate_phase(
        &entries,
        merge_cwd,
        task_id_str,
        app_handle,
        cached_log,
        &resolve,
        validation_mode,
        cancel,
    )
    .await;

    log.extend(validate_log);

    if !ran_any {
        let overall_duration_ms = overall_start.elapsed().as_millis() as u64;
        tracing::info!(
            task_id = task_id_str,
            duration_ms = overall_duration_ms,
            "run_validation_commands: no validation commands to run"
        );
        return None;
    }

    let overall_duration_ms = overall_start.elapsed().as_millis() as u64;
    let all_passed = failures.is_empty();
    tracing::info!(
        task_id = task_id_str,
        duration_ms = overall_duration_ms,
        all_passed = all_passed,
        failure_count = failures.len(),
        total_commands = log.len(),
        "run_validation_commands: completed validation"
    );

    // Write full output to disk for failed commands so the fixer agent
    // can read untruncated logs.
    if !all_passed {
        for entry in &mut log {
            if entry.status == STATUS_FAILED {
                entry.attach_failure_logs(task_id_str);
            }
        }
    }

    Some(ValidationResult {
        all_passed,
        failures,
        log,
    })
}
