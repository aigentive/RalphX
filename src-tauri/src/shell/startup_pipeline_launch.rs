use std::sync::Arc;

use crate::shell::startup_pipeline::{StartupPipelineDeps, StartupPipelineMode};
use crate::application::startup_status::{StartupCoordinator, StartupStage};
use crate::commands::{ActiveProjectState, ExecutionState};
use crate::AppState;
use tauri::State;

fn build_startup_pipeline_deps(
    app_state: &AppState,
    startup_execution_state: Arc<ExecutionState>,
    startup_active_project_state: Arc<ActiveProjectState>,
    startup_app_handle: tauri::AppHandle,
    mode: StartupPipelineMode,
    startup_boundary: Option<(Arc<StartupCoordinator>, u64)>,
    pr_fix_review_publish_resumer: Option<
        Arc<
            dyn crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
        >,
    >,
) -> StartupPipelineDeps {
    // Spawn startup job runner to resume tasks in agent-active states
    // Clone references needed for the async task
    let startup_task_repo = Arc::clone(&app_state.task_repo);
    let startup_project_repo = Arc::clone(&app_state.project_repo);
    let startup_task_dependency_repo = Arc::clone(&app_state.task_dependency_repo);
    let startup_execution_plan_repo = Arc::clone(&app_state.execution_plan_repo);
    let startup_plan_branch_repo = Arc::clone(&app_state.plan_branch_repo);
    let startup_step_repo = Arc::clone(&app_state.task_step_repo);
    let startup_chat_message_repo = Arc::clone(&app_state.chat_message_repo);
    let startup_chat_attachment_repo = Arc::clone(&app_state.chat_attachment_repo);
    let startup_artifact_repo = Arc::clone(&app_state.artifact_repo);
    let startup_conversation_repo = Arc::clone(&app_state.chat_conversation_repo);
    let startup_agent_conversation_workspace_repo =
        Arc::clone(&app_state.agent_conversation_workspace_repo);
    let startup_agent_conversation_jira_issue_repo =
        Arc::clone(&app_state.agent_conversation_jira_issue_repo);
    let startup_agent_conversation_linear_issue_repo =
        Arc::clone(&app_state.agent_conversation_linear_issue_repo);
    let startup_agent_conversation_granola_note_repo =
        Arc::clone(&app_state.agent_conversation_granola_note_repo);
    let startup_orphan_worktree_cleanup_marker_repo =
        Arc::clone(&app_state.orphan_worktree_cleanup_marker_repo);
    let startup_automation_repo = Arc::clone(&app_state.automation_repo);
    let startup_automation_run_repo = Arc::clone(&app_state.automation_run_repo);
    let startup_agent_run_repo = Arc::clone(&app_state.agent_run_repo);
    let startup_ideation_session_repo = Arc::clone(&app_state.ideation_session_repo);
    let startup_activity_event_repo = Arc::clone(&app_state.activity_event_repo);
    let startup_message_queue = Arc::clone(&app_state.message_queue);
    let startup_running_agent_registry = Arc::clone(&app_state.running_agent_registry);
    let startup_memory_event_repo = Arc::clone(&app_state.memory_event_repo);
    let startup_app_state_repo = Arc::clone(&app_state.app_state_repo);
    let startup_memory_archive_repo = Arc::clone(&app_state.memory_archive_repo);
    let startup_memory_entry_repo = Arc::clone(&app_state.memory_entry_repo);
    let startup_execution_settings_repo = Arc::clone(&app_state.execution_settings_repo);
    let startup_agent_lane_settings_repo = Arc::clone(&app_state.agent_lane_settings_repo);
    let startup_agent_provider_settings_repo = Arc::clone(&app_state.agent_provider_settings_repo);
    let startup_ideation_effort_settings_repo =
        Arc::clone(&app_state.ideation_effort_settings_repo);
    let startup_ideation_model_settings_repo = Arc::clone(&app_state.ideation_model_settings_repo);
    let startup_interactive_process_registry = Arc::clone(&app_state.interactive_process_registry);
    let startup_review_repo = Arc::clone(&app_state.review_repo);
    let startup_external_events_repo = Arc::clone(&app_state.external_events_repo);
    let startup_github_service = app_state.github_service.as_ref().map(Arc::clone);
    let startup_pr_poller_registry = Arc::clone(&app_state.pr_poller_registry);
    let startup_agent_clients = app_state.agent_client_bundle();
    let startup_webhook_publisher = app_state.webhook_publisher.clone();
    let startup_session_merge_locks = Arc::clone(&app_state.session_merge_locks);
    let startup_git_auth_recovery_state = Arc::clone(&app_state.startup_git_auth_recovery_state);

    let (startup_coordinator, startup_attempt_id) = startup_boundary
        .map(|(coordinator, attempt_id)| (Some(coordinator), Some(attempt_id)))
        .unwrap_or((None, None));

    StartupPipelineDeps {
        app_state: app_state.clone(),
        execution_state: Arc::clone(&startup_execution_state),
        active_project_state: Arc::clone(&startup_active_project_state),
        task_repo: startup_task_repo,
        project_repo: startup_project_repo,
        task_dependency_repo: startup_task_dependency_repo,
        execution_plan_repo: startup_execution_plan_repo,
        plan_branch_repo: startup_plan_branch_repo,
        step_repo: startup_step_repo,
        chat_message_repo: startup_chat_message_repo,
        chat_attachment_repo: startup_chat_attachment_repo,
        artifact_repo: startup_artifact_repo,
        conversation_repo: startup_conversation_repo,
        agent_conversation_workspace_repo: startup_agent_conversation_workspace_repo,
        agent_conversation_jira_issue_repo: startup_agent_conversation_jira_issue_repo,
        agent_conversation_linear_issue_repo: startup_agent_conversation_linear_issue_repo,
        agent_conversation_granola_note_repo: startup_agent_conversation_granola_note_repo,
        orphan_worktree_cleanup_marker_repo: startup_orphan_worktree_cleanup_marker_repo,
        automation_repo: startup_automation_repo,
        automation_run_repo: startup_automation_run_repo,
        agent_run_repo: startup_agent_run_repo,
        ideation_session_repo: startup_ideation_session_repo,
        activity_event_repo: startup_activity_event_repo,
        message_queue: startup_message_queue,
        running_agent_registry: startup_running_agent_registry,
        memory_event_repo: startup_memory_event_repo,
        app_state_repo: startup_app_state_repo,
        memory_archive_repo: startup_memory_archive_repo,
        memory_entry_repo: startup_memory_entry_repo,
        execution_settings_repo: startup_execution_settings_repo,
        agent_lane_settings_repo: startup_agent_lane_settings_repo,
        agent_provider_settings_repo: startup_agent_provider_settings_repo,
        ideation_effort_settings_repo: startup_ideation_effort_settings_repo,
        ideation_model_settings_repo: startup_ideation_model_settings_repo,
        interactive_process_registry: startup_interactive_process_registry,
        review_repo: startup_review_repo,
        external_events_repo: startup_external_events_repo,
        github_service: startup_github_service,
        pr_poller_registry: startup_pr_poller_registry,
        agent_clients: startup_agent_clients,
        webhook_publisher: startup_webhook_publisher,
        session_merge_locks: startup_session_merge_locks,
        app_handle: startup_app_handle,
        git_auth_recovery_state: startup_git_auth_recovery_state,
        mode,
        startup_coordinator,
        startup_attempt_id,
        pr_fix_review_publish_resumer,
    }
}

/// Starts existing recovery only after dynamic AppState registration and a
/// listener-backed runtime-ready handshake.
pub(crate) fn launch_startup_pipeline_from_handle(
    startup_app_handle: tauri::AppHandle,
    app_state: &AppState,
    startup_execution_state: Arc<ExecutionState>,
    startup_active_project_state: Arc<ActiveProjectState>,
    pr_fix_review_publish_resumer: Option<
        Arc<
            dyn crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
        >,
    >,
    startup_coordinator: Arc<StartupCoordinator>,
    attempt_id: u64,
) {
    let deps = build_startup_pipeline_deps(
        app_state,
        startup_execution_state,
        startup_active_project_state,
        startup_app_handle,
        StartupPipelineMode::Full,
        Some((Arc::clone(&startup_coordinator), attempt_id)),
        pr_fix_review_publish_resumer,
    );
    tauri::async_runtime::spawn(async move {
        match crate::shell::startup_pipeline::run_startup_pipeline(deps).await {
            Ok(()) => {
                let _ = startup_coordinator.advance(attempt_id, StartupStage::Ready);
            }
            Err(error) => {
                tracing::error!(error = %error, "Startup recovery pipeline failed");
                if startup_coordinator.snapshot().runtime_ready {
                    let _ = startup_coordinator.advance(attempt_id, StartupStage::Degraded);
                } else {
                    startup_coordinator.fail(
                        attempt_id,
                        crate::application::startup_status::StartupFailureCode::SafetyRecovery,
                        "RalphX could not safely restore interrupted work.",
                    );
                }
            }
        }
    });
}

pub(crate) async fn resume_deferred_git_startup_pipeline(
    app_state: &AppState,
    startup_execution_state: Arc<ExecutionState>,
    startup_active_project_state: Arc<ActiveProjectState>,
    startup_app_handle: tauri::AppHandle,
    pr_fix_review_publish_resumer: Option<
        Arc<
            dyn crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
        >,
    >,
) -> Result<bool, String> {
    let recovery_state = Arc::clone(&app_state.startup_git_auth_recovery_state);
    if !recovery_state.try_begin_resume() {
        return Ok(false);
    }

    let result = async {
        let deps = build_startup_pipeline_deps(
            app_state,
            startup_execution_state,
            startup_active_project_state,
            startup_app_handle,
            StartupPipelineMode::DeferredGitResume,
            None,
            pr_fix_review_publish_resumer,
        );
        crate::shell::startup_pipeline::run_startup_pipeline(deps)
            .await
            .map_err(|error| error.to_string())?;
        Ok(!recovery_state.is_pending())
    }
    .await;

    recovery_state.finish_resume();
    result
}

/// Resume Git/GitHub-dependent startup work that was deferred by startup preflight.
///
/// Hosted in the shell rather than `commands::project_commands` because it
/// drives the startup pipeline directly; keeping it in `commands` would
/// require an upward `crate::shell` import.
#[tauri::command]
pub async fn resume_deferred_git_startup(
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let pr_fix_review_publish_resumer = Arc::new(
        crate::commands::unified_chat_commands::AgentWorkspacePrFixReviewPublishCommandResumer {
            app_state: state.inner().clone(),
            execution_state: Arc::clone(&execution_state),
        },
    )
        as Arc<
            dyn crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
        >;
    resume_deferred_git_startup_pipeline(
        &state,
        Arc::clone(&execution_state),
        Arc::clone(&active_project_state),
        app,
        Some(pr_fix_review_publish_resumer),
    )
    .await
}
