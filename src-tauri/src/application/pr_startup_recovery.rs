//! PR startup recovery: restart pollers for PR-backed merge tasks after app restart.
//!
//! On shutdown, pollers are killed without cleanup. On next startup,
//! this module scans for tasks that were actively polling (`pr_polling_active = true`)
//! and restarts their pollers with staggered jitter to avoid thundering herd.
//!
//! Called from `lib.rs` after dual-AppState block, inside the startup async task,
//! BEFORE `StartupJobRunner::run()` to ensure pollers exist before the reconciler
//! can re-enter PR-mode entry actions for waiting-on-PR tasks.

use std::collections::HashSet;
use std::sync::Arc;

use futures::StreamExt as _;

use crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path;
use crate::application::chat_service::ChatService;
use crate::application::services::PrPollerRegistry;
use crate::application::task_transition_service::PrBranchFreshnessOutcome;
use crate::application::TaskTransitionService;
use crate::domain::entities::{
    AgentConversationWorkspace, ExecutionPlanId, ExecutionPlanStatus, InternalStatus, PlanBranch,
    PlanBranchStatus, Project, ProjectId, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ArtifactRepository, ExecutionPlanRepository,
    IdeationSessionRepository, PlanBranchRepository, ProjectRepository, TaskRepository,
};
use crate::domain::services::{GithubServiceTrait, PlanPrPublisher, PrReviewState};
use crate::domain::state_machine::transition_handler::{
    create_draft_pr_if_needed, plan_branch_has_reviewable_diff, plan_regular_tasks_complete,
    sync_plan_branch_pr_if_needed,
};

const PR_METADATA_REFRESH_CONCURRENCY: usize = 8;
const PR_POLLER_RECOVERY_CONCURRENCY: usize = 4;
const AGENT_WORKSPACE_PR_POLLER_RECOVERY_CONCURRENCY: usize = 4;

#[derive(Clone)]
struct PrMetadataRefreshJob {
    project: Project,
    merge_task: Task,
    plan_branch: PlanBranch,
    review_state: PrReviewState,
}

/// Re-create draft PRs that should already exist for active PR-mode plans.
///
/// This runs once on startup to repair the gap where an executing plan branch was
/// marked `pr_eligible=true` but never persisted a `pr_number` because early PR
/// creation failed before app shutdown/restart. The helper reuses the same
/// duplicate-safe `create_draft_pr_if_needed` flow used during normal execution.
///
/// # Errors
/// Logs warnings on repo failures; never panics or returns an error to the caller.
pub async fn recover_missing_draft_prs(
    task_repo: Arc<dyn TaskRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    github_service: Arc<dyn GithubServiceTrait>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let pr_creation_guard = Arc::new(dashmap::DashMap::new());
    let mut metadata_refresh_jobs = Vec::new();

    let projects = match project_repo.get_all().await {
        Ok(projects) => projects,
        Err(e) => {
            tracing::warn!(error = %e, "PR startup recovery: failed to list projects");
            return;
        }
    };

    for project in projects {
        if blocked_git_project_ids.contains(&project.id) {
            tracing::warn!(
                project_id = project.id.as_str(),
                "PR startup recovery: skipping missing-draft-PR recovery due to Git auth preflight"
            );
            continue;
        }

        let plan_branches = match plan_branch_repo.get_by_project_id(&project.id).await {
            Ok(branches) => branches,
            Err(e) => {
                tracing::warn!(
                    project_id = project.id.as_str(),
                    error = %e,
                    "PR startup recovery: failed to load plan branches for project"
                );
                continue;
            }
        };

        for plan_branch in plan_branches {
            let Some(merge_task_id) = plan_branch.merge_task_id.as_ref() else {
                tracing::debug!(
                    branch_id = plan_branch.id.as_str(),
                    branch = %plan_branch.branch_name,
                    "PR startup recovery: active PR-eligible plan branch has no merge task"
                );
                continue;
            };

            let merge_task = match task_repo.get_by_id(merge_task_id).await {
                Ok(Some(task)) => task,
                Ok(None) => {
                    tracing::debug!(
                        branch_id = plan_branch.id.as_str(),
                        branch = %plan_branch.branch_name,
                        merge_task_id = merge_task_id.as_str(),
                        "PR startup recovery: merge task not found for PR-eligible plan branch"
                    );
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        branch_id = plan_branch.id.as_str(),
                        branch = %plan_branch.branch_name,
                        merge_task_id = merge_task_id.as_str(),
                        error = %e,
                        "PR startup recovery: failed to load merge task for PR-eligible plan branch"
                    );
                    continue;
                }
            };

            if !plan_branch_needs_pr_recovery(
                &task_repo,
                &execution_plan_repo,
                &project,
                &plan_branch,
                &merge_task,
            )
            .await
            {
                continue;
            }

            let review_state =
                if plan_regular_tasks_complete(&merge_task, &plan_branch, Some(&task_repo)).await {
                    PrReviewState::Ready
                } else {
                    PrReviewState::Draft
                };

            if plan_branch.pr_number.is_some() {
                if !matches!(
                    plan_branch.pr_push_status,
                    crate::domain::entities::plan_branch::PrPushStatus::Pushed
                ) {
                    tracing::info!(
                        branch_id = plan_branch.id.as_str(),
                        branch = %plan_branch.branch_name,
                        merge_task_id = merge_task.id.as_str(),
                        status = ?merge_task.internal_status,
                        push_status = %plan_branch.pr_push_status,
                        "PR startup recovery: syncing pending PR branch push for active plan branch"
                    );
                    sync_plan_branch_pr_if_needed(
                        &project,
                        &plan_branch,
                        &github_service,
                        &plan_branch_repo,
                    )
                    .await;
                }

                let refreshed_plan_branch = plan_branch_repo
                    .get_by_id(&plan_branch.id)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| plan_branch.clone());
                metadata_refresh_jobs.push(PrMetadataRefreshJob {
                    project: project.clone(),
                    merge_task: merge_task.clone(),
                    plan_branch: refreshed_plan_branch,
                    review_state,
                });
                continue;
            }

            let branch_has_reviewable_diff = match plan_branch_has_reviewable_diff(
                &project,
                &plan_branch,
            )
            .await
            {
                Ok(has_diff) => has_diff,
                Err(e) => {
                    tracing::warn!(
                        branch_id = plan_branch.id.as_str(),
                        branch = %plan_branch.branch_name,
                        merge_task_id = merge_task.id.as_str(),
                        error = %e,
                        "PR startup recovery: failed to determine whether the active plan branch is ahead of base"
                    );
                    false
                }
            };
            if !branch_has_reviewable_diff {
                tracing::debug!(
                    branch_id = plan_branch.id.as_str(),
                    branch = %plan_branch.branch_name,
                    merge_task_id = merge_task.id.as_str(),
                    status = ?merge_task.internal_status,
                    "PR startup recovery: skipping active plan branch with no reviewable diff"
                );
                continue;
            }

            tracing::info!(
                branch_id = plan_branch.id.as_str(),
                branch = %plan_branch.branch_name,
                merge_task_id = merge_task.id.as_str(),
                status = ?merge_task.internal_status,
                "PR startup recovery: repairing missing draft PR for active plan branch"
            );

            create_draft_pr_if_needed(
                &merge_task,
                &project,
                &plan_branch,
                &pr_creation_guard,
                &github_service,
                &plan_branch_repo,
                Some(&ideation_session_repo),
                Some(&artifact_repo),
            )
            .await;

            if let Ok(Some(refreshed_plan_branch)) =
                plan_branch_repo.get_by_id(&plan_branch.id).await
            {
                if refreshed_plan_branch.pr_number.is_some() {
                    metadata_refresh_jobs.push(PrMetadataRefreshJob {
                        project: project.clone(),
                        merge_task: merge_task.clone(),
                        plan_branch: refreshed_plan_branch,
                        review_state,
                    });
                }
            }
        }
    }

    refresh_existing_pr_metadata(
        metadata_refresh_jobs,
        github_service,
        ideation_session_repo,
        artifact_repo,
    )
    .await;
}

async fn refresh_existing_pr_metadata(
    jobs: Vec<PrMetadataRefreshJob>,
    github_service: Arc<dyn GithubServiceTrait>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
) {
    if jobs.is_empty() {
        return;
    }

    tracing::info!(
        count = jobs.len(),
        concurrency = PR_METADATA_REFRESH_CONCURRENCY,
        "PR startup recovery: refreshing existing PR title/body metadata"
    );

    futures::stream::iter(jobs)
        .for_each_concurrent(PR_METADATA_REFRESH_CONCURRENCY, |job| {
            let github_service = Arc::clone(&github_service);
            let ideation_session_repo = Arc::clone(&ideation_session_repo);
            let artifact_repo = Arc::clone(&artifact_repo);
            async move {
                let publisher = PlanPrPublisher::new(
                    &github_service,
                    Some(&ideation_session_repo),
                    Some(&artifact_repo),
                );
                if let Err(e) = publisher
                    .sync_existing_pr(
                        &job.merge_task,
                        &job.project,
                        &job.plan_branch,
                        job.review_state,
                    )
                    .await
                {
                    tracing::warn!(
                        branch_id = job.plan_branch.id.as_str(),
                        branch = %job.plan_branch.branch_name,
                        error = %e,
                        "PR startup recovery: failed to refresh PR title/body"
                    );
                    return;
                }

                if job.review_state == PrReviewState::Ready {
                    if let Some(pr_number) = job.plan_branch.pr_number {
                        if let Err(e) = github_service
                            .mark_pr_ready(
                                std::path::Path::new(&job.project.working_directory),
                                pr_number,
                            )
                            .await
                        {
                            tracing::warn!(
                                branch_id = job.plan_branch.id.as_str(),
                                branch = %job.plan_branch.branch_name,
                                pr_number,
                                error = %e,
                                "PR startup recovery: failed to mark refreshed PR ready"
                            );
                        }
                    }
                }
            }
        })
        .await;
}

async fn plan_branch_needs_pr_recovery(
    task_repo: &Arc<dyn TaskRepository>,
    execution_plan_repo: &Arc<dyn ExecutionPlanRepository>,
    project: &Project,
    plan_branch: &PlanBranch,
    merge_task: &Task,
) -> bool {
    if project.archived_at.is_some() {
        tracing::debug!(
            project_id = project.id.as_str(),
            branch_id = plan_branch.id.as_str(),
            branch = %plan_branch.branch_name,
            "PR startup recovery: skipping archived project"
        );
        return false;
    }

    if !project.github_pr_enabled {
        tracing::debug!(
            project_id = project.id.as_str(),
            branch_id = plan_branch.id.as_str(),
            branch = %plan_branch.branch_name,
            "PR startup recovery: skipping project with GitHub PR mode disabled"
        );
        return false;
    }

    if !plan_branch.pr_eligible || plan_branch.status != PlanBranchStatus::Active {
        return false;
    }

    if merge_task.project_id != project.id
        || merge_task.category != TaskCategory::PlanMerge
        || merge_task.archived_at.is_some()
        || merge_task.is_terminal()
    {
        tracing::debug!(
            branch_id = plan_branch.id.as_str(),
            branch = %plan_branch.branch_name,
            merge_task_id = merge_task.id.as_str(),
            status = ?merge_task.internal_status,
            category = %merge_task.category,
            archived = merge_task.archived_at.is_some(),
            "PR startup recovery: skipping inactive plan merge task"
        );
        return false;
    }

    let Some(execution_plan_id) =
        active_execution_plan_id_for_branch(execution_plan_repo, plan_branch).await
    else {
        return false;
    };

    match task_repo.get_by_project_filtered(&project.id, false).await {
        Ok(tasks) => {
            let has_merged_plan_task = tasks.iter().any(|task| {
                task.category == TaskCategory::Regular
                    && task.internal_status == InternalStatus::Merged
                    && task.archived_at.is_none()
                    && task.ideation_session_id.as_ref() == Some(&plan_branch.session_id)
                    && task.execution_plan_id.as_ref() == Some(&execution_plan_id)
            });

            if !has_merged_plan_task {
                tracing::debug!(
                    branch_id = plan_branch.id.as_str(),
                    branch = %plan_branch.branch_name,
                    execution_plan_id = execution_plan_id.as_str(),
                    "PR startup recovery: skipping active plan branch with no merged regular task"
                );
            }

            has_merged_plan_task
        }
        Err(e) => {
            tracing::warn!(
                branch_id = plan_branch.id.as_str(),
                branch = %plan_branch.branch_name,
                execution_plan_id = execution_plan_id.as_str(),
                error = %e,
                "PR startup recovery: failed to inspect plan tasks"
            );
            false
        }
    }
}

async fn active_execution_plan_id_for_branch(
    execution_plan_repo: &Arc<dyn ExecutionPlanRepository>,
    plan_branch: &PlanBranch,
) -> Option<ExecutionPlanId> {
    if let Some(execution_plan_id) = plan_branch.execution_plan_id.as_ref() {
        match execution_plan_repo.get_by_id(execution_plan_id).await {
            Ok(Some(plan))
                if plan.status == ExecutionPlanStatus::Active
                    && plan.session_id == plan_branch.session_id =>
            {
                Some(plan.id)
            }
            Ok(Some(plan)) => {
                tracing::debug!(
                    branch_id = plan_branch.id.as_str(),
                    branch = %plan_branch.branch_name,
                    execution_plan_id = execution_plan_id.as_str(),
                    status = %plan.status,
                    "PR startup recovery: skipping non-active or mismatched execution plan"
                );
                None
            }
            Ok(None) => {
                tracing::debug!(
                    branch_id = plan_branch.id.as_str(),
                    branch = %plan_branch.branch_name,
                    execution_plan_id = execution_plan_id.as_str(),
                    "PR startup recovery: skipping missing execution plan"
                );
                None
            }
            Err(e) => {
                tracing::warn!(
                    branch_id = plan_branch.id.as_str(),
                    branch = %plan_branch.branch_name,
                    execution_plan_id = execution_plan_id.as_str(),
                    error = %e,
                    "PR startup recovery: failed to load execution plan"
                );
                None
            }
        }
    } else {
        match execution_plan_repo
            .get_active_for_session(&plan_branch.session_id)
            .await
        {
            Ok(Some(plan)) => Some(plan.id),
            Ok(None) => {
                tracing::debug!(
                    branch_id = plan_branch.id.as_str(),
                    branch = %plan_branch.branch_name,
                    session_id = plan_branch.session_id.as_str(),
                    "PR startup recovery: skipping branch with no active execution plan"
                );
                None
            }
            Err(e) => {
                tracing::warn!(
                    branch_id = plan_branch.id.as_str(),
                    branch = %plan_branch.branch_name,
                    session_id = plan_branch.session_id.as_str(),
                    error = %e,
                    "PR startup recovery: failed to load active execution plan"
                );
                None
            }
        }
    }
}

/// Restart PR merge pollers for tasks that were polling when the app last shut down.
///
/// Scans `plan_branches` for rows with `pr_polling_active = 1`, repairs eligible
/// PR-backed merge tasks, then calls `registry.start_polling()` for tasks that
/// are still waiting on GitHub. The registry applies staggered jitter to prevent
/// thundering herd. (AD9)
///
/// # Errors
/// Logs warnings on repo failures; never panics or returns an error to the caller.
pub async fn recover_pr_pollers(
    task_repo: Arc<dyn TaskRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pr_poller_registry: Arc<PrPollerRegistry>,
    project_repo: Arc<dyn ProjectRepository>,
    transition_service: Arc<TaskTransitionService<tauri::Wry>>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let task_ids = match plan_branch_repo.find_pr_polling_task_ids().await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(error = %e, "PR startup recovery: failed to query pr_polling task IDs");
            return;
        }
    };

    if task_ids.is_empty() {
        tracing::debug!("PR startup recovery: no tasks with pr_polling_active=true");
        return;
    }

    tracing::info!(
        count = task_ids.len(),
        concurrency = PR_POLLER_RECOVERY_CONCURRENCY,
        "PR startup recovery: found tasks with active polling"
    );

    futures::stream::iter(task_ids)
        .for_each_concurrent(PR_POLLER_RECOVERY_CONCURRENCY, |task_id| {
            let task_repo = Arc::clone(&task_repo);
            let plan_branch_repo = Arc::clone(&plan_branch_repo);
            let pr_poller_registry = Arc::clone(&pr_poller_registry);
            let project_repo = Arc::clone(&project_repo);
            let transition_service = Arc::clone(&transition_service);
            let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
            async move {
                recover_one_pr_poller(
                    task_id,
                    task_repo,
                    plan_branch_repo,
                    pr_poller_registry,
                    project_repo,
                    transition_service,
                    blocked_git_project_ids,
                )
                .await;
            }
        })
        .await;
}

pub async fn recover_agent_workspace_pr_pollers(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    pr_poller_registry: Arc<PrPollerRegistry>,
    chat_service: Arc<dyn ChatService>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let workspaces = match workspace_repo
        .list_active_direct_published_workspaces()
        .await
    {
        Ok(workspaces) => workspaces,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Agent workspace PR startup recovery: failed to list published workspaces"
            );
            return;
        }
    };

    if workspaces.is_empty() {
        tracing::debug!("Agent workspace PR startup recovery: no published workspaces");
        return;
    }

    tracing::info!(
        count = workspaces.len(),
        concurrency = AGENT_WORKSPACE_PR_POLLER_RECOVERY_CONCURRENCY,
        "Agent workspace PR startup recovery: found active published workspaces"
    );

    futures::stream::iter(workspaces)
        .for_each_concurrent(
            AGENT_WORKSPACE_PR_POLLER_RECOVERY_CONCURRENCY,
            |workspace| {
                let workspace_repo = Arc::clone(&workspace_repo);
                let project_repo = Arc::clone(&project_repo);
                let pr_poller_registry = Arc::clone(&pr_poller_registry);
                let chat_service = Arc::clone(&chat_service);
                let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
                async move {
                    recover_one_agent_workspace_pr_poller(
                        workspace,
                        workspace_repo,
                        project_repo,
                        pr_poller_registry,
                        chat_service,
                        blocked_git_project_ids,
                    )
                    .await;
                }
            },
        )
        .await;
}

async fn recover_one_agent_workspace_pr_poller(
    workspace: AgentConversationWorkspace,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    pr_poller_registry: Arc<PrPollerRegistry>,
    chat_service: Arc<dyn ChatService>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let Some(pr_number) = workspace.publication_pr_number else {
        return;
    };

    let project = match project_repo.get_by_id(&workspace.project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                project_id = workspace.project_id.as_str(),
                "Agent workspace PR startup recovery: project not found"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                project_id = workspace.project_id.as_str(),
                error = %error,
                "Agent workspace PR startup recovery: failed to load project"
            );
            return;
        }
    };

    if blocked_git_project_ids.contains(&project.id) {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            project_id = project.id.as_str(),
            pr_number,
            "Agent workspace PR startup recovery: skipping poller recovery due to Git auth preflight"
        );
        return;
    }

    let worktree_path =
        match resolve_valid_agent_conversation_workspace_path(&project, &workspace).await {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Agent workspace PR startup recovery: workspace path is not usable"
                );
                let _ = workspace_repo
                    .update_status(
                        &workspace.conversation_id,
                        crate::domain::entities::AgentConversationWorkspaceStatus::Missing,
                    )
                    .await;
                return;
            }
        };

    match pr_poller_registry
        .process_agent_workspace_review_feedback_once(
            &workspace.conversation_id,
            pr_number,
            &worktree_path,
            Arc::clone(&workspace_repo),
            Arc::clone(&chat_service),
        )
        .await
    {
        Ok(true) => {
            tracing::info!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number,
                "Agent workspace PR startup recovery: routed GitHub requested-changes review before restarting poller"
            );
            return;
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number,
                error = %error,
                "Agent workspace PR startup recovery: failed to inspect GitHub review feedback before poller restart"
            );
        }
    }

    pr_poller_registry.start_agent_workspace_polling(
        workspace.conversation_id,
        pr_number,
        worktree_path,
        workspace_repo,
        chat_service,
    );
}

async fn recover_one_pr_poller(
    task_id: TaskId,
    task_repo: Arc<dyn TaskRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pr_poller_registry: Arc<PrPollerRegistry>,
    project_repo: Arc<dyn ProjectRepository>,
    transition_service: Arc<TaskTransitionService<tauri::Wry>>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let mut task = match task_repo.get_by_id(&task_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            tracing::debug!(
                task_id = task_id.as_str(),
                "PR startup recovery: task not found, skipping"
            );
            return;
        }
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                error = %e,
                "PR startup recovery: failed to load task"
            );
            return;
        }
    };

    // Load plan branch
    let plan_branch = match plan_branch_repo.get_by_merge_task_id(&task_id).await {
        Ok(Some(pb)) => pb,
        Ok(None) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                "PR startup recovery: no plan branch found for task"
            );
            return;
        }
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                error = %e,
                "PR startup recovery: failed to load plan branch"
            );
            return;
        }
    };

    if should_restore_false_pr_merge_timeout(&task, &plan_branch) {
        tracing::warn!(
            task_id = task_id.as_str(),
            branch_id = plan_branch.id.as_str(),
            branch = %plan_branch.branch_name,
            pr_number = ?plan_branch.pr_number,
            "PR startup recovery: restoring PR-backed merge task that was incorrectly escalated by local merge timeout"
        );
        match transition_service
            .transition_task(&task.id, InternalStatus::WaitingOnPr)
            .await
        {
            Ok(restored) => {
                task = restored;
            }
            Err(e) => {
                tracing::warn!(
                    task_id = task_id.as_str(),
                    error = %e,
                    "PR startup recovery: failed to restore PR-backed merge timeout task"
                );
                return;
            }
        }
    }

    if task.internal_status == InternalStatus::Merging
        && task_metadata_bool(&task, "pr_branch_update_conflict")
    {
        tracing::info!(
            task_id = task_id.as_str(),
            pr_number = ?plan_branch.pr_number,
            "PR startup recovery: PR branch update conflict is already being resolved; not restarting poller"
        );
        let _ = plan_branch_repo
            .clear_polling_active_by_task(&task_id)
            .await;
        return;
    }

    if task.internal_status == InternalStatus::Merging {
        tracing::info!(
            task_id = task_id.as_str(),
            "PR startup recovery: migrating legacy PR-backed Merging task to WaitingOnPr"
        );
        match transition_service
            .transition_task(&task.id, InternalStatus::WaitingOnPr)
            .await
        {
            Ok(restored) => {
                task = restored;
            }
            Err(e) => {
                tracing::warn!(
                    task_id = task_id.as_str(),
                    error = %e,
                    "PR startup recovery: failed to migrate PR-backed Merging task"
                );
                return;
            }
        }
    }

    if task.internal_status != InternalStatus::WaitingOnPr {
        tracing::debug!(
            task_id = task_id.as_str(),
            status = ?task.internal_status,
            "PR startup recovery: task not in WaitingOnPr, skipping"
        );
        return;
    }

    let pr_number = match plan_branch.pr_number {
        Some(n) => n,
        None => {
            tracing::debug!(
                task_id = task_id.as_str(),
                "PR startup recovery: no pr_number on plan branch, skipping"
            );
            return;
        }
    };

    if !plan_branch.pr_eligible {
        tracing::debug!(
            task_id = task_id.as_str(),
            "PR startup recovery: pr_eligible=false, skipping"
        );
        return;
    }

    // Load project for working_dir and base_branch
    let project = match project_repo.get_by_id(&plan_branch.project_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                "PR startup recovery: project not found"
            );
            return;
        }
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                error = %e,
                "PR startup recovery: failed to load project"
            );
            return;
        }
    };

    if blocked_git_project_ids.contains(&project.id) {
        tracing::warn!(
            task_id = task_id.as_str(),
            project_id = project.id.as_str(),
            "PR startup recovery: skipping poller recovery due to Git auth preflight"
        );
        return;
    }

    let working_dir = std::path::PathBuf::from(&project.working_directory);
    // source_branch = the base branch the plan was branched from (e.g. "main")
    let base_branch = plan_branch.source_branch.clone();

    match pr_poller_registry
        .process_review_feedback_once(
            &task_id,
            pr_number,
            &working_dir,
            Arc::clone(&transition_service),
            "github_pr_startup_recovery",
        )
        .await
    {
        Ok(true) => {
            tracing::info!(
                task_id = task_id.as_str(),
                pr_number = pr_number,
                "PR startup recovery: routed GitHub requested-changes review before restarting poller"
            );
            return;
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                pr_number = pr_number,
                error = %e,
                "PR startup recovery: failed to inspect GitHub review feedback before poller restart"
            );
        }
    }

    match transition_service
        .reconcile_pr_branch_freshness(
            &task_id,
            &plan_branch.id,
            pr_number,
            "github_pr_startup_recovery",
        )
        .await
    {
        Ok(PrBranchFreshnessOutcome::ConflictRouted) => {
            tracing::info!(
                task_id = task_id.as_str(),
                pr_number = pr_number,
                "PR startup recovery: routed stale PR branch conflict before poller restart"
            );
            return;
        }
        Ok(PrBranchFreshnessOutcome::Updated) => {
            tracing::info!(
                task_id = task_id.as_str(),
                pr_number = pr_number,
                "PR startup recovery: updated stale PR branch before poller restart"
            );
        }
        Ok(PrBranchFreshnessOutcome::NotApplicable | PrBranchFreshnessOutcome::UpToDate) => {}
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                pr_number = pr_number,
                error = %e,
                "PR startup recovery: failed to reconcile PR branch freshness before poller restart"
            );
        }
    }

    tracing::info!(
        task_id = task_id.as_str(),
        pr_number = pr_number,
        "PR startup recovery: restarting poller (staggered jitter applied by registry)"
    );

    pr_poller_registry.start_polling(
        task_id,
        plan_branch.id,
        pr_number,
        working_dir,
        base_branch,
        Arc::clone(&transition_service),
    );
}

fn should_restore_false_pr_merge_timeout(task: &Task, plan_branch: &PlanBranch) -> bool {
    task.internal_status == InternalStatus::MergeIncomplete
        && task.category == TaskCategory::PlanMerge
        && task.archived_at.is_none()
        && plan_branch.pr_eligible
        && plan_branch.pr_polling_active
        && plan_branch.pr_number.is_some()
        && metadata_indicates_local_merge_timeout(task.metadata.as_deref())
}

fn metadata_indicates_local_merge_timeout(metadata: Option<&str>) -> bool {
    let Some(metadata) = metadata else {
        return false;
    };

    if metadata.contains("Merge timed out")
        && (metadata.contains("complete_merge") || metadata.contains("completion signal"))
    {
        return true;
    }

    serde_json::from_str::<serde_json::Value>(metadata)
        .ok()
        .and_then(|value| value.get("merge_timeout_seconds").cloned())
        .is_some()
}

fn task_metadata_bool(task: &Task, key: &str) -> bool {
    task.metadata
        .as_deref()
        .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
        .and_then(|value| value.get(key)?.as_bool())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::AppState;
    use crate::commands::ExecutionState;
    use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as DbPrStatus};
    use crate::domain::entities::{ArtifactId, IdeationSessionId};
    use crate::domain::services::github_service::{
        PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState,
    };
    use crate::tests::mock_github_service::MockGithubService;

    fn open_pr_sync_state(head_ref_name: &str) -> PrSyncState {
        PrSyncState {
            status: PrStatus::Open,
            merge_state_status: Some(PrMergeStateStatus::Clean),
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: head_ref_name.to_owned(),
            base_ref_name: "main".to_owned(),
            head_ref_oid: None,
            base_ref_oid: None,
        }
    }

    async fn create_waiting_pr_merge_task(
        app_state: &AppState,
        project: &Project,
        branch_name: String,
        pr_number: i64,
    ) -> (Task, PlanBranch) {
        let mut task = Task::new(project.id.clone(), "Merge plan into main".to_owned());
        task.category = TaskCategory::PlanMerge;
        task.internal_status = InternalStatus::WaitingOnPr;
        let task = app_state.task_repo.create(task).await.unwrap();

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string(format!("plan-artifact-{pr_number}")),
            IdeationSessionId::from_string(format!("session-{pr_number}")),
            project.id.clone(),
            branch_name,
            "main".to_owned(),
        );
        plan_branch.merge_task_id = Some(task.id.clone());
        plan_branch.pr_eligible = true;
        plan_branch.pr_polling_active = true;
        plan_branch.pr_number = Some(pr_number);
        plan_branch.pr_status = Some(DbPrStatus::Open);
        plan_branch.pr_push_status = PrPushStatus::Pushed;
        let plan_branch = app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .unwrap();

        (task, plan_branch)
    }

    #[tokio::test]
    async fn recover_pr_pollers_checks_branch_freshness_before_restarting_poller() {
        let app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());

        let mut project = Project::new("Test Project".to_owned(), "/tmp/test-repo".to_owned());
        project.github_pr_enabled = true;
        app_state
            .project_repo
            .create(project.clone())
            .await
            .unwrap();

        let mut task = Task::new(project.id.clone(), "Merge plan into main".to_owned());
        task.category = TaskCategory::PlanMerge;
        task.internal_status = InternalStatus::WaitingOnPr;
        let task = app_state.task_repo.create(task).await.unwrap();

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("plan-artifact"),
            IdeationSessionId::from_string("session-1"),
            project.id.clone(),
            "plan/feature".to_owned(),
            "main".to_owned(),
        );
        plan_branch.merge_task_id = Some(task.id.clone());
        plan_branch.pr_eligible = true;
        plan_branch.pr_polling_active = true;
        plan_branch.pr_number = Some(68);
        plan_branch.pr_status = Some(DbPrStatus::Open);
        plan_branch.pr_push_status = PrPushStatus::Pushed;
        let plan_branch = app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .unwrap();

        github.will_return_sync_state(open_pr_sync_state(&plan_branch.branch_name));

        let registry = Arc::new(PrPollerRegistry::new(
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::clone(&app_state.plan_branch_repo),
        ));
        let transition_service = Arc::new(
            app_state
                .build_transition_service_for_runtime::<tauri::Wry>(
                    Arc::new(ExecutionState::new()),
                    None,
                )
                .with_github_service(Arc::clone(&github) as Arc<dyn GithubServiceTrait>)
                .with_pr_poller_registry(Arc::clone(&registry)),
        );

        recover_pr_pollers(
            Arc::clone(&app_state.task_repo),
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&registry),
            Arc::clone(&app_state.project_repo),
            transition_service,
            Arc::new(HashSet::new()),
        )
        .await;

        let state = github.state();
        assert_eq!(state.check_pr_review_feedback_calls, 1);
        assert_eq!(state.check_pr_sync_state_calls, 1);
        assert_eq!(state.last_check_pr_sync_state_number, Some(68));
        drop(state);

        registry.stop_polling(&task.id);
    }

    #[tokio::test]
    async fn recover_pr_pollers_reconciles_startup_prs_with_bounded_parallelism() {
        let app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());

        let mut project = Project::new("Test Project".to_owned(), "/tmp/test-repo".to_owned());
        project.github_pr_enabled = true;
        app_state
            .project_repo
            .create(project.clone())
            .await
            .unwrap();

        let mut task_ids = Vec::new();
        for index in 0..(PR_POLLER_RECOVERY_CONCURRENCY + 2) {
            let pr_number = 80 + index as i64;
            let (task, _) = create_waiting_pr_merge_task(
                &app_state,
                &project,
                format!("plan/feature-{index}"),
                pr_number,
            )
            .await;
            task_ids.push(task.id);
        }

        github.with_review_feedback_delay_ms(25);

        let registry = Arc::new(PrPollerRegistry::new(
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::clone(&app_state.plan_branch_repo),
        ));
        let transition_service = Arc::new(
            app_state
                .build_transition_service_for_runtime::<tauri::Wry>(
                    Arc::new(ExecutionState::new()),
                    None,
                )
                .with_github_service(Arc::clone(&github) as Arc<dyn GithubServiceTrait>)
                .with_pr_poller_registry(Arc::clone(&registry)),
        );

        recover_pr_pollers(
            Arc::clone(&app_state.task_repo),
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&registry),
            Arc::clone(&app_state.project_repo),
            transition_service,
            Arc::new(HashSet::new()),
        )
        .await;

        let state = github.state();
        assert_eq!(
            state.check_pr_review_feedback_calls as usize,
            PR_POLLER_RECOVERY_CONCURRENCY + 2
        );
        assert!(
            state.max_concurrent_check_pr_review_feedback_calls > 1,
            "startup PR recovery should process independent PRs concurrently"
        );
        assert!(
            state.max_concurrent_check_pr_review_feedback_calls as usize
                <= PR_POLLER_RECOVERY_CONCURRENCY,
            "startup PR recovery must stay within the configured concurrency cap"
        );
        drop(state);

        for task_id in task_ids {
            registry.stop_polling(&task_id);
        }
    }
}
