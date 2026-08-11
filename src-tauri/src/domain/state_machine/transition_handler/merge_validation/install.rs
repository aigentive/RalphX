use std::path::Path;

use tokio_util::sync::CancellationToken;

use crate::domain::entities::{Project, Task};

use super::{
    setup::run_setup_phase, spawn_cancellable_command, truncate_output, CancellableCommandResult,
    MergeAnalysisEntry, PreExecAnalysisEntry, PreExecSetupResult, ValidationLogEntry,
    INSTALL_RETRY_DELAY_MS, STATUS_FAILED,
};

fn node_modules_available_for_install_skip(nm_path: &Path) -> bool {
    if nm_path.is_symlink() {
        let Ok(target) = std::fs::read_link(nm_path) else {
            tracing::warn!(
                path = %nm_path.display(),
                "node_modules symlink could not be read; running install"
            );
            return false;
        };
        let resolved_target = if target.is_absolute() {
            target
        } else {
            nm_path
                .parent()
                .map(|parent| parent.join(&target))
                .unwrap_or(target)
        };
        if resolved_target.exists() {
            return true;
        }

        tracing::warn!(
            path = %nm_path.display(),
            target = %resolved_target.display(),
            "Removing broken node_modules symlink before install"
        );
        if let Err(error) = std::fs::remove_file(nm_path) {
            tracing::warn!(
                path = %nm_path.display(),
                error = %error,
                "Failed to remove broken node_modules symlink before install"
            );
        }
        return false;
    }

    nm_path.exists()
}

/// Run install commands for pre-execution setup.
/// Returns (log_entries, had_failures).
pub(crate) async fn run_install_phase(
    entries: &[PreExecAnalysisEntry],
    exec_cwd: &Path,
    task_id_str: &str,
    event_sink: Option<&dyn ralphx_events::EventSink>,
    resolve: &(dyn Fn(&str) -> String + Send + Sync),
    context: &str,
    cancel: &CancellationToken,
) -> (Vec<ValidationLogEntry>, bool) {
    let mut log: Vec<ValidationLogEntry> = Vec::new();
    let mut install_had_failures = false;

    for entry in entries {
        let Some(ref cmd_str) = entry.install else {
            continue;
        };

        let resolved_cmd = resolve(cmd_str);
        let resolved_path = resolve(&entry.path);
        let cmd_cwd = if resolved_path == "." {
            exec_cwd.to_path_buf()
        } else {
            exec_cwd.join(&resolved_path)
        };

        // Skip install if node_modules already exists (symlink from setup phase or prior install)
        let nm_path = cmd_cwd.join("node_modules");
        if node_modules_available_for_install_skip(&nm_path) {
            tracing::info!(
                command = %resolved_cmd,
                cwd = %cmd_cwd.display(),
                is_symlink = nm_path.is_symlink(),
                "Skipping install: node_modules already exists"
            );
            log.push(ValidationLogEntry {
                phase: "install".to_string(),
                command: resolved_cmd.clone(),
                path: resolved_path.clone(),
                label: entry.label.clone(),
                status: "skipped".to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: "node_modules already exists — install skipped".to_string(),
                duration_ms: 0,
                ..Default::default()
            });
            continue;
        }

        // Emit "running" event before execution
        if let Some(sink) = event_sink {
            sink.emit(
                "merge:validation_step",
                serde_json::json!({
                    "task_id": task_id_str,
                    "phase": "install",
                    "command": resolved_cmd,
                    "path": resolved_path,
                    "label": entry.label,
                    "status": "running",
                    "context": context,
                }),
            );
        }

        tracing::info!(
            command = %resolved_cmd,
            cwd = %cmd_cwd.display(),
            "Running pre-execution install command"
        );

        let start = std::time::Instant::now();

        let result = spawn_cancellable_command(&resolved_cmd, &cmd_cwd, cancel).await;

        let mut log_entry = match result {
            CancellableCommandResult::Completed(output) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr_raw = String::from_utf8_lossy(&output.stderr).to_string();
                let status = if output.status.success() {
                    "success"
                } else {
                    "failed"
                };

                if !output.status.success() {
                    tracing::warn!(
                        command = %resolved_cmd,
                        stderr = %stderr_raw,
                        "Pre-execution install command failed"
                    );
                }

                ValidationLogEntry {
                    phase: "install".to_string(),
                    command: resolved_cmd.clone(),
                    path: resolved_path.clone(),
                    label: entry.label.clone(),
                    status: status.to_string(),
                    exit_code: output.status.code(),
                    stdout: truncate_output(&stdout_raw, 2000),
                    stderr: truncate_output(&stderr_raw, 2000),
                    duration_ms,
                    ..Default::default()
                }
            }
            CancellableCommandResult::SpawnError(e) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                tracing::warn!(
                    command = %resolved_cmd,
                    error = %e,
                    "Pre-execution install command failed"
                );

                ValidationLogEntry {
                    phase: "install".to_string(),
                    command: resolved_cmd.clone(),
                    path: resolved_path.clone(),
                    label: entry.label.clone(),
                    status: STATUS_FAILED.to_string(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: truncate_output(&format!("Failed to execute: {}", e), 2000),
                    duration_ms,
                    ..Default::default()
                }
            }
            CancellableCommandResult::Cancelled => {
                let duration_ms = start.elapsed().as_millis() as u64;
                tracing::warn!(
                    command = %resolved_cmd,
                    "Pre-execution install command cancelled"
                );

                ValidationLogEntry {
                    phase: "install".to_string(),
                    command: resolved_cmd.clone(),
                    path: resolved_path.clone(),
                    label: entry.label.clone(),
                    status: STATUS_FAILED.to_string(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: "Command cancelled".to_string(),
                    duration_ms,
                    ..Default::default()
                }
            }
        };

        // Retry once if the install command failed (transient errors like ENOTEMPTY)
        if log_entry.status == STATUS_FAILED {
            tracing::warn!(
                command = %resolved_cmd,
                delay_ms = INSTALL_RETRY_DELAY_MS,
                "Install command failed, retrying after {}ms (attempt 2/2)",
                INSTALL_RETRY_DELAY_MS
            );
            tokio::time::sleep(std::time::Duration::from_millis(INSTALL_RETRY_DELAY_MS)).await;

            // Emit "running" event for retry attempt
            if let Some(sink) = event_sink {
                sink.emit(
                    "merge:validation_step",
                    serde_json::json!({
                        "task_id": task_id_str,
                        "phase": "install",
                        "command": resolved_cmd,
                        "path": resolved_path,
                        "label": entry.label,
                        "status": "running",
                        "context": context,
                    }),
                );
            }

            let retry_start = std::time::Instant::now();
            let retry_result = spawn_cancellable_command(&resolved_cmd, &cmd_cwd, cancel).await;

            if let CancellableCommandResult::Completed(output) = retry_result {
                let duration_ms = retry_start.elapsed().as_millis() as u64;
                let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr_raw = String::from_utf8_lossy(&output.stderr).to_string();
                if output.status.success() {
                    tracing::info!(
                        command = %resolved_cmd,
                        "Install command succeeded on retry (attempt 2/2)"
                    );
                    log_entry = ValidationLogEntry {
                        phase: "install".to_string(),
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        label: entry.label.clone(),
                        status: "success".to_string(),
                        exit_code: output.status.code(),
                        stdout: truncate_output(&stdout_raw, 2000),
                        stderr: truncate_output(&stderr_raw, 2000),
                        duration_ms,
                        ..Default::default()
                    };
                } else {
                    tracing::warn!(
                        command = %resolved_cmd,
                        stderr = %stderr_raw,
                        "Install command retry also failed (attempt 2/2)"
                    );
                    log_entry = ValidationLogEntry {
                        phase: "install".to_string(),
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        label: entry.label.clone(),
                        status: STATUS_FAILED.to_string(),
                        exit_code: output.status.code(),
                        stdout: truncate_output(&stdout_raw, 2000),
                        stderr: truncate_output(&stderr_raw, 2000),
                        duration_ms,
                        ..Default::default()
                    };
                }
            }
            // If spawn failed or was cancelled, keep the original failure log_entry
        }

        // Set failure flag based on final outcome
        if log_entry.status == STATUS_FAILED {
            install_had_failures = true;
        }

        if let Some(sink) = event_sink {
            sink.emit(
                "merge:validation_step",
                serde_json::json!({
                    "task_id": task_id_str,
                    "phase": log_entry.phase,
                    "command": log_entry.command,
                    "path": log_entry.path,
                    "label": log_entry.label,
                    "status": log_entry.status,
                    "exit_code": log_entry.exit_code,
                    "stdout": log_entry.stdout,
                    "stderr": log_entry.stderr,
                    "duration_ms": log_entry.duration_ms,
                    "context": context,
                }),
            );
        }
        log.push(log_entry);
    }

    (log, install_had_failures)
}

/// Load effective analysis, resolve template vars, and run pre-execution setup commands.
///
/// Returns `None` if no analysis entries exist (backward compatible — skip setup).
/// Returns `Some(PreExecSetupResult)` with success/failure details otherwise.
///
/// When `event_sink` is `Some`, emits `merge:validation_step` events with the provided context
/// for real-time UI streaming. All executed commands are recorded in `PreExecSetupResult::log`
/// for metadata storage.
///
/// This function runs worktree_setup + install commands only. No validate steps.
pub async fn run_pre_execution_setup(
    project: &Project,
    task: &Task,
    exec_cwd: &Path,
    task_id_str: &str,
    event_sink: Option<&dyn ralphx_events::EventSink>,
    context: &str,
    cancel: &CancellationToken,
) -> Option<PreExecSetupResult> {
    let overall_start = std::time::Instant::now();
    tracing::info!(
        task_id = task_id_str,
        cwd = %exec_cwd.display(),
        "run_pre_execution_setup: starting pre-execution setup"
    );

    // Load effective analysis: custom_analysis ?? detected_analysis
    let analysis_json = project
        .custom_analysis
        .as_ref()
        .or(project.detected_analysis.as_ref())?;

    let entries: Vec<PreExecAnalysisEntry> = match serde_json::from_str(analysis_json) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to parse project analysis JSON, skipping pre-execution setup");
            return None;
        }
    };

    if entries.is_empty() {
        return None;
    }

    // NOTE: Previously, hardening code here would merge worktree_setup from
    // detected_analysis into custom_analysis entries that had empty worktree_setup.
    // This was removed because it treated intentionally empty worktree_setup []
    // (e.g., user explicitly removing target/ symlinks) as "not configured" and
    // overrode the user's choice. If custom_analysis exists, trust it fully.

    // Build template resolver
    let project_root = &project.working_directory;
    let worktree_path = exec_cwd.to_str().unwrap_or(project_root);
    let task_branch = task.task_branch.as_deref().unwrap_or("");

    let project_root_owned = project_root.clone();
    let worktree_path_owned = worktree_path.to_string();
    let task_branch_owned = task_branch.to_string();

    let resolve = |s: &str| -> String {
        s.replace("{project_root}", &project_root_owned)
            .replace("{worktree_path}", &worktree_path_owned)
            .replace("{task_branch}", &task_branch_owned)
    };

    // Phase 1: Worktree Setup (non-fatal, reuse existing run_setup_phase logic)
    // Convert PreExecAnalysisEntry to MergeAnalysisEntry for reuse
    let merge_entries: Vec<MergeAnalysisEntry> = entries
        .iter()
        .map(|e| MergeAnalysisEntry {
            path: e.path.clone(),
            label: e.label.clone(),
            validate: Vec::new(),
            worktree_setup: e.worktree_setup.clone(),
        })
        .collect();

    // Skip worktree setup when exec_cwd == project_root (no worktree created).
    // Running symlink setup with {worktree_path} == {project_root} would create
    // circular symlinks (source == target), potentially destroying node_modules/target.
    let skip_setup = worktree_path == project_root.as_str();
    let (mut log, _setup_had_failures) = if skip_setup {
        tracing::info!(
            task_id = task_id_str,
            "Skipping worktree setup phase (pre-exec): exec_cwd equals project_root (no worktree)"
        );
        (Vec::new(), false)
    } else {
        run_setup_phase(
            &merge_entries,
            exec_cwd,
            task_id_str,
            event_sink,
            &resolve,
            Some(context),
            cancel,
        )
        .await
    };

    // Phase 2: Install (fatal based on merge_validation_mode)
    let (install_log, install_had_failures) = run_install_phase(
        &entries,
        exec_cwd,
        task_id_str,
        event_sink,
        &resolve,
        context,
        cancel,
    )
    .await;

    log.extend(install_log);

    let overall_duration_ms = overall_start.elapsed().as_millis() as u64;
    let success = !install_had_failures;
    tracing::info!(
        task_id = task_id_str,
        duration_ms = overall_duration_ms,
        success = success,
        total_commands = log.len(),
        "run_pre_execution_setup: completed pre-execution setup"
    );

    // Write full output to disk for failed setup commands
    if !success {
        for entry in &mut log {
            if entry.status == STATUS_FAILED {
                entry.attach_failure_logs(task_id_str);
            }
        }
    }

    Some(PreExecSetupResult { success, log })
}
