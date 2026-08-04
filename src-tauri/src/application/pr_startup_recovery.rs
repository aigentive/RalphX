//! PR startup recovery: restart pollers for PR-backed merge tasks after app restart.
//!
//! On shutdown, pollers are killed without cleanup. On next startup,
//! this module scans for tasks that were actively polling (`pr_polling_active = true`)
//! and restarts their pollers with staggered jitter to avoid thundering herd.
//!
//! Called from `lib.rs` after dual-AppState block, inside the startup async task,
//! BEFORE `StartupJobRunner::run()` to ensure pollers exist before the reconciler
//! can re-enter PR-mode entry actions for waiting-on-PR tasks.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use futures::StreamExt as _;

use crate::application::agent_conversation_workspace::{
    ensure_linked_plan_branch_agent_worktree, resolve_linked_plan_branch_agent_worktree_path,
    resolve_valid_agent_conversation_workspace_path,
};
use crate::application::agent_workspace_terminal_cleanup::settle_review_pr_terminal_observation;
use crate::application::chat_service::ChatService;
use crate::application::git_artifact_cleanup::{
    cleanup_merged_plan_branch_local_artifacts_with_known_local_branches,
    terminal_plan_branch_cleanup_marker_for_report, LocalGitArtifactCleanupReport,
};
use crate::application::git_service::{git_cmd, FetchOriginOutcome, GitService};
use crate::application::services::PrPollerRegistry;
use crate::application::task_transition_service::PrBranchFreshnessOutcome;
use crate::application::{AppState, NotificationService, TaskTransitionService};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, ExecutionPlanId, ExecutionPlanStatus, InternalStatus,
    PlanBranch, PlanBranchId, PlanBranchStatus, Project, ProjectId, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AgentWorkspaceRepairRepository,
    ArtifactRepository, ExecutionPlanRepository, IdeationSessionRepository, PlanBranchRepository,
    ProjectRepository, TaskRepository,
};
use crate::domain::services::{
    GithubServiceTrait, PlanPrDescriptionDrafter, PlanPrPublisher, PrReviewState, PrStatus,
    RunningAgentRegistry,
};
use crate::domain::state_machine::transition_handler::{
    create_draft_pr_if_needed, plan_branch_has_reviewable_diff, plan_regular_tasks_complete,
    sync_plan_branch_pr_if_needed,
};
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::git_auth::{inspect_repository_capability, RepositoryCapability};

const PR_METADATA_REFRESH_CONCURRENCY: usize = 8;
const PR_CREATION_RECOVERY_PROJECT_CONCURRENCY: usize = 4;
const PR_POLLER_RECOVERY_CONCURRENCY: usize = 4;
const AGENT_WORKSPACE_PR_POLLER_RECOVERY_CONCURRENCY: usize = 4;
const SLOW_PR_RECOVERY_CANDIDATE_MS: u64 = 100;
const STARTUP_BACKGROUND_DB_GRACE: Duration = Duration::from_millis(750);

#[derive(Clone)]
struct PrMetadataRefreshJob {
    project: Project,
    merge_task: Task,
    plan_branch: PlanBranch,
    review_state: PrReviewState,
}

#[derive(Default)]
struct PrCreationRecoveryProjectResult {
    projects_blocked: usize,
    plan_branches_seen: usize,
    existing_pr_branches: usize,
    missing_pr_candidates: usize,
    missing_pr_repairs: usize,
    pending_push_syncs: usize,
    candidate_load_elapsed_ms: u64,
    project_task_load_elapsed_ms: u64,
    project_tasks_seen: usize,
    merge_task_read_elapsed_ms: u64,
    needs_recovery_elapsed_ms: u64,
    review_state_elapsed_ms: u64,
    existing_pr_refresh_lookup_elapsed_ms: u64,
    reviewable_diff_elapsed_ms: u64,
    create_pr_elapsed_ms: u64,
    slow_candidates: usize,
    metadata_refresh_jobs: Vec<PrMetadataRefreshJob>,
}

impl PrCreationRecoveryProjectResult {
    fn merge(&mut self, other: Self) {
        self.projects_blocked += other.projects_blocked;
        self.plan_branches_seen += other.plan_branches_seen;
        self.existing_pr_branches += other.existing_pr_branches;
        self.missing_pr_candidates += other.missing_pr_candidates;
        self.missing_pr_repairs += other.missing_pr_repairs;
        self.pending_push_syncs += other.pending_push_syncs;
        self.candidate_load_elapsed_ms += other.candidate_load_elapsed_ms;
        self.project_task_load_elapsed_ms += other.project_task_load_elapsed_ms;
        self.project_tasks_seen += other.project_tasks_seen;
        self.merge_task_read_elapsed_ms += other.merge_task_read_elapsed_ms;
        self.needs_recovery_elapsed_ms += other.needs_recovery_elapsed_ms;
        self.review_state_elapsed_ms += other.review_state_elapsed_ms;
        self.existing_pr_refresh_lookup_elapsed_ms += other.existing_pr_refresh_lookup_elapsed_ms;
        self.reviewable_diff_elapsed_ms += other.reviewable_diff_elapsed_ms;
        self.create_pr_elapsed_ms += other.create_pr_elapsed_ms;
        self.slow_candidates += other.slow_candidates;
        self.metadata_refresh_jobs
            .extend(other.metadata_refresh_jobs);
    }
}

struct ProjectPrRecoveryTaskSnapshot {
    by_id: HashMap<String, Task>,
    merged_regular_plan_keys: HashSet<(String, String)>,
    task_count: usize,
}

impl ProjectPrRecoveryTaskSnapshot {
    fn from_targeted_tasks(
        tasks: Vec<Task>,
        merged_regular_plan_keys: HashSet<(
            crate::domain::entities::IdeationSessionId,
            ExecutionPlanId,
        )>,
    ) -> Self {
        let task_count = tasks.len();
        let mut by_id = HashMap::with_capacity(task_count);

        for task in tasks {
            by_id.insert(task.id.as_str().to_string(), task);
        }

        Self {
            by_id,
            merged_regular_plan_keys: merged_regular_plan_keys
                .into_iter()
                .map(|(session_id, execution_plan_id)| {
                    (
                        session_id.as_str().to_string(),
                        execution_plan_id.as_str().to_string(),
                    )
                })
                .collect(),
            task_count,
        }
    }

    fn get_task(&self, task_id: &TaskId) -> Option<&Task> {
        self.by_id.get(task_id.as_str())
    }

    fn has_merged_regular_plan_task(
        &self,
        session_id: &crate::domain::entities::IdeationSessionId,
        execution_plan_id: &ExecutionPlanId,
    ) -> bool {
        self.merged_regular_plan_keys.contains(&(
            session_id.as_str().to_string(),
            execution_plan_id.as_str().to_string(),
        ))
    }
}

fn elapsed_ms_u64(started_at: Instant) -> u64 {
    started_at
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn record_slow_pr_recovery_candidate(
    result: &mut PrCreationRecoveryProjectResult,
    project: &Project,
    plan_branch: &PlanBranch,
    candidate_started_at: Instant,
    outcome: &'static str,
) {
    let candidate_elapsed_ms = elapsed_ms_u64(candidate_started_at);
    if candidate_elapsed_ms < SLOW_PR_RECOVERY_CANDIDATE_MS {
        return;
    }

    result.slow_candidates += 1;
    tracing::info!(
        project_id = project.id.as_str(),
        branch_id = plan_branch.id.as_str(),
        branch = %plan_branch.branch_name,
        pr_number = plan_branch.pr_number,
        outcome,
        elapsed_ms = candidate_elapsed_ms,
        "PR startup recovery: slow candidate scan completed"
    );
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
    plan_pr_description_drafter: Arc<dyn PlanPrDescriptionDrafter>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let started_at = Instant::now();
    let pr_creation_guard = Arc::new(dashmap::DashMap::new());

    let projects = match project_repo.get_all().await {
        Ok(projects) => projects,
        Err(e) => {
            tracing::warn!(error = %e, "PR startup recovery: failed to list projects");
            return;
        }
    };
    let projects_seen = projects.len();

    let project_results = futures::stream::iter(projects)
        .map(|project| {
            let task_repo = Arc::clone(&task_repo);
            let plan_branch_repo = Arc::clone(&plan_branch_repo);
            let execution_plan_repo = Arc::clone(&execution_plan_repo);
            let ideation_session_repo = Arc::clone(&ideation_session_repo);
            let artifact_repo = Arc::clone(&artifact_repo);
            let github_service = Arc::clone(&github_service);
            let plan_pr_description_drafter = Arc::clone(&plan_pr_description_drafter);
            let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
            let pr_creation_guard = Arc::clone(&pr_creation_guard);
            async move {
                recover_missing_draft_prs_for_project(
                    project,
                    task_repo,
                    plan_branch_repo,
                    execution_plan_repo,
                    ideation_session_repo,
                    artifact_repo,
                    github_service,
                    plan_pr_description_drafter,
                    blocked_git_project_ids,
                    pr_creation_guard,
                )
                .await
            }
        })
        .buffer_unordered(PR_CREATION_RECOVERY_PROJECT_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    let mut totals = PrCreationRecoveryProjectResult::default();
    for result in project_results {
        totals.merge(result);
    }

    tracing::info!(
        projects_seen,
        projects_blocked = totals.projects_blocked,
        plan_branches_seen = totals.plan_branches_seen,
        existing_pr_branches = totals.existing_pr_branches,
        missing_pr_candidates = totals.missing_pr_candidates,
        missing_pr_repairs = totals.missing_pr_repairs,
        pending_push_syncs = totals.pending_push_syncs,
        metadata_refresh_jobs = totals.metadata_refresh_jobs.len(),
        candidate_load_elapsed_ms = totals.candidate_load_elapsed_ms,
        project_task_load_elapsed_ms = totals.project_task_load_elapsed_ms,
        project_tasks_seen = totals.project_tasks_seen,
        merge_task_read_elapsed_ms = totals.merge_task_read_elapsed_ms,
        needs_recovery_elapsed_ms = totals.needs_recovery_elapsed_ms,
        review_state_elapsed_ms = totals.review_state_elapsed_ms,
        existing_pr_refresh_lookup_elapsed_ms = totals.existing_pr_refresh_lookup_elapsed_ms,
        reviewable_diff_elapsed_ms = totals.reviewable_diff_elapsed_ms,
        create_pr_elapsed_ms = totals.create_pr_elapsed_ms,
        slow_candidates = totals.slow_candidates,
        concurrency = PR_CREATION_RECOVERY_PROJECT_CONCURRENCY,
        elapsed_ms = started_at.elapsed().as_millis(),
        "PR startup recovery: missing draft PR scan completed"
    );

    if !totals.metadata_refresh_jobs.is_empty() {
        tracing::info!(
            count = totals.metadata_refresh_jobs.len(),
            "PR startup recovery: scheduling existing PR metadata refresh in background"
        );
        let metadata_refresh_jobs = totals.metadata_refresh_jobs;
        let plan_pr_description_drafter = Arc::clone(&plan_pr_description_drafter);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(STARTUP_BACKGROUND_DB_GRACE).await;
            git_cmd::with_git_command_lane(git_cmd::GitCommandLane::Background, async move {
                refresh_existing_pr_metadata(
                    metadata_refresh_jobs,
                    github_service,
                    ideation_session_repo,
                    artifact_repo,
                    plan_pr_description_drafter,
                )
                .await;
            })
            .await;
        });
    }
}

#[allow(clippy::too_many_arguments)]
async fn recover_missing_draft_prs_for_project(
    project: Project,
    task_repo: Arc<dyn TaskRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    github_service: Arc<dyn GithubServiceTrait>,
    plan_pr_description_drafter: Arc<dyn PlanPrDescriptionDrafter>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
    pr_creation_guard: Arc<dashmap::DashMap<PlanBranchId, ()>>,
) -> PrCreationRecoveryProjectResult {
    let mut result = PrCreationRecoveryProjectResult::default();
    if blocked_git_project_ids.contains(&project.id) {
        result.projects_blocked += 1;
        tracing::warn!(
            project_id = project.id.as_str(),
            "PR startup recovery: skipping missing-draft-PR recovery due to Git auth preflight"
        );
        return result;
    }

    let candidate_load_started_at = Instant::now();
    let plan_branches = match plan_branch_repo
        .get_startup_pr_recovery_candidates_by_project_id(&project.id)
        .await
    {
        Ok(branches) => branches,
        Err(e) => {
            tracing::warn!(
                project_id = project.id.as_str(),
                error = %e,
                "PR startup recovery: failed to load startup PR recovery candidates for project"
            );
            return result;
        }
    };
    result.candidate_load_elapsed_ms += elapsed_ms_u64(candidate_load_started_at);

    if plan_branches.is_empty() {
        return result;
    }

    let project_task_load_started_at = Instant::now();
    let merge_task_ids = plan_branches
        .iter()
        .filter_map(|branch| branch.merge_task_id.clone())
        .collect::<Vec<_>>();
    let merge_tasks = match task_repo.get_by_ids(&merge_task_ids).await {
        Ok(tasks) => tasks,
        Err(e) => {
            result.project_task_load_elapsed_ms += elapsed_ms_u64(project_task_load_started_at);
            tracing::warn!(
                project_id = project.id.as_str(),
                error = %e,
                "PR startup recovery: failed to load candidate merge tasks"
            );
            return result;
        }
    };
    let mut active_execution_plan_ids = HashMap::new();
    let mut active_plan_keys = Vec::new();
    for plan_branch in &plan_branches {
        let Some(execution_plan_id) =
            active_execution_plan_id_for_branch(&execution_plan_repo, plan_branch).await
        else {
            continue;
        };
        active_plan_keys.push((plan_branch.session_id.clone(), execution_plan_id.clone()));
        active_execution_plan_ids.insert(plan_branch.id.clone(), execution_plan_id);
    }
    let merged_regular_plan_keys = match task_repo
        .find_merged_regular_plan_keys(&project.id, &active_plan_keys)
        .await
    {
        Ok(keys) => keys,
        Err(e) => {
            result.project_task_load_elapsed_ms += elapsed_ms_u64(project_task_load_started_at);
            tracing::warn!(
                project_id = project.id.as_str(),
                error = %e,
                "PR startup recovery: failed to load merged regular plan keys"
            );
            return result;
        }
    };
    let task_snapshot =
        ProjectPrRecoveryTaskSnapshot::from_targeted_tasks(merge_tasks, merged_regular_plan_keys);
    result.project_task_load_elapsed_ms += elapsed_ms_u64(project_task_load_started_at);
    result.project_tasks_seen += task_snapshot.task_count;

    for plan_branch in plan_branches {
        let candidate_started_at = Instant::now();
        result.plan_branches_seen += 1;
        let Some(merge_task_id) = plan_branch.merge_task_id.as_ref() else {
            tracing::debug!(
                branch_id = plan_branch.id.as_str(),
                branch = %plan_branch.branch_name,
                "PR startup recovery: active PR-eligible plan branch has no merge task"
            );
            continue;
        };

        if plan_branch.pr_number.is_none() && plan_branch.pr_eligible {
            match inspect_repository_capability(std::path::Path::new(&project.working_directory))
                .await
            {
                RepositoryCapability::Github { .. } => {}
                RepositoryCapability::LocalOnly | RepositoryCapability::OtherRemote { .. } => {
                    if let Err(error) = plan_branch_repo
                        .update_pr_eligible(&plan_branch.id, false)
                        .await
                    {
                        tracing::warn!(
                            project_id = project.id.as_str(),
                            branch_id = plan_branch.id.as_str(),
                            branch = %plan_branch.branch_name,
                            error = %error,
                            "PR startup recovery: failed to clear stale pre-PR eligibility for a non-GitHub origin"
                        );
                    } else {
                        tracing::info!(
                            project_id = project.id.as_str(),
                            branch_id = plan_branch.id.as_str(),
                            branch = %plan_branch.branch_name,
                            "PR startup recovery: non-GitHub origin disabled stale pre-PR eligibility"
                        );
                    }
                    record_slow_pr_recovery_candidate(
                        &mut result,
                        &project,
                        &plan_branch,
                        candidate_started_at,
                        "non_github_origin",
                    );
                    continue;
                }
                RepositoryCapability::InspectionFailed { message } => {
                    tracing::warn!(
                        project_id = project.id.as_str(),
                        branch_id = plan_branch.id.as_str(),
                        branch = %plan_branch.branch_name,
                        error = %message,
                        "PR startup recovery: refusing to create a PR because origin inspection failed"
                    );
                    record_slow_pr_recovery_candidate(
                        &mut result,
                        &project,
                        &plan_branch,
                        candidate_started_at,
                        "origin_inspection_failed",
                    );
                    continue;
                }
            }
        }

        let merge_task_read_started_at = Instant::now();
        let Some(merge_task) = task_snapshot.get_task(merge_task_id) else {
            result.merge_task_read_elapsed_ms += elapsed_ms_u64(merge_task_read_started_at);
            tracing::debug!(
                branch_id = plan_branch.id.as_str(),
                branch = %plan_branch.branch_name,
                merge_task_id = merge_task_id.as_str(),
                "PR startup recovery: merge task not found for PR-eligible plan branch"
            );
            record_slow_pr_recovery_candidate(
                &mut result,
                &project,
                &plan_branch,
                candidate_started_at,
                "merge_task_missing",
            );
            continue;
        };
        result.merge_task_read_elapsed_ms += elapsed_ms_u64(merge_task_read_started_at);

        let needs_recovery_started_at = Instant::now();
        if !plan_branch_needs_pr_recovery(
            &task_snapshot,
            &project,
            &plan_branch,
            merge_task,
            active_execution_plan_ids.get(&plan_branch.id),
        )
        .await
        {
            result.needs_recovery_elapsed_ms += elapsed_ms_u64(needs_recovery_started_at);
            record_slow_pr_recovery_candidate(
                &mut result,
                &project,
                &plan_branch,
                candidate_started_at,
                "not_needed",
            );
            continue;
        }
        result.needs_recovery_elapsed_ms += elapsed_ms_u64(needs_recovery_started_at);

        let review_state_started_at = Instant::now();
        let review_state =
            if plan_regular_tasks_complete(merge_task, &plan_branch, Some(&task_repo)).await {
                PrReviewState::Ready
            } else {
                PrReviewState::Draft
            };
        result.review_state_elapsed_ms += elapsed_ms_u64(review_state_started_at);

        if plan_branch.pr_number.is_some() {
            result.existing_pr_branches += 1;
            if !matches!(
                plan_branch.pr_push_status,
                crate::domain::entities::plan_branch::PrPushStatus::Pushed
            ) {
                result.pending_push_syncs += 1;
                tracing::info!(
                    branch_id = plan_branch.id.as_str(),
                    branch = %plan_branch.branch_name,
                    merge_task_id = merge_task.id.as_str(),
                    status = ?merge_task.internal_status,
                    push_status = %plan_branch.pr_push_status,
                    "PR startup recovery: syncing pending PR branch push for active plan branch"
                );
                if let Err(error) = sync_plan_branch_pr_if_needed(
                    &project,
                    &plan_branch,
                    &github_service,
                    &plan_branch_repo,
                )
                .await
                {
                    tracing::warn!(
                        branch_id = plan_branch.id.as_str(),
                        branch = %plan_branch.branch_name,
                        merge_task_id = merge_task.id.as_str(),
                        error = %error,
                        "PR startup recovery: pending PR branch push sync failed"
                    );
                }
            }

            let refresh_lookup_started_at = Instant::now();
            let refreshed_plan_branch = plan_branch_repo
                .get_by_id(&plan_branch.id)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| plan_branch.clone());
            result.existing_pr_refresh_lookup_elapsed_ms +=
                elapsed_ms_u64(refresh_lookup_started_at);
            result.metadata_refresh_jobs.push(PrMetadataRefreshJob {
                project: project.clone(),
                merge_task: (*merge_task).clone(),
                plan_branch: refreshed_plan_branch,
                review_state,
            });
            record_slow_pr_recovery_candidate(
                &mut result,
                &project,
                &plan_branch,
                candidate_started_at,
                "existing_pr",
            );
            continue;
        }

        result.missing_pr_candidates += 1;
        let reviewable_diff_started_at = Instant::now();
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
        result.reviewable_diff_elapsed_ms += elapsed_ms_u64(reviewable_diff_started_at);
        if !branch_has_reviewable_diff {
            tracing::debug!(
                branch_id = plan_branch.id.as_str(),
                branch = %plan_branch.branch_name,
                merge_task_id = merge_task.id.as_str(),
                status = ?merge_task.internal_status,
                "PR startup recovery: skipping active plan branch with no reviewable diff"
            );
            record_slow_pr_recovery_candidate(
                &mut result,
                &project,
                &plan_branch,
                candidate_started_at,
                "no_reviewable_diff",
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

        let create_pr_started_at = Instant::now();
        create_draft_pr_if_needed(
            merge_task,
            &project,
            &plan_branch,
            &pr_creation_guard,
            &github_service,
            &plan_branch_repo,
            Some(&plan_pr_description_drafter),
            Some(&ideation_session_repo),
            Some(&artifact_repo),
        )
        .await;
        result.create_pr_elapsed_ms += elapsed_ms_u64(create_pr_started_at);

        if let Ok(Some(refreshed_plan_branch)) = plan_branch_repo.get_by_id(&plan_branch.id).await {
            if refreshed_plan_branch.pr_number.is_some() {
                result.missing_pr_repairs += 1;
                result.metadata_refresh_jobs.push(PrMetadataRefreshJob {
                    project: project.clone(),
                    merge_task: (*merge_task).clone(),
                    plan_branch: refreshed_plan_branch,
                    review_state,
                });
            }
        }

        record_slow_pr_recovery_candidate(
            &mut result,
            &project,
            &plan_branch,
            candidate_started_at,
            "missing_pr_attempted",
        );
    }

    result
}

async fn refresh_existing_pr_metadata(
    jobs: Vec<PrMetadataRefreshJob>,
    github_service: Arc<dyn GithubServiceTrait>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    plan_pr_description_drafter: Arc<dyn PlanPrDescriptionDrafter>,
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
            let plan_pr_description_drafter = Arc::clone(&plan_pr_description_drafter);
            let refreshed_count = Arc::clone(&refreshed_count);
            let refresh_failed_count = Arc::clone(&refresh_failed_count);
            let mark_ready_count = Arc::clone(&mark_ready_count);
            let mark_ready_failed_count = Arc::clone(&mark_ready_failed_count);
            async move {
                let job_started_at = Instant::now();
                let review_base = crate::domain::state_machine::transition_handler::resolve_plan_branch_pr_base(
                    &job.project,
                    &job.plan_branch,
                );
                let description = match plan_pr_description_drafter
                    .draft_plan_description(
                        &job.project,
                        &job.plan_branch,
                        &review_base,
                        job.review_state,
                    )
                    .await
                {
                    Ok(description) => description,
                    Err(e) => {
                        tracing::warn!(
                            branch_id = job.plan_branch.id.as_str(),
                            branch = %job.plan_branch.branch_name,
                            error = %e,
                            "PR startup recovery: failed to draft PR description for metadata refresh"
                        );
                        refresh_failed_count.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };
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
                        &description,
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
    task_snapshot: &ProjectPrRecoveryTaskSnapshot,
    project: &Project,
    plan_branch: &PlanBranch,
    merge_task: &Task,
    active_execution_plan_id: Option<&ExecutionPlanId>,
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

    let has_persisted_pr = plan_branch.pr_number.is_some();
    if !has_persisted_pr && !project.github_pr_enabled {
        tracing::debug!(
            project_id = project.id.as_str(),
            branch_id = plan_branch.id.as_str(),
            branch = %plan_branch.branch_name,
            "PR startup recovery: skipping project with GitHub PR mode disabled"
        );
        return false;
    }

    if (!has_persisted_pr && !plan_branch.pr_eligible)
        || plan_branch.status != PlanBranchStatus::Active
    {
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

    let Some(execution_plan_id) = active_execution_plan_id else {
        return false;
    };

    let has_merged_plan_task =
        task_snapshot.has_merged_regular_plan_task(&plan_branch.session_id, execution_plan_id);

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
    transition_service: Arc<TaskTransitionService>,
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
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pr_poller_registry: Arc<PrPollerRegistry>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    chat_service: Arc<dyn ChatService>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    recover_agent_workspace_pr_pollers_with_notifications(
        workspace_repo,
        project_repo,
        plan_branch_repo,
        pr_poller_registry,
        agent_run_repo,
        chat_service,
        None,
        None,
        None,
        blocked_git_project_ids,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub async fn recover_agent_workspace_pr_pollers_with_notifications(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pr_poller_registry: Arc<PrPollerRegistry>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    chat_service: Arc<dyn ChatService>,
    notification_service: Option<Arc<NotificationService>>,
    agent_workspace_repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    recovery_state: Option<Arc<AppState>>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let mut workspaces = match workspace_repo
        .list_active_pr_poller_recovery_workspaces()
        .await
    {
        Ok(workspaces) => workspaces,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Agent workspace PR startup recovery: failed to list published workspaces"
            );
            Vec::new()
        }
    };

    let mut seen_conversations = workspaces
        .iter()
        .map(|workspace| workspace.conversation_id.as_str().to_string())
        .collect::<HashSet<_>>();
    match workspace_repo
        .list_pr_review_lifecycle_recovery_workspaces()
        .await
    {
        Ok(review_workspaces) => {
            for workspace in review_workspaces {
                if !seen_conversations.insert(workspace.conversation_id.as_str().to_string()) {
                    continue;
                }
                workspaces.push(workspace);
            }
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Agent workspace PR startup recovery: failed to list Review PR lifecycle recovery workspaces"
            );
        }
    }

    if workspaces.is_empty() {
        tracing::debug!("Agent workspace PR startup recovery: no PR poller workspaces");
        return;
    }

    tracing::info!(
        count = workspaces.len(),
        concurrency = AGENT_WORKSPACE_PR_POLLER_RECOVERY_CONCURRENCY,
        "Agent workspace PR startup recovery: found active PR poller workspaces"
    );

    futures::stream::iter(workspaces)
        .for_each_concurrent(
            AGENT_WORKSPACE_PR_POLLER_RECOVERY_CONCURRENCY,
            |workspace| {
                let workspace_repo = Arc::clone(&workspace_repo);
                let project_repo = Arc::clone(&project_repo);
                let plan_branch_repo = Arc::clone(&plan_branch_repo);
                let pr_poller_registry = Arc::clone(&pr_poller_registry);
                let agent_run_repo = Arc::clone(&agent_run_repo);
                let chat_service = Arc::clone(&chat_service);
                let notification_service = notification_service.as_ref().map(Arc::clone);
                let agent_workspace_repair_repo =
                    agent_workspace_repair_repo.as_ref().map(Arc::clone);
                let recovery_state = recovery_state.as_ref().map(Arc::clone);
                let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
                async move {
                    recover_one_agent_workspace_pr_poller(
                        workspace,
                        workspace_repo,
                        project_repo,
                        plan_branch_repo,
                        pr_poller_registry,
                        agent_run_repo,
                        chat_service,
                        notification_service,
                        agent_workspace_repair_repo,
                        recovery_state,
                        blocked_git_project_ids,
                    )
                    .await;
                }
            },
        )
        .await;
}

pub(crate) async fn recover_one_agent_workspace_pr_poller(
    workspace: AgentConversationWorkspace,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pr_poller_registry: Arc<PrPollerRegistry>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    chat_service: Arc<dyn ChatService>,
    notification_service: Option<Arc<NotificationService>>,
    agent_workspace_repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    recovery_state: Option<Arc<AppState>>,
    blocked_git_project_ids: Arc<HashSet<ProjectId>>,
) {
    let Some(pr_number) = agent_workspace_pr_poller_number(&workspace) else {
        return;
    };

    // The durable attempt is the repair authority. Check it before creating a review monitor,
    // reading GitHub, or registering a poller so a second startup/periodic pass cannot replay
    // legacy repair state beside an active leased attempt. A repository read failure is
    // deliberately fail-closed: the canonical recovery pass will retry it on a later cycle.
    if let Some(repair_repo) = agent_workspace_repair_repo.as_ref() {
        match repair_repo
            .get_current_repair_attempt(&workspace.conversation_id)
            .await
        {
            Ok(Some(attempt)) => {
                tracing::info!(
                    conversation_id = workspace.conversation_id.as_str(),
                    attempt_id = attempt.id.as_str(),
                    generation = attempt.generation,
                    phase = ?attempt.phase,
                    "Agent workspace PR startup recovery: durable repair remains authoritative"
                );
                return;
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    error = %error,
                    "Agent workspace PR startup recovery: durable repair authority could not be read; skipping poller recovery"
                );
                return;
            }
        }
    }

    let review_pr_monitor = if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
        let existing = match workspace_repo
            .get_pr_review_monitor(&workspace.conversation_id)
            .await
        {
            Ok(monitor) => monitor,
            Err(error) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    error = %error,
                    "Agent workspace PR startup recovery: failed to load Review PR monitor"
                );
                return;
            }
        };
        if existing.is_none() && !workspace.has_terminal_publication_pr_status() {
            let head_sha = workspace
                .source_pull_request
                .as_ref()
                .and_then(|pull_request| pull_request.head_ref_oid.clone());
            let mut monitor = AgentWorkspacePrReviewMonitor::new(
                workspace.conversation_id.clone(),
                workspace.project_id.clone(),
                pr_number,
                head_sha,
            );
            monitor.monitor_enabled = true;
            monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
            match workspace_repo.upsert_pr_review_monitor(monitor).await {
                Ok(monitor) => Some(monitor),
                Err(error) => {
                    tracing::warn!(
                        conversation_id = workspace.conversation_id.as_str(),
                        error = %error,
                        "Agent workspace PR startup recovery: failed to create missing Review PR monitor"
                    );
                    return;
                }
            }
        } else {
            existing
        }
    } else {
        None
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

    if workspace.mode == AgentConversationWorkspaceMode::ReviewPr
        && workspace.has_terminal_publication_pr_status()
    {
        let status = workspace
            .publication_pr_status
            .as_deref()
            .expect("terminal status checked above");
        let summary = if status == "merged" {
            "Pull request merged"
        } else {
            "Pull request closed without merging"
        };
        let Some(repair_repo) = agent_workspace_repair_repo.as_ref().map(Arc::clone) else {
            tracing::error!(
                conversation_id = workspace.conversation_id.as_str(),
                "Terminal workspace recovery requires durable repair authority"
            );
            return;
        };
        match settle_review_pr_terminal_observation(
            Arc::clone(&workspace_repo),
            repair_repo,
            Arc::clone(&agent_run_repo),
            Some(Arc::clone(&plan_branch_repo)),
            Some(Arc::clone(&chat_service)),
            notification_service,
            &workspace.conversation_id,
            &project,
            pr_number,
            status,
            summary,
        )
        .await
        {
            Ok(outcome) => {
                if let Err(error) = outcome.require_runtime_shutdown() {
                    tracing::warn!(
                        conversation_id = workspace.conversation_id.as_str(),
                        pr_number,
                        error,
                        "Agent workspace PR startup recovery: terminal authority converged with local cleanup pending"
                    );
                }
            }
            Err(error) => tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number,
                error = %error,
                "Agent workspace PR startup recovery: failed to converge persisted terminal Review PR state"
            ),
        }
        return;
    }

    if blocked_git_project_ids.contains(&project.id) {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            project_id = project.id.as_str(),
            pr_number,
            "Agent workspace PR startup recovery: skipping poller recovery due to Git auth preflight"
        );
        return;
    }

    let worktree_path = match resolve_agent_workspace_pr_poller_worktree_path(
        &project,
        &workspace,
        plan_branch_repo.as_ref(),
    )
    .await
    {
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

    if review_pr_monitor
        .as_ref()
        .is_some_and(|monitor| monitor.status == AgentWorkspacePrReviewMonitorStatus::Terminal)
    {
        let live_status = match pr_poller_registry
            .check_agent_workspace_pr_status_once(&worktree_path, pr_number)
            .await
        {
            Ok(Some(status)) => status,
            Ok(None) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    "Agent workspace PR startup recovery: GitHub status is unavailable for terminal-monitor repair"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Agent workspace PR startup recovery: live terminal-monitor repair check failed"
                );
                return;
            }
        };
        match live_status {
            PrStatus::Open => {
                match workspace_repo
                    .rearm_terminal_pr_review_monitor_after_live_open(
                        &workspace.conversation_id,
                        pr_number,
                    )
                    .await
                {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        tracing::warn!(
                            conversation_id = workspace.conversation_id.as_str(),
                            pr_number,
                            "Agent workspace PR startup recovery: legacy terminal monitor lost repair authority"
                        );
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(
                            conversation_id = workspace.conversation_id.as_str(),
                            pr_number,
                            error = %error,
                            "Agent workspace PR startup recovery: failed to rearm open legacy terminal monitor"
                        );
                        return;
                    }
                }
            }
            PrStatus::Merged { .. } | PrStatus::Closed => {
                let (status, summary) = match live_status {
                    PrStatus::Merged { .. } => ("merged", "Pull request merged"),
                    PrStatus::Closed => ("closed", "Pull request closed without merging"),
                    PrStatus::Open => unreachable!(),
                };
                let Some(repair_repo) = agent_workspace_repair_repo.as_ref().map(Arc::clone) else {
                    tracing::error!(
                        conversation_id = workspace.conversation_id.as_str(),
                        "Terminal workspace recovery requires durable repair authority"
                    );
                    return;
                };
                match settle_review_pr_terminal_observation(
                    Arc::clone(&workspace_repo),
                    repair_repo,
                    Arc::clone(&agent_run_repo),
                    Some(Arc::clone(&plan_branch_repo)),
                    Some(Arc::clone(&chat_service)),
                    notification_service,
                    &workspace.conversation_id,
                    &project,
                    pr_number,
                    status,
                    summary,
                )
                .await
                {
                    Ok(outcome) => {
                        if let Err(error) = outcome.require_runtime_shutdown() {
                            tracing::warn!(
                                conversation_id = workspace.conversation_id.as_str(),
                                pr_number,
                                error,
                                "Agent workspace PR startup recovery: live terminal authority converged with local cleanup pending"
                            );
                        }
                    }
                    Err(error) => tracing::warn!(
                        conversation_id = workspace.conversation_id.as_str(),
                        pr_number,
                        error = %error,
                        "Agent workspace PR startup recovery: failed to settle live terminal Review PR authority"
                    ),
                }
                return;
            }
        }
    }

    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr {
        let review_feedback = if let Some(repair_repo) = agent_workspace_repair_repo.as_ref() {
            pr_poller_registry
                .process_agent_workspace_review_feedback_once_with_repair_repo(
                    &workspace.conversation_id,
                    pr_number,
                    &worktree_path,
                    Arc::clone(&workspace_repo),
                    Arc::clone(&agent_run_repo),
                    Arc::clone(repair_repo),
                    Arc::clone(&chat_service),
                )
                .await
        } else {
            #[cfg(test)]
            {
                pr_poller_registry
                    .process_agent_workspace_review_feedback_once(
                        &workspace.conversation_id,
                        pr_number,
                        &worktree_path,
                        Arc::clone(&workspace_repo),
                        Arc::clone(&agent_run_repo),
                        Arc::clone(&chat_service),
                    )
                    .await
            }
            #[cfg(not(test))]
            {
                tracing::error!(
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    "Agent workspace PR startup recovery: refusing legacy review-feedback dispatch without durable repair authority"
                );
                return;
            }
        };
        match review_feedback {
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
    }

    if let Some(repair_repo) = agent_workspace_repair_repo {
        pr_poller_registry.start_agent_workspace_polling_with_repair_repo_and_recovery_state(
            workspace.conversation_id,
            pr_number,
            project,
            worktree_path,
            workspace_repo,
            agent_run_repo,
            repair_repo,
            chat_service,
            recovery_state,
        );
    } else {
        #[cfg(test)]
        pr_poller_registry.start_agent_workspace_polling(
            workspace.conversation_id,
            pr_number,
            project,
            worktree_path,
            workspace_repo,
            agent_run_repo,
            chat_service,
        );
        #[cfg(not(test))]
        tracing::error!(
            conversation_id = workspace.conversation_id.as_str(),
            pr_number,
            "Agent workspace PR startup recovery: refusing legacy poller construction without durable repair authority"
        );
    }
}

fn agent_workspace_pr_poller_number(workspace: &AgentConversationWorkspace) -> Option<i64> {
    if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
        workspace
            .source_pull_request
            .as_ref()
            .map(|pull_request| pull_request.number)
            .or(workspace.publication_pr_number)
    } else {
        workspace.publication_pr_number
    }
}

async fn resolve_agent_workspace_pr_poller_worktree_path(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    plan_branch_repo: &dyn PlanBranchRepository,
) -> Result<std::path::PathBuf, String> {
    if workspace.mode == AgentConversationWorkspaceMode::Ideation
        && workspace.linked_plan_branch_id.is_some()
    {
        let plan_branch_id = workspace
            .linked_plan_branch_id
            .as_ref()
            .expect("checked above");
        let plan_branch = plan_branch_repo
            .get_by_id(plan_branch_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Plan branch not found: {}", plan_branch_id))?;
        return ensure_linked_plan_branch_agent_worktree(project, &plan_branch)
            .await
            .map_err(|error| error.to_string());
    }

    resolve_valid_agent_conversation_workspace_path(project, workspace)
        .await
        .map_err(|error| error.to_string())
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
                match terminal_plan_branch_candidate_is_busy(
                    &project,
                    plan_branch,
                    &running_agent_registry,
                )
                .await
                {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(
                            plan_branch_id = plan_branch.id.as_str(),
                            error = %error,
                            "Terminal plan branch cleanup: target resolution failed closed"
                        );
                        continue;
                    }
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
            match terminal_plan_branch_candidate_is_busy(
                &project,
                &plan_branch,
                &running_agent_registry,
            )
            .await
            {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        plan_branch_id = plan_branch.id.as_str(),
                        error = %error,
                        "Terminal plan branch cleanup: target resolution failed closed"
                    );
                    continue;
                }
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
                    if let Some(status) = terminal_plan_branch_cleanup_marker_for_report(&report) {
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
                    if let Some(status) = terminal_plan_branch_cleanup_marker_for_report(&report) {
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
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    _github_service: Option<Arc<dyn GithubServiceTrait>>,
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
            continue;
        }

        for workspace in terminal_workspaces {
            match crate::application::agent_workspace_terminal_cleanup::terminal_cleanup_target_path(
                &workspace,
                &project,
                plan_branch_repo.as_ref(),
            )
            .await
            {
                Ok(path)
                    if terminal_cleanup_candidate_is_busy(&running_agent_registry, &path).await =>
                {
                    tracing::info!(
                        conversation_id = workspace.conversation_id.as_str(),
                        worktree_path = %path.display(),
                        "Terminal agent workspace cleanup: exact target is busy"
                    );
                    continue;
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    error,
                    "Terminal agent workspace cleanup: target resolution failed closed"
                ),
            }

            let outcome =
                crate::application::agent_workspace_terminal_cleanup::cleanup_terminal_agent_workspace_after_pr(
                    Arc::clone(&workspace_repo),
                    Some(Arc::clone(&plan_branch_repo)),
                    &workspace.conversation_id,
                    &project,
                )
                .await;
            stats.cleanup_markers_written += usize::from(matches!(
                outcome.cleanup_claim,
                crate::application::agent_workspace_terminal_cleanup::TerminalCleanupClaimState::Claimed
            ));
            if matches!(
                outcome.local_cleanup,
                crate::application::agent_workspace_terminal_cleanup::TerminalLocalCleanupResult::FailedOperational
                    | crate::application::agent_workspace_terminal_cleanup::TerminalLocalCleanupResult::FailedUnsafe
            ) {
                stats.branches_failed += 1;
            }
        }
    }

    stats.log_summary("agent_workspace", started_at, false);
}

pub async fn run_periodic_terminal_pr_local_cleanup(
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    github_service: Option<Arc<dyn GithubServiceTrait>>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
) {
    let Some(interval) = terminal_pr_local_cleanup_interval() else {
        tracing::info!("Terminal PR local cleanup: periodic cleanup disabled by runtime config");
        return;
    };

    run_periodic_terminal_pr_local_cleanup_with_interval(
        interval,
        plan_branch_repo,
        workspace_repo,
        project_repo,
        github_service,
        running_agent_registry,
    )
    .await;
}

async fn run_periodic_terminal_pr_local_cleanup_with_interval(
    interval: Duration,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    github_service: Option<Arc<dyn GithubServiceTrait>>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
) {
    loop {
        tokio::time::sleep(interval).await;
        run_terminal_pr_local_cleanup_once(
            Arc::clone(&plan_branch_repo),
            Arc::clone(&workspace_repo),
            Arc::clone(&project_repo),
            github_service.as_ref().map(Arc::clone),
            Arc::clone(&running_agent_registry),
        )
        .await;
    }
}

pub(crate) async fn run_terminal_pr_local_cleanup_once(
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    github_service: Option<Arc<dyn GithubServiceTrait>>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
) {
    git_cmd::with_git_command_lane(git_cmd::GitCommandLane::Background, async move {
        let unblocked_git_projects = Arc::new(HashSet::new());
        cleanup_terminal_plan_branch_local_artifacts_on_startup(
            Arc::clone(&plan_branch_repo),
            Arc::clone(&project_repo),
            github_service.as_ref().map(Arc::clone),
            Arc::clone(&unblocked_git_projects),
            Arc::clone(&running_agent_registry),
        )
        .await;
        cleanup_terminal_agent_workspace_local_artifacts_on_startup(
            workspace_repo,
            plan_branch_repo,
            project_repo,
            github_service,
            unblocked_git_projects,
            running_agent_registry,
        )
        .await;
    })
    .await;
}

fn terminal_pr_local_cleanup_interval() -> Option<Duration> {
    terminal_pr_local_cleanup_interval_from_secs(
        git_runtime_config().terminal_pr_local_cleanup_interval_secs,
    )
}

fn terminal_pr_local_cleanup_interval_from_secs(interval_secs: u64) -> Option<Duration> {
    if interval_secs == 0 {
        None
    } else {
        Some(Duration::from_secs(interval_secs))
    }
}

async fn terminal_plan_branch_candidate_is_busy(
    project: &Project,
    plan_branch: &PlanBranch,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
) -> crate::error::AppResult<bool> {
    let path = resolve_linked_plan_branch_agent_worktree_path(project, plan_branch)?;
    Ok(terminal_cleanup_candidate_is_busy(running_agent_registry, &path).await)
}

async fn terminal_cleanup_candidate_is_busy(
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    candidate_path: &std::path::Path,
) -> bool {
    let candidate = candidate_path
        .canonicalize()
        .unwrap_or_else(|_| candidate_path.to_path_buf());
    running_agent_registry
        .list_all()
        .await
        .into_iter()
        .filter_map(|(_, info)| info.worktree_path)
        .map(std::path::PathBuf::from)
        .map(|path| path.canonicalize().unwrap_or(path))
        .any(|path| path == candidate)
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
    transition_service: Arc<TaskTransitionService>,
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
        AgentConversationWorkspaceStatus, Artifact, ArtifactId, ArtifactType, ChatConversationId,
        ExecutionPlan, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionId,
    };
    use crate::domain::services::github_service::{
        PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState,
    };
    use crate::domain::services::RunningAgentKey;
    use crate::tests::mock_github_service::MockGithubService;
    use async_trait::async_trait;
    use tokio::sync::Mutex as TokioMutex;

    static TERMINAL_CLEANUP_FETCH_TEST_LOCK: LazyLock<TokioMutex<()>> =
        LazyLock::new(|| TokioMutex::new(()));

    struct StaticPlanPrDescriptionDrafter;

    #[async_trait]
    impl PlanPrDescriptionDrafter for StaticPlanPrDescriptionDrafter {
        async fn draft_plan_description(
            &self,
            _project: &Project,
            _plan_branch: &PlanBranch,
            _review_base: &str,
            _review_state: PrReviewState,
        ) -> crate::error::AppResult<crate::domain::entities::AgentWorkspacePrDescription> {
            Ok(crate::domain::entities::AgentWorkspacePrDescription::new(
                None,
                "## Summary\n\nStartup recovery drafted body".to_string(),
            ))
        }
    }

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

    async fn create_existing_pr_recovery_candidate(
        app_state: &AppState,
        project: &Project,
        branch_name: &str,
        pr_number: i64,
    ) -> (Task, PlanBranch) {
        let mut session = IdeationSession::new_with_title(project.id.clone(), "Startup PR Plan");
        session.mark_accepted();
        let session = app_state
            .ideation_session_repo
            .create(session)
            .await
            .expect("create ideation session");
        let execution_plan = app_state
            .execution_plan_repo
            .create(ExecutionPlan::new(session.id.clone()))
            .await
            .expect("create execution plan");
        let plan_artifact = app_state
            .artifact_repo
            .create(Artifact::new_inline(
                format!("Plan artifact {pr_number}"),
                ArtifactType::Specification,
                "Deliver the startup performance recovery plan.",
                "test",
            ))
            .await
            .expect("create artifact");

        let mut merge_task = Task::new(project.id.clone(), "Merge plan into main".to_string());
        merge_task.category = TaskCategory::PlanMerge;
        merge_task.internal_status = InternalStatus::WaitingOnPr;
        merge_task.ideation_session_id = Some(session.id.clone());
        merge_task.execution_plan_id = Some(execution_plan.id.clone());
        let merge_task = app_state
            .task_repo
            .create(merge_task)
            .await
            .expect("create merge task");

        let mut regular_task = Task::new(project.id.clone(), "Implement plan".to_string());
        regular_task.category = TaskCategory::Regular;
        regular_task.internal_status = InternalStatus::Merged;
        regular_task.ideation_session_id = Some(session.id.clone());
        regular_task.execution_plan_id = Some(execution_plan.id.clone());
        app_state
            .task_repo
            .create(regular_task)
            .await
            .expect("create completed regular task");

        let mut plan_branch = PlanBranch::new(
            plan_artifact.id,
            session.id,
            project.id.clone(),
            branch_name.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Active;
        plan_branch.execution_plan_id = Some(execution_plan.id);
        plan_branch.merge_task_id = Some(merge_task.id.clone());
        plan_branch.pr_eligible = true;
        plan_branch.pr_number = Some(pr_number);
        plan_branch.pr_status = Some(DbPrStatus::Open);
        plan_branch.pr_push_status = PrPushStatus::Pending;
        let plan_branch = app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("create plan branch");

        (merge_task, plan_branch)
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
            terminal_plan_branch_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                branch_deleted: true,
                ..LocalGitArtifactCleanupReport::default()
            }),
            Some("cleaned")
        );
        assert_eq!(
            terminal_plan_branch_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                skipped_reason: Some("branch_missing".to_string()),
                ..LocalGitArtifactCleanupReport::default()
            }),
            Some("branch_missing")
        );
        assert_eq!(
            terminal_plan_branch_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                skipped_reason: Some("target_ref_missing:main".to_string()),
                ..LocalGitArtifactCleanupReport::default()
            }),
            Some("target_ref_missing")
        );
        assert_eq!(
            terminal_plan_branch_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                skipped_reason: Some("branch_not_merged:main".to_string()),
                ..LocalGitArtifactCleanupReport::default()
            }),
            Some("unsafe")
        );
        assert_eq!(
            terminal_plan_branch_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                skipped_reason: Some("workspace_path_mismatch".to_string()),
                ..LocalGitArtifactCleanupReport::default()
            }),
            Some("unsafe")
        );
        assert_eq!(
            terminal_plan_branch_cleanup_marker_for_report(&LocalGitArtifactCleanupReport {
                skipped_reason: Some("agent_running".to_string()),
                ..LocalGitArtifactCleanupReport::default()
            }),
            None
        );
    }

    #[test]
    fn terminal_pr_local_cleanup_interval_can_be_disabled() {
        assert_eq!(terminal_pr_local_cleanup_interval_from_secs(0), None);
        assert_eq!(
            terminal_pr_local_cleanup_interval_from_secs(15),
            Some(Duration::from_secs(15))
        );
    }

    #[tokio::test]
    async fn periodic_terminal_cleanup_loop_retries_until_cancelled() {
        let app_state = AppState::new_test();

        let result = tokio::time::timeout(
            Duration::from_millis(25),
            run_periodic_terminal_pr_local_cleanup_with_interval(
                Duration::from_millis(1),
                Arc::clone(&app_state.plan_branch_repo),
                Arc::clone(&app_state.agent_conversation_workspace_repo),
                Arc::clone(&app_state.project_repo),
                None,
                Arc::clone(&app_state.running_agent_registry),
            ),
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn refresh_existing_pr_metadata_updates_pr_and_marks_ready() {
        let app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        let project = Project::new("Startup Metadata".to_string(), "/tmp/repo".to_string());
        let mut task = Task::new(project.id.clone(), "Merge plan into main".to_string());
        task.category = TaskCategory::PlanMerge;
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("metadata-artifact".to_string()),
            IdeationSessionId::from_string("metadata-session".to_string()),
            project.id.clone(),
            "ralphx/startup/metadata".to_string(),
            "main".to_string(),
        );
        plan_branch.merge_task_id = Some(task.id.clone());
        plan_branch.pr_number = Some(42);

        refresh_existing_pr_metadata(
            vec![PrMetadataRefreshJob {
                project,
                merge_task: task,
                plan_branch,
                review_state: PrReviewState::Ready,
            }],
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            Arc::clone(&app_state.ideation_session_repo),
            Arc::clone(&app_state.artifact_repo),
            Arc::new(StaticPlanPrDescriptionDrafter),
        )
        .await;

        let state = github.state();
        assert_eq!(state.update_pr_details_calls, 1);
        assert_eq!(state.mark_pr_ready_calls, 1);
        assert_eq!(state.last_mark_pr_ready_number, Some(42));
        let body = state
            .last_update_pr_details_body
            .as_deref()
            .expect("updated PR body should be captured");
        assert!(body.starts_with("## Summary\n\nStartup recovery drafted body"));
        assert!(!body.contains("## RalphX Status"));
        assert!(!body.contains("## How To Review"));
    }

    #[test]
    fn pr_creation_project_result_merge_accumulates_timings_and_jobs() {
        let project = Project::new("Startup Metadata".to_string(), "/tmp/repo".to_string());
        let merge_task = Task::new(project.id.clone(), "Merge plan into main".to_string());
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("metadata-artifact".to_string()),
            IdeationSessionId::from_string("metadata-session".to_string()),
            project.id.clone(),
            "ralphx/startup/metadata".to_string(),
            "main".to_string(),
        );
        plan_branch.pr_number = Some(42);

        let mut left = PrCreationRecoveryProjectResult {
            projects_blocked: 1,
            plan_branches_seen: 2,
            existing_pr_branches: 3,
            missing_pr_candidates: 4,
            missing_pr_repairs: 5,
            pending_push_syncs: 6,
            candidate_load_elapsed_ms: 7,
            project_task_load_elapsed_ms: 8,
            project_tasks_seen: 9,
            merge_task_read_elapsed_ms: 10,
            needs_recovery_elapsed_ms: 11,
            review_state_elapsed_ms: 12,
            existing_pr_refresh_lookup_elapsed_ms: 13,
            reviewable_diff_elapsed_ms: 14,
            create_pr_elapsed_ms: 15,
            slow_candidates: 16,
            metadata_refresh_jobs: Vec::new(),
        };
        let right = PrCreationRecoveryProjectResult {
            projects_blocked: 2,
            plan_branches_seen: 3,
            existing_pr_branches: 4,
            missing_pr_candidates: 5,
            missing_pr_repairs: 6,
            pending_push_syncs: 7,
            candidate_load_elapsed_ms: 8,
            project_task_load_elapsed_ms: 9,
            project_tasks_seen: 10,
            merge_task_read_elapsed_ms: 11,
            needs_recovery_elapsed_ms: 12,
            review_state_elapsed_ms: 13,
            existing_pr_refresh_lookup_elapsed_ms: 14,
            reviewable_diff_elapsed_ms: 15,
            create_pr_elapsed_ms: 16,
            slow_candidates: 17,
            metadata_refresh_jobs: vec![PrMetadataRefreshJob {
                project,
                merge_task,
                plan_branch,
                review_state: PrReviewState::Draft,
            }],
        };

        left.merge(right);

        assert_eq!(left.projects_blocked, 3);
        assert_eq!(left.plan_branches_seen, 5);
        assert_eq!(left.existing_pr_branches, 7);
        assert_eq!(left.missing_pr_candidates, 9);
        assert_eq!(left.missing_pr_repairs, 11);
        assert_eq!(left.pending_push_syncs, 13);
        assert_eq!(left.candidate_load_elapsed_ms, 15);
        assert_eq!(left.project_task_load_elapsed_ms, 17);
        assert_eq!(left.project_tasks_seen, 19);
        assert_eq!(left.merge_task_read_elapsed_ms, 21);
        assert_eq!(left.needs_recovery_elapsed_ms, 23);
        assert_eq!(left.review_state_elapsed_ms, 25);
        assert_eq!(left.existing_pr_refresh_lookup_elapsed_ms, 27);
        assert_eq!(left.reviewable_diff_elapsed_ms, 29);
        assert_eq!(left.create_pr_elapsed_ms, 31);
        assert_eq!(left.slow_candidates, 33);
        assert_eq!(left.metadata_refresh_jobs.len(), 1);
    }

    #[tokio::test]
    async fn recover_missing_draft_prs_uses_targeted_candidates_and_refreshes_existing_prs() {
        let app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        let project = app_state
            .project_repo
            .create(Project::new(
                "Startup PR Recovery".to_string(),
                "/tmp/startup-pr-recovery".to_string(),
            ))
            .await
            .expect("create project");
        let (merge_task, plan_branch) = create_existing_pr_recovery_candidate(
            &app_state,
            &project,
            "ralphx/startup/existing-pr",
            77,
        )
        .await;

        recover_missing_draft_prs(
            Arc::clone(&app_state.task_repo),
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&app_state.project_repo),
            Arc::clone(&app_state.execution_plan_repo),
            Arc::clone(&app_state.ideation_session_repo),
            Arc::clone(&app_state.artifact_repo),
            Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            Arc::new(StaticPlanPrDescriptionDrafter),
            Arc::new(HashSet::new()),
        )
        .await;

        let refreshed_branch = app_state
            .plan_branch_repo
            .get_by_id(&plan_branch.id)
            .await
            .expect("load refreshed branch")
            .expect("plan branch exists");
        assert_eq!(refreshed_branch.pr_push_status, PrPushStatus::Pushed);
        assert_eq!(github.state().push_branch_calls, 1);

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if github.state().update_pr_details_calls > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("metadata refresh should run in background");

        {
            let state = github.state();
            assert_eq!(state.update_pr_details_calls, 1);
            assert_eq!(state.mark_pr_ready_calls, 1);
            assert_eq!(state.last_mark_pr_ready_number, Some(77));
            assert_eq!(
                state.last_push_branch_name.as_deref(),
                Some("ralphx/startup/existing-pr")
            );
            assert!(state
                .last_update_pr_details_body
                .as_deref()
                .unwrap_or_default()
                .starts_with("## Summary\n\nStartup recovery drafted body"));
            assert!(!state
                .last_update_pr_details_body
                .as_deref()
                .unwrap_or_default()
                .contains("## RalphX Status"));
            assert!(!state
                .last_update_pr_details_body
                .as_deref()
                .unwrap_or_default()
                .contains("## How To Review"));
        }

        let stored_merge_task = app_state
            .task_repo
            .get_by_id(&merge_task.id)
            .await
            .expect("load merge task")
            .expect("merge task exists");
        assert_eq!(
            stored_merge_task.internal_status,
            InternalStatus::WaitingOnPr
        );
    }

    #[tokio::test]
    async fn pr_recovery_snapshot_and_execution_plan_checks_are_targeted() {
        let app_state = AppState::new_test();
        let project = Project::new(
            "Startup PR Snapshot".to_string(),
            "/tmp/startup-pr-snapshot".to_string(),
        );
        let (_, plan_branch) = create_existing_pr_recovery_candidate(
            &app_state,
            &project,
            "ralphx/startup/snapshot",
            78,
        )
        .await;
        let execution_plan_id = plan_branch
            .execution_plan_id
            .clone()
            .expect("plan branch has execution plan");
        let merge_task_id = plan_branch
            .merge_task_id
            .clone()
            .expect("plan branch has merge task");
        let merge_task = app_state
            .task_repo
            .get_by_id(&merge_task_id)
            .await
            .expect("load merge task")
            .expect("merge task exists");
        let snapshot = ProjectPrRecoveryTaskSnapshot::from_targeted_tasks(
            vec![merge_task.clone()],
            HashSet::from([(plan_branch.session_id.clone(), execution_plan_id.clone())]),
        );

        assert_eq!(snapshot.task_count, 1);
        assert!(snapshot.get_task(&merge_task_id).is_some());
        assert!(snapshot.has_merged_regular_plan_task(&plan_branch.session_id, &execution_plan_id));
        assert!(
            plan_branch_needs_pr_recovery(
                &snapshot,
                &project,
                &plan_branch,
                &merge_task,
                Some(&execution_plan_id),
            )
            .await
        );

        let mut archived_project = project.clone();
        archived_project.archived_at = Some(Utc::now());
        assert!(
            !plan_branch_needs_pr_recovery(
                &snapshot,
                &archived_project,
                &plan_branch,
                &merge_task,
                Some(&execution_plan_id),
            )
            .await
        );
        assert_eq!(
            active_execution_plan_id_for_branch(&app_state.execution_plan_repo, &plan_branch).await,
            Some(execution_plan_id.clone())
        );

        let mut fallback_branch = plan_branch.clone();
        fallback_branch.execution_plan_id = None;
        assert_eq!(
            active_execution_plan_id_for_branch(&app_state.execution_plan_repo, &fallback_branch)
                .await,
            Some(execution_plan_id)
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
    async fn startup_terminal_plan_cleanup_ignores_unrelated_running_agent() {
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

        assert!(!branch_exists(repo.path(), branch));
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
            .create_or_update(workspace.clone())
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());

        cleanup_terminal_agent_workspace_local_artifacts_on_startup(
            Arc::clone(&app_state.agent_conversation_workspace_repo),
            Arc::clone(&app_state.plan_branch_repo),
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
    async fn periodic_terminal_cleanup_once_removes_merged_workspace_artifacts() {
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

        run_terminal_pr_local_cleanup_once(
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&app_state.agent_conversation_workspace_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(!worktree_path.exists());
        assert!(!branch_exists(repo.path(), &branch));
        assert_eq!(github.state().fetch_remote_calls, 0);
    }

    #[tokio::test]
    async fn periodic_terminal_cleanup_once_removes_merged_plan_branch_artifacts() {
        let app_state = AppState::new_test();
        let repo = init_cleanup_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = app_state
            .project_repo
            .create(cleanup_project(repo.path(), worktrees.path()))
            .await
            .unwrap();
        let branch = "ralphx/startup-cleanup/plan-periodic";

        run_git(repo.path(), &["checkout", "-b", branch]);
        std::fs::write(repo.path().join("periodic-plan.txt"), "plan\n").expect("write plan");
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "periodic plan work"]);
        run_git(repo.path(), &["checkout", "main"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", branch, "-m", "merge periodic plan"],
        );

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("periodic-plan-artifact"),
            IdeationSessionId::from_string("periodic-plan-session"),
            project.id.clone(),
            branch.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Merged;
        plan_branch.pr_eligible = true;
        plan_branch.pr_number = Some(130);
        plan_branch.pr_status = Some(DbPrStatus::Merged);
        app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .unwrap();
        let github = Arc::new(MockGithubService::new());

        run_terminal_pr_local_cleanup_once(
            Arc::clone(&app_state.plan_branch_repo),
            Arc::clone(&app_state.agent_conversation_workspace_repo),
            Arc::clone(&app_state.project_repo),
            Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
            Arc::clone(&app_state.running_agent_registry),
        )
        .await;

        assert!(!branch_exists(repo.path(), branch));
        assert_eq!(github.state().fetch_remote_calls, 0);
    }

    #[tokio::test]
    async fn startup_terminal_agent_workspace_cleanup_ignores_unrelated_running_agent() {
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
            Arc::clone(&app_state.plan_branch_repo),
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
            Arc::clone(&app_state.plan_branch_repo),
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
            Arc::clone(&app_state.plan_branch_repo),
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
                .build_transition_service_for_runtime(Arc::new(ExecutionState::new()), None)
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
                .build_transition_service_for_runtime(Arc::new(ExecutionState::new()), None)
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
