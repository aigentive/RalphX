// Merge helper utilities: path computation, metadata parsing, branch resolution,
// worktree cleanup, and metadata merge.
//
// Extracted from side_effects.rs — pure helpers with no side effects beyond metadata mutation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lazy_static::lazy_static;
use regex::Regex;

use crate::application::publish_resilience::{
    count_existing_publish_branch_reviewable_commits, push_publish_branch,
};
use crate::application::GitService;
use crate::domain::entities::plan_branch::{PlanBranchId, PrPushStatus, PrStatus};
use crate::domain::entities::task_metadata::{MergeRecoveryMetadata, MergeRecoveryState};
use crate::domain::entities::InternalStatus;
use crate::domain::entities::{
    IdeationSessionId, PlanBranch, PlanBranchStatus, Project, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{
    ArtifactRepository, IdeationSessionRepository, PlanBranchRepository, TaskRepository,
};
use crate::domain::services::{GithubServiceTrait, PlanPrPublisher, PrReviewState};
use crate::domain::state_machine::context::TaskServices;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;

const COMMIT_HOOK_PATTERNS: &[&str] = &[
    "pre-commit",
    "[pre-commit]",
    "commit-msg",
    "prepare-commit-msg",
    "husky",
    "hook declined",
];

const COMMIT_HOOK_ENVIRONMENT_PATTERNS: &[&str] = &[
    "cannot find module",
    "module not found",
    "modulenotfounderror",
    "no module named",
    "importerror",
    "command not found",
    "no such file or directory",
    "enoent",
    "permission denied",
    "node_modules is missing",
    "could not find executable",
    "failed to spawn",
    "failed to load config",
];

const COMMIT_HOOK_POLICY_PATTERNS: &[&str] = &[
    "design-token",
    "eslint",
    "lint",
    "typecheck",
    "tsc",
    "prettier",
    "clippy",
    "fmt",
    "test failed",
    "tests failed",
    "error:",
];

lazy_static! {
    static ref ANSI_ESCAPE_RE: Regex =
        Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("valid ansi escape regex");
    static ref ABSOLUTE_PATH_RE: Regex =
        Regex::new(r#"(?m)(^|[\s=])(?:/[^\s:'"`]+)+"#).expect("valid path regex");
    static ref WINDOWS_PATH_RE: Regex =
        Regex::new(r#"(?i)[a-z]:\\[^\s:'"`]+(?:\\[^\s:'"`]+)*"#).expect("valid windows path regex");
    static ref UUID_RE: Regex =
        Regex::new(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
            .expect("valid uuid regex");
    static ref SHA_RE: Regex = Regex::new(r"\b[0-9a-f]{12,40}\b").expect("valid sha regex");
    static ref TIMESTAMP_RE: Regex =
        Regex::new(r"\b\d{4}-\d{2}-\d{2}[t ][0-9:.+-z]+\b").expect("valid timestamp regex");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitHookFailureKind {
    PolicyFailure,
    EnvironmentFailure,
    Unknown,
}

impl CommitHookFailureKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PolicyFailure => "policy_failure",
            Self::EnvironmentFailure => "environment_failure",
            Self::Unknown => "unknown",
        }
    }
}

// ===== Stale git state cleanup =====

/// Abort any stale MERGE_HEAD and rebase state in a worktree.
/// Idempotent: safe to call on clean worktrees.
pub(crate) async fn clean_stale_git_state(wt_path: &Path, task_id_str: &str) {
    if GitService::is_rebase_in_progress(wt_path) {
        tracing::info!(task_id = task_id_str, path = %wt_path.display(), "Aborting stale rebase");
        if let Err(e) = GitService::abort_rebase(wt_path).await {
            tracing::warn!(task_id = task_id_str, error = %e, "Failed to abort stale rebase (non-fatal)");
        }
    }
    if GitService::is_merge_in_progress(wt_path) {
        tracing::info!(task_id = task_id_str, path = %wt_path.display(), "Aborting stale merge");
        if let Err(e) = GitService::abort_merge(wt_path).await {
            tracing::warn!(task_id = task_id_str, error = %e, "Failed to abort stale merge (non-fatal)");
        }
    }
}

// ===== Worktree pre-deletion =====

/// Pre-delete stale worktree(s) using `run_cleanup_step` for uniform timeout and logging.
pub(crate) async fn pre_delete_worktree(repo_path: &Path, worktree: &Path, task_id: &str) {
    // Skip silently if the path was never created — avoids spurious WARN-level
    // "git worktree remove: not a working tree" logs on paths that don't exist.
    if !crate::utils::path_safety::checked_exists(worktree, "stale worktree").unwrap_or(false) {
        return;
    }

    use super::cleanup_helpers::CleanupStepResult;

    let wt_display = worktree.display().to_string();
    let label = format!("delete_stale_worktree({})", wt_display);
    let wt = worktree.to_path_buf();
    let rp = repo_path.to_path_buf();
    match super::cleanup_helpers::run_cleanup_step(
        &label,
        git_runtime_config().cleanup_worktree_timeout_secs,
        task_id,
        async move { GitService::delete_worktree(&rp, &wt).await },
    )
    .await
    {
        CleanupStepResult::Ok => {}
        CleanupStepResult::TimedOut { elapsed } => {
            tracing::warn!(
                task_id = task_id,
                worktree_path = %wt_display,
                elapsed_ms = elapsed.as_millis() as u64,
                "Stale worktree deletion timed out — merge worktree may fail to create"
            );
        }
        CleanupStepResult::Error { ref message } => {
            tracing::warn!(
                task_id = task_id,
                worktree_path = %wt_display,
                error = %message,
                "Stale worktree deletion failed — attempting second-chance force removal"
            );
            // Second-chance fallback: brief wait → direct rm-rf → git worktree prune.
            // Covers file-lock scenarios where the first attempt races a process still
            // holding handles inside the worktree directory.
            tokio::time::sleep(Duration::from_millis(100)).await;
            let second_chance_ok = match crate::utils::path_safety::checked_remove_dir_all(
                worktree,
                "stale worktree second-chance cleanup",
            )
            .await
            {
                Ok(()) => {
                    tracing::info!(
                        task_id = task_id,
                        worktree_path = %wt_display,
                        "Second-chance remove_dir_all succeeded — worktree cleared"
                    );
                    true
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = task_id,
                        worktree_path = %wt_display,
                        error = %e,
                        "Second-chance remove_dir_all also failed — worktree may block creation"
                    );
                    // Emit a directory listing to help diagnose which process holds the lock.
                    if let Ok(mut entries) = crate::utils::path_safety::checked_read_dir(
                        worktree,
                        "locked worktree diagnostics",
                    )
                    .await
                    {
                        let mut names = Vec::new();
                        while let Ok(Some(entry)) = entries.next_entry().await {
                            names.push(entry.file_name().to_string_lossy().into_owned());
                        }
                        tracing::error!(
                            task_id = task_id,
                            worktree_path = %wt_display,
                            entries = ?names,
                            "Locked worktree directory listing for diagnostics"
                        );
                    }
                    false
                }
            };
            // Run git worktree prune unconditionally — cleans stale internal git entries
            // even if the directory removal succeeded (git may still track the old path).
            super::cleanup_helpers::git_worktree_prune(repo_path).await;
            if !second_chance_ok
                && crate::utils::path_safety::checked_exists(
                    worktree,
                    "stale worktree post-cleanup check",
                )
                .unwrap_or(false)
            {
                tracing::error!(
                    task_id = task_id,
                    worktree_path = %wt_display,
                    "Worktree still present after second-chance cleanup — merge worktree creation will likely fail"
                );
            }
        }
    }
}

// ===== Metadata merge =====

/// Merge new_fields INTO task's existing metadata, preserving all existing keys.
/// Prevents RC#10-class bugs where metadata replacement clobbers recovery history.
pub(crate) fn merge_metadata_into(task: &mut Task, new_fields: &serde_json::Value) {
    let mut existing = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
    if let (Some(target), Some(source)) = (existing.as_object_mut(), new_fields.as_object()) {
        for (k, v) in source {
            target.insert(k.clone(), v.clone());
        }
    }
    task.metadata = Some(existing.to_string());
}

pub(crate) fn is_commit_hook_merge_error_text(message: &str) -> bool {
    let lowered = message.to_lowercase();
    lowered.contains("failed to commit")
        && COMMIT_HOOK_PATTERNS
            .iter()
            .any(|pattern| lowered.contains(pattern))
}

pub(crate) fn strip_ansi_escape_sequences(text: &str) -> String {
    ANSI_ESCAPE_RE.replace_all(text, "").into_owned()
}

pub(crate) fn sanitize_commit_hook_feedback_text(text: &str) -> String {
    strip_ansi_escape_sequences(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

pub(crate) fn classify_commit_hook_failure_text(text: &str) -> CommitHookFailureKind {
    let lowered = sanitize_commit_hook_feedback_text(text).to_lowercase();

    if COMMIT_HOOK_ENVIRONMENT_PATTERNS
        .iter()
        .any(|pattern| lowered.contains(pattern))
    {
        return CommitHookFailureKind::EnvironmentFailure;
    }

    if COMMIT_HOOK_POLICY_PATTERNS
        .iter()
        .any(|pattern| lowered.contains(pattern))
    {
        return CommitHookFailureKind::PolicyFailure;
    }

    CommitHookFailureKind::Unknown
}

pub(crate) fn commit_hook_failure_fingerprint(text: &str) -> String {
    let mut normalized = sanitize_commit_hook_feedback_text(text).to_lowercase();
    normalized = TIMESTAMP_RE
        .replace_all(&normalized, "<timestamp>")
        .into_owned();
    normalized = UUID_RE.replace_all(&normalized, "<uuid>").into_owned();
    normalized = SHA_RE.replace_all(&normalized, "<sha>").into_owned();
    normalized = WINDOWS_PATH_RE
        .replace_all(&normalized, "<path>")
        .into_owned();
    normalized = ABSOLUTE_PATH_RE
        .replace_all(&normalized, |captures: &regex::Captures<'_>| {
            format!(
                "{}<path>",
                captures.get(1).map(|m| m.as_str()).unwrap_or("")
            )
        })
        .into_owned();
    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn extract_commit_hook_merge_error(task: &Task) -> Option<String> {
    let meta = parse_metadata(task)?;
    for key in ["merge_revision_error", "error"] {
        if let Some(candidate) = meta.get(key).and_then(|value| value.as_str()) {
            if is_commit_hook_merge_error_text(candidate) {
                return Some(candidate.to_string());
            }
        }
    }

    meta.get("merge_recovery")
        .and_then(|value| value.get("events"))
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .rev()
        .filter_map(|event| event.get("message").and_then(|value| value.as_str()))
        .find(|message| is_commit_hook_merge_error_text(message))
        .map(str::to_string)
}

pub(crate) fn task_has_commit_hook_merge_failure(task: &Task) -> bool {
    extract_commit_hook_merge_error(task).is_some()
}

pub(crate) fn is_repeated_commit_hook_failure(task: &Task, fingerprint: &str) -> bool {
    let Some(meta) = parse_metadata(task) else {
        return false;
    };

    meta.get("merge_hook_failure_fingerprint")
        .and_then(|value| value.as_str())
        .map(|existing| existing == fingerprint)
        .unwrap_or(false)
        && meta
            .get("merge_hook_reexecution_requested")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
}

pub(crate) fn commit_hook_repeat_count(task: &Task, fingerprint: &str) -> u64 {
    let Some(meta) = parse_metadata(task) else {
        return 0;
    };

    if meta
        .get("merge_hook_failure_fingerprint")
        .and_then(|value| value.as_str())
        .map(|existing| existing == fingerprint)
        .unwrap_or(false)
    {
        meta.get("merge_hook_failure_repeat_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
    } else {
        0
    }
}

pub(crate) fn build_commit_hook_revision_feedback(error: &str) -> String {
    let sanitized = sanitize_commit_hook_feedback_text(error);
    let condensed = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    let lowered = condensed.to_lowercase();
    let excerpt = if let Some(start) = COMMIT_HOOK_PATTERNS
        .iter()
        .filter_map(|pattern| lowered.find(pattern))
        .min()
    {
        condensed[start..].to_string()
    } else {
        condensed
    };
    let excerpt = if excerpt.chars().count() > 220 {
        let truncated = excerpt.chars().take(220).collect::<String>();
        format!("{truncated}...")
    } else {
        excerpt
    };
    format!(
        "Repository commit hooks rejected the merge commit. Rework the task so it passes the repository's commit-time checks before merge. Key hook output: {}",
        excerpt
    )
}

pub(crate) fn build_commit_hook_review_note_body(error: &str) -> String {
    let sanitized = sanitize_commit_hook_feedback_text(error);
    let summary = build_commit_hook_revision_feedback(&sanitized);
    let fenced_output = sanitized.replace("```", "``\u{200B}`");

    format!("{summary}\n\nFull hook output:\n```text\n{fenced_output}\n```")
}

// ===== Pre-merge validation =====

/// Errors surfaced when pre-merge preconditions fail for a `plan_merge` task.
///
/// Each variant carries a human-readable message suitable for storing in task metadata
/// and displaying in the UI as an actionable error.
#[derive(Debug, PartialEq)]
pub(crate) enum PreMergeValidationError {
    /// `plan_branch_repo` is not wired in the service context.
    PlanBranchRepoNotWired,
    /// The `PlanBranch` record for this task's session has `status != Active`.
    PlanBranchNotActive { status: String },
    /// The feature branch does not exist in the git repository.
    FeatureBranchMissing { branch_name: String },
}

impl PreMergeValidationError {
    /// A short, human-readable error message for UI display.
    pub(crate) fn message(&self) -> String {
        match self {
            PreMergeValidationError::PlanBranchRepoNotWired => {
                "Plan branch repository is not configured. \
                 This is a server configuration error — please restart the application."
                    .to_string()
            }
            PreMergeValidationError::PlanBranchNotActive { status } => {
                format!(
                    "The plan branch is not active (current status: {status}). \
                     It may have already been merged or was abandoned. \
                     Check the plan branch status before retrying."
                )
            }
            PreMergeValidationError::FeatureBranchMissing { branch_name } => {
                format!(
                    "Feature branch '{branch_name}' does not exist in git. \
                     It may have been deleted. Re-create the branch or reset the plan to retry."
                )
            }
        }
    }

    /// A short machine-readable error code for metadata storage.
    pub(crate) fn error_code(&self) -> &'static str {
        match self {
            PreMergeValidationError::PlanBranchRepoNotWired => "plan_branch_repo_not_wired",
            PreMergeValidationError::PlanBranchNotActive { .. } => "plan_branch_not_active",
            PreMergeValidationError::FeatureBranchMissing { .. } => "feature_branch_missing",
        }
    }
}

/// Validate preconditions required before attempting a `plan_merge` task merge.
///
/// Checks:
/// 1. `plan_branch_repo` is wired (Some) in the context
/// 2. The `PlanBranch` record for this task has `status == Active`
/// 3. The feature branch referenced by the `PlanBranch` exists in git
///
/// Returns `Ok(())` when all checks pass, `Err(PreMergeValidationError)` on first failure.
/// Callers should transition the task to `MergeIncomplete` with the error's `message()` on failure.
///
/// Non-`plan_merge` tasks always pass (returns `Ok(())`).
pub(crate) async fn validate_plan_merge_preconditions(
    task: &Task,
    project: &Project,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
) -> Result<(), PreMergeValidationError> {
    // Only validate plan_merge category tasks
    if task.category != TaskCategory::PlanMerge {
        return Ok(());
    }

    // Check 1: plan_branch_repo must be wired
    let Some(ref pb_repo) = plan_branch_repo else {
        return Err(PreMergeValidationError::PlanBranchRepoNotWired);
    };

    // Check 2: PlanBranch must exist and have Active status
    let plan_branch = pb_repo.get_by_merge_task_id(&task.id).await.ok().flatten();

    let Some(pb) = plan_branch else {
        // No PlanBranch record for this merge task — treat as not active
        return Err(PreMergeValidationError::PlanBranchNotActive {
            status: "not_found".to_string(),
        });
    };

    if pb.status != PlanBranchStatus::Active {
        return Err(PreMergeValidationError::PlanBranchNotActive {
            status: pb.status.to_string(),
        });
    }

    // Check 3: Feature branch must exist in git
    let repo_path = Path::new(&project.working_directory);
    if !GitService::branch_exists(repo_path, &pb.branch_name)
        .await
        .unwrap_or(false)
    {
        return Err(PreMergeValidationError::FeatureBranchMissing {
            branch_name: pb.branch_name.clone(),
        });
    }

    Ok(())
}

/// Convert project name to a URL-safe slug for branch naming
pub(super) fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Expand `~/` prefix to the user's home directory
pub(super) fn expand_home(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/{}", home, stripped);
        }
    }
    path.to_string()
}

/// Compute the worktree path for a task execution worktree.
///
/// Convention: `{worktree_parent}/{slug}/task-{task_id}`
/// This is the worktree created during on_enter(Executing) for task isolation.
/// Used by pre_merge_cleanup to delete the task worktree even when
/// `task.worktree_path` has been overwritten by a prior merge attempt.
pub(crate) fn compute_task_worktree_path(project: &Project, task_id: &str) -> String {
    project
        .task_worktree_path(task_id)
        .to_string_lossy()
        .into_owned()
}

/// Compute the worktree path for a merge operation.
///
/// Convention: `{worktree_parent}/{slug}/merge-{task_id}`
/// This is separate from the task worktree (`task-{task_id}`) to allow
/// the merge to happen in isolation while the task worktree is deleted.
pub(crate) fn compute_merge_worktree_path(project: &Project, task_id: &str) -> String {
    let worktree_parent = project
        .worktree_parent_directory
        .as_deref()
        .unwrap_or("~/ralphx-worktrees");
    let expanded = expand_home(worktree_parent);
    format!("{}/{}/merge-{}", expanded, slugify(&project.name), task_id)
}

/// Compute the worktree path for a rebase operation.
///
/// Convention: `{worktree_parent}/{slug}/rebase-{task_id}`
/// This is separate from the merge worktree (`merge-{task_id}`) to allow
/// the rebase and merge steps to use different worktrees.
pub(crate) fn compute_rebase_worktree_path(project: &Project, task_id: &str) -> String {
    let worktree_parent = project
        .worktree_parent_directory
        .as_deref()
        .unwrap_or("~/ralphx-worktrees");
    let expanded = expand_home(worktree_parent);
    format!("{}/{}/rebase-{}", expanded, slugify(&project.name), task_id)
}

/// Compute the worktree path for a source-update operation (merging target into source branch).
///
/// Convention: `{worktree_parent}/{slug}/source-update-{task_id}`
/// This is a short-lived worktree used only to bring the feature/task branch up-to-date
/// with its target branch before the actual merge runs.
pub(super) fn compute_source_update_worktree_path(project: &Project, task_id: &str) -> String {
    let worktree_parent = project
        .worktree_parent_directory
        .as_deref()
        .unwrap_or("~/ralphx-worktrees");
    let expanded = expand_home(worktree_parent);
    format!(
        "{}/{}/source-update-{}",
        expanded,
        slugify(&project.name),
        task_id
    )
}

/// Compute the worktree path for a plan-update operation (merging main into plan branch).
///
/// Convention: `{worktree_parent}/{slug}/plan-update-{task_id}`
/// This is a short-lived worktree used only to bring the plan branch up-to-date with main
/// before the actual task→plan merge runs.
pub(crate) fn compute_plan_update_worktree_path(project: &Project, task_id: &str) -> String {
    let worktree_parent = project
        .worktree_parent_directory
        .as_deref()
        .unwrap_or("~/ralphx-worktrees");
    let expanded = expand_home(worktree_parent);
    format!(
        "{}/{}/plan-update-{}",
        expanded,
        slugify(&project.name),
        task_id
    )
}

/// Extract a task ID from a merge worktree path.
///
/// Merge worktree paths follow the convention: `{parent}/{slug}/merge-{task_id}`
/// Returns `Some(task_id)` if the path matches, `None` otherwise.
///
/// Currently unused in production (Step 5 orphan scan moved to Phase 3 deferred cleanup)
/// but retained for Phase 3 implementation and existing test coverage.
#[allow(dead_code)]
pub(super) fn extract_task_id_from_merge_path(path: &str) -> Option<&str> {
    let basename = path.rsplit('/').next()?;
    basename.strip_prefix("merge-")
}

/// Check if a task is currently in an active merge state.
///
/// Only covers `PendingMerge` and `Merging` where a merge worktree is actively in use.
/// Excludes `MergeIncomplete` and `MergeConflict` (human-waiting states) to allow
/// other tasks to clean up orphaned worktrees when merging to the same branch.
///
/// Currently unused in production (Step 5 orphan scan moved to Phase 3 deferred cleanup)
/// but retained for Phase 3 implementation and existing test coverage.
#[allow(dead_code)]
pub(super) async fn is_task_in_merge_workflow(
    task_repo: &Arc<dyn TaskRepository>,
    task_id_str: &str,
) -> bool {
    let task_id = TaskId::from_string(task_id_str.to_string());
    match task_repo.get_by_id(&task_id).await {
        Ok(Some(task)) => matches!(
            task.internal_status,
            InternalStatus::PendingMerge | InternalStatus::Merging
        ),
        _ => false,
    }
}

/// Check if a task's merge would target the given branch.
///
/// Resolves the task's merge target branch the same way `resolve_merge_branches()` does,
/// then compares against `target_branch`. Used by the concurrent merge guard to detect
/// tasks that would conflict with the same target.
pub(super) async fn task_targets_branch(
    task: &Task,
    project: &Project,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    target_branch: &str,
) -> bool {
    let (_, resolved_target) = resolve_merge_branches(task, project, plan_branch_repo).await;
    resolved_target == target_branch
}

/// Parse a task's metadata JSON string into a `serde_json::Value`.
///
/// Returns `None` if the task has no metadata or if parsing fails.
#[doc(hidden)]
pub fn parse_metadata(task: &Task) -> Option<serde_json::Value> {
    task.metadata
        .as_ref()
        .and_then(|m| serde_json::from_str(m).ok())
}

/// Check if a task is currently deferred from merging.
pub(crate) fn has_merge_deferred_metadata(task: &Task) -> bool {
    let legacy_deferred = parse_metadata(task)
        .and_then(|v| v.get("merge_deferred")?.as_bool())
        .unwrap_or(false);
    if legacy_deferred {
        return true;
    }

    MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
        .ok()
        .flatten()
        .map(|metadata| metadata.last_state == MergeRecoveryState::Deferred)
        .unwrap_or(false)
}

/// Check if a task had a prior rebase conflict (merger was invoked for rebase conflicts).
///
/// Returns `true` if metadata contains `conflict_type: "rebase"`, set when a RebaseSquash
/// or Rebase strategy returned NeedsAgent. Used to skip the rebase step on retry and use
/// squash-only instead, avoiding re-encountering the same conflicts.
pub(crate) fn has_prior_rebase_conflict(task: &Task) -> bool {
    parse_metadata(task)
        .and_then(|v| v.get("conflict_type")?.as_str().map(|s| s == "rebase"))
        .unwrap_or(false)
}

/// Check if a task had a source_update_conflict that was resolved by the merger agent.
///
/// Returns `true` if metadata contains `source_conflict_resolved: true`. Set by
/// `handle_source_update_resolution` after the agent merges the target INTO the source
/// branch. Used to skip the rebase step on the PendingMerge retry — rebasing would drop
/// the agent's merge commit and replay individual commits, re-encountering the same conflicts.
pub(crate) fn has_source_conflict_resolved(task: &Task) -> bool {
    parse_metadata(task)
        .and_then(|v| v.get("source_conflict_resolved")?.as_bool())
        .unwrap_or(false)
}

/// Set the `source_conflict_resolved` flag in a task's metadata.
///
/// Called after the merger agent successfully resolves a source←target conflict.
/// Signals `dispatch_merge_strategy` to use squash-only instead of rebase on retry.
pub(crate) fn set_source_conflict_resolved(task: &mut Task) {
    let mut meta = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert(
            "source_conflict_resolved".to_string(),
            serde_json::json!(true),
        );
    }
    task.metadata = Some(meta.to_string());
}

/// Check if a task has the `branch_missing` flag set in its metadata.
pub(crate) fn has_branch_missing_metadata(task: &Task) -> bool {
    parse_metadata(task)
        .and_then(|v| v.get("branch_missing")?.as_bool())
        .unwrap_or(false)
}

/// Check if task metadata indicates prior validation failures that should block
/// fast-path merge completion (used by `check_already_merged` and
/// `recover_deleted_source_branch` to avoid completing merges with broken code).
///
/// Returns `true` if ANY of these are set:
/// - `merge_commit_unrevertable`: prior merge commit couldn't be reverted
/// - `merge_failure_source` == `"validation_failed"`: prior merge failed validation
/// - `validation_revert_count` > 0: prior validation-triggered reverts
pub(super) fn has_prior_validation_failure(task: &Task) -> bool {
    let Some(meta) = parse_metadata(task) else {
        return false;
    };
    if meta
        .get("merge_commit_unrevertable")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    if meta
        .get("merge_failure_source")
        .and_then(|v| v.as_str())
        .map(|s| s == "validation_failed")
        .unwrap_or(false)
    {
        return true;
    }
    if meta
        .get("validation_revert_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        > 0
    {
        return true;
    }
    false
}

/// Check whether all tasks in a session have reached the terminal `Merged` status.
///
/// Returns `Ok(true)` if every task associated with the session is in `InternalStatus::Merged`.
/// Returns `Ok(false)` if any task has not yet merged.
/// Returns `Err(...)` on repository failure (logged at WARN level before propagating).
#[allow(dead_code)] // Phase 0 infrastructure — called in Phase 1+2
pub(crate) async fn check_session_all_merged(
    session_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
) -> AppResult<bool> {
    let session_id_typed = IdeationSessionId::from_string(session_id.to_string());
    let tasks = task_repo
        .get_by_ideation_session(&session_id_typed)
        .await
        .map_err(|e| {
            tracing::warn!(
                session_id = session_id,
                error = %e,
                "Failed to fetch session tasks for plan:delivered check"
            );
            e
        })?;
    Ok(tasks
        .iter()
        .all(|t| matches!(t.internal_status, InternalStatus::Merged)))
}

/// Check if a task has the `main_merge_deferred` flag set in its metadata.
/// This flag indicates a merge to main was deferred because agents were running.
pub(crate) fn has_main_merge_deferred_metadata(task: &Task) -> bool {
    parse_metadata(task)
        .and_then(|v| v.get("main_merge_deferred")?.as_bool())
        .unwrap_or(false)
}

/// Set the `main_merge_deferred` flag and `main_merge_deferred_at` timestamp in a task's metadata.
///
/// This is called when a merge to main is deferred because agents are running.
/// Mutates the task in-place, creating metadata if it doesn't exist.
pub(crate) fn set_main_merge_deferred_metadata(task: &mut Task) {
    let mut meta = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("main_merge_deferred".to_string(), serde_json::json!(true));
        obj.insert(
            "main_merge_deferred_at".to_string(),
            serde_json::json!(chrono::Utc::now().to_rfc3339()),
        );
    }
    task.metadata = Some(meta.to_string());
}

/// Clear the `main_merge_deferred` and `main_merge_deferred_at` fields from a task's metadata.
///
/// Called when retrying a main-merge-deferred task after agents go idle.
/// Mutates the task in-place. If the metadata becomes an empty object after removal,
/// clears metadata entirely.
/// TODO(Phase 3): Used by try_retry_main_merges() when all agents go idle
#[allow(dead_code)]
pub(crate) fn clear_main_merge_deferred_metadata(task: &mut Task) {
    let Some(mut meta) = parse_metadata(task) else {
        return;
    };
    if let Some(obj) = meta.as_object_mut() {
        obj.remove("main_merge_deferred");
        obj.remove("main_merge_deferred_at");
        if obj.is_empty() {
            task.metadata = None;
        } else {
            task.metadata = Some(meta.to_string());
        }
    }
}

/// Clear the `merge_deferred` and `merge_deferred_at` fields from a task's metadata.
///
/// Mutates the task in-place. If the metadata becomes an empty object after removal,
/// clears metadata entirely.
pub(crate) fn clear_merge_deferred_metadata(task: &mut Task) {
    let Some(mut meta) = parse_metadata(task) else {
        return;
    };
    if let Some(obj) = meta.as_object_mut() {
        obj.remove("merge_deferred");
        obj.remove("merge_deferred_at");
        if obj.is_empty() {
            task.metadata = None;
        } else {
            task.metadata = Some(meta.to_string());
        }
    }
}

/// Default timeout in seconds after which a deferred merge is forced to retry.
pub const DEFERRED_MERGE_TIMEOUT_SECONDS: i64 = 120;

/// Check if a `merge_deferred` task has exceeded the configured timeout.
///
/// Returns true if the `merge_deferred_at` timestamp in metadata is older than
/// `DEFERRED_MERGE_TIMEOUT_SECONDS`. Returns false if the timestamp is missing or unparseable
/// (no timeout enforcement in that case — the reconciliation watchdog handles it instead).
pub(crate) fn is_merge_deferred_timed_out(task: &Task) -> bool {
    let deferred_at =
        parse_metadata(task).and_then(|v| v.get("merge_deferred_at")?.as_str().map(String::from));

    let Some(deferred_at_str) = deferred_at else {
        return false;
    };

    let Ok(deferred_at) = chrono::DateTime::parse_from_rfc3339(&deferred_at_str) else {
        return false;
    };

    let elapsed = chrono::Utc::now().signed_duration_since(deferred_at.with_timezone(&chrono::Utc));
    elapsed.num_seconds() >= DEFERRED_MERGE_TIMEOUT_SECONDS
}

/// Check if a `main_merge_deferred` task has exceeded the configured timeout.
///
/// Returns true if the `main_merge_deferred_at` timestamp in metadata is older than
/// `DEFERRED_MERGE_TIMEOUT_SECONDS`. Returns false if the timestamp is missing or unparseable.
pub(crate) fn is_main_merge_deferred_timed_out(task: &Task) -> bool {
    let deferred_at = parse_metadata(task)
        .and_then(|v| v.get("main_merge_deferred_at")?.as_str().map(String::from));

    let Some(deferred_at_str) = deferred_at else {
        return false;
    };

    let Ok(deferred_at) = chrono::DateTime::parse_from_rfc3339(&deferred_at_str) else {
        return false;
    };

    let elapsed = chrono::Utc::now().signed_duration_since(deferred_at.with_timezone(&chrono::Utc));
    elapsed.num_seconds() >= DEFERRED_MERGE_TIMEOUT_SECONDS
}

/// Set the `trigger_origin` field in a task's metadata.
///
/// Valid origins: "scheduler", "revision", "recovery", "retry", "qa".
/// Mutates the task in-place, creating metadata if it doesn't exist.
#[doc(hidden)]
pub fn set_trigger_origin(task: &mut Task, origin: &str) {
    let mut meta = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("trigger_origin".to_string(), serde_json::json!(origin));
    }
    task.metadata = Some(meta.to_string());
}

/// Get the `trigger_origin` field from a task's metadata.
///
/// Returns the origin string if present, otherwise `None`.
pub(crate) fn get_trigger_origin(task: &Task) -> Option<String> {
    parse_metadata(task).and_then(|v| v.get("trigger_origin")?.as_str().map(String::from))
}

/// Clear the `trigger_origin` field from a task's metadata.
///
/// Mutates the task in-place. If the metadata becomes an empty object after removal,
/// clears metadata entirely.
pub(crate) fn clear_trigger_origin(task: &mut Task) {
    let Some(mut meta) = parse_metadata(task) else {
        return;
    };
    if let Some(obj) = meta.as_object_mut() {
        obj.remove("trigger_origin");
        if obj.is_empty() {
            task.metadata = None;
        } else {
            task.metadata = Some(meta.to_string());
        }
    }
}

/// Set conflict metadata in a task's metadata.
///
/// Stores:
/// - `conflict_files`: array of file paths with conflicts
/// - `conflict_snapshot_at`: ISO 8601 timestamp when conflicts were detected
/// - `conflict_detected_by`: "programmatic" (system) or "agent" (via report_conflict)
///
/// Mutates the task in-place, creating metadata if it doesn't exist.
pub(crate) fn set_conflict_metadata(task: &mut Task, conflict_files: &[String], detected_by: &str) {
    let mut meta = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert(
            "conflict_files".to_string(),
            serde_json::json!(conflict_files),
        );
        obj.insert(
            "conflict_snapshot_at".to_string(),
            serde_json::json!(chrono::Utc::now().to_rfc3339()),
        );
        obj.insert(
            "conflict_detected_by".to_string(),
            serde_json::json!(detected_by),
        );
    }
    task.metadata = Some(meta.to_string());
}

/// Get the `revision_count` from a task's metadata.
///
/// Returns the current revision cycle count, or 0 if not set.
pub(crate) fn get_revision_count(task: &Task) -> u32 {
    parse_metadata(task)
        .and_then(|v| v.get("revision_count")?.as_u64())
        .unwrap_or(0) as u32
}

/// Increment the `revision_count` in a task's metadata.
///
/// Mutates the task in-place, creating metadata if it doesn't exist.
/// Returns the new revision count after incrementing.
pub(crate) fn increment_revision_count(task: &mut Task) -> u32 {
    let mut meta = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
    let current = meta
        .get("revision_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let new_count = current + 1;
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("revision_count".to_string(), serde_json::json!(new_count));
    }
    task.metadata = Some(meta.to_string());
    new_count
}

/// Resolve the base branch for a task's working branch.
///
/// If the task belongs to a plan with an active feature branch, returns the feature
/// branch name so the task branch is created from it. Otherwise falls back to the
/// project's base branch.
///
/// When the plan branch is Merged, returns the project base branch (defense-in-depth).
/// This prevents resurrecting completed plans by recreating deleted branches.
///
/// When `pr_creation_guard` and `github_service` are provided and the plan branch has
/// `pr_eligible = true`, attempts to create a draft PR for the plan branch (non-blocking).
#[allow(clippy::too_many_arguments)]
pub(super) async fn resolve_task_base_branch(
    task: &Task,
    project: &Project,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    _task_repo: &Option<Arc<dyn TaskRepository>>,
    pr_creation_guard: &Option<Arc<dashmap::DashMap<PlanBranchId, ()>>>,
    github_service: &Option<Arc<dyn GithubServiceTrait>>,
) -> String {
    let default = project.base_branch.as_deref().unwrap_or("main").to_string();

    let Some(ref plan_branch_repo) = plan_branch_repo else {
        return default;
    };

    let Some(pb) = resolve_task_plan_branch_record(task, plan_branch_repo).await else {
        return default;
    };

    match pb.status {
        PlanBranchStatus::Active => {
            let repo_path = Path::new(&project.working_directory);
            // Lazily create git branch on first task execution
            if !GitService::branch_exists(repo_path, &pb.branch_name)
                .await
                .unwrap_or(false)
            {
                match GitService::create_feature_branch(
                    repo_path,
                    &pb.branch_name,
                    &pb.source_branch,
                )
                .await
                {
                    Ok(_) => {
                        tracing::info!(
                            branch = %pb.branch_name,
                            source = %pb.source_branch,
                            "Created deferred plan branch"
                        );
                    }
                    Err(e) => {
                        // Race condition: another task may have created it concurrently
                        if GitService::branch_exists(repo_path, &pb.branch_name)
                            .await
                            .unwrap_or(false)
                        {
                            tracing::info!(
                                branch = %pb.branch_name,
                                "Deferred plan branch created by concurrent task"
                            );
                        } else {
                            tracing::warn!(
                                error = %e,
                                branch = %pb.branch_name,
                                "Failed to create deferred plan branch, falling back to project base"
                            );
                            return default;
                        }
                    }
                }
            }

            // Draft PR creation (AD10) — only when eligible and github_service is available
            if pb.pr_eligible {
                if let (Some(guard), Some(gh_svc)) =
                    (pr_creation_guard.as_ref(), github_service.as_ref())
                {
                    create_draft_pr_if_needed(
                        task,
                        project,
                        &pb,
                        guard,
                        gh_svc,
                        &Arc::clone(plan_branch_repo),
                        None,
                        None,
                    )
                    .await;
                }
            }

            tracing::info!(
                task_id = task.id.as_str(),
                feature_branch = %pb.branch_name,
                "Resolved task base branch to plan feature branch"
            );
            pb.branch_name
        }
        PlanBranchStatus::Merged => {
            // Plan branch is already merged — do NOT resurrect it.
            // Recreating a merged branch would undo the completed plan merge,
            // allowing tasks to execute against a stale branch.
            tracing::warn!(
                task_id = task.id.as_str(),
                branch = %pb.branch_name,
                plan_branch_id = pb.id.as_str(),
                "Plan branch is merged — refusing to resurrect, falling back to project base"
            );
            default
        }
        PlanBranchStatus::Abandoned => default,
    }
}

pub(crate) async fn resolve_task_plan_branch_record(
    task: &Task,
    plan_branch_repo: &Arc<dyn PlanBranchRepository>,
) -> Option<PlanBranch> {
    if let Some(exec_plan_id) = task.execution_plan_id.as_ref() {
        if let Ok(Some(pb)) = plan_branch_repo
            .get_by_execution_plan_id(exec_plan_id)
            .await
        {
            return Some(pb);
        }
    }

    let session_id = task.ideation_session_id.as_ref()?;
    plan_branch_repo
        .get_by_session_id(session_id)
        .await
        .ok()
        .flatten()
}

pub(crate) async fn resolve_effective_base_branch(
    task: &Task,
    project: &Project,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    merge_target_branch: Option<&str>,
) -> String {
    let default = project.base_branch.as_deref().unwrap_or("main").to_string();

    let Some(plan_branch_repo) = plan_branch_repo.as_ref() else {
        return default;
    };
    let Some(pb) = resolve_task_plan_branch_record(task, plan_branch_repo).await else {
        return default;
    };

    match merge_target_branch {
        Some(target_branch) if target_branch == pb.branch_name => pb.source_branch,
        Some(_) => pb.base_branch_override.unwrap_or(default),
        None => pb.source_branch,
    }
}

pub(crate) async fn plan_regular_tasks_complete(
    current_task: &Task,
    pb: &PlanBranch,
    task_repo: Option<&Arc<dyn TaskRepository>>,
) -> bool {
    let Some(task_repo) = task_repo else {
        return false;
    };

    let tasks = if let Some(execution_plan_id) = pb.execution_plan_id.as_ref() {
        match task_repo
            .list_paginated(
                &pb.project_id,
                None,
                0,
                10_000,
                false,
                None,
                Some(execution_plan_id.as_str()),
                None,
            )
            .await
        {
            Ok(tasks) => tasks,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    execution_plan_id = execution_plan_id.as_str(),
                    "PR mode: failed to query execution-plan tasks for ready check"
                );
                return false;
            }
        }
    } else {
        match task_repo.get_by_ideation_session(&pb.session_id).await {
            Ok(tasks) => tasks,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    session_id = pb.session_id.as_str(),
                    "PR mode: failed to query session tasks for ready check"
                );
                return false;
            }
        }
    };

    tasks
        .iter()
        .filter(|task| task.archived_at.is_none())
        .filter(|task| task.category != TaskCategory::PlanMerge)
        .all(|task| {
            task.id == current_task.id || matches!(task.internal_status, InternalStatus::Merged)
        })
}

async fn push_pr_branch_to_remote(
    project: &Project,
    pb: &PlanBranch,
    github: &Arc<dyn GithubServiceTrait>,
    plan_branch_repo: &Arc<dyn PlanBranchRepository>,
) -> bool {
    let repo_path = Path::new(&project.working_directory);
    tracing::info!(
        branch = %pb.branch_name,
        pr_number = ?pb.pr_number,
        "sync_plan_branch_pr_if_needed: pushing updated plan branch to GitHub"
    );

    match push_publish_branch(github, repo_path, &pb.branch_name).await {
        Ok(()) => {
            if let Err(e) = plan_branch_repo
                .update_pr_push_status(&pb.id, PrPushStatus::Pushed)
                .await
            {
                tracing::warn!(error = %e, "Failed to update pr_push_status=pushed");
            }
            true
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                branch = %pb.branch_name,
                "sync_plan_branch_pr_if_needed: push failed — leaving PR branch out of date until retry"
            );
            let _ = plan_branch_repo
                .update_pr_push_status(&pb.id, PrPushStatus::Failed)
                .await;
            false
        }
    }
}

pub(crate) async fn sync_plan_branch_pr_if_needed(
    project: &Project,
    pb: &PlanBranch,
    github: &Arc<dyn GithubServiceTrait>,
    plan_branch_repo: &Arc<dyn PlanBranchRepository>,
) {
    if !pb.pr_eligible
        || pb.status != PlanBranchStatus::Active
        || pb.pr_number.is_none()
        || matches!(pb.pr_push_status, PrPushStatus::Pushed)
    {
        return;
    }

    let _ = push_pr_branch_to_remote(project, pb, github, plan_branch_repo).await;
}

pub(crate) async fn sync_existing_plan_branch_pr_details(
    task: &Task,
    project: &Project,
    pb: &PlanBranch,
    github: &Arc<dyn GithubServiceTrait>,
    ideation_session_repo: Option<&Arc<dyn IdeationSessionRepository>>,
    artifact_repo: Option<&Arc<dyn ArtifactRepository>>,
    review_state: PrReviewState,
) -> AppResult<()> {
    PlanPrPublisher::new(github, ideation_session_repo, artifact_repo)
        .sync_existing_pr(task, project, pb, review_state)
        .await
}

#[derive(Clone, Default)]
pub(crate) struct PlanBranchPrSyncServices {
    pub task_repo: Option<Arc<dyn TaskRepository>>,
    pub plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    pub pr_creation_guard: Option<Arc<dashmap::DashMap<PlanBranchId, ()>>>,
    pub github_service: Option<Arc<dyn GithubServiceTrait>>,
    pub ideation_session_repo: Option<Arc<dyn IdeationSessionRepository>>,
    pub artifact_repo: Option<Arc<dyn ArtifactRepository>>,
}

impl PlanBranchPrSyncServices {
    pub(crate) fn from_task_services(services: &TaskServices) -> Self {
        Self {
            task_repo: services.task_repo.clone(),
            plan_branch_repo: services.plan_branch_repo.clone(),
            pr_creation_guard: services.pr_creation_guard.clone(),
            github_service: services.github_service.clone(),
            ideation_session_repo: services.ideation_session_repo.clone(),
            artifact_repo: services.artifact_repo.clone(),
        }
    }
}

pub(crate) async fn sync_plan_branch_pr_after_regular_task_merge(
    task: &Task,
    project: &Project,
    services: &PlanBranchPrSyncServices,
) {
    if task.category == TaskCategory::PlanMerge {
        return;
    }

    let Some(plan_branch_repo) = services.plan_branch_repo.as_ref() else {
        return;
    };
    let Some(plan_branch) = resolve_task_plan_branch_record(task, plan_branch_repo).await else {
        return;
    };

    if !plan_branch.pr_eligible || plan_branch.status != PlanBranchStatus::Active {
        return;
    }

    if plan_branch.pr_number.is_some() {
        let _ = plan_branch_repo
            .update_pr_push_status(&plan_branch.id, PrPushStatus::Pending)
            .await;
    }

    let Some(github_service) = services.github_service.as_ref() else {
        return;
    };

    let ready_for_review =
        plan_regular_tasks_complete(task, &plan_branch, services.task_repo.as_ref()).await;

    if plan_branch.pr_number.is_some() {
        let mut refreshed_plan_branch = plan_branch.clone();
        refreshed_plan_branch.pr_push_status = PrPushStatus::Pending;
        let pushed = push_pr_branch_to_remote(
            project,
            &refreshed_plan_branch,
            github_service,
            plan_branch_repo,
        )
        .await;
        if pushed {
            let review_state = if ready_for_review {
                PrReviewState::Ready
            } else {
                PrReviewState::Draft
            };
            if let Err(e) = sync_existing_plan_branch_pr_details(
                task,
                project,
                &refreshed_plan_branch,
                github_service,
                services.ideation_session_repo.as_ref(),
                services.artifact_repo.as_ref(),
                review_state,
            )
            .await
            {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    error = %e,
                    "PR mode: failed to refresh PR details after plan branch push"
                );
            }
            if ready_for_review {
                if let Some(pr_number) = refreshed_plan_branch.pr_number {
                    if let Err(e) = github_service
                        .mark_pr_ready(Path::new(&project.working_directory), pr_number)
                        .await
                    {
                        tracing::warn!(
                            task_id = task.id.as_str(),
                            pr_number,
                            error = %e,
                            "PR mode: failed to mark PR ready after final plan task merge"
                        );
                    }
                }
            }
        }
    } else if let Some(pr_creation_guard) = services.pr_creation_guard.as_ref() {
        create_draft_pr_if_needed(
            task,
            project,
            &plan_branch,
            pr_creation_guard,
            github_service,
            plan_branch_repo,
            services.ideation_session_repo.as_ref(),
            services.artifact_repo.as_ref(),
        )
        .await;
        if ready_for_review {
            match plan_branch_repo.get_by_id(&plan_branch.id).await {
                Ok(Some(refreshed_plan_branch)) if refreshed_plan_branch.pr_number.is_some() => {
                    if let Err(e) = sync_existing_plan_branch_pr_details(
                        task,
                        project,
                        &refreshed_plan_branch,
                        github_service,
                        services.ideation_session_repo.as_ref(),
                        services.artifact_repo.as_ref(),
                        PrReviewState::Ready,
                    )
                    .await
                    {
                        tracing::warn!(
                            task_id = task.id.as_str(),
                            error = %e,
                            "PR mode: failed to refresh newly-created PR details before ready"
                        );
                    }
                    if let Some(pr_number) = refreshed_plan_branch.pr_number {
                        if let Err(e) = github_service
                            .mark_pr_ready(Path::new(&project.working_directory), pr_number)
                            .await
                        {
                            tracing::warn!(
                                task_id = task.id.as_str(),
                                pr_number,
                                error = %e,
                                "PR mode: failed to mark newly-created PR ready after final plan task merge"
                            );
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "PR mode: failed to reload plan branch after draft PR creation"
                    );
                }
            }
        }
    }
}

pub(crate) fn resolve_plan_branch_pr_base(project: &Project, pb: &PlanBranch) -> String {
    pb.base_branch_override
        .clone()
        .or_else(|| project.base_branch.clone())
        .unwrap_or_else(|| pb.source_branch.clone())
}

pub(crate) async fn plan_branch_reviewable_commit_count(
    project: &Project,
    pb: &PlanBranch,
) -> AppResult<u32> {
    let repo_path = Path::new(&project.working_directory);
    let pr_base = resolve_plan_branch_pr_base(project, pb);
    count_existing_publish_branch_reviewable_commits(repo_path, &pb.branch_name, &pr_base).await
}

pub(crate) async fn plan_branch_has_reviewable_diff(
    project: &Project,
    pb: &PlanBranch,
) -> AppResult<bool> {
    Ok(plan_branch_reviewable_commit_count(project, pb).await? > 0)
}

/// Create a draft PR for the plan branch if not already created.
///
/// CAS guard (AD10): DashMap entry prevents duplicate creation across concurrent task executions.
/// Idempotent: re-reads pr_number from DB inside guard — skips if already set.
/// Non-blocking: errors are logged and silently skipped (PR can be created at PendingMerge time).
/// Timeout: entire flow wrapped in 30s timeout.
pub(crate) async fn create_draft_pr_if_needed(
    task: &Task,
    project: &Project,
    pb: &crate::domain::entities::PlanBranch,
    guard: &Arc<dashmap::DashMap<crate::domain::entities::PlanBranchId, ()>>,
    github: &Arc<dyn GithubServiceTrait>,
    plan_branch_repo: &Arc<dyn PlanBranchRepository>,
    ideation_session_repo: Option<&Arc<dyn IdeationSessionRepository>>,
    artifact_repo: Option<&Arc<dyn ArtifactRepository>>,
) {
    use tokio::time::{timeout, Duration};

    let plan_branch_id = pb.id.clone();
    let branch_name = pb.branch_name.clone();
    let repo_path = Path::new(&project.working_directory);

    // Acquire CAS guard — prevents concurrent PR creation for same plan branch.
    // We insert without holding the RefMut so the shard lock is immediately released,
    // allowing the defer! below to call guard.remove() without deadlocking.
    guard.entry(plan_branch_id.clone()).or_insert(());
    // Remove the guard entry when function exits (regardless of success/failure/panic)
    let guard_ref = Arc::clone(guard);
    let plan_branch_id_defer = plan_branch_id.clone();
    scopeguard::defer! { guard_ref.remove(&plan_branch_id_defer); };

    // Re-read from DB inside guard — if pr_number already set, skip (idempotent)
    let current_pb = match plan_branch_repo.get_by_session_id(&pb.session_id).await {
        Ok(Some(fresh)) => fresh,
        _ => {
            tracing::warn!(
                branch = %branch_name,
                "create_draft_pr_if_needed: failed to re-read PlanBranch"
            );
            return;
        }
    };
    if current_pb.pr_number.is_some() {
        tracing::debug!(
            branch = %branch_name,
            pr_number = ?current_pb.pr_number,
            "create_draft_pr_if_needed: PR already exists — skipping"
        );
        return;
    }

    let reviewable_commit_count = match plan_branch_reviewable_commit_count(project, &current_pb)
        .await
    {
        Ok(count) => count,
        Err(e) => {
            tracing::warn!(
                branch = %branch_name,
                error = %e,
                "create_draft_pr_if_needed: failed to determine whether the plan branch is ahead of base"
            );
            return;
        }
    };
    if reviewable_commit_count == 0 {
        tracing::debug!(
            branch = %branch_name,
            "create_draft_pr_if_needed: skipping PR creation because the plan branch has no reviewable changes yet"
        );
        return;
    }

    // Run with timeout — task proceeds normally on timeout
    let timed_out = timeout(Duration::from_secs(30), async {
        // --- PUSH ---
        let needs_push = !matches!(current_pb.pr_push_status, PrPushStatus::Pushed);
        if needs_push {
            tracing::info!(branch = %branch_name, "create_draft_pr_if_needed: pushing branch");
            match push_publish_branch(github, repo_path, &branch_name).await {
                Ok(()) => {
                    if let Err(e) = plan_branch_repo
                        .update_pr_push_status(&plan_branch_id, PrPushStatus::Pushed)
                        .await
                    {
                        tracing::warn!(error = %e, "Failed to update pr_push_status=pushed");
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        branch = %branch_name,
                        "create_draft_pr_if_needed: push failed — will retry next execution"
                    );
                    let _ = plan_branch_repo
                        .update_pr_push_status(&plan_branch_id, PrPushStatus::Failed)
                        .await;
                    return;
                }
            }
        }

        // --- CREATE DRAFT PR ---
        let base = resolve_plan_branch_pr_base(project, &current_pb);
        let publisher = PlanPrPublisher::new(github, ideation_session_repo, artifact_repo);

        tracing::info!(
            branch = %branch_name,
            base = %base,
            "create_draft_pr_if_needed: creating draft PR"
        );
        match publisher.create_draft_pr(task, project, &current_pb).await {
            Ok((pr_number, pr_url)) => {
                tracing::info!(pr_number, %pr_url, "Draft PR created");
                if let Err(e) = plan_branch_repo
                    .update_pr_info(
                        &plan_branch_id,
                        pr_number,
                        pr_url,
                        PrStatus::Open,
                        true,
                    )
                    .await
                {
                    tracing::warn!(error = %e, "Failed to persist PR info after creation");
                }
            }
            Err(AppError::DuplicatePr) => {
                // PR already exists — recover existing PR number
                tracing::info!(
                    branch = %branch_name,
                    "create_draft_pr_if_needed: duplicate PR detected — recovering existing PR"
                );
                match github.find_pr_by_head_branch(repo_path, &branch_name).await {
                    Ok(Some((pr_number, pr_url))) => {
                        tracing::info!(pr_number, %pr_url, "Recovered existing PR");
                        if let Err(e) = plan_branch_repo
                            .update_pr_info(
                                &plan_branch_id,
                                pr_number,
                                pr_url.clone(),
                                PrStatus::Open,
                                true,
                            )
                            .await
                        {
                            tracing::warn!(error = %e, "Failed to persist recovered PR info");
                        }
                        let mut recovered_pb = current_pb.clone();
                        recovered_pb.pr_number = Some(pr_number);
                        recovered_pb.pr_url = Some(pr_url);
                        if let Err(e) = publisher
                            .sync_existing_pr(task, project, &recovered_pb, PrReviewState::Draft)
                            .await
                        {
                            tracing::warn!(
                                error = %e,
                                "Failed to refresh recovered existing PR after duplicate error"
                            );
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(
                            branch = %branch_name,
                            "Duplicate PR but find_pr_by_head_branch found nothing"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to recover existing PR after duplicate error"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    branch = %branch_name,
                    "create_draft_pr_if_needed: PR creation failed — will retry at PendingMerge time"
                );
            }
        }
    })
    .await;

    if timed_out.is_err() {
        tracing::warn!(
            branch = %branch_name,
            "create_draft_pr_if_needed: timed out after 30s — task proceeds normally"
        );
    }
}

/// Resolve the source and target branches for a merge operation.
///
/// Returns `(source_branch, target_branch)`:
/// - **Merge task** (task is `plan_branches.merge_task_id`): merge feature branch into project base
/// - **Plan task with feature branch**: merge task branch into feature branch
/// - **Regular task**: merge task branch into project base branch
pub async fn resolve_merge_branches(
    task: &Task,
    project: &Project,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
) -> (String, String) {
    let base_branch = project.base_branch.as_deref().unwrap_or("main").to_string();
    let task_branch = task.task_branch.clone().unwrap_or_default();

    tracing::debug!(
        task_id = task.id.as_str(),
        category = %task.category,
        plan_branch_repo_available = plan_branch_repo.is_some(),
        ideation_session_id = ?task.ideation_session_id.as_ref().map(|s| s.as_str()),
        task_branch = %task_branch,
        base_branch = %base_branch,
        "resolve_merge_branches: entry"
    );

    let Some(ref plan_branch_repo) = plan_branch_repo else {
        if task.category == TaskCategory::PlanMerge {
            tracing::warn!(
                task_id = task.id.as_str(),
                "resolve_merge_branches: plan_branch_repo is None for plan_merge task — \
                 merge branch resolution will fall back to task_branch/base_branch"
            );
        }
        return (task_branch, base_branch);
    };

    // Check if this task IS the merge task for a plan branch
    if let Ok(Some(pb)) = plan_branch_repo.get_by_merge_task_id(&task.id).await {
        if pb.status != PlanBranchStatus::Active {
            tracing::warn!(
                task_id = task.id.as_str(),
                feature_branch = %pb.branch_name,
                plan_branch_status = ?pb.status,
                "Merge task: plan branch is not Active, but still using it as source \
                 to avoid incorrect merge direction"
            );
        }
        tracing::debug!(
            task_id = task.id.as_str(),
            feature_branch = %pb.branch_name,
            base_branch = %base_branch,
            "Merge task: merging feature branch into base"
        );
        return (
            pb.branch_name,
            pb.base_branch_override.clone().unwrap_or(base_branch),
        );
    }

    // Check if this task belongs to a plan with a feature branch
    if task.ideation_session_id.is_some() || task.execution_plan_id.is_some() {
        if let Some(pb) = resolve_task_plan_branch_record(task, plan_branch_repo).await {
            if pb.status == PlanBranchStatus::Active {
                tracing::debug!(
                    task_id = task.id.as_str(),
                    task_branch = %task_branch,
                    feature_branch = %pb.branch_name,
                    "Plan task: merging task branch into feature branch"
                );
                return (task_branch, pb.branch_name);
            }
            // Plan branch exists but isn't Active — still use it as the target.
            // Falling through to base_branch would merge task→main instead of task→plan,
            // which is incorrect for tasks that belong to a plan.
            tracing::warn!(
                task_id = task.id.as_str(),
                task_branch = %task_branch,
                feature_branch = %pb.branch_name,
                plan_branch_status = ?pb.status,
                "Plan task: plan branch is not Active, but still using it as merge target \
                 to avoid incorrect task→main merge"
            );
            return (task_branch, pb.branch_name);
        }

        tracing::warn!(
            task_id = task.id.as_str(),
            ideation_session_id = ?task.ideation_session_id.as_ref().map(|id| id.as_str()),
            execution_plan_id = ?task.execution_plan_id.as_ref().map(|id| id.as_str()),
            "Plan task: no plan branch found for task — falling back to base branch"
        );
    }

    (task_branch, base_branch)
}

/// Discover and re-attach an orphaned task branch to a task record.
///
/// When tasks recover from Failed/Critical states and retry merge, the task may have
/// `task_branch` set to `None` even though the git branch exists with committed work.
/// This helper:
/// 1. Early-returns `Ok(false)` if `task.task_branch` is already set
/// 2. Constructs the expected branch name: `ralphx/{project_slug}/task-{task_id}`
/// 3. Checks if the branch exists in the git repository
/// 4. If found: updates `task.task_branch`, calls `task.touch()`, persists via `task_repo.update()`
/// 5. Returns `Ok(true)` if branch was discovered and attached, `Ok(false)` otherwise
///
/// This is called before `resolve_merge_branches()` to ensure merge operations have
/// a valid source branch reference.
pub(super) async fn discover_and_attach_task_branch(
    task: &mut Task,
    project: &Project,
    task_repo: &Arc<dyn TaskRepository>,
) -> AppResult<bool> {
    // Early return if task_branch already set
    if task.task_branch.is_some() {
        return Ok(false);
    }

    // Construct expected branch name using same convention as on_enter_states.rs:92
    let branch_name = format!(
        "ralphx/{}/task-{}",
        slugify(&project.name),
        task.id.as_str()
    );

    // Check if branch exists in the repository
    let repo_path = Path::new(&project.working_directory);
    if !GitService::branch_exists(repo_path, &branch_name)
        .await
        .unwrap_or(false)
    {
        return Ok(false);
    }

    // Branch exists - re-attach it to the task record
    tracing::info!(
        task_id = task.id.as_str(),
        branch = %branch_name,
        "Discovered orphaned task branch - re-attaching to task record"
    );

    task.task_branch = Some(branch_name.clone());
    task.touch();
    task_repo.update(task).await?;

    tracing::info!(
        task_id = task.id.as_str(),
        branch = %branch_name,
        "Successfully re-attached orphaned task branch"
    );

    Ok(true)
}

// ===== Worktree restoration =====

/// Check if a path points to a merge/rebase/source-update/plan-update worktree.
///
/// Detects the basename prefix used by all temporary merge-pipeline worktrees.
/// Used to identify stale `worktree_path` values that must be restored before
/// a reviewer can spawn.
pub(crate) fn is_merge_worktree_path(path: &str) -> bool {
    let basename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    basename.starts_with("merge-")
        || basename.starts_with("rebase-")
        || basename.starts_with("source-update-")
        || basename.starts_with("plan-update-")
}

/// Restore a task's `worktree_path` to its execution worktree (`task-{task_id}`).
///
/// Called when `worktree_path` is stale — pointing to a merge worktree that was
/// cleaned up during the merge pipeline. The task execution worktree is the correct
/// state for review.
///
/// Decision tree:
/// 1. If `task-{task_id}` directory exists on disk → update `task.worktree_path` in memory.
/// 2. If the task branch exists in git → recreate the worktree via `checkout_existing_branch_worktree`.
/// 3. Otherwise → return `Err(AppError::ReviewWorktreeMissing)`.
///
/// **Caller MUST persist `task` via `task_repo.update()` after calling this function.**
/// This function mutates `task.worktree_path` in memory only.
///
/// # Errors
///
/// Returns [`AppError::ReviewWorktreeMissing`] when neither the worktree directory
/// nor the task branch exists and the worktree cannot be recreated.
pub(crate) async fn restore_task_worktree(
    task: &mut Task,
    project: &Project,
    repo_path: &Path,
) -> Result<PathBuf, AppError> {
    let task_id_str = task.id.as_str();
    let task_wt_str = compute_task_worktree_path(project, task_id_str);
    let task_wt_path = PathBuf::from(&task_wt_str);

    if crate::utils::path_safety::checked_exists(&task_wt_path, "task worktree restore")
        .unwrap_or(false)
    {
        tracing::info!(
            task_id = task_id_str,
            worktree_path = %task_wt_path.display(),
            "restore_task_worktree: task worktree exists on disk — updating path only"
        );
        task.worktree_path = Some(task_wt_str);
        return Ok(task_wt_path);
    }

    if let Some(ref branch) = task.task_branch {
        if GitService::branch_exists(repo_path, branch).await? {
            tracing::info!(
                task_id = task_id_str,
                branch = %branch,
                worktree_path = %task_wt_path.display(),
                "restore_task_worktree: recreating task worktree from existing branch"
            );
            GitService::checkout_existing_branch_worktree(repo_path, &task_wt_path, branch).await?;
            task.worktree_path = Some(task_wt_str);
            return Ok(task_wt_path);
        }
    }

    tracing::warn!(
        task_id = task_id_str,
        "restore_task_worktree: no task worktree or branch found — cannot restore"
    );
    Err(AppError::ReviewWorktreeMissing)
}
