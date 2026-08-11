use std::path::Path;

use tokio_util::sync::CancellationToken;

use crate::domain::entities::merge_progress_event::{map_command_to_phase, MergePhaseStatus};

use super::{
    emit_merge_progress, spawn_cancellable_command, truncate_output, CancellableCommandResult,
    MergeAnalysisEntry, ValidationFailure, ValidationLogEntry, STATUS_FAILED,
    VALIDATE_RETRY_DELAY_MS,
};

/// Emit `Skipped` progress events and log entries for all validate commands
/// that were not yet executed (fail-fast abort).
///
/// Iterates through all entries/commands and skips those already in the log,
/// emitting `MergePhaseStatus::Skipped` for the remainder.
fn emit_skipped_for_remaining(
    entries: &[MergeAnalysisEntry],
    _merge_cwd: &Path,
    task_id_str: &str,
    event_sink: Option<&dyn ralphx_events::EventSink>,
    resolve: &(dyn Fn(&str) -> String + Send + Sync),
    log: &mut Vec<ValidationLogEntry>,
    failed_path: &str,
    failed_cmd: &str,
) {
    let mut past_failure = false;
    for entry in entries {
        let resolved_path = resolve(&entry.path);
        for cmd_str in &entry.validate {
            let resolved_cmd = resolve(cmd_str);

            // Skip commands we already ran (they're already in the log)
            if !past_failure {
                if resolved_cmd == failed_cmd && resolved_path == failed_path {
                    past_failure = true;
                }
                continue;
            }

            // Emit high-level skipped event
            let phase = map_command_to_phase(&resolved_cmd);
            emit_merge_progress(
                event_sink,
                task_id_str,
                phase,
                MergePhaseStatus::Skipped,
                format!("{} skipped (fail-fast)", resolved_cmd),
            );

            // Emit step-level skipped event
            if let Some(sink) = event_sink {
                sink.emit(
                    "merge:validation_step",
                    serde_json::json!({
                        "task_id": task_id_str,
                        "phase": "validate",
                        "command": resolved_cmd,
                        "path": resolved_path,
                        "label": entry.label,
                        "status": "skipped",
                        "exit_code": null,
                        "stdout": "",
                        "stderr": "Skipped due to prior validation failure (fail-fast)",
                        "duration_ms": 0,
                    }),
                );
            }

            log.push(ValidationLogEntry {
                phase: "validate".to_string(),
                command: resolved_cmd,
                path: resolved_path.clone(),
                label: entry.label.clone(),
                status: "skipped".to_string(),
                exit_code: None,
                stdout: String::new(),
                stderr: "Skipped due to prior validation failure (fail-fast)".to_string(),
                duration_ms: 0,
                ..Default::default()
            });
        }
    }
}

/// Run validate-phase commands, checking cache where possible.
///
/// When `validation_mode` is `Block` or `AutoFix`, aborts on first failure (fail-fast)
/// and emits `Skipped` events for remaining commands. In `Warn` mode, runs all commands.
///
/// Returns (log_entries, failures, ran_any).
pub(super) async fn run_validate_phase(
    entries: &[MergeAnalysisEntry],
    merge_cwd: &Path,
    task_id_str: &str,
    event_sink: Option<&dyn ralphx_events::EventSink>,
    cached_log: Option<&[ValidationLogEntry]>,
    resolve: &(dyn Fn(&str) -> String + Send + Sync),
    validation_mode: &crate::domain::entities::MergeValidationMode,
    cancel: &CancellationToken,
) -> (Vec<ValidationLogEntry>, Vec<ValidationFailure>, bool) {
    let mut log: Vec<ValidationLogEntry> = Vec::new();
    let mut failures = Vec::new();
    let mut ran_any = false;

    let validate_count: usize = entries.iter().map(|e| e.validate.len()).sum();
    let validate_phase_start = std::time::Instant::now();
    if validate_count > 0 {
        tracing::info!(
            task_id = task_id_str,
            command_count = validate_count,
            "run_validation_commands: starting validate phase"
        );
    }

    for entry in entries {
        if entry.validate.is_empty() {
            continue;
        }

        let resolved_path = resolve(&entry.path);
        let cmd_cwd = if resolved_path == "." {
            merge_cwd.to_path_buf()
        } else {
            merge_cwd.join(&resolved_path)
        };

        for cmd_str in &entry.validate {
            let resolved_cmd = resolve(cmd_str);
            ran_any = true;

            // Check cache: skip previously-passed validate commands when SHA matches
            if let Some(cached) = cached_log {
                let cached_hit = cached.iter().find(|c| {
                    c.phase == "validate"
                        && c.command == resolved_cmd
                        && c.path == resolved_path
                        && (c.status == "success" || c.status == "cached")
                });
                if let Some(prev) = cached_hit {
                    tracing::info!(
                        command = %resolved_cmd,
                        "Skipping validation command (cached, SHA unchanged)"
                    );

                    let phase = map_command_to_phase(&resolved_cmd);
                    emit_merge_progress(
                        event_sink,
                        task_id_str,
                        phase,
                        MergePhaseStatus::Passed,
                        format!("{} (cached)", resolved_cmd),
                    );

                    let log_entry = ValidationLogEntry {
                        phase: "validate".to_string(),
                        command: resolved_cmd,
                        path: resolved_path.clone(),
                        label: entry.label.clone(),
                        status: "cached".to_string(),
                        exit_code: prev.exit_code,
                        stdout: String::new(),
                        stderr: String::new(),
                        duration_ms: 0,
                        ..Default::default()
                    };
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
                                "duration_ms": log_entry.duration_ms,
                            }),
                        );
                    }
                    log.push(log_entry);
                    continue;
                }
            }

            // Emit high-level merge progress event
            let phase = map_command_to_phase(&resolved_cmd);
            emit_merge_progress(
                event_sink,
                task_id_str,
                phase.clone(),
                MergePhaseStatus::Started,
                format!("Running {}", resolved_cmd),
            );

            // Emit "running" event before execution
            if let Some(sink) = event_sink {
                sink.emit(
                    "merge:validation_step",
                    serde_json::json!({
                        "task_id": task_id_str,
                        "phase": "validate",
                        "command": resolved_cmd,
                        "path": resolved_path,
                        "label": entry.label,
                        "status": "running",
                    }),
                );
            }

            tracing::info!(
                command = %resolved_cmd,
                cwd = %cmd_cwd.display(),
                "Running post-merge validation command"
            );

            let start = std::time::Instant::now();

            let result = spawn_cancellable_command(&resolved_cmd, &cmd_cwd, cancel).await;

            let (mut log_entry, mut failure) = match result {
                CancellableCommandResult::Completed(output) => {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    let stderr_raw = String::from_utf8_lossy(&output.stderr).to_string();
                    let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();

                    let failure = if !output.status.success() {
                        tracing::warn!(
                            command = %resolved_cmd,
                            exit_code = ?output.status.code(),
                            stderr = %stderr_raw,
                            "Post-merge validation command failed"
                        );
                        Some(ValidationFailure {
                            command: resolved_cmd.clone(),
                            path: resolved_path.clone(),
                            exit_code: output.status.code(),
                            stderr: format!(
                                "{}{}",
                                if stderr_raw.is_empty() {
                                    ""
                                } else {
                                    &stderr_raw
                                },
                                if stdout_raw.is_empty() {
                                    String::new()
                                } else {
                                    format!("\nstdout: {}", stdout_raw)
                                },
                            ),
                        })
                    } else {
                        tracing::info!(command = %resolved_cmd, "Post-merge validation command passed");
                        None
                    };

                    // Emit high-level merge progress completion event
                    if output.status.success() {
                        emit_merge_progress(
                            event_sink,
                            task_id_str,
                            phase,
                            MergePhaseStatus::Passed,
                            format!("{} passed", resolved_cmd),
                        );
                    } else {
                        emit_merge_progress(
                            event_sink,
                            task_id_str,
                            phase,
                            MergePhaseStatus::Failed,
                            format!("{} failed", resolved_cmd),
                        );
                    }

                    let status = if output.status.success() {
                        "success"
                    } else {
                        "failed"
                    };
                    let entry = ValidationLogEntry {
                        phase: "validate".to_string(),
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        label: entry.label.clone(),
                        status: status.to_string(),
                        exit_code: output.status.code(),
                        stdout: truncate_output(&stdout_raw, 2000),
                        stderr: truncate_output(&stderr_raw, 2000),
                        duration_ms,
                        ..Default::default()
                    };

                    (entry, failure)
                }
                CancellableCommandResult::SpawnError(e) => {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    tracing::error!(command = %resolved_cmd, error = %e, "Failed to execute validation command");

                    emit_merge_progress(
                        event_sink,
                        task_id_str,
                        phase,
                        MergePhaseStatus::Failed,
                        format!("Failed to execute: {}", e),
                    );

                    let failure = ValidationFailure {
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        exit_code: None,
                        stderr: format!("Failed to execute: {}", e),
                    };

                    let entry = ValidationLogEntry {
                        phase: "validate".to_string(),
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        label: entry.label.clone(),
                        status: STATUS_FAILED.to_string(),
                        exit_code: None,
                        stdout: String::new(),
                        stderr: truncate_output(&format!("Failed to execute: {}", e), 2000),
                        duration_ms,
                        ..Default::default()
                    };

                    (entry, Some(failure))
                }
                CancellableCommandResult::Cancelled => {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    tracing::warn!(
                        command = %resolved_cmd,
                        "Validation command cancelled"
                    );

                    emit_merge_progress(
                        event_sink,
                        task_id_str,
                        phase,
                        MergePhaseStatus::Failed,
                        format!("{} cancelled", resolved_cmd),
                    );

                    let failure = ValidationFailure {
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        exit_code: None,
                        stderr: "Command cancelled".to_string(),
                    };

                    let entry = ValidationLogEntry {
                        phase: "validate".to_string(),
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        label: entry.label.clone(),
                        status: STATUS_FAILED.to_string(),
                        exit_code: None,
                        stdout: String::new(),
                        stderr: "Command cancelled".to_string(),
                        duration_ms,
                        ..Default::default()
                    };

                    (entry, Some(failure))
                }
            };

            // Retry once if the validation command failed with a real exit code
            // (not spawn error, not cancelled, not already cancelled by token).
            if log_entry.status == STATUS_FAILED
                && log_entry.exit_code.is_some()
                && !cancel.is_cancelled()
            {
                tracing::warn!(
                    command = %resolved_cmd,
                    delay_ms = VALIDATE_RETRY_DELAY_MS,
                    "Validation command failed, retrying once after {}ms (attempt 2/2)",
                    VALIDATE_RETRY_DELAY_MS
                );

                let retry_phase = map_command_to_phase(&resolved_cmd);
                emit_merge_progress(
                    event_sink,
                    task_id_str,
                    retry_phase,
                    MergePhaseStatus::Started,
                    format!("Retrying {} ...", resolved_cmd),
                );

                tokio::time::sleep(std::time::Duration::from_millis(VALIDATE_RETRY_DELAY_MS)).await;

                // Abort retry if cancelled during the delay
                if !cancel.is_cancelled() {
                    let retry_start = std::time::Instant::now();
                    let retry_result =
                        spawn_cancellable_command(&resolved_cmd, &cmd_cwd, cancel).await;

                    if let CancellableCommandResult::Completed(output) = retry_result {
                        let retry_duration_ms = retry_start.elapsed().as_millis() as u64;
                        let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();
                        let stderr_raw = String::from_utf8_lossy(&output.stderr).to_string();

                        if output.status.success() {
                            tracing::info!(
                                command = %resolved_cmd,
                                "Validation command succeeded on retry (attempt 2/2)"
                            );

                            let retry_phase = map_command_to_phase(&resolved_cmd);
                            emit_merge_progress(
                                event_sink,
                                task_id_str,
                                retry_phase,
                                MergePhaseStatus::Passed,
                                format!("{} passed on retry", resolved_cmd),
                            );

                            log_entry = ValidationLogEntry {
                                phase: "validate".to_string(),
                                command: resolved_cmd.clone(),
                                path: resolved_path.clone(),
                                label: entry.label.clone(),
                                status: "success".to_string(),
                                exit_code: output.status.code(),
                                stdout: truncate_output(&stdout_raw, 2000),
                                stderr: truncate_output(&stderr_raw, 2000),
                                duration_ms: retry_duration_ms,
                                retried: true,
                                ..Default::default()
                            };
                            failure = None;
                        } else {
                            tracing::warn!(
                                command = %resolved_cmd,
                                stderr = %stderr_raw,
                                "Validation command retry also failed (attempt 2/2)"
                            );

                            let retry_phase = map_command_to_phase(&resolved_cmd);
                            emit_merge_progress(
                                event_sink,
                                task_id_str,
                                retry_phase,
                                MergePhaseStatus::Failed,
                                format!("{} failed on retry", resolved_cmd),
                            );

                            log_entry = ValidationLogEntry {
                                phase: "validate".to_string(),
                                command: resolved_cmd.clone(),
                                path: resolved_path.clone(),
                                label: entry.label.clone(),
                                status: STATUS_FAILED.to_string(),
                                exit_code: output.status.code(),
                                stdout: truncate_output(&stdout_raw, 2000),
                                stderr: truncate_output(&stderr_raw, 2000),
                                duration_ms: retry_duration_ms,
                                retried: true,
                                ..Default::default()
                            };
                            failure = Some(ValidationFailure {
                                command: resolved_cmd.clone(),
                                path: resolved_path.clone(),
                                exit_code: output.status.code(),
                                stderr: format!(
                                    "{}{}",
                                    if stderr_raw.is_empty() {
                                        ""
                                    } else {
                                        &stderr_raw
                                    },
                                    if stdout_raw.is_empty() {
                                        String::new()
                                    } else {
                                        format!("\nstdout: {}", stdout_raw)
                                    },
                                ),
                            });
                        }
                    }
                    // If spawn failed or was cancelled on retry, keep the original failure
                }
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
                    }),
                );
            }
            log.push(log_entry);
            if let Some(f) = failure {
                failures.push(f);

                // Fail-fast: in Block/AutoFix mode, abort remaining commands on first failure
                use crate::domain::entities::MergeValidationMode;
                if matches!(
                    validation_mode,
                    MergeValidationMode::Block | MergeValidationMode::AutoFix
                ) {
                    tracing::info!(
                        task_id = task_id_str,
                        failed_command = %resolved_cmd,
                        "Fail-fast: aborting remaining validation commands (mode={validation_mode})"
                    );

                    // Emit Skipped events for all remaining commands
                    emit_skipped_for_remaining(
                        entries,
                        merge_cwd,
                        task_id_str,
                        event_sink,
                        resolve,
                        &mut log,
                        &resolved_path,
                        &resolved_cmd,
                    );

                    // Break out of both loops (entry + command)
                    // We return early — the outer loop won't continue
                    let validate_duration_ms = validate_phase_start.elapsed().as_millis() as u64;
                    tracing::info!(
                        task_id = task_id_str,
                        duration_ms = validate_duration_ms,
                        command_count = validate_count,
                        failure_count = failures.len(),
                        "run_validation_commands: completed validate phase (fail-fast)"
                    );
                    return (log, failures, ran_any);
                }
            }
        }
    }

    if validate_count > 0 {
        let validate_duration_ms = validate_phase_start.elapsed().as_millis() as u64;
        tracing::info!(
            task_id = task_id_str,
            duration_ms = validate_duration_ms,
            command_count = validate_count,
            failure_count = failures.len(),
            "run_validation_commands: completed validate phase"
        );
    }

    (log, failures, ran_any)
}
