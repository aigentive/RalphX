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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use futures::StreamExt as _;

use crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path;
use crate::application::chat_service::ChatService;
use crate::application::git_artifact_cleanup::{
    cleanup_merged_plan_branch_local_artifacts_with_known_local_branches,
    cleanup_terminal_agent_workspace_local_artifacts_with_known_local_branches,
    LocalGitArtifactCleanupReport,
};
use crate::application::git_service::{FetchOriginOutcome, GitService};
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
use crate::domain::services::{
    GithubServiceTrait, PlanPrPublisher, PrReviewState, RunningAgentRegistry,
};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalCleanupFetchResult {
    Fetched,
    RemoteRefMissing,
    FailedNonFatal,
    NoOriginRemote,
    SkippedBusy,
    SkippedUserWork,
    Failed,
}

#[derive(Debug, Default)]
struct TerminalCleanupStats {
    projects_seen: usize,
    projects_blocked: usize,
    records_seen: usize,
    terminal_records: usize,
    local_branch_scans: usize,
    local_branch_scan_failed: usize,
    fetch_attempts: usize,
    fetch_fetched: usize,
    fetch_remote_ref_missing: usize,
    fetch_no_origin: usize,
    fetch_skipped_busy: usize,
    fetch_skipped_user_work: usize,
    fetch_failed: usize,
    branches_deleted: usize,
    branches_missing: usize,
    branches_skipped: usize,
    branches_failed: usize,
    worktrees_removed: usize,
    cleanup_markers_written: usize,
}

impl TerminalCleanupStats {
    fn observe_fetch(&mut self, result: TerminalCleanupFetchResult) {
        self.fetch_attempts += 1;
        match result {
            TerminalCleanupFetchResult::Fetched => self.fetch_fetched += 1,
            TerminalCleanupFetchResult::RemoteRefMissing => self.fetch_remote_ref_missing += 1,
            TerminalCleanupFetchResult::FailedNonFatal => self.fetch_failed += 1,
            TerminalCleanupFetchResult::NoOriginRemote => self.fetch_no_origin += 1,
            TerminalCleanupFetchResult::SkippedBusy => self.fetch_skipped_busy += 1,
            TerminalCleanupFetchResult::SkippedUserWork => self.fetch_skipped_user_work += 1,
            TerminalCleanupFetchResult::Failed => self.fetch_failed += 1,
        }
    }

    fn observe_report(&mut self, report: &LocalGitArtifactCleanupReport) {
        if report.branch_deleted {
            self.branches_deleted += 1;
        }
        if report.worktree_removed {
            self.worktrees_removed += 1;
        }

        match report.skipped_reason.as_deref() {
            Some("branch_missing") => self.branches_missing += 1,
            Some(_) => self.branches_skipped += 1,
            None if !report.branch_deleted && !report.worktree_removed => {
                self.branches_skipped += 1
            }
            None => {}
        }
    }

    fn log_summary(&self, cleanup_scope: &'static str, started_at: Instant, paused: bool) {
        tracing::info!(
            cleanup_scope,
            paused,
            projects_seen = self.projects_seen,
            projects_blocked = self.projects_blocked,
            records_seen = self.records_seen,
            terminal_records = self.terminal_records,
            local_branch_scans = self.local_branch_scans,
            local_branch_scan_failed = self.local_branch_scan_failed,
            fetch_attempts = self.fetch_attempts,
            fetch_fetched = self.fetch_fetched,
            fetch_remote_ref_missing = self.fetch_remote_ref_missing,
            fetch_no_origin = self.fetch_no_origin,
            fetch_skipped_busy = self.fetch_skipped_busy,
            fetch_skipped_user_work = self.fetch_skipped_user_work,
            fetch_failed = self.fetch_failed,
            branches_deleted = self.branches_deleted,
            branches_missing = self.branches_missing,
            branches_skipped = self.branches_skipped,
            branches_failed = self.branches_failed,
            worktrees_removed = self.worktrees_removed,
            cleanup_markers_written = self.cleanup_markers_written,
            elapsed_ms = started_at.elapsed().as_millis(),
            "Terminal cleanup: startup local artifact cleanup summary"
        );
    }
}

fn terminal_cleanup_marker_for_report(
    report: &LocalGitArtifactCleanupReport,
) -> Option<&'static str> {
    if report.branch_deleted || report.worktree_removed {
        return Some("cleaned");
    }

    match report.skipped_reason.as_deref() {
        Some("branch_missing") => Some("branch_missing"),
        Some(reason) if reason.starts_with("branch_not_merged:") => Some("unsafe"),
        Some(reason) if reason.starts_with("target_ref_missing:") => Some("target_ref_missing"),
        _ => None,
    }
}

async fn mark_plan_branch_local_cleanup_status(
    plan_branch_repo: &Arc<dyn PlanBranchRepository>,
    plan_branch: &PlanBranch,
    status: &'static str,
    stats: &mut TerminalCleanupStats,
) {
    match plan_branch_repo
        .mark_local_cleanup_status(&plan_branch.id, status, Utc::now())
        .await
    {
        Ok(()) => stats.cleanup_markers_written += 1,
        Err(error) => {
            tracing::warn!(
                plan_branch_id = plan_branch.id.as_str(),
                branch = plan_branch.branch_name.as_str(),
                status,
                error = %error,
                "Terminal PR local cleanup: failed to persist cleanup marker"
            );
        }
    }
}

async fn mark_workspace_local_cleanup_status(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    workspace: &AgentConversationWorkspace,
    status: &'static str,
    stats: &mut TerminalCleanupStats,
) {
    match workspace_repo
        .mark_local_cleanup_status(&workspace.conversation_id, status, Utc::now())
        .await
    {
        Ok(()) => stats.cleanup_markers_written += 1,
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                branch = workspace.branch_name.as_str(),
                status,
                error = %error,
                "Terminal agent workspace cleanup: failed to persist cleanup marker"
            );
        }
    }
}

fn base_ref_available_from_local_branch_set(
    base_ref: &str,
    local_branches: Option<&HashSet<String>>,
) -> bool {
    let Some(local_branches) = local_branches else {
        return false;
    };

    local_branches.contains(base_ref)
        || base_ref
            .strip_prefix("origin/")
            .is_some_and(|branch| local_branches.contains(branch))
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

    if !metadata_refresh_jobs.is_empty() {
        tracing::info!(
            count = metadata_refresh_jobs.len(),
            "PR startup recovery: scheduling existing PR metadata refresh in background"
        );
        tauri::async_runtime::spawn(async move {
            refresh_existing_pr_metadata(
                metadata_refresh_jobs,
                github_service,
                ideation_session_repo,
                artifact_repo,
            )
            .await;
        });
    }
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

    let started_at = Instant::now();
    let job_count = jobs.len();
    let refreshed_count = Arc::new(AtomicUsize::new(0));
    let refresh_failed_count = Arc::new(AtomicUsize::new(0));
    let mark_ready_count = Arc::new(AtomicUsize::new(0));
    let mark_ready_failed_count = Arc::new(AtomicUsize::new(0));

    tracing::info!(
        count = job_count,
        concurrency = PR_METADATA_REFRESH_CONCURRENCY,
        "PR startup recovery: refreshing existing PR title/body metadata"
    );

    futures::stream::iter(jobs)
        .for_each_concurrent(PR_METADATA_REFRESH_CONCURRENCY, |job| {
            let github_service = Arc::clone(&github_service);
            let ideation_session_repo = Arc::clone(&ideation_session_repo);
            let artifact_repo = Arc::clone(&artifact_repo);
            let refreshed_count = Arc::clone(&refreshed_count);
            let refresh_failed_count = Arc::clone(&refresh_failed_count);
            let mark_ready_count = Arc::clone(&mark_ready_count);
            let mark_ready_failed_count = Arc::clone(&mark_ready_failed_count);
            async move {
                let job_started_at = Instant::now();
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
                    refresh_failed_count.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                refreshed_count.fetch_add(1, Ordering::Relaxed);

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
                            mark_ready_failed_count.fetch_add(1, Ordering::Relaxed);
                        } else {
                            mark_ready_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }

                let elapsed_ms = job_started_at.elapsed().as_millis();
                if elapsed_ms >= 5_000 {
                    tracing::warn!(
                        project_id = job.project.id.as_str(),
                        branch_id = job.plan_branch.id.as_str(),
                        branch = %job.plan_branch.branch_name,
                        pr_number = job.plan_branch.pr_number,
                        elapsed_ms,
                        "PR startup recovery: slow PR metadata refresh completed"
                    );
                } else {
                    tracing::debug!(
                        project_id = job.project.id.as_str(),
                        branch_id = job.plan_branch.id.as_str(),
                        branch = %job.plan_branch.branch_name,
                        pr_number = job.plan_branch.pr_number,
                        elapsed_ms,
                        "PR startup recovery: PR metadata refresh completed"
                    );
                }
            }
        })
        .await;

    tracing::info!(
        count = job_count,
        refreshed = refreshed_count.load(Ordering::Relaxed),
        refresh_failed = refresh_failed_count.load(Ordering::Relaxed),
        mark_ready = mark_ready_count.load(Ordering::Relaxed),
        mark_ready_failed = mark_ready_failed_count.load(Ordering::Relaxed),
        elapsed_ms = started_at.elapsed().as_millis(),
        "PR startup recovery: existing PR metadata refresh completed"
    );
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
        project,
        worktree_path,
        workspace_repo,
        chat_service,
    );
}

pub async fn cleanup_terminal_plan_branch_local_artifacts_on_startup(
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    github_service: Option<Arc<dyn GithubServiceTrait>>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
) {
    let started_at = Instant::now();
    let mut stats = TerminalCleanupStats::default();
    let projects = match project_repo.get_all().await {
        Ok(projects) => projects,
        Err(error) => {
            tracing::warn!(error = %error, "Terminal PR local cleanup: failed to list projects");
            return;
        }
    };

    for project in projects {
        stats.projects_seen += 1;
        if terminal_cleanup_should_pause_for_user_work(
            &running_agent_registry,
            "plan_branch",
            project.id.as_str(),
        )
        .await
        {
            stats.log_summary("plan_branch", started_at, true);
            return;
        }

        let terminal_plan_branches = match plan_branch_repo
            .get_terminal_local_cleanup_candidates_by_project_id(&project.id)
            .await
        {
            Ok(plan_branches) => plan_branches,
            Err(error) => {
                tracing::warn!(project_id = project.id.as_str(), error = %error, "Terminal PR local cleanup: failed to load plan branches");
                continue;
            }
        };

        stats.records_seen += terminal_plan_branches.len();
        stats.terminal_records += terminal_plan_branches.len();
        if terminal_plan_branches.is_empty() {
            continue;
        }

        if blocked_git_project_ids.contains(&project.id) {
            stats.projects_blocked += 1;
            tracing::warn!(
                project_id = project.id.as_str(),
                terminal_records = terminal_plan_branches.len(),
                "Terminal PR local cleanup: skipping project with terminal plan branches due to startup Git preflight"
            );
            continue;
        }

        let repo_path = std::path::Path::new(&project.working_directory);
        let mut local_branches = match GitService::list_local_branch_names(repo_path).await {
            Ok(local_branches) => {
                stats.local_branch_scans += 1;
                Some(local_branches)
            }
            Err(error) => {
                stats.local_branch_scans += 1;
                stats.local_branch_scan_failed += 1;
                tracing::warn!(
                    project_id = project.id.as_str(),
                    error = %error,
                    "Terminal PR local cleanup: failed to preload local branches; falling back to per-branch probes"
                );
                None
            }
        };

        let cleanup_plan_branches = match local_branches.as_ref() {
            Some(local_branches) => {
                let missing_plan_branches = terminal_plan_branches
                    .iter()
                    .filter(|plan_branch| !local_branches.contains(&plan_branch.branch_name))
                    .collect::<Vec<_>>();
                stats.branches_missing += missing_plan_branches.len();
                for plan_branch in missing_plan_branches {
                    mark_plan_branch_local_cleanup_status(
                        &plan_branch_repo,
                        plan_branch,
                        "branch_missing",
                        &mut stats,
                    )
                    .await;
                }
                terminal_plan_branches
                    .into_iter()
                    .filter(|plan_branch| local_branches.contains(&plan_branch.branch_name))
                    .collect::<Vec<_>>()
            }
            None => terminal_plan_branches,
        };

        if cleanup_plan_branches.is_empty() {
            continue;
        }

        if github_service.is_some() {
            let mut fetched_base_refs = HashSet::new();
            for plan_branch in &cleanup_plan_branches {
                if terminal_cleanup_should_pause_for_user_work(
                    &running_agent_registry,
                    "plan_branch",
                    plan_branch.branch_name.as_str(),
                )
                .await
                {
                    stats.log_summary("plan_branch", started_at, true);
                    return;
                }

                let base_ref =
                    crate::domain::state_machine::transition_handler::resolve_plan_branch_pr_base(
                        &project,
                        plan_branch,
                    );
                if base_ref_available_from_local_branch_set(&base_ref, local_branches.as_ref()) {
                    continue;
                }
                if !fetched_base_refs.insert(base_ref.clone()) {
                    continue;
                }

                let fetch_result = try_terminal_cleanup_maintenance_fetch(
                    repo_path,
                    &base_ref,
                    &running_agent_registry,
                    "plan_branch",
                    project.id.as_str(),
                )
                .await;
                stats.observe_fetch(fetch_result);
            }
        }

        for plan_branch in cleanup_plan_branches {
            if terminal_cleanup_should_pause_for_user_work(
                &running_agent_registry,
                "plan_branch",
                plan_branch.branch_name.as_str(),
            )
            .await
            {
                stats.log_summary("plan_branch", started_at, true);
                return;
            }

            match cleanup_merged_plan_branch_local_artifacts_with_known_local_branches(
                &project,
                &plan_branch,
                local_branches.as_ref(),
            )
            .await
            {
                Ok(report) if report.branch_deleted => {
                    stats.observe_report(&report);
                    if let Some(status) = terminal_cleanup_marker_for_report(&report) {
                        mark_plan_branch_local_cleanup_status(
                            &plan_branch_repo,
                            &plan_branch,
                            status,
                            &mut stats,
                        )
                        .await;
                    }
                    if let Some(local_branches) = local_branches.as_mut() {
                        local_branches.remove(&plan_branch.branch_name);
                    }
                    tracing::info!(project_id = project.id.as_str(), branch = %plan_branch.branch_name, "Terminal PR local cleanup: deleted local plan branch")
                }
                Ok(report) => {
                    stats.observe_report(&report);
                    if let Some(status) = terminal_cleanup_marker_for_report(&report) {
                        mark_plan_branch_local_cleanup_status(
                            &plan_branch_repo,
                            &plan_branch,
                            status,
                            &mut stats,
                        )
                        .await;
                    }
                    tracing::debug!(project_id = project.id.as_str(), branch = %plan_branch.branch_name, skipped_reason = report.skipped_reason.as_deref(), "Terminal PR local cleanup: skipped local plan branch")
                }
                Err(error) => {
                    stats.branches_failed += 1;
                    tracing::warn!(project_id = project.id.as_str(), branch = %plan_branch.branch_name, error = %error, "Terminal PR local cleanup: failed to clean local plan branch")
                }
            }
        }
    }

    stats.log_summary("plan_branch", started_at, false);
}

pub async fn cleanup_terminal_agent_workspace_local_artifacts_on_startup(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    github_service: Option<Arc<dyn GithubServiceTrait>>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
) {
    let started_at = Instant::now();
    let mut stats = TerminalCleanupStats::default();
    let projects = match project_repo.get_all().await {
        Ok(projects) => projects,
        Err(error) => {
            tracing::warn!(error = %error, "Terminal agent workspace cleanup: failed to list projects");
            return;
        }
    };

    for project in projects {
        stats.projects_seen += 1;
        if terminal_cleanup_should_pause_for_user_work(
            &running_agent_registry,
            "agent_workspace",
            project.id.as_str(),
        )
        .await
        {
            stats.log_summary("agent_workspace", started_at, true);
            return;
        }

        let terminal_workspaces = match workspace_repo
            .get_terminal_local_cleanup_candidates_by_project_id(&project.id)
            .await
        {
            Ok(workspaces) => workspaces,
            Err(error) => {
                tracing::warn!(project_id = project.id.as_str(), error = %error, "Terminal agent workspace cleanup: failed to load workspaces");
                continue;
            }
        };

        stats.records_seen += terminal_workspaces.len();
        stats.terminal_records += terminal_workspaces.len();
        if terminal_workspaces.is_empty() {
            continue;
        }

        if blocked_git_project_ids.contains(&project.id) {
            stats.projects_blocked += 1;
            tracing::warn!(
                project_id = project.id.as_str(),
                terminal_records = terminal_workspaces.len(),
                "Terminal agent workspace cleanup: skipping project with terminal workspaces due to startup Git preflight"
            );
            continue;
        }

        let repo_path = std::path::Path::new(&project.working_directory);
        let needs_branch_delete = terminal_workspaces
            .iter()
            .any(|workspace| workspace.publication_pr_status.as_deref() == Some("merged"));
        let mut local_branches = if needs_branch_delete {
            match GitService::list_local_branch_names(repo_path).await {
                Ok(local_branches) => {
                    stats.local_branch_scans += 1;
                    Some(local_branches)
                }
                Err(error) => {
                    stats.local_branch_scans += 1;
                    stats.local_branch_scan_failed += 1;
                    tracing::warn!(
                        project_id = project.id.as_str(),
                        error = %error,
                        "Terminal agent workspace cleanup: failed to preload local branches; falling back to per-branch probes"
                    );
                    None
                }
            }
        } else {
            None
        };

        if github_service.is_some() && needs_branch_delete {
            let mut fetched_base_refs = HashSet::new();
            for workspace in &terminal_workspaces {
                if workspace.publication_pr_status.as_deref() != Some("merged") {
                    continue;
                }
                if local_branches
                    .as_ref()
                    .is_some_and(|local_branches| !local_branches.contains(&workspace.branch_name))
                {
                    continue;
                }
                let cleanup_context = workspace.conversation_id.as_str();
                if terminal_cleanup_should_pause_for_user_work(
                    &running_agent_registry,
                    "agent_workspace",
                    cleanup_context.as_str(),
                )
                .await
                {
                    stats.log_summary("agent_workspace", started_at, true);
                    return;
                }

                if base_ref_available_from_local_branch_set(
                    &workspace.base_ref,
                    local_branches.as_ref(),
                ) {
                    continue;
                }
                if !fetched_base_refs.insert(workspace.base_ref.clone()) {
                    continue;
                }

                let fetch_result = try_terminal_cleanup_maintenance_fetch(
                    repo_path,
                    &workspace.base_ref,
                    &running_agent_registry,
                    "agent_workspace",
                    project.id.as_str(),
                )
                .await;
                stats.observe_fetch(fetch_result);
            }
        }

        for workspace in terminal_workspaces {
            let cleanup_context = workspace.conversation_id.as_str();
            if terminal_cleanup_should_pause_for_user_work(
                &running_agent_registry,
                "agent_workspace",
                cleanup_context.as_str(),
            )
            .await
            {
                stats.log_summary("agent_workspace", started_at, true);
                return;
            }

            let delete_branch_if_merged =
                workspace.publication_pr_status.as_deref() == Some("merged");
            match cleanup_terminal_agent_workspace_local_artifacts_with_known_local_branches(
                &project,
                &workspace,
                delete_branch_if_merged,
                local_branches.as_ref(),
            )
            .await
            {
                Ok(report) => {
                    stats.observe_report(&report);
                    if let Some(status) = terminal_cleanup_marker_for_report(&report) {
                        mark_workspace_local_cleanup_status(
                            &workspace_repo,
                            &workspace,
                            status,
                            &mut stats,
                        )
                        .await;
                    }
                    if report.branch_deleted {
                        if let Some(local_branches) = local_branches.as_mut() {
                            local_branches.remove(&workspace.branch_name);
                        }
                    }
                    tracing::info!(
                        conversation_id = workspace.conversation_id.as_str(),
                        worktree_removed = report.worktree_removed,
                        branch_deleted = report.branch_deleted,
                        skipped_reason = report.skipped_reason.as_deref(),
                        "Terminal agent workspace cleanup: local artifact cleanup completed"
                    )
                }
                Err(error) => {
                    stats.branches_failed += 1;
                    tracing::warn!(conversation_id = workspace.conversation_id.as_str(), error = %error, "Terminal agent workspace cleanup: local artifact cleanup failed")
                }
            }
        }
    }

    stats.log_summary("agent_workspace", started_at, false);
}

async fn terminal_cleanup_should_pause_for_user_work(
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    cleanup_scope: &'static str,
    cleanup_context: &str,
) -> bool {
    if running_agent_registry.list_all().await.is_empty() {
        return false;
    }

    tracing::info!(
        cleanup_scope,
        cleanup_context,
        "Terminal cleanup: paused local artifact cleanup because user work is active"
    );
    true
}

async fn terminal_cleanup_should_skip_maintenance_fetch(
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
) -> bool {
    !running_agent_registry.list_all().await.is_empty()
}

async fn try_terminal_cleanup_maintenance_fetch(
    repo_path: &std::path::Path,
    base_ref: &str,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    cleanup_scope: &'static str,
    cleanup_context: &str,
) -> TerminalCleanupFetchResult {
    if terminal_cleanup_should_skip_maintenance_fetch(running_agent_registry).await {
        tracing::info!(
            cleanup_scope,
            cleanup_context,
            base_ref,
            "Terminal cleanup: skipped low-priority base fetch because user work is active"
        );
        return TerminalCleanupFetchResult::SkippedUserWork;
    }

    match GitService::try_fetch_origin_ref_for_maintenance(repo_path, base_ref).await {
        Ok(FetchOriginOutcome::Fetched) => {
            tracing::debug!(
                cleanup_scope,
                cleanup_context,
                base_ref,
                "Terminal cleanup: fetched base before cleanup"
            );
            TerminalCleanupFetchResult::Fetched
        }
        Ok(FetchOriginOutcome::RemoteRefMissing) => {
            tracing::info!(
                cleanup_scope,
                cleanup_context,
                base_ref,
                "Terminal cleanup: skipped base fetch because remote ref is missing"
            );
            TerminalCleanupFetchResult::RemoteRefMissing
        }
        Ok(FetchOriginOutcome::FailedNonFatal) => {
            tracing::warn!(
                cleanup_scope,
                cleanup_context,
                base_ref,
                "Terminal cleanup: base fetch failed non-fatally"
            );
            TerminalCleanupFetchResult::FailedNonFatal
        }
        Ok(FetchOriginOutcome::NoOriginRemote) => {
            tracing::debug!(
                cleanup_scope,
                cleanup_context,
                base_ref,
                "Terminal cleanup: skipped base fetch because origin is not configured"
            );
            TerminalCleanupFetchResult::NoOriginRemote
        }
        Ok(FetchOriginOutcome::SkippedBusy) => {
            tracing::info!(
                cleanup_scope,
                cleanup_context,
                base_ref,
                "Terminal cleanup: skipped low-priority base fetch because git fetch is busy"
            );
            TerminalCleanupFetchResult::SkippedBusy
        }
        Err(error) => {
            tracing::warn!(
                cleanup_scope,
                cleanup_context,
                base_ref,
                error = %error,
                "Terminal cleanup: failed to fetch base before cleanup"
            );
            TerminalCleanupFetchResult::Failed
        }
    }
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
    use std::path::Path;
    use std::process::Command;
    use std::sync::LazyLock;

    use crate::application::agent_conversation_workspace::{
        agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
    };
    use crate::application::git_service::GitService;
    use crate::application::AppState;
    use crate::commands::ExecutionState;
    use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus as DbPrStatus};
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentConversationWorkspaceStatus, ArtifactId, ChatConversationId,
        IdeationAnalysisBaseRefKind, IdeationSessionId,
    };
    use crate::domain::services::github_service::{
        PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState,
    };
    use crate::domain::services::RunningAgentKey;
    use crate::tests::mock_github_service::MockGithubService;
    use tokio::sync::Mutex as TokioMutex;

    static TERMINAL_CLEANUP_FETCH_TEST_LOCK: LazyLock<TokioMutex<()>> =
        LazyLock::new(|| TokioMutex::new(()));

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_cleanup_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test User"]);
        run_git(dir.path(), &["checkout", "-b", "main"]);
        std::fs::write(dir.path().join("README.md"), "base\n").expect("write readme");
        run_git(dir.path(), &["add", "."]);
        run_git(dir.path(), &["commit", "-m", "initial"]);
        dir
    }

    fn add_origin_remote(repo: &Path) -> tempfile::TempDir {
        let remote = tempfile::tempdir().expect("remote");
        run_git(remote.path(), &["init", "--bare"]);
        run_git(
            repo,
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        run_git(repo, &["push", "-u", "origin", "main"]);
        remote
    }

    fn branch_exists(repo: &Path, branch: &str) -> bool {
        Command::new("git")
            .args(["rev-parse", "--verify", branch])
            .current_dir(repo)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn cleanup_project(repo: &Path, worktree_parent: &Path) -> Project {
        let mut project = Project::new(
            "Startup Cleanup".to_string(),
            repo.to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        project.github_pr_enabled = true;
        project
    }

    fn startup_workspace(project: &Project, branch_name: &str) -> AgentConversationWorkspace {
        let conversation_id = ChatConversationId::from_string("startup-cleanup-conversation");
        let worktree_path =
            resolve_agent_conversation_workspace_path(project, &conversation_id).unwrap();
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id,
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            branch_name.to_string(),
            worktree_path.to_string_lossy().to_string(),
        );
        workspace.publication_pr_number = Some(101);
        workspace.publication_pr_status = Some("merged".to_string());
        workspace.publication_push_status = Some("pushed".to_string());
        workspace.status = AgentConversationWorkspaceStatus::Active;
        workspace
    }

    fn startup_workspace_branch(project: &Project) -> String {
        let conversation_id = ChatConversationId::from_string("startup-cleanup-conversation");
        agent_conversation_branch_name(project, &conversation_id)
    }

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

    #[test]
    fn terminal_cleanup_stats_track_fetch_and_cleanup_outcomes() {
        let mut stats = TerminalCleanupStats::default();
        for result in [
            TerminalCleanupFetchResult::Fetched,
            TerminalCleanupFetchResult::RemoteRefMissing,
            TerminalCleanupFetchResult::FailedNonFatal,
            TerminalCleanupFetchResult::NoOriginRemote,
            TerminalCleanupFetchResult::SkippedBusy,
            TerminalCleanupFetchResult::SkippedUserWork,
            TerminalCleanupFetchResult::Failed,
        ] {
            stats.observe_fetch(result);
        }

        stats.observe_report(&LocalGitArtifactCleanupReport {
            branch_deleted: true,
            worktree_removed: true,
            skipped_reason: None,
        });
        stats.observe_report(&LocalGitArtifactCleanupReport {
            skipped_reason: Some("branch_missing".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
        stats.observe_report(&LocalGitArtifactCleanupReport {
            skipped_reason: Some("branch_not_merged:main".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
        stats.observe_report(&LocalGitArtifactCleanupReport::default());
        stats.log_summary("plan_branch", Instant::now(), false);

        assert_eq!(stats.fetch_attempts, 7);
        assert_eq!(stats.fetch_fetched, 1);
        assert_eq!(stats.fetch_remote_ref_missing, 1);
        assert_eq!(stats.fetch_no_origin, 1);
        assert_eq!(stats.fetch_skipped_busy, 1);
        assert_eq!(stats.fetch_skipped_user_work, 1);
        assert_eq!(stats.fetch_failed, 2);
        assert_eq!(stats.branches_deleted, 1);
        assert_eq!(stats.worktrees_removed, 1);
        assert_eq!(stats.branches_missing, 1);
        assert_eq!(stats.branches_skipped, 2);
    }

    #[test]
    fn local_branch_base_ref_availability_accepts_origin_prefix_alias() {
        let local_branches = HashSet::from(["main".to_string(), "feature/demo".to_string()]);

        assert!(base_ref_available_from_local_branch_set(
            "main",
            Some(&local_branches)
        ));
        assert!(base_ref_available_from_local_branch_set(
            "origin/main",
            Some(&local_branches)
        ));
        assert!(!base_ref_available_from_local_branch_set(
            "origin/missing",
            Some(&local_branches)
        ));
        assert!(!base_ref_available_from_local_branch_set("main", None));
    }

    #[test]
    fn terminal_cleanup_markers_are_derived_from_cleanup_reports() {
        assert_eq!(
            terminal_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                branch_deleted: true,
                ..LocalGitArtifactCleanupReport::default()
            }),
            Some("cleaned")
        );
        assert_eq!(
            terminal_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                skipped_reason: Some("branch_missing".to_string()),
                ..LocalGitArtifactCleanupReport::default()
            }),
            Some("branch_missing")
        );
        assert_eq!(
            terminal_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                skipped_reason: Some("target_ref_missing:main".to_string()),
                ..LocalGitArtifactCleanupReport::default()
            }),
            Some("target_ref_missing")
        );
        assert_eq!(
            terminal_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                skipped_reason: Some("branch_not_merged:main".to_string()),
                ..LocalGitArtifactCleanupReport::default()
            }),
            Some("unsafe")
        );
        assert_eq!(
            terminal_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                skipped_reason: Some("agent_running".to_string()),
                ..LocalGitArtifactCleanupReport::default()
            }),
            None
        );
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
    async fn startup_terminal_plan_cleanup_deletes_merged_local_branch() {
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = "ralphx/startup-cleanup/plan-merged";

        run_git(repo.path(), &["checkout", "-b", branch]);
        std::fs::write(repo.path().join("plan.txt"), "plan\n").expect("write plan");
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "plan work"]);
        run_git(repo.path(), &["checkout", "main"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", branch, "-m", "merge plan"],
        );

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("startup-plan-artifact"),
            IdeationSessionId::from_string("startup-session"),
            project.id.clone(),
            branch.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Merged;
        plan_branch.pr_eligible = true;
        plan_branch.pr_number = Some(101);
        plan_branch.pr_status = Some(DbPrStatus::Merged);
        app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());

        cleanup_terminal_plan_branch_local_artifacts_on_startup(
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::new(HashSet::new()),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(!branch_exists(repo.path(), branch));
        assert_eq!(
            github.state().fetch_remote_calls,
            0,
            "startup cleanup should use GitService maintenance fetches, not GithubService fetch_remote"
        );
    }

    #[tokio::test]
    async fn startup_terminal_plan_cleanup_fetches_base_through_git_service_when_origin_available()
    {
        let _fetch_test_guard = TERMINAL_CLEANUP_FETCH_TEST_LOCK.lock().await;
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let _remote = add_origin_remote(repo.path());
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = "ralphx/startup-cleanup/plan-fetches-origin";

        run_git(repo.path(), &["checkout", "-b", branch]);
        std::fs::write(repo.path().join("plan-origin.txt"), "plan\n").expect("write plan");
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "plan work"]);
        run_git(repo.path(), &["checkout", "main"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", branch, "-m", "merge plan"],
        );
        run_git(repo.path(), &["push", "origin", "main"]);

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("startup-plan-fetch-origin-artifact"),
            IdeationSessionId::from_string("startup-plan-fetch-origin-session"),
            project.id.clone(),
            branch.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Merged;
        plan_branch.pr_eligible = true;
        plan_branch.pr_number = Some(120);
        plan_branch.pr_status = Some(DbPrStatus::Merged);
        app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());

        cleanup_terminal_plan_branch_local_artifacts_on_startup(
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::new(HashSet::new()),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(!branch_exists(repo.path(), branch));
        assert_eq!(github.state().fetch_remote_calls, 0);
    }

    #[tokio::test]
    async fn startup_terminal_plan_cleanup_pauses_when_agent_running() {
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = "ralphx/startup-cleanup/plan-active-agent";

        run_git(repo.path(), &["checkout", "-b", branch]);
        std::fs::write(repo.path().join("plan-active-agent.txt"), "plan\n").expect("write plan");
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "plan work"]);
        run_git(repo.path(), &["checkout", "main"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", branch, "-m", "merge plan"],
        );

        app_state
            .running_agent_registry
            .register(
                RunningAgentKey::new("project", project.id.as_str()),
                0,
                "startup-active-conversation".to_string(),
                "startup-active-run".to_string(),
                None,
                None,
            )
            .await;

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("startup-plan-active-agent-artifact"),
            IdeationSessionId::from_string("startup-plan-active-agent-session"),
            project.id.clone(),
            branch.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Merged;
        plan_branch.pr_eligible = true;
        plan_branch.pr_number = Some(121);
        plan_branch.pr_status = Some(DbPrStatus::Merged);
        app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());

        cleanup_terminal_plan_branch_local_artifacts_on_startup(
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::new(HashSet::new()),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(branch_exists(repo.path(), branch));
        assert_eq!(github.state().fetch_remote_calls, 0);
    }

    #[tokio::test]
    async fn startup_terminal_plan_cleanup_skips_maintenance_fetch_when_fetch_lock_busy() {
        let _fetch_test_guard = TERMINAL_CLEANUP_FETCH_TEST_LOCK.lock().await;
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let _remote = add_origin_remote(repo.path());
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = "ralphx/startup-cleanup/plan-fetch-busy";

        run_git(repo.path(), &["checkout", "-b", branch]);
        std::fs::write(repo.path().join("plan-fetch-busy.txt"), "plan\n").expect("write plan");
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "plan work"]);
        run_git(repo.path(), &["checkout", "main"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", branch, "-m", "merge plan"],
        );
        run_git(repo.path(), &["push", "origin", "main"]);

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("startup-plan-fetch-busy-artifact"),
            IdeationSessionId::from_string("startup-plan-fetch-busy-session"),
            project.id.clone(),
            branch.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Merged;
        plan_branch.pr_eligible = true;
        plan_branch.pr_number = Some(122);
        plan_branch.pr_status = Some(DbPrStatus::Merged);
        app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());
        let _guard = GitService::fetch_lock_guard_for_test().await;

        cleanup_terminal_plan_branch_local_artifacts_on_startup(
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::new(HashSet::new()),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(!branch_exists(repo.path(), branch));
        assert_eq!(github.state().fetch_remote_calls, 0);
    }

    #[tokio::test]
    async fn startup_terminal_agent_workspace_cleanup_removes_merged_worktree_and_branch() {
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = startup_workspace_branch(&project);
        let workspace = startup_workspace(&project, &branch);
        let worktree_path = Path::new(&workspace.worktree_path);

        GitService::create_worktree(repo.path(), worktree_path, &branch, "main")
            .await
            .expect("create worktree");
        std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
        run_git(worktree_path, &["add", "."]);
        run_git(worktree_path, &["commit", "-m", "agent work"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", &branch, "-m", "merge agent"],
        );
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());

        cleanup_terminal_agent_workspace_local_artifacts_on_startup(
            Arc::clone(&app_state.agent_conversation_workspace_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::new(HashSet::new()),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(!worktree_path.exists());
        assert!(!branch_exists(repo.path(), &branch));
        assert_eq!(
            github.state().fetch_remote_calls,
            0,
            "startup cleanup should use GitService maintenance fetches, not GithubService fetch_remote"
        );
    }

    #[tokio::test]
    async fn startup_terminal_agent_workspace_cleanup_pauses_when_agent_running() {
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = startup_workspace_branch(&project);
        let workspace = startup_workspace(&project, &branch);
        let worktree_path = Path::new(&workspace.worktree_path);

        GitService::create_worktree(repo.path(), worktree_path, &branch, "main")
            .await
            .expect("create worktree");
        std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
        run_git(worktree_path, &["add", "."]);
        run_git(worktree_path, &["commit", "-m", "agent work"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", &branch, "-m", "merge agent"],
        );
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .unwrap();
        app_state
            .running_agent_registry
            .register(
                RunningAgentKey::new("project", project.id.as_str()),
                0,
                "startup-active-conversation".to_string(),
                "startup-active-run".to_string(),
                None,
                None,
            )
            .await;
        let github = Arc::new(MockGithubService::new());

        cleanup_terminal_agent_workspace_local_artifacts_on_startup(
            Arc::clone(&app_state.agent_conversation_workspace_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::new(HashSet::new()),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(worktree_path.exists());
        assert!(branch_exists(repo.path(), &branch));
        assert_eq!(github.state().fetch_remote_calls, 0);
    }

    #[tokio::test]
    async fn startup_terminal_agent_workspace_cleanup_continues_without_origin_fetch() {
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = startup_workspace_branch(&project);
        let workspace = startup_workspace(&project, &branch);
        let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);

        GitService::create_worktree(repo.path(), &worktree_path, &branch, "main")
            .await
            .expect("create worktree");
        std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
        run_git(&worktree_path, &["add", "."]);
        run_git(&worktree_path, &["commit", "-m", "agent work"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", &branch, "-m", "merge agent"],
        );
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());

        cleanup_terminal_agent_workspace_local_artifacts_on_startup(
            Arc::clone(&app_state.agent_conversation_workspace_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::new(HashSet::new()),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(!worktree_path.exists());
        assert!(!branch_exists(repo.path(), &branch));
        assert_eq!(github.state().fetch_remote_calls, 0);
    }

    #[tokio::test]
    async fn startup_terminal_cleanup_skips_blocked_projects() {
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = "ralphx/startup-cleanup/blocked-plan";
        run_git(repo.path(), &["checkout", "-b", branch]);
        run_git(repo.path(), &["checkout", "main"]);

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("startup-blocked-artifact"),
            IdeationSessionId::from_string("startup-blocked-session"),
            project.id.clone(),
            branch.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Merged;
        plan_branch.pr_eligible = true;
        plan_branch.pr_number = Some(111);
        plan_branch.pr_status = Some(DbPrStatus::Merged);
        app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .unwrap();

        let workspace_branch = "ralphx/startup-cleanup/blocked-agent";
        let workspace = startup_workspace(&project, workspace_branch);
        let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);
        GitService::create_worktree(repo.path(), &worktree_path, workspace_branch, "main")
            .await
            .expect("create worktree");
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());
        let blocked = Arc::new(HashSet::from([project.id.clone()]));

        cleanup_terminal_plan_branch_local_artifacts_on_startup(
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::clone(&blocked),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;
        cleanup_terminal_agent_workspace_local_artifacts_on_startup(
            Arc::clone(&app_state.agent_conversation_workspace_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            blocked,
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(branch_exists(repo.path(), branch));
        assert!(worktree_path.exists());
        assert_eq!(github.state().fetch_remote_calls, 0);
    }

    #[tokio::test]
    async fn startup_terminal_plan_cleanup_continues_without_origin_fetch() {
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = "ralphx/startup-cleanup/plan-fetch-failure";
        run_git(repo.path(), &["checkout", "-b", branch]);
        std::fs::write(repo.path().join("plan-fetch.txt"), "plan\n").expect("write plan");
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "plan work"]);
        run_git(repo.path(), &["checkout", "main"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", branch, "-m", "merge plan"],
        );

        let mut active_plan_branch = PlanBranch::new(
            ArtifactId::from_string("startup-active-artifact"),
            IdeationSessionId::from_string("startup-fetch-session"),
            project.id.clone(),
            "ralphx/startup-cleanup/plan-active".to_string(),
            "main".to_string(),
        );
        active_plan_branch.status = PlanBranchStatus::Active;
        app_state
            .plan_branch_repo
            .create(active_plan_branch)
            .await
            .unwrap();

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("startup-fetch-artifact"),
            IdeationSessionId::from_string("startup-fetch-session"),
            project.id.clone(),
            branch.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Merged;
        plan_branch.pr_eligible = true;
        plan_branch.pr_number = Some(112);
        plan_branch.pr_status = Some(DbPrStatus::Merged);
        app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());

        cleanup_terminal_plan_branch_local_artifacts_on_startup(
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::new(HashSet::new()),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(!branch_exists(repo.path(), branch));
        assert_eq!(github.state().fetch_remote_calls, 0);
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
