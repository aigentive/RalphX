// State entry side effects
// This module contains the on_enter implementation that handles state-specific actions
//
// Sibling modules (extracted for maintainability, declared in transition_handler/mod.rs):
// - merge_helpers: path computation, metadata parsing, branch resolution
// - merge_completion: finalize merge and cleanup branch/worktree
// - merge_validation: post-merge validation gate (setup + validate phases)
// - merge_orchestrator: sub-functions extracted from attempt_programmatic_merge

use super::merge_helpers::{
    plan_branch_has_reviewable_diff, resolve_merge_branches, validate_plan_merge_preconditions,
};
use super::merge_orchestrator::ConcurrentGuardResult;
use super::merge_validation::{format_validation_error_metadata, ValidationLogEntry};
use crate::utils::truncate_str;

use super::merge_validation::{emit_merge_progress, ValidationFailure};
use crate::domain::entities::{ActivityEvent, ActivityEventRole, ActivityEventType};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::super::machine::State;
use crate::application::GitService;
use crate::domain::entities::{
    merge_progress_event::{MergePhase, MergePhaseStatus},
    task_metadata::{CleanupPhase, MergeFailureSource},
    InternalStatus, MergeValidationMode, PlanBranch, Project, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{PlanBranchRepository, TaskRepository};
use crate::domain::services::github_service::GithubServiceTrait;
use crate::error::AppResult;
use crate::infrastructure::agents::claude::{reconciliation_config, scheduler_config};

mod merge_attempt;
#[cfg(test)]
mod merge_attempt_tests;
mod transitions;
mod validation;

impl<'a> super::TransitionHandler<'a> {
    /// Execute on-enter action for a state
    ///
    /// This method is public to allow `TaskTransitionService` to trigger entry actions
    /// for direct status changes (e.g., Kanban drag-drop) without going through the
    /// full event-based transition flow.
    ///
    /// Returns an error if the state entry cannot be completed (e.g., execution blocked
    /// due to blocked execution).
    pub async fn on_enter(&self, state: &State) -> AppResult<()> {
        self.on_enter_dispatch(state).await
    }

    /// Attempt programmatic rebase and merge (Phase 1 of merge workflow).
    ///
    /// This is the "fast path" - try to rebase task branch onto base and merge.
    /// If successful, transition directly to Merged and cleanup branch/worktree.
    /// If conflicts occur, transition to Merging for agent-assisted resolution.
    pub(super) async fn attempt_programmatic_merge(&self) {
        let task_id_str = &self.machine.context.task_id;
        let project_id_str = &self.machine.context.project_id;
        let attempt_start = std::time::Instant::now();

        // --- Self-dedup guard ---
        if !self.try_acquire_in_flight_guard(task_id_str) {
            return;
        }
        let _in_flight_guard = InFlightGuard {
            set: std::sync::Arc::clone(&self.machine.context.services.merges_in_flight),
            id: task_id_str.clone(),
        };

        // Load task and project from repos
        let Some(inputs) = self.fetch_merge_context(task_id_str, project_id_str).await else {
            return;
        };
        let mut task = inputs.task;
        let project = inputs.project;

        // Only proceed if task_repo is available (guaranteed by fetch_merge_context)
        let task_repo = self.machine.context.services.task_repo.as_ref().unwrap();

        // === PR MODE FORK (AD17) ===
        // Check if this is a plan merge task in PR mode. If so, use the PR path.
        let plan_branch_repo = &self.machine.context.services.plan_branch_repo;
        let github_service = self.machine.context.services.github_service.clone();
        let task_id_for_fork = TaskId::from_string(task_id_str.to_string());

        if task.category == TaskCategory::PlanMerge && plan_branch_repo.is_none() {
            tracing::warn!(
                task_id = task_id_str,
                "PendingMerge: plan branch repository is unavailable for a plan merge"
            );
            self.transition_to_merge_incomplete(
                super::TaskCore {
                    task: &mut task,
                    task_id: &task_id_for_fork,
                    task_id_str,
                    task_repo,
                },
                serde_json::json!({
                    "error": "RalphX cannot load the plan branch required to merge this plan because its repository is unavailable.",
                    "error_code": "plan_branch_repository_unavailable",
                }),
                true,
            )
            .await;
            return;
        }

        if let Some(ref pbr) = plan_branch_repo {
            let plan_branch = match pbr.get_by_merge_task_id(&task_id_for_fork).await {
                Ok(Some(plan_branch)) => Some(plan_branch),
                Ok(None) if task.category == TaskCategory::PlanMerge => {
                    tracing::warn!(
                        task_id = task_id_str,
                        "PendingMerge: required plan branch record is missing for a plan merge"
                    );
                    self.transition_to_merge_incomplete(
                        super::TaskCore {
                            task: &mut task,
                            task_id: &task_id_for_fork,
                            task_id_str,
                            task_repo,
                        },
                        serde_json::json!({
                            "error": "RalphX cannot merge this plan because its required plan branch record is missing.",
                            "error_code": "plan_branch_missing",
                        }),
                        true,
                    )
                    .await;
                    return;
                }
                Ok(None) => None,
                Err(error) if task.category == TaskCategory::PlanMerge => {
                    tracing::warn!(
                        task_id = task_id_str,
                        error = %error,
                        "PR-mode PendingMerge: failed to load plan branch for a plan merge"
                    );
                    self.transition_to_merge_incomplete(
                        super::TaskCore {
                            task: &mut task,
                            task_id: &task_id_for_fork,
                            task_id_str,
                            task_repo,
                        },
                        serde_json::json!({
                            "error": "RalphX could not load the plan branch required to merge this plan.",
                            "error_code": "plan_branch_lookup_failed",
                            "cause": error.to_string(),
                        }),
                        true,
                    )
                    .await;
                    return;
                }
                Err(error) => {
                    tracing::warn!(
                        task_id = task_id_str,
                        error = %error,
                        "PendingMerge: failed to load optional plan branch for a non-plan merge"
                    );
                    None
                }
            };
            if let Some(plan_branch) = plan_branch {
                let persisted_pr_is_authoritative = plan_branch.pr_number.is_some();
                let should_use_pr_path = if persisted_pr_is_authoritative {
                    true
                } else if plan_branch.pr_eligible {
                    match GitService::supports_github_prs(Path::new(&project.working_directory))
                        .await
                    {
                        Ok(true) if github_service.is_some() => {
                            match plan_branch_has_reviewable_diff(&project, &plan_branch).await {
                                Ok(true) => true,
                                Ok(false) => {
                                    tracing::info!(
                                        task_id = task_id_str,
                                        branch = %plan_branch.branch_name,
                                        "PR-mode PendingMerge: no reviewable diff yet, falling back to local merge path"
                                    );
                                    false
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        task_id = task_id_str,
                                        branch = %plan_branch.branch_name,
                                        error = %e,
                                        "PR-mode PendingMerge: failed to determine whether the plan branch is ahead of base"
                                    );
                                    self.transition_to_merge_incomplete(
                                        super::TaskCore {
                                            task: &mut task,
                                            task_id: &task_id_for_fork,
                                            task_id_str,
                                            task_repo,
                                        },
                                        serde_json::json!({
                                            "error": "RalphX could not determine whether the plan branch has reviewable changes.",
                                            "error_code": "plan_branch_reviewable_diff_check_failed",
                                            "branch_name": plan_branch.branch_name,
                                            "cause": e.to_string(),
                                        }),
                                        true,
                                    )
                                    .await;
                                    return;
                                }
                            }
                        }
                        Ok(true) => {
                            tracing::warn!(
                                task_id = task_id_str,
                                branch = %plan_branch.branch_name,
                                "PR-mode PendingMerge: GitHub origin requires GitHub supervision, but no GitHub service is configured"
                            );
                            self.transition_to_merge_incomplete(
                                super::TaskCore {
                                    task: &mut task,
                                    task_id: &task_id_for_fork,
                                    task_id_str,
                                    task_repo,
                                },
                                serde_json::json!({
                                    "error": "This GitHub plan branch cannot be supervised because GitHub capability is unavailable.",
                                    "error_code": "github_pr_capability_unavailable",
                                    "branch_name": plan_branch.branch_name,
                                }),
                                true,
                            )
                            .await;
                            return;
                        }
                        Ok(false) => {
                            if let Err(error) = pbr.update_pr_eligible(&plan_branch.id, false).await {
                                tracing::warn!(
                                    task_id = task_id_str,
                                    plan_branch_id = plan_branch.id.as_str(),
                                    error = %error,
                                    "PR-mode PendingMerge: failed to clear stale pre-PR eligibility for a non-GitHub origin"
                                );
                                self.transition_to_merge_incomplete(
                                    super::TaskCore {
                                        task: &mut task,
                                        task_id: &task_id_for_fork,
                                        task_id_str,
                                        task_repo,
                                    },
                                    serde_json::json!({
                                        "error": "RalphX could not persist the local merge route for this plan branch.",
                                        "error_code": "plan_branch_pr_eligibility_update_failed",
                                    }),
                                    true,
                                )
                                .await;
                                return;
                            }
                            tracing::info!(
                                task_id = task_id_str,
                                branch = %plan_branch.branch_name,
                                "PR-mode PendingMerge: non-GitHub origin routes stale pre-PR branch through the local merge path"
                            );
                            false
                        }
                        Err(error) => {
                            tracing::warn!(
                                task_id = task_id_str,
                                branch = %plan_branch.branch_name,
                                error = %error,
                                "PR-mode PendingMerge: failed to inspect origin before starting a new PR"
                            );
                            self.transition_to_merge_incomplete(
                                super::TaskCore {
                                    task: &mut task,
                                    task_id: &task_id_for_fork,
                                    task_id_str,
                                    task_repo,
                                },
                                serde_json::json!({
                                    "error": "RalphX could not inspect the repository origin before starting a pull request.",
                                    "error_code": "repository_capability_inspection_failed",
                                    "repository_capability": "inspection_failed",
                                }),
                                true,
                            )
                            .await;
                            return;
                        }
                    }
                } else {
                    false
                };
                if should_use_pr_path {
                    let Some(github) = github_service else {
                        tracing::warn!(
                            task_id = task_id_str,
                            pr_number = ?plan_branch.pr_number,
                            "PR-mode PendingMerge: persisted PR cannot be supervised because GitHub capability is unavailable"
                        );
                        self.transition_to_merge_incomplete(
                            super::TaskCore {
                                task: &mut task,
                                task_id: &task_id_for_fork,
                                task_id_str,
                                task_repo,
                            },
                            serde_json::json!({
                                "error": "Persisted pull request cannot be supervised because GitHub capability is unavailable.",
                                "error_code": "github_pr_capability_unavailable",
                                "pr_number": plan_branch.pr_number,
                            }),
                            true,
                        )
                        .await;
                        return;
                    };
                    let pbr_arc = Arc::clone(pbr);
                    Box::pin(self.run_pr_mode_pending_merge(
                        &mut task,
                        &project,
                        plan_branch,
                        task_id_for_fork,
                        task_id_str,
                        task_repo,
                        &github,
                        &pbr_arc,
                    ))
                    .await;
                    return;
                }
            }
        }

        // Push-to-main path (existing, unchanged)
        // Set merge_pipeline_active flag so the reconciler skips this task
        // while the pipeline is running (cleanup+freshness can exceed stale threshold).
        // Auto-expires after attempt_merge_deadline_secs as crash safety net.
        set_merge_pipeline_active(&mut task, task_id_str, task_repo).await;

        // Run the full pipeline body — ALL early returns stay inside the body,
        // so the clear below always executes.
        // heap-allocate to prevent stack overflow from large inlined future
        Box::pin(self.run_merge_pipeline_body(
            &mut task,
            &project,
            task_id_str,
            task_repo,
            attempt_start,
        ))
        .await;

        // Always clear the flag — reloads from DB to avoid clobbering concurrent changes.
        clear_merge_pipeline_active_from_db(task_id_str, task_repo).await;
    }
}

/// RAII guard that removes a task ID from the `merges_in_flight` set on drop.
struct InFlightGuard {
    set: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    id: String,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.set.lock() {
            guard.remove(&self.id);
        }
    }
}

/// Clear `merge_pipeline_active` dedicated column.
/// Reloads from DB to avoid clobbering concurrent changes to other fields.
async fn clear_merge_pipeline_active_from_db(
    task_id_str: &str,
    task_repo: &Arc<dyn TaskRepository>,
) {
    let task_id = TaskId(task_id_str.to_string());
    let mut task = match task_repo.get_by_id(&task_id).await {
        Ok(Some(t)) => t,
        _ => return,
    };
    if task.merge_pipeline_active.is_none() {
        return; // Already cleared — skip redundant update
    }
    task.merge_pipeline_active = None;
    task.touch();
    if let Err(e) = task_repo.update(&task).await {
        tracing::warn!(
            task_id = task_id_str,
            error = %e,
            "Failed to clear merge_pipeline_active flag (non-fatal, auto-expires)"
        );
    } else {
        tracing::info!(task_id = task_id_str, "merge_pipeline_active flag cleared");
    }
}

/// Set `merge_pipeline_active` dedicated column so the reconciler skips the task
/// while the merge pipeline is running. Auto-expires after `attempt_merge_deadline_secs`
/// as a crash safety net. Uses a dedicated column (not JSON metadata) to prevent
/// race-condition clobbers by concurrent metadata writers.
async fn set_merge_pipeline_active(
    task: &mut Task,
    task_id_str: &str,
    task_repo: &Arc<dyn TaskRepository>,
) {
    task.merge_pipeline_active = Some(chrono::Utc::now().to_rfc3339());
    task.touch();
    if let Err(e) = task_repo.update(task).await {
        tracing::warn!(
            task_id = task_id_str,
            error = %e,
            "Failed to persist merge_pipeline_active flag (non-fatal)"
        );
    } else {
        tracing::info!(task_id = task_id_str, "merge_pipeline_active flag set");
    }
}
