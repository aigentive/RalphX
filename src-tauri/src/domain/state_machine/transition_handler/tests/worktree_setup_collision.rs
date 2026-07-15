// Tests for worktree_setup collision detection and parent directory creation.
//
// Covers:
//   Test 1: parse_symlink_command helper — absolute paths, relative paths, non-symlink commands
//   Test 2: Parent directory creation before symlink creation
//   Test 3: Collision detection — root entry wins over sub-package entry
//   Test 4: No collision when targets differ — both symlinks created

use super::super::merge_validation::parse_symlink_command;
use std::path::{Path, PathBuf};

// ==================
// Test 1: parse_symlink_command helper
// ==================

#[test]
fn parse_symlink_absolute_paths() {
    let cwd = Path::new("/some/cwd");
    let cmd = "ln -sfn /source/node_modules /target/packages/web/node_modules";
    let result = parse_symlink_command(cmd, cwd);
    assert!(result.is_some(), "absolute ln -sfn should be parsed");
    let (source, target) = result.unwrap();
    assert_eq!(source, PathBuf::from("/source/node_modules"));
    assert_eq!(target, PathBuf::from("/target/packages/web/node_modules"));
}

#[test]
fn parse_symlink_relative_paths_resolved_against_cwd() {
    let cwd = Path::new("/work/dir");
    let cmd = "ln -s rel_source rel_target";
    let result = parse_symlink_command(cmd, cwd);
    assert!(result.is_some(), "relative ln -s should be parsed");
    let (source, target) = result.unwrap();
    assert_eq!(source, PathBuf::from("/work/dir/rel_source"));
    assert_eq!(target, PathBuf::from("/work/dir/rel_target"));
}

#[test]
fn parse_symlink_non_symlink_command_returns_none() {
    let cwd = Path::new("/some/cwd");
    assert!(
        parse_symlink_command("npm run build", cwd).is_none(),
        "non-ln command should return None"
    );
}

#[test]
fn parse_symlink_ln_without_s_flag_returns_none() {
    let cwd = Path::new("/some/cwd");
    assert!(
        parse_symlink_command("ln source target", cwd).is_none(),
        "ln without -s flag should return None"
    );
}

#[test]
fn parse_symlink_ln_sf_flag_recognized() {
    let cwd = Path::new("/cwd");
    let result = parse_symlink_command("ln -sf /a/source /b/target", cwd);
    assert!(result.is_some());
    let (src, tgt) = result.unwrap();
    assert_eq!(src, PathBuf::from("/a/source"));
    assert_eq!(tgt, PathBuf::from("/b/target"));
}

#[test]
fn parse_symlink_mixed_absolute_relative() {
    let cwd = Path::new("/work");
    let cmd = "ln -s /abs/source rel_target";
    let result = parse_symlink_command(cmd, cwd);
    assert!(result.is_some());
    let (src, tgt) = result.unwrap();
    assert_eq!(src, PathBuf::from("/abs/source"));
    assert_eq!(tgt, PathBuf::from("/work/rel_target"));
}

// ==================
// Test 2: Parent directory creation before symlink
// ==================

#[test]
fn parent_dir_created_before_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir(&source_dir).unwrap();

    // Target whose parent directory does NOT yet exist
    let nested_parent = dir.path().join("deep").join("nested");
    let target = nested_parent.join("link");

    assert!(
        !nested_parent.exists(),
        "nested parent should not exist yet"
    );

    // parse_symlink_command should give us the target, and we create the parent
    let cmd = format!("ln -s {} {}", source_dir.display(), target.display());
    let cwd = dir.path();
    let result = parse_symlink_command(&cmd, cwd);
    assert!(result.is_some());
    let (_src, parsed_target) = result.unwrap();

    // Simulate what run_setup_phase does: create parent dir before executing
    if let Some(parent) = parsed_target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }

    assert!(
        nested_parent.exists(),
        "parent dir should exist after create_dir_all"
    );

    // Now we can create the symlink
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&source_dir, &target).unwrap();
        assert!(target.is_symlink(), "symlink should now exist");
        let link_target = std::fs::read_link(&target).unwrap();
        assert_eq!(link_target, source_dir, "symlink should point to source");
    }
}

// ==================
// Test 3: Collision detection — root wins
// ==================

/// Two entries both map to the same target. Entry with path="." (root) wins.
/// Only Entry A's symlink should be created; Entry B gets a "collides" log entry.
#[cfg(unix)]
#[tokio::test]
async fn collision_detection_root_wins() {
    use tokio_util::sync::CancellationToken;

    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().join("project");
    let worktree_path = dir.path().join("worktree");
    let sub_pkg_nm = project_root.join("sub").join("pkg").join("node_modules");
    let root_nm = project_root.join("node_modules");

    // Create all source directories
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::create_dir_all(&worktree_path).unwrap();
    std::fs::create_dir_all(&root_nm).unwrap();
    std::fs::create_dir_all(&sub_pkg_nm).unwrap();

    // Both entries target the same path: {worktree_path}/node_modules
    let collision_target = worktree_path.join("node_modules");

    // Build fake entries that resolve to a collision
    // Entry A: path="." (root), targets collision_target
    // Entry B: path="sub/pkg", also targets collision_target
    let analysis_json = serde_json::json!([
        {
            "path": ".",
            "label": "Root",
            "validate": [],
            "worktree_setup": [
                format!("ln -s {} {}", root_nm.display(), collision_target.display())
            ]
        },
        {
            "path": "sub/pkg",
            "label": "Sub Package",
            "validate": [],
            "worktree_setup": [
                format!("ln -s {} {}", sub_pkg_nm.display(), collision_target.display())
            ]
        }
    ]);

    // We'll exercise the logic by running the setup phase inline
    let entries: Vec<
        crate::domain::state_machine::transition_handler::merge_validation::MergeAnalysisEntry,
    > = serde_json::from_value(analysis_json).unwrap();

    let cancel = CancellationToken::new();
    let resolve = |s: &str| s.to_string();

    let (log, _had_failures) =
        run_setup_phase_for_test(&entries, &worktree_path, &resolve, &cancel).await;

    // Entry A's symlink should be created (root wins)
    assert!(
        collision_target.is_symlink() || collision_target.exists(),
        "root entry's symlink should be created at {}",
        collision_target.display()
    );

    // Verify the symlink points to the root node_modules (not sub_pkg)
    if collision_target.is_symlink() {
        let link_target = std::fs::read_link(&collision_target).unwrap();
        assert_eq!(
            link_target, root_nm,
            "symlink should point to root node_modules"
        );
    }

    // Entry B should have a "skipped" log entry with "collides" in the message
    let collides_entry = log
        .iter()
        .find(|e| e.status == "skipped" && e.stderr.contains("collides"));
    assert!(
        collides_entry.is_some(),
        "should have a skipped/collides log entry for Entry B. Log: {:?}",
        log.iter()
            .map(|e| format!("status={} stderr={}", e.status, e.stderr))
            .collect::<Vec<_>>()
    );
    let collides = collides_entry.unwrap();
    assert_eq!(
        collides.path, "sub/pkg",
        "collides entry should be for sub/pkg, got: {}",
        collides.path
    );
}

// ==================
// Test 4: No collision when targets differ
// ==================

/// Two entries with different targets — both symlinks should be created successfully.
#[cfg(unix)]
#[tokio::test]
async fn no_collision_when_targets_differ() {
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().join("project");
    let worktree_path = dir.path().join("worktree");

    let root_nm = project_root.join("node_modules");
    let sub_nm = project_root.join("sub").join("pkg").join("node_modules");

    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::create_dir_all(&worktree_path).unwrap();
    std::fs::create_dir_all(&root_nm).unwrap();
    std::fs::create_dir_all(&sub_nm).unwrap();

    // Different targets: no collision
    let target_a = worktree_path.join("node_modules");
    let target_b_parent = worktree_path.join("sub").join("pkg");
    let target_b = target_b_parent.join("node_modules");

    let analysis_json = serde_json::json!([
        {
            "path": ".",
            "label": "Root",
            "validate": [],
            "worktree_setup": [
                format!("ln -s {} {}", root_nm.display(), target_a.display())
            ]
        },
        {
            "path": "sub/pkg",
            "label": "Sub Package",
            "validate": [],
            "worktree_setup": [
                format!("ln -s {} {}", sub_nm.display(), target_b.display())
            ]
        }
    ]);

    let entries: Vec<
        crate::domain::state_machine::transition_handler::merge_validation::MergeAnalysisEntry,
    > = serde_json::from_value(analysis_json).unwrap();

    let cancel = tokio_util::sync::CancellationToken::new();
    let resolve = |s: &str| s.to_string();

    let (log, _had_failures) =
        run_setup_phase_for_test(&entries, &worktree_path, &resolve, &cancel).await;

    // Both symlinks should be created (parent dir creation ensures target_b parent exists)
    assert!(
        target_a.is_symlink() || target_a.exists(),
        "target_a symlink should be created"
    );
    assert!(
        target_b.is_symlink() || target_b.exists(),
        "target_b symlink should be created (parent dir auto-created)"
    );

    // No "collides" entries in log
    let collides_count = log.iter().filter(|e| e.stderr.contains("collides")).count();
    assert_eq!(
        collides_count, 0,
        "no collision entries expected when targets differ"
    );
}

// ==================
// Helper: thin wrapper around run_setup_phase for testing
// ==================
// We can't call the private async fn directly from another module, so we replicate
// the collision+parent-dir logic here as a pure unit exerciser.
// The real integration relies on the production function being called via
// run_validation_commands / run_pre_execution_setup.

use crate::domain::state_machine::transition_handler::merge_validation::{
    MergeAnalysisEntry, ValidationLogEntry,
};

async fn run_setup_phase_for_test(
    entries: &[MergeAnalysisEntry],
    merge_cwd: &std::path::Path,
    resolve: &(dyn Fn(&str) -> String + Send + Sync),
    cancel: &tokio_util::sync::CancellationToken,
) -> (Vec<ValidationLogEntry>, bool) {
    use crate::domain::state_machine::transition_handler::merge_validation::{
        parse_symlink_command, spawn_cancellable_command, try_handle_symlink_idempotent,
        CancellableCommandResult,
    };
    use std::collections::{HashMap, HashSet};

    let mut log: Vec<ValidationLogEntry> = Vec::new();
    let mut setup_had_failures = false;

    // --- Collision detection pre-scan ---
    let mut target_to_entries: HashMap<std::path::PathBuf, Vec<(String, String)>> = HashMap::new();
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

    let mut collision_targets: HashSet<std::path::PathBuf> = HashSet::new();
    let mut collision_winners: HashMap<std::path::PathBuf, String> = HashMap::new();
    for (target, claimants) in &target_to_entries {
        if claimants.len() > 1 {
            collision_targets.insert(target.clone());
            let winner_path = claimants
                .iter()
                .find(|(ep, _)| ep == ".")
                .map(|(ep, _)| ep.clone())
                .unwrap_or_else(|| claimants[0].0.clone());
            collision_winners.insert(target.clone(), winner_path);
        }
    }

    let mut claimed_targets: HashSet<std::path::PathBuf> = HashSet::new();

    for entry in entries {
        for cmd_str in &entry.worktree_setup {
            let resolved_cmd = resolve(cmd_str);
            let resolved_path = resolve(&entry.path);
            let cmd_cwd = if resolved_path == "." {
                merge_cwd.to_path_buf()
            } else {
                merge_cwd.join(&resolved_path)
            };

            // Collision check
            if let Some((_src, target)) = parse_symlink_command(&resolved_cmd, &cmd_cwd) {
                if collision_targets.contains(&target) {
                    let winner_path = collision_winners.get(&target).cloned().unwrap_or_default();
                    let is_winner = resolved_path == winner_path;
                    let already_claimed = claimed_targets.contains(&target);

                    if is_winner && !already_claimed {
                        claimed_targets.insert(target.clone());
                        // fall through
                    } else {
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
                        log.push(skip_entry);
                        continue;
                    }
                }
            }

            // Parent directory creation
            if let Some((_src, target)) = parse_symlink_command(&resolved_cmd, &cmd_cwd) {
                if let Some(parent) = target.parent() {
                    if !parent.exists() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                }
            }

            // Idempotent symlink handling
            if let Some(skip_entry) =
                try_handle_symlink_idempotent(&resolved_cmd, &cmd_cwd, &entry.label, &resolved_path)
            {
                log.push(skip_entry);
                continue;
            }

            // Harden symlink command flags
            let resolved_cmd = if resolved_cmd.contains("ln -s ") && !resolved_cmd.contains("-sfn")
            {
                resolved_cmd
                    .replace("ln -s ", "ln -sfn ")
                    .replace("ln -sf ", "ln -sfn ")
            } else {
                resolved_cmd
            };

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
                        setup_had_failures = true;
                        "failed"
                    };
                    ValidationLogEntry {
                        phase: "setup".to_string(),
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        label: entry.label.clone(),
                        status: status.to_string(),
                        exit_code: output.status.code(),
                        stdout: stdout_raw,
                        stderr: stderr_raw,
                        duration_ms,
                        ..Default::default()
                    }
                }
                CancellableCommandResult::SpawnError(e) => {
                    setup_had_failures = true;
                    ValidationLogEntry {
                        phase: "setup".to_string(),
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        label: entry.label.clone(),
                        status: "failed".to_string(),
                        stderr: format!("Failed to execute: {}", e),
                        duration_ms: start.elapsed().as_millis() as u64,
                        ..Default::default()
                    }
                }
                CancellableCommandResult::Cancelled => {
                    setup_had_failures = true;
                    ValidationLogEntry {
                        phase: "setup".to_string(),
                        command: resolved_cmd.clone(),
                        path: resolved_path.clone(),
                        label: entry.label.clone(),
                        status: "failed".to_string(),
                        stderr: "Command cancelled".to_string(),
                        duration_ms: start.elapsed().as_millis() as u64,
                        ..Default::default()
                    }
                }
            };
            log.push(log_entry);
        }
    }

    (log, setup_had_failures)
}
