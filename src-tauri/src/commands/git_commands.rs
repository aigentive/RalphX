// Git commands for task commits, diff, merge, and cleanup
// Thin layer that delegates to GitService and TaskTransitionService

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Manager, State};

use crate::application::git_service::{CommitInfo, DiffStats, GitService};
use crate::application::runtime_factory::{
    RuntimeFactoryDeps, build_task_scheduler_with_fallback,
    build_transition_service_with_fallback,
};
use crate::application::{AppState, TaskTransitionService};
use crate::commands::execution_commands::AGENT_ACTIVE_STATUSES;
use crate::commands::ExecutionState;
use crate::domain::entities::{GitMode, InternalStatus, PlanBranch, ProjectId, TaskId};
use crate::domain::repositories::ExecutionSettingsRepository;
use crate::domain::state_machine::services::TaskScheduler;

/// Response for get_task_commits command
#[derive(Debug, Serialize)]
pub struct TaskCommitsResponse {
    pub commits: Vec<CommitInfoResponse>,
}

/// Individual commit info for response
#[derive(Debug, Serialize)]
pub struct CommitInfoResponse {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
}

impl From<CommitInfo> for CommitInfoResponse {
    fn from(info: CommitInfo) -> Self {
        Self {
            sha: info.sha,
            short_sha: info.short_sha,
            message: info.message,
            author: info.author,
            timestamp: info.timestamp,
        }
    }
}

/// Response for get_task_diff_stats command
#[derive(Debug, Serialize)]
pub struct TaskDiffStatsResponse {
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
    pub changed_files: Vec<String>,
}

impl From<DiffStats> for TaskDiffStatsResponse {
    fn from(stats: DiffStats) -> Self {
        Self {
            files_changed: stats.files_changed,
            insertions: stats.insertions,
            deletions: stats.deletions,
            changed_files: stats.changed_files,
        }
    }
}

/// Get commits on task branch since it diverged from base
///
/// Returns the list of commits made on this task's branch.
/// Used by UI to show commit history in task detail views.
///
/// For merged tasks (where branch/worktree is deleted), uses the merge_commit_sha
/// to query commit history from the repository.
#[tauri::command]
pub async fn get_task_commits(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<TaskCommitsResponse, String> {
    get_task_commits_for_state(TaskId::from_string(task_id), state.inner()).await
}

#[doc(hidden)]
pub async fn get_task_commits_for_state(
    task_id: TaskId,
    state: &AppState,
) -> Result<TaskCommitsResponse, String> {
    // Get task
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    // Get project for base branch and working directory
    let project = state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", task.project_id.as_str()))?;

    let base_branch = project.base_branch.as_deref().unwrap_or("main");
    let repo_path = PathBuf::from(&project.working_directory);
    let plan_branch = get_branchless_plan_branch(state, &task, &task_id).await?;

    // Handle merged tasks specially - branch/worktree is deleted, use merge_commit_sha
    if task.internal_status == InternalStatus::Merged {
        let merge_sha = task.merge_commit_sha.as_deref().or_else(|| {
            plan_branch
                .as_ref()
                .and_then(|branch| branch.merge_commit_sha.as_deref())
        });
        if let Some(merge_sha) = merge_sha {
            let base_ref = plan_branch
                .as_ref()
                .map(|branch| plan_branch_review_base_ref(branch, base_branch))
                .unwrap_or_else(|| base_branch.to_string());
            let commits = GitService::get_merged_task_commits(&repo_path, &base_ref, merge_sha)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(TaskCommitsResponse {
                commits: commits.into_iter().map(CommitInfoResponse::from).collect(),
            });
        }

        if let Some(plan_branch) = plan_branch {
            let base_ref = plan_branch_review_base_ref(&plan_branch, base_branch);
            let commits =
                GitService::get_commits_between(&repo_path, &base_ref, &plan_branch.branch_name)
                    .await
                    .map_err(|e| e.to_string())?;
            return Ok(TaskCommitsResponse {
                commits: commits.into_iter().map(CommitInfoResponse::from).collect(),
            });
        }

        // Merged but no merge_commit_sha - return empty (shouldn't happen)
        return Ok(TaskCommitsResponse { commits: vec![] });
    }

    if let Some(plan_branch) = plan_branch {
        let base_ref = plan_branch_review_base_ref(&plan_branch, base_branch);
        let commits =
            GitService::get_commits_between(&repo_path, &base_ref, &plan_branch.branch_name)
                .await
                .map_err(|e| e.to_string())?;
        return Ok(TaskCommitsResponse {
            commits: commits.into_iter().map(CommitInfoResponse::from).collect(),
        });
    }

    if task.task_branch.is_none() {
        return Err("Task has no branch assigned".to_string());
    }

    // Determine working path — worktree path if available, else project dir
    let working_path = task
        .worktree_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&project.working_directory));

    // Get commits since base (from HEAD of the working path)
    let commits = GitService::get_commits_since(&working_path, base_branch)
        .await
        .map_err(|e| e.to_string())?;

    Ok(TaskCommitsResponse {
        commits: commits.into_iter().map(CommitInfoResponse::from).collect(),
    })
}

async fn get_branchless_plan_branch(
    state: &AppState,
    task: &crate::domain::entities::Task,
    task_id: &TaskId,
) -> Result<Option<PlanBranch>, String> {
    if task.task_branch.is_some() {
        return Ok(None);
    }

    state
        .plan_branch_repo
        .get_by_merge_task_id(task_id)
        .await
        .map_err(|e| e.to_string())
}

fn plan_branch_review_base_ref(plan_branch: &PlanBranch, project_base_branch: &str) -> String {
    plan_branch
        .base_branch_override
        .as_deref()
        .filter(|branch| !branch.is_empty())
        .or_else(|| {
            (!plan_branch.source_branch.is_empty()).then_some(plan_branch.source_branch.as_str())
        })
        .unwrap_or(project_base_branch)
        .to_string()
}

/// Get diff statistics for task branch compared to base
///
/// Returns stats about files changed, lines added/deleted.
/// Used by UI to show change summary in task detail views.
#[tauri::command]
pub async fn get_task_diff_stats(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<TaskDiffStatsResponse, String> {
    let task_id = TaskId::from_string(task_id);

    // Get task
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    // Get project for base branch and working directory
    let project = state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", task.project_id.as_str()))?;

    let base_branch = project.base_branch.as_deref().unwrap_or("main");

    // Determine working path — worktree path if available, else project dir
    let working_path = task
        .worktree_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&project.working_directory));

    // Get diff stats
    let stats = GitService::get_diff_stats(&working_path, base_branch)
        .await
        .map_err(|e| e.to_string())?;

    Ok(TaskDiffStatsResponse::from(stats))
}

/// Mark merge conflict as resolved after manual user resolution
///
/// User has resolved conflicts externally (in IDE), this command:
/// 1. Commits the resolved state
/// 2. Transitions task from MergeConflict to Merged
/// 3. Cleans up branch/worktree
#[tauri::command]
pub async fn resolve_merge_conflict(
    task_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<(), String> {
    let task_id_parsed = TaskId::from_string(task_id);

    // Get task
    let task = state
        .task_repo
        .get_by_id(&task_id_parsed)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id_parsed.as_str()))?;

    // Validate task is in a resolvable merge state
    ensure_resolve_merge_status(task.internal_status)?;

    // Get project
    let project = state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", task.project_id.as_str()))?;

    // Determine working path (prefer existing worktree, fallback to project repo)
    let working_path = task
        .worktree_path
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(&project.working_directory));

    // Only enforce marker checks while merge/rebase is still in progress.
    // If git history is already complete, marker-like literals in unrelated files
    // should not block manual completion.
    let merge_in_progress = GitService::is_merge_in_progress(&working_path);
    let rebase_in_progress = GitService::is_rebase_in_progress(&working_path);
    if (merge_in_progress || rebase_in_progress)
        && GitService::has_conflict_markers(&working_path)
            .await
            .unwrap_or(true)
    {
        return Err(
            "Conflict markers still present in merge-related changed files. Please resolve all conflicts before marking as complete."
                .to_string(),
        );
    }

    // Commit the resolved merge
    let commit_message = format!("Merge resolution for task: {}", task.title);
    let commit_sha =
        GitService::commit_all_including_deletions(&working_path, &commit_message)
            .await
            .map_err(|e| format!("Failed to commit resolved merge: {}", e))?;

    // Update task with merge commit SHA if commit was made
    if let Some(sha) = &commit_sha {
        let mut updated_task = task.clone();
        updated_task.merge_commit_sha = Some(sha.clone());
        state
            .task_repo
            .update(&updated_task)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Create transition service and transition to Merged
    let transition_service = create_transition_service(&state, &execution_state);

    transition_service
        .transition_task(&task_id_parsed, InternalStatus::Merged)
        .await
        .map_err(|e| e.to_string())?;

    // Cleanup branch/worktree
    cleanup_task_git_resources(&task, &project).await?;

    Ok(())
}

/// Re-attempt merge after user made changes
///
/// Transitions task back to PendingMerge to trigger programmatic merge attempt.
/// Used when user wants to retry after resolving issues.
///
/// This command returns immediately after scheduling the retry. The actual merge
/// execution happens asynchronously in the background.
#[tauri::command]
pub async fn retry_merge(
    task_id: String,
    skip_validation: Option<bool>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<(), String> {
    retry_merge_inner(
        TaskId::from_string(task_id),
        skip_validation,
        state.inner(),
        Arc::clone(execution_state.inner()),
    )
    .await
}

#[doc(hidden)]
pub async fn retry_merge_for_test(
    task_id: TaskId,
    skip_validation: Option<bool>,
    app_state: &AppState,
    execution_state: Arc<ExecutionState>,
) -> Result<(), String> {
    retry_merge_inner(task_id, skip_validation, app_state, execution_state).await
}

async fn retry_merge_inner(
    task_id_parsed: TaskId,
    skip_validation: Option<bool>,
    app_state: &AppState,
    execution_state: Arc<ExecutionState>,
) -> Result<(), String> {
    // Get task
    let mut task = app_state
        .task_repo
        .get_by_id(&task_id_parsed)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id_parsed.as_str()))?;

    // Validate task is in a mergeable retry state
    ensure_retry_merge_status(task.internal_status)?;

    if task.internal_status == InternalStatus::MergeIncomplete
        && crate::domain::state_machine::transition_handler::task_has_commit_hook_merge_failure(
            &task,
        )
    {
        let hook_error =
            crate::domain::state_machine::transition_handler::extract_commit_hook_merge_error(
                &task,
            );
        let should_reroute = hook_error
            .as_deref()
            .map(|error| {
                let kind =
                    crate::domain::state_machine::transition_handler::classify_commit_hook_failure_text(
                        error,
                    );
                let fingerprint =
                    crate::domain::state_machine::transition_handler::commit_hook_failure_fingerprint(
                        error,
                    );
                !matches!(
                    kind,
                    crate::domain::state_machine::transition_handler::CommitHookFailureKind::EnvironmentFailure
                ) && !crate::domain::state_machine::transition_handler::is_repeated_commit_hook_failure(
                    &task,
                    &fingerprint,
                )
            })
            .unwrap_or(false);

        if should_reroute {
            tracing::info!(
                task_id = task_id_parsed.as_str(),
                "Manual retry detected commit-hook MergeIncomplete — rerouting to revision flow"
            );
            let transition_service = app_state
                .build_transition_service_with_execution_state(Arc::clone(&execution_state));
            transition_service
                .reroute_commit_hook_merge_failure(&task_id_parsed, None, true, "manual_retry")
                .await
                .map_err(|e| e.to_string())?;
            return Ok(());
        }

        tracing::info!(
            task_id = task_id_parsed.as_str(),
            "Manual retry detected blocked/repeated commit-hook MergeIncomplete — retrying merge path"
        );
    }

    // Check if merge retry is already in progress
    let metadata_json = task
        .metadata
        .as_ref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    // Check with 60s staleness: timestamp-based guard auto-expires if background task crashed
    let retry_in_progress = metadata_json
        .get("merge_retry_in_progress")
        .and_then(|v| {
            // Timestamp guard — check staleness
            if let Some(ts) = v.as_str() {
                let started = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
                let age = chrono::Utc::now() - started.with_timezone(&chrono::Utc);
                return Some(age < chrono::Duration::seconds(60));
            }
            // Legacy boolean or other non-string: treat as stale (no timestamp = cannot verify freshness)
            tracing::warn!(
                task_id = task_id_parsed.as_str(),
                value = ?v,
                "Legacy non-timestamp merge_retry_in_progress found, treating as stale"
            );
            Some(false)
        })
        .unwrap_or(false);
    if retry_in_progress {
        tracing::info!(
            task_id = task_id_parsed.as_str(),
            "Merge retry already in progress, ignoring duplicate request"
        );
        return Ok(());
    }

    // Set in-flight guard and optional skip_validation flag
    let mut meta_obj = metadata_json.as_object().cloned().unwrap_or_default();
    meta_obj.insert(
        "merge_retry_in_progress".to_string(),
        serde_json::json!(chrono::Utc::now().to_rfc3339()),
    );

    if skip_validation == Some(true) {
        meta_obj.insert("skip_validation".to_string(), serde_json::json!(true));
    }

    // User-initiated retry: reset all loop-prevention counters so the reconciler
    // won't block subsequent auto-retries if this attempt fails again.
    // These counters are meant to prevent automated infinite loops, not block
    // explicit user actions.
    meta_obj.insert("validation_revert_count".to_string(), serde_json::json!(0));
    meta_obj.remove("merge_failure_source");
    // Reset merge recovery event log so auto-retry counters (which count
    // AutoRetryTriggered / AttemptFailed events) restart from zero.
    // Also clear the circuit breaker on manual retry — user is explicitly requesting a retry.
    if let Some(recovery_val) = meta_obj.get_mut("merge_recovery") {
        if let Some(recovery_obj) = recovery_val.as_object_mut() {
            recovery_obj.insert("events".to_string(), serde_json::json!([]));
            recovery_obj.insert("last_state".to_string(), serde_json::json!("retrying"));
            // Clear circuit breaker on manual retry
            recovery_obj.insert("circuit_breaker_active".to_string(), serde_json::json!(false));
            recovery_obj.remove("circuit_breaker_reason");
        }
    }

    task.metadata = Some(serde_json::Value::Object(meta_obj).to_string());
    app_state
        .task_repo
        .update(&task)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!(
        task_id = task_id_parsed.as_str(),
        skip_validation = skip_validation.unwrap_or(false),
        "Retry merge accepted, spawning background execution"
    );

    // Clone necessary repositories and state for background task
    let task_repo = Arc::clone(&app_state.task_repo);
    let task_dependency_repo = Arc::clone(&app_state.task_dependency_repo);
    let project_repo = Arc::clone(&app_state.project_repo);
    let artifact_repo = Arc::clone(&app_state.artifact_repo);
    let chat_message_repo = Arc::clone(&app_state.chat_message_repo);
    let chat_attachment_repo = Arc::clone(&app_state.chat_attachment_repo);
    let chat_conversation_repo = Arc::clone(&app_state.chat_conversation_repo);
    let agent_run_repo = Arc::clone(&app_state.agent_run_repo);
    let ideation_session_repo = Arc::clone(&app_state.ideation_session_repo);
    let activity_event_repo = Arc::clone(&app_state.activity_event_repo);
    let message_queue = Arc::clone(&app_state.message_queue);
    let running_agent_registry = Arc::clone(&app_state.running_agent_registry);
    let plan_branch_repo = Arc::clone(&app_state.plan_branch_repo);
    let memory_event_repo = Arc::clone(&app_state.memory_event_repo);
    let execution_settings_repo = Arc::clone(&app_state.execution_settings_repo);
    let agent_lane_settings_repo = Arc::clone(&app_state.agent_lane_settings_repo);
    let execution_state_clone = Arc::clone(&execution_state);
    let app_handle_opt = app_state.app_handle.clone();
    let task_id_for_spawn = task_id_parsed.clone();
    let ipr = Arc::clone(&app_state.interactive_process_registry);

    // Spawn background task for merge execution
    tokio::spawn(async move {
        execute_merge_retry_background(
            task_id_for_spawn,
            task_repo,
            task_dependency_repo,
            project_repo,
            artifact_repo,
            chat_message_repo,
            chat_attachment_repo,
            chat_conversation_repo,
            agent_run_repo,
            ideation_session_repo,
            activity_event_repo,
            message_queue,
            running_agent_registry,
            plan_branch_repo,
            memory_event_repo,
            execution_settings_repo,
            agent_lane_settings_repo,
            execution_state_clone,
            app_handle_opt,
            ipr,
        )
        .await;
    });

    Ok(())
}

/// Background execution of merge retry
///
/// This function runs the actual merge transition in the background.
/// It ensures the in-flight guard is cleared on completion.
#[allow(clippy::too_many_arguments)]
async fn execute_merge_retry_background(
    task_id: TaskId,
    task_repo: Arc<dyn crate::domain::repositories::TaskRepository>,
    task_dependency_repo: Arc<dyn crate::domain::repositories::TaskDependencyRepository>,
    project_repo: Arc<dyn crate::domain::repositories::ProjectRepository>,
    artifact_repo: Arc<dyn crate::domain::repositories::ArtifactRepository>,
    chat_message_repo: Arc<dyn crate::domain::repositories::ChatMessageRepository>,
    chat_attachment_repo: Arc<dyn crate::domain::repositories::ChatAttachmentRepository>,
    chat_conversation_repo: Arc<dyn crate::domain::repositories::ChatConversationRepository>,
    agent_run_repo: Arc<dyn crate::domain::repositories::AgentRunRepository>,
    ideation_session_repo: Arc<dyn crate::domain::repositories::IdeationSessionRepository>,
    activity_event_repo: Arc<dyn crate::domain::repositories::ActivityEventRepository>,
    message_queue: Arc<crate::domain::services::MessageQueue>,
    running_agent_registry: Arc<dyn crate::domain::services::RunningAgentRegistry>,
    plan_branch_repo: Arc<dyn crate::domain::repositories::PlanBranchRepository>,
    memory_event_repo: Arc<dyn crate::domain::repositories::MemoryEventRepository>,
    execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
    agent_lane_settings_repo: Arc<dyn crate::domain::repositories::AgentLaneSettingsRepository>,
    execution_state: Arc<ExecutionState>,
    app_handle_opt: Option<tauri::AppHandle>,
    interactive_process_registry: Arc<crate::application::InteractiveProcessRegistry>,
) {
    tracing::info!(
        task_id = task_id.as_str(),
        "Background merge retry execution started"
    );

    let deps = RuntimeFactoryDeps {
        task_repo: Arc::clone(&task_repo),
        task_dependency_repo: Arc::clone(&task_dependency_repo),
        project_repo: Arc::clone(&project_repo),
        artifact_repo: Arc::clone(&artifact_repo),
        chat_message_repo: Arc::clone(&chat_message_repo),
        chat_attachment_repo: Arc::clone(&chat_attachment_repo),
        conversation_repo: Arc::clone(&chat_conversation_repo),
        agent_run_repo: Arc::clone(&agent_run_repo),
        ideation_session_repo: Arc::clone(&ideation_session_repo),
        activity_event_repo: Arc::clone(&activity_event_repo),
        message_queue: Arc::clone(&message_queue),
        running_agent_registry: Arc::clone(&running_agent_registry),
        memory_event_repo: Arc::clone(&memory_event_repo),
        agent_clients: None,
        execution_plan_repo: None,
        execution_settings_repo: Some(Arc::clone(&execution_settings_repo)),
        agent_lane_settings_repo: Some(Arc::clone(&agent_lane_settings_repo)),
        review_repo: app_handle_opt
            .as_ref()
            .and_then(|handle| handle.try_state::<AppState>())
            .map(|app_state| Arc::clone(&app_state.review_repo)),
        plan_branch_repo: Some(Arc::clone(&plan_branch_repo)),
        interactive_process_registry: Some(Arc::clone(&interactive_process_registry)),
        github_service: None,
        pr_poller_registry: None,
    };

    // Create transition service with all necessary dependencies
    let scheduler_concrete = Arc::new(build_task_scheduler_with_fallback(
        &app_handle_opt,
        Arc::clone(&execution_state),
        &deps,
    ));
    scheduler_concrete.set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
    let task_scheduler: Arc<dyn TaskScheduler> = scheduler_concrete;

    let transition_service = build_transition_service_with_fallback(
        &app_handle_opt,
        Arc::clone(&execution_state),
        &deps,
    )
    .with_task_scheduler(task_scheduler)
    .into_arc();

    let result = transition_service
        .transition_task(&task_id, InternalStatus::PendingMerge)
        .await;

    // Clear in-flight guard regardless of success/failure
    if let Err(e) = clear_merge_retry_guard(&task_id, &task_repo).await {
        tracing::warn!(
            task_id = task_id.as_str(),
            error = %e,
            "Failed to clear merge retry guard after completion"
        );
    }

    match result {
        Ok(_) => {
            tracing::info!(
                task_id = task_id.as_str(),
                "Background merge retry execution completed successfully"
            );
        }
        Err(e) => {
            tracing::error!(
                task_id = task_id.as_str(),
                error = %e,
                "Background merge retry execution failed"
            );
        }
    }
}

/// Clear the merge_retry_in_progress flag from task metadata
async fn clear_merge_retry_guard(
    task_id: &TaskId,
    task_repo: &Arc<dyn crate::domain::repositories::TaskRepository>,
) -> Result<(), String> {
    let mut task = task_repo
        .get_by_id(task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    let metadata_json = task
        .metadata
        .as_ref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let mut meta_obj = metadata_json.as_object().cloned().unwrap_or_default();

    meta_obj.remove("merge_retry_in_progress");

    task.metadata = Some(serde_json::Value::Object(meta_obj).to_string());
    task_repo.update(&task).await.map_err(|e| e.to_string())?;

    tracing::debug!(
        task_id = task_id.as_str(),
        "Cleared merge retry in-flight guard"
    );

    Ok(())
}

fn ensure_resolve_merge_status(status: InternalStatus) -> Result<(), String> {
    if matches!(
        status,
        InternalStatus::MergeConflict | InternalStatus::MergeIncomplete
    ) {
        return Ok(());
    }

    Err(format!(
        "Task is not in MergeConflict or MergeIncomplete state (current: {:?})",
        status
    ))
}

fn ensure_retry_merge_status(status: InternalStatus) -> Result<(), String> {
    if matches!(
        status,
        InternalStatus::MergeConflict | InternalStatus::MergeIncomplete | InternalStatus::Merging
    ) {
        return Ok(());
    }

    Err(format!(
        "Task is not in a state that allows merge retry (current: {:?})",
        status
    ))
}

/// Manual cleanup for task branch/worktree
///
/// Used for failed/cancelled tasks that have git resources to clean up.
/// Does not change task status.
#[tauri::command]
pub async fn cleanup_task_branch(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let task_id_parsed = TaskId::from_string(task_id);

    // Get task
    let task = state
        .task_repo
        .get_by_id(&task_id_parsed)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id_parsed.as_str()))?;

    // Task should have a branch to clean up
    if task.task_branch.is_none() {
        return Err("Task has no branch to clean up".to_string());
    }

    // Get project
    let project = state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", task.project_id.as_str()))?;

    // Cleanup git resources
    cleanup_task_git_resources(&task, &project).await?;

    // Clear git fields on task
    let mut updated_task = task.clone();
    updated_task.task_branch = None;
    updated_task.worktree_path = None;
    state
        .task_repo
        .update(&updated_task)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod transition_guard_tests {
    use super::*;
    use crate::application::AppState;
    use crate::commands::ExecutionState;
    use crate::domain::entities::{Project, Task};
    use std::sync::Arc;

    #[test]
    fn resolve_merge_accepts_conflict_states() {
        assert!(ensure_resolve_merge_status(InternalStatus::MergeConflict).is_ok());
        assert!(ensure_resolve_merge_status(InternalStatus::MergeIncomplete).is_ok());
    }

    #[test]
    fn resolve_merge_rejects_non_conflict_states() {
        let error = ensure_resolve_merge_status(InternalStatus::Approved)
            .expect_err("approved task must not resolve merge conflict");
        assert!(error.contains("MergeConflict"));
        assert!(error.contains("MergeIncomplete"));
        assert!(error.contains("Approved"));
    }

    #[test]
    fn retry_merge_accepts_retryable_states() {
        assert!(ensure_retry_merge_status(InternalStatus::MergeConflict).is_ok());
        assert!(ensure_retry_merge_status(InternalStatus::MergeIncomplete).is_ok());
        assert!(ensure_retry_merge_status(InternalStatus::Merging).is_ok());
    }

    #[test]
    fn retry_merge_rejects_terminal_states() {
        let error =
            ensure_retry_merge_status(InternalStatus::Merged).expect_err("merged task must reject retry");
        assert!(error.contains("allows merge retry"));
        assert!(error.contains("Merged"));
    }

    #[tokio::test]
    async fn retry_merge_reroutes_commit_hook_failures_to_reexecution() {
        let app_state = AppState::new_test();
        let execution_state = Arc::new(ExecutionState::new());
        execution_state.set_max_concurrent(10);

        let project = Project::new("Test Project".to_string(), "/test/path".to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .unwrap();

        let mut task = Task::new(project.id.clone(), "Commit Hook Retry".to_string());
        task.internal_status = InternalStatus::MergeIncomplete;
        task.metadata = Some(
            serde_json::json!({
                "error": "Git operation error: Failed to commit rebase+squash in worktree: stderr=[pre-commit] design-token guards failed"
            })
            .to_string(),
        );
        let task_id = task.id.clone();
        app_state.task_repo.create(task).await.unwrap();

        retry_merge_inner(task_id.clone(), None, &app_state, Arc::clone(&execution_state))
            .await
            .expect("retry_merge should reroute hook failures");

        let updated = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.internal_status,
            InternalStatus::ReExecuting,
            "manual retry should reroute commit-hook merge failures into re-execution"
        );

        let meta: serde_json::Value =
            serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
        assert_eq!(
            meta.get("merge_hook_reexecution_requested"),
            Some(&serde_json::json!(true))
        );
        assert!(
            meta.get("merge_retry_in_progress").is_none(),
            "manual reroute must not arm merge retry guard"
        );

        let review_notes = app_state
            .review_repo
            .get_notes_by_task_id(&task_id)
            .await
            .expect("review notes query should succeed");
        assert_eq!(review_notes.len(), 1);
        assert_eq!(
            review_notes[0].reviewer,
            crate::domain::entities::ReviewerType::System
        );
    }
}

/// Input for changing project git mode
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeGitModeInput {
    pub git_mode: String,
    pub worktree_parent_directory: Option<String>,
}

/// Change project git mode
///
/// Validates and updates git mode settings.
/// In-progress tasks continue in their current mode.
#[tauri::command]
pub async fn change_project_git_mode(
    project_id: String,
    input: ChangeGitModeInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let project_id = ProjectId::from_string(project_id);

    // Get project
    let mut project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", project_id.as_str()))?;

    // A7: Block git mode change when tasks are actively running
    let active_tasks = state
        .task_repo
        .get_by_project(&project_id)
        .await
        .map_err(|e| e.to_string())?;
    let in_flight: Vec<_> = active_tasks
        .iter()
        .filter(|t| AGENT_ACTIVE_STATUSES.contains(&t.internal_status))
        .collect();
    if !in_flight.is_empty() {
        let task_ids: Vec<_> = in_flight
            .iter()
            .map(|t| t.id.as_str().to_string())
            .collect();
        return Err(format!(
            "Cannot change git mode while {} task(s) are in active states: {}",
            in_flight.len(),
            task_ids.join(", ")
        ));
    }

    // Parse git mode
    let new_mode: GitMode = input.git_mode.parse().map_err(|_| {
        format!(
            "Invalid git mode: {}. Valid values: 'worktree'",
            input.git_mode
        )
    })?;

    // Update project
    project.git_mode = new_mode;
    if let Some(worktree_parent) = input.worktree_parent_directory {
        project.worktree_parent_directory = Some(worktree_parent);
    }

    // Ensure defaults for worktree mode
    if project.base_branch.as_deref().unwrap_or("").is_empty() {
        project.base_branch = Some("main".to_string());
    }
    if project
        .worktree_parent_directory
        .as_deref()
        .unwrap_or("")
        .is_empty()
    {
        project.worktree_parent_directory = Some("~/ralphx-worktrees".to_string());
    }
    project.touch();

    state
        .project_repo
        .update(&project)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ============================================================================
// Helper functions
// ============================================================================

/// Create a TaskTransitionService with required dependencies
fn create_transition_service(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> TaskTransitionService<tauri::Wry> {
    // Create scheduler for post-merge scheduling (unblocked plan_merge tasks)
    let scheduler_concrete = Arc::new(
        state.build_task_scheduler_for_runtime(
            Arc::clone(execution_state),
            state.app_handle.clone(),
        ),
    );
    scheduler_concrete.set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
    let task_scheduler: Arc<dyn TaskScheduler> = scheduler_concrete;

    let mut svc = state
        .build_transition_service_with_execution_state(Arc::clone(execution_state))
        .with_task_scheduler(task_scheduler)
        .with_pr_poller_registry(Arc::clone(&state.pr_poller_registry));
    if let Some(ref github_svc) = state.github_service {
        svc = svc.with_github_service(Arc::clone(github_svc));
    }
    svc
}

/// Clean up git resources (branch/worktree) for a task
async fn cleanup_task_git_resources(
    task: &crate::domain::entities::Task,
    project: &crate::domain::entities::Project,
) -> Result<(), String> {
    use tracing::warn;

    let repo_path = PathBuf::from(&project.working_directory);
    let base_branch = project.base_branch.as_deref().unwrap_or("main");

    // Get task branch
    let task_branch = match &task.task_branch {
        Some(branch) => branch.clone(),
        None => return Ok(()), // Nothing to clean up
    };

    // Delete worktree first if it exists
    if let Some(worktree_path) = &task.worktree_path {
        let worktree_path_buf = PathBuf::from(worktree_path);
        if let Err(e) = GitService::delete_worktree(&repo_path, &worktree_path_buf).await {
            warn!(
                "Failed to delete worktree {}: {} (non-fatal)",
                worktree_path, e
            );
        }
    }

    // Only checkout base branch if the task branch is currently checked out in main repo.
    // In Worktree mode the task branch lives in a worktree, not the main checkout,
    // so this is normally a no-op. Guards against edge cases from old Local mode.
    let current_branch = GitService::get_current_branch(&repo_path)
        .await
        .unwrap_or_default();
    if current_branch == task_branch {
        if let Err(e) = GitService::checkout_branch(&repo_path, base_branch).await {
            warn!(
                "Failed to checkout base branch {} after merge: {} (non-fatal)",
                base_branch, e
            );
        }
    }

    // Delete task branch
    if let Err(e) = GitService::delete_branch(&repo_path, &task_branch, true).await {
        warn!("Failed to delete branch {}: {} (non-fatal)", task_branch, e);
    }

    Ok(())
}
