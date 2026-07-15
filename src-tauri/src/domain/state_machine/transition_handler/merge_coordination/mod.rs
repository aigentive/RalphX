// Merge coordination helpers — deferral logic, plan branch management, and pre-merge cleanup.
//
// Extracted from side_effects.rs for maintainability.
// - ensure_plan_branch_exists: lazy git ref creation for plan merge targets
// - check_main_merge_deferral: defer main-branch merges until siblings terminal / agents idle
// - pre_merge_cleanup: remove debris from prior failed attempts before merge

use std::path::Path;
use std::sync::Arc;

use crate::application::GitService;
use crate::domain::entities::{PlanBranchStatus, Task};
use crate::domain::repositories::PlanBranchRepository;

pub(super) use super::cleanup_helpers;
pub(super) use super::merge_helpers;
pub(super) use super::merge_validation::emit_merge_progress;
pub(super) use super::{BranchPair, TaskCore, TransitionHandler};

mod cleanup;
mod deferral;
mod plan_update;
mod source_update;

pub(crate) use deferral::check_main_merge_deferral;
#[cfg(test)]
pub(crate) use plan_update::update_plan_from_main;
pub(crate) use plan_update::{update_plan_from_main_isolated, PlanUpdateResult};
pub(crate) use source_update::{update_source_from_target, SourceUpdateResult};

/// Metadata keys that indicate a prior merge attempt has been made.
///
/// If any of these keys are present in `task.metadata`, the task has been through
/// a merge cycle before and cleanup must run (debris may exist).
const MERGE_DEBRIS_METADATA_KEYS: &[&str] = &[
    "merge_failure_source",
    "source_conflict_resolved",
    "plan_update_conflict",
    "merge_error",
    "conflict_type",
    "source_update_conflict",
    "conflict_files",
    "error",
    "merge_pipeline_active", // Legacy: pre-v53 stored as JSON metadata (evidence: side_effects.rs:142)
];

/// Check whether this is a first clean merge attempt with no prior debris.
///
/// Returns `true` when the task has never been through a merge failure cycle —
/// meaning there's no debris from prior attempts that needs cleaning up.
/// When `true`, `pre_merge_cleanup` can skip all cleanup steps (Phase 1 GUARD fast-path).
///
/// Uses a 3-tier check:
/// 1. Dedicated `merge_pipeline_active` DB column — set when pipeline starts, cleared on success.
///    Non-null means a prior run crashed mid-pipeline.
/// 2. JSON metadata debris keys (including legacy `merge_pipeline_active` JSON key from pre-v53).
/// 3. Disk-presence check — if `worktree_path` is set AND the directory still exists on disk,
///    treat as potential debris (process may have crashed before writing metadata).
pub(crate) fn is_first_clean_attempt(task: &Task) -> bool {
    let pipeline_active = task.merge_pipeline_active.is_some();
    if pipeline_active {
        return false;
    }

    let has_debris_metadata = match task.metadata.as_ref() {
        None => false,
        Some(metadata_str) => match serde_json::from_str::<serde_json::Value>(metadata_str) {
            Err(_) => true,
            Ok(metadata) => match metadata.as_object() {
                None => false,
                Some(obj) => MERGE_DEBRIS_METADATA_KEYS
                    .iter()
                    .any(|key| obj.contains_key(*key)),
            },
        },
    };
    if has_debris_metadata {
        return false;
    }

    let disk_exists = task
        .worktree_path
        .as_ref()
        .map_or(false, |p| std::path::Path::new(p).exists());
    if disk_exists {
        return false;
    }

    true
}

/// Ensure the plan branch exists as a git ref (lazy creation for merge target).
///
/// Handles the case where the plan branch DB record exists but the git branch
/// was never created (e.g., lazy creation failed at execution time).
pub(super) async fn ensure_plan_branch_exists(
    task: &Task,
    repo_path: &Path,
    target_branch: &str,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
) {
    let Some(ref session_id) = task.ideation_session_id else {
        return;
    };
    let Some(ref pb_repo) = plan_branch_repo else {
        return;
    };
    let Ok(Some(pb)) = pb_repo.get_by_session_id(session_id).await else {
        return;
    };
    if pb.status != PlanBranchStatus::Active
        || pb.branch_name != target_branch
        || GitService::branch_exists(repo_path, target_branch)
            .await
            .unwrap_or(false)
    {
        return;
    }

    let task_id_str = task.id.as_str();
    match GitService::create_feature_branch(repo_path, &pb.branch_name, &pb.source_branch).await {
        Ok(_) => {
            tracing::info!(
                task_id = task_id_str,
                branch = %pb.branch_name,
                source = %pb.source_branch,
                "Lazily created plan branch for merge target"
            );
        }
        Err(_)
            if GitService::branch_exists(repo_path, &pb.branch_name)
                .await
                .unwrap_or(false) => {}
        Err(e) => {
            tracing::warn!(
                task_id = task_id_str,
                error = %e,
                branch = %pb.branch_name,
                "Failed to lazily create plan branch for merge"
            );
        }
    }
}

#[allow(dead_code)]
pub(crate) async fn clear_stale_worktree_path_on_timeout(
    task_id: &crate::domain::entities::TaskId,
    task_id_str: &str,
    task_repo: &Arc<dyn crate::domain::repositories::TaskRepository>,
) {
    cleanup::clear_stale_worktree_path_on_timeout(task_id, task_id_str, task_repo).await;
}
