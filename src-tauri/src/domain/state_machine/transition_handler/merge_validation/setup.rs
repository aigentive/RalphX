use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::domain::entities::merge_progress_event::{MergePhase, MergePhaseStatus};

use super::{
    emit_merge_progress, spawn_cancellable_command, truncate_output, CancellableCommandResult,
    MergeAnalysisEntry, ValidationLogEntry, STATUS_FAILED,
};

/// Parse an `ln -s` or `ln -sfn` command to extract `(source, target)` as absolute PathBufs.
///
/// Resolves relative paths against `cwd`. Returns `None` for non-symlink commands
/// or commands that can't be parsed (wrong arg count, missing -s flag, etc.).
///
/// This is the canonical parser used by both `try_handle_symlink_idempotent` and the
/// collision detection pre-scan — a single source of truth for symlink argument extraction.
pub(crate) fn parse_symlink_command(cmd: &str, cwd: &Path) -> Option<(PathBuf, PathBuf)> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() || parts[0] != "ln" {
        return None;
    }

    // Must have -s flag (could be -s, -sf, -sn, -sfn, etc.)
    let has_symlink_flag = parts.iter().any(|p| p.starts_with('-') && p.contains('s'));
    if !has_symlink_flag {
        return None;
    }

    // Extract non-flag arguments (source and target)
    let args: Vec<&str> = parts
        .iter()
        .filter(|p| !p.starts_with('-') && **p != "ln")
        .copied()
        .collect();
    if args.len() != 2 {
        return None;
    }

    let source = if Path::new(args[0]).is_absolute() {
        PathBuf::from(args[0])
    } else {
        cwd.join(args[0])
    };
    let target = if Path::new(args[1]).is_absolute() {
        PathBuf::from(args[1])
    } else {
        cwd.join(args[1])
    };

    Some((source, target))
}

/// Idempotent symlink handling for worktree setup commands.
///
/// Parses `ln -s[f] <source> <target>` commands and handles existing targets:
/// - Source == target (circular) → returns `Some(log_entry)` (skip, prevents damage)
/// - Correct symlink already exists → returns `Some(log_entry)` (skip)
/// - Circular self-symlink exists → removes it, returns `None` (re-run command)
/// - Wrong symlink or real file/dir → removes target, returns `None` (re-run command)
/// - Target doesn't exist → returns `None` (run normally)
/// - Non-symlink commands → returns `None` (pass through)
pub(crate) fn try_handle_symlink_idempotent(
    cmd: &str,
    cwd: &Path,
    label: &str,
    resolved_path: &str,
) -> Option<ValidationLogEntry> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() || parts[0] != "ln" {
        return None;
    }

    // Must have -s flag (could be -s, -sf, -sn, -sfn, etc.)
    let has_symlink_flag = parts.iter().any(|p| p.starts_with('-') && p.contains('s'));
    if !has_symlink_flag {
        return None;
    }

    // Extract non-flag arguments (source and target)
    let args: Vec<&str> = parts
        .iter()
        .filter(|p| !p.starts_with('-') && **p != "ln")
        .copied()
        .collect();
    if args.len() != 2 {
        return None;
    }

    let source = PathBuf::from(args[0]);
    let target_path = if Path::new(args[1]).is_absolute() {
        PathBuf::from(args[1])
    } else {
        cwd.join(args[1])
    };

    // Layer 2: Prevent circular symlinks where source == target.
    // This happens when {worktree_path} resolves to the main repo path
    // (e.g., merge succeeded without creating a worktree).
    let resolved_source = if source.is_absolute() {
        source.clone()
    } else {
        cwd.join(&source)
    };
    if resolved_source == target_path {
        tracing::warn!(
            command = %cmd,
            source = %resolved_source.display(),
            target = %target_path.display(),
            "Worktree setup: skipping circular symlink (source == target)"
        );
        return Some(ValidationLogEntry::new(
            "setup",
            cmd,
            resolved_path,
            label,
            "skipped",
            Some(0),
            String::new(),
            "Skipped: circular symlink (source == target)".to_string(),
            0,
        ));
    }

    if target_path.is_symlink() {
        if let Ok(existing_target) = std::fs::read_link(&target_path) {
            // Layer 3: Detect and remove circular self-symlinks left by previous runs.
            if existing_target == target_path || existing_target == Path::new(args[1]) {
                tracing::warn!(
                    command = %cmd,
                    target = %target_path.display(),
                    "Worktree setup: removing circular self-symlink from previous run"
                );
                let _ = std::fs::remove_file(&target_path);
                return None; // Proceed with normal command execution
            }

            if existing_target == source {
                // Correct symlink already exists — report as cached (no action needed)
                tracing::info!(
                    command = %cmd,
                    target = %target_path.display(),
                    "Worktree setup: symlink already correct, cached"
                );
                return Some(ValidationLogEntry::new(
                    "setup",
                    cmd,
                    resolved_path,
                    label,
                    "cached",
                    Some(0),
                    String::new(),
                    "Symlink already exists and is correct".to_string(),
                    0,
                ));
            }
        }
        // Wrong symlink — remove so command can recreate
        tracing::info!(
            command = %cmd,
            target = %target_path.display(),
            "Worktree setup: removing incorrect symlink before re-creation"
        );
        let _ = std::fs::remove_file(&target_path);
    } else if target_path.exists() {
        // Real file/dir exists at target — preserve it. This typically means a fixer
        // agent built locally (creating a real target/ dir) and re-validation should
        // use those artifacts instead of destroying them and symlinking to main repo.
        tracing::info!(
            command = %cmd,
            target = %target_path.display(),
            "Worktree setup: preserving existing real path (skipping symlink)"
        );
        return Some(ValidationLogEntry::new(
            "setup",
            cmd,
            resolved_path,
            label,
            "cached",
            Some(0),
            String::new(),
            "Skipped: real directory exists (preserved)".to_string(),
            0,
        ));
    }

    None // Proceed with normal command execution
}

/// Run worktree setup commands (symlinks, etc.) — non-fatal.
///
/// Returns the log entries and whether any setup command failed.
pub(super) async fn run_setup_phase(
    entries: &[MergeAnalysisEntry],
    merge_cwd: &Path,
    task_id_str: &str,
    event_sink: Option<&dyn ralphx_events::EventSink>,
    resolve: &(dyn Fn(&str) -> String + Send + Sync),
    context: Option<&str>,
    cancel: &CancellationToken,
) -> (Vec<ValidationLogEntry>, bool) {
    let mut log: Vec<ValidationLogEntry> = Vec::new();
    let mut setup_had_failures = false;

    let has_setup_commands = entries.iter().any(|e| !e.worktree_setup.is_empty());
    if has_setup_commands {
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::worktree_setup(),
            MergePhaseStatus::Started,
            "Setting up worktree environment".to_string(),
        );
    }

    let setup_phase_start = std::time::Instant::now();
    let mut setup_count = 0;
    for entry in entries {
        setup_count += entry.worktree_setup.len();
    }
    if setup_count > 0 {
        tracing::info!(
            task_id = task_id_str,
            command_count = setup_count,
            "run_validation_commands: starting setup phase"
        );
    }

    // --- Collision detection pre-scan ---
    // Build a map from resolved symlink target → [(entry_path, full_resolved_cmd)]
    // to detect when multiple entries map to the same target path.
    use std::collections::{HashMap, HashSet};
    let mut target_to_entries: HashMap<PathBuf, Vec<(String, String)>> = HashMap::new();
    for entry in entries {
        for cmd_str in &entry.worktree_setup {
            let resolved_cmd = resolve(cmd_str);
            let resolved_path = resolve(&entry.path);
            let cmd_cwd = if resolved_path == "." {
                merge_cwd.to_path_buf()
            } else {
                merge_cwd.join(&resolved_path)
            };
            if let Some((_src, target)) = parse_symlink_command(&resolved_cmd, &cmd_cwd) {
                target_to_entries
                    .entry(target)
                    .or_default()
                    .push((resolved_path.clone(), resolved_cmd.clone()));
            }
        }
    }

    // Find targets claimed by more than one entry (collisions).
    // Determine the winner for each colliding target:
    //   - If the root entry (entry.path == ".") is a collider → root wins
    //   - Otherwise → first collider by JSON order wins
    let mut collision_targets: HashSet<PathBuf> = HashSet::new();
    let mut collision_winners: HashMap<PathBuf, String> = HashMap::new(); // target → winning entry_path
    for (target, claimants) in &target_to_entries {
        if claimants.len() > 1 {
            collision_targets.insert(target.clone());
            // Winner: root entry ("." after resolve) if present, else first by JSON order
            let winner_path = claimants
                .iter()
                .find(|(ep, _)| ep == ".")
                .map(|(ep, _)| ep.clone())
                .unwrap_or_else(|| claimants[0].0.clone());
            collision_winners.insert(target.clone(), winner_path);
        }
    }

    // Track which collision targets have already been claimed (to handle the case
    // where the same winner has multiple commands mapping to the same target).
    let mut claimed_targets: HashSet<PathBuf> = HashSet::new();

    for entry in entries {
        for cmd_str in &entry.worktree_setup {
            let resolved_cmd = resolve(cmd_str);
            let resolved_path = resolve(&entry.path);
            let cmd_cwd = if resolved_path == "." {
                merge_cwd.to_path_buf()
            } else {
                merge_cwd.join(&resolved_path)
            };

            // --- Collision check ---
            // If this command's target collides with another entry, apply winner-based skipping.
            if let Some((_src, target)) = parse_symlink_command(&resolved_cmd, &cmd_cwd) {
                if collision_targets.contains(&target) {
                    let winner_path = collision_winners.get(&target).cloned().unwrap_or_default();
                    let is_winner = resolved_path == winner_path;
                    let already_claimed = claimed_targets.contains(&target);

                    if is_winner && !already_claimed {
                        // This entry wins the collision — let it proceed, mark target claimed
                        claimed_targets.insert(target.clone());
                        // (fall through to normal processing below)
                    } else {
                        // Loser or duplicate winner claim — skip
                        tracing::warn!(
                            entry_path = %resolved_path,
                            target = %target.display(),
                            winner_path = %winner_path,
                            "Skipping colliding worktree_setup for entry '{}' — target '{}' collides with entry '{}'",
                            resolved_path,
                            target.display(),
                            winner_path,
                        );
                        let skip_entry = ValidationLogEntry::new(
                            "setup",
                            &resolved_cmd,
                            &resolved_path,
                            &entry.label,
                            "skipped",
                            Some(0),
                            String::new(),
                            format!(
                                "Skipped: target '{}' collides with entry '{}'",
                                target.display(),
                                winner_path,
                            ),
                            0,
                        );
                        if let Some(sink) = event_sink {
                            let mut event_data = serde_json::json!({
                                "task_id": task_id_str,
                                "phase": skip_entry.phase,
                                "command": skip_entry.command,
                                "path": skip_entry.path,
                                "label": skip_entry.label,
                                "status": skip_entry.status,
                                "exit_code": skip_entry.exit_code,
                                "stderr": skip_entry.stderr,
                                "duration_ms": skip_entry.duration_ms,
                            });
                            if let Some(ctx) = context {
                                event_data["context"] = serde_json::json!(ctx);
                            }
                            sink.emit("merge:validation_step", event_data);
                        }
                        log.push(skip_entry);
                        continue;
                    }
                }
            }

            // --- Parent directory creation ---
            // Ensure the symlink target's parent directory exists before running the command.
            if let Some((_src, target)) = parse_symlink_command(&resolved_cmd, &cmd_cwd) {
                if let Some(parent) = target.parent() {
                    if !parent.exists() {
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            tracing::warn!(
                                command = %resolved_cmd,
                                parent = %parent.display(),
                                error = %e,
                                "Worktree setup: failed to create parent dir for symlink target (continuing)"
                            );
                        }
                    }
                }
            }

            // Idempotent symlink handling: skip if correct symlink exists,
            // remove stale target if wrong, pass through for non-symlink commands
            if let Some(skip_entry) =
                try_handle_symlink_idempotent(&resolved_cmd, &cmd_cwd, &entry.label, &resolved_path)
            {
                if let Some(sink) = event_sink {
                    let mut event_data = serde_json::json!({
                        "task_id": task_id_str,
                        "phase": skip_entry.phase,
                        "command": skip_entry.command,
                        "path": skip_entry.path,
                        "label": skip_entry.label,
                        "status": skip_entry.status,
                        "exit_code": skip_entry.exit_code,
                        "stderr": skip_entry.stderr,
                        "duration_ms": skip_entry.duration_ms,
                    });
                    if let Some(ctx) = context {
                        event_data["context"] = serde_json::json!(ctx);
                    }
                    sink.emit("merge:validation_step", event_data);
                }
                log.push(skip_entry);
                continue;
            }

            // Harden symlink commands: use -sfn flags to prevent nesting bugs when
            // the target already exists as a symlink (handles complex/chained commands
            // that try_handle_symlink_idempotent can't parse)
            let resolved_cmd = if resolved_cmd.contains("ln -s ") && !resolved_cmd.contains("-sfn")
            {
                resolved_cmd
                    .replace("ln -s ", "ln -sfn ")
                    .replace("ln -sf ", "ln -sfn ")
            } else {
                resolved_cmd
            };

            // Emit "running" event before execution
            if let Some(sink) = event_sink {
                let mut event_data = serde_json::json!({
                    "task_id": task_id_str,
                    "phase": "setup",
                    "command": resolved_cmd,
                    "path": resolved_path,
                    "label": entry.label,
                    "status": "running",
                });
                if let Some(ctx) = context {
                    event_data["context"] = serde_json::json!(ctx);
                }
                sink.emit("merge:validation_step", event_data);
            }

            tracing::info!(
                command = %resolved_cmd,
                cwd = %cmd_cwd.display(),
                "Running worktree setup command"
            );

            let start = std::time::Instant::now();

            let result = spawn_cancellable_command(&resolved_cmd, &cmd_cwd, cancel).await;

            let log_entry = match result {
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
                        setup_had_failures = true;
                        tracing::warn!(
                            command = %resolved_cmd,
                            stderr = %stderr_raw,
                            "Worktree setup command failed (non-fatal)"
                        );
                    }

                    ValidationLogEntry {
                        phase: "setup".to_string(),
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
                    setup_had_failures = true;
                    let duration_ms = start.elapsed().as_millis() as u64;
                    tracing::warn!(
                        command = %resolved_cmd,
                        error = %e,
                        "Worktree setup command failed (non-fatal)"
                    );

                    ValidationLogEntry {
                        phase: "setup".to_string(),
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
                    setup_had_failures = true;
                    let duration_ms = start.elapsed().as_millis() as u64;
                    tracing::warn!(
                        command = %resolved_cmd,
                        "Worktree setup command cancelled"
                    );

                    ValidationLogEntry {
                        phase: "setup".to_string(),
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

            tracing::info!(
                command = %resolved_cmd,
                cwd = %cmd_cwd.display(),
                status = %log_entry.status,
                exit_code = ?log_entry.exit_code,
                duration_ms = log_entry.duration_ms,
                "Worktree setup command completed"
            );

            if let Some(sink) = event_sink {
                let mut event_data = serde_json::json!({
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
                });
                if let Some(ctx) = context {
                    event_data["context"] = serde_json::json!(ctx);
                }
                sink.emit("merge:validation_step", event_data);
            }
            log.push(log_entry);
        }
    }

    if setup_count > 0 {
        let setup_duration_ms = setup_phase_start.elapsed().as_millis() as u64;
        tracing::info!(
            task_id = task_id_str,
            duration_ms = setup_duration_ms,
            command_count = setup_count,
            "run_validation_commands: completed setup phase"
        );
    }

    // Always emit worktree setup completion event so the frontend can show
    // a checkmark (Passed) or skip indicator, even when there are no setup commands.
    if setup_had_failures {
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::worktree_setup(),
            MergePhaseStatus::Failed,
            "Worktree setup completed with warnings (non-fatal)".to_string(),
        );
    } else {
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::worktree_setup(),
            MergePhaseStatus::Passed,
            if has_setup_commands {
                "Worktree setup completed successfully".to_string()
            } else {
                "Worktree setup skipped (no commands)".to_string()
            },
        );
    }

    (log, setup_had_failures)
}
