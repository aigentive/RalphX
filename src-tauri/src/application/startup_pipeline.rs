use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::application::agent_workspace_bridge::AgentWorkspaceBridgeDeps;
use crate::application::agent_workspace_publish_recovery::recover_stale_agent_workspace_publish_repairs_on_startup_for_state;
use crate::application::git_service::git_cmd::{self, GitCommandLane};
use crate::application::runtime_factory::{ChatRuntimeFactoryDeps, RuntimeFactoryDeps};
use crate::application::startup_git_auth_preflight::StartupGitAuthRecoveryState;
use crate::application::startup_runtime_builders::{
    build_startup_chat_resumption_runner, build_startup_reconciliation_runner,
    build_startup_recovery_chat_service, build_startup_task_scheduler, StartupChatResumptionDeps,
    StartupReconciliationDeps, StartupSchedulerDeps,
};
use crate::application::startup_transition_factory::StartupTransitionFactory;
use crate::application::{
    startup_background, startup_jobs, AgentClientBundle, AppState, InteractiveProcessRegistry,
    StartupJobRunner,
};
use crate::commands::{ActiveProjectState, ExecutionState};
use crate::domain::repositories::{
    ActivityEventRepository, AgentConversationGranolaNoteRepository,
    AgentConversationJiraIssueRepository, AgentConversationLinearIssueRepository,
    AgentConversationWorkspaceRepository, AgentLaneSettingsRepository,
    AgentProviderSettingsRepository, AgentRunRepository, AppStateRepository, ArtifactRepository,
    AutomationRepository, AutomationRunRepository, ChatAttachmentRepository,
    ChatConversationRepository, ChatMessageRepository, ExecutionPlanRepository,
    ExecutionSettingsRepository, ExternalEventsRepository, IdeationEffortSettingsRepository,
    IdeationModelSettingsRepository, IdeationSessionRepository, MemoryArchiveRepository,
    MemoryEntryRepository, MemoryEventRepository, OrphanWorktreeCleanupMarkerRepository,
    PlanBranchRepository, ProjectRepository, ReviewRepository, TaskDependencyRepository,
    TaskRepository, TaskStepRepository,
};
use crate::domain::services::{
    running_agent_registry::kill_orphaned_mcp_servers, MessageQueue, RunningAgentRegistry,
};
use crate::domain::state_machine::services::WebhookPublisher;
use crate::error::AppError;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupPipelineMode {
    Full,
    DeferredGitResume,
}

const STARTUP_BACKGROUND_DB_GRACE: Duration = Duration::from_millis(750);

pub(crate) struct StartupPipelineDeps {
    pub app_state: AppState,
    pub execution_state: Arc<ExecutionState>,
    pub active_project_state: Arc<ActiveProjectState>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    pub execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pub step_repo: Arc<dyn TaskStepRepository>,
    pub chat_message_repo: Arc<dyn ChatMessageRepository>,
    pub chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    pub artifact_repo: Arc<dyn ArtifactRepository>,
    pub conversation_repo: Arc<dyn ChatConversationRepository>,
    pub agent_conversation_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    pub agent_conversation_jira_issue_repo: Arc<dyn AgentConversationJiraIssueRepository>,
    pub agent_conversation_linear_issue_repo: Arc<dyn AgentConversationLinearIssueRepository>,
    pub agent_conversation_granola_note_repo: Arc<dyn AgentConversationGranolaNoteRepository>,
    pub orphan_worktree_cleanup_marker_repo: Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    pub automation_repo: Arc<dyn AutomationRepository>,
    pub automation_run_repo: Arc<dyn AutomationRunRepository>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    pub activity_event_repo: Arc<dyn ActivityEventRepository>,
    pub message_queue: Arc<MessageQueue>,
    pub running_agent_registry: Arc<dyn RunningAgentRegistry>,
    pub memory_event_repo: Arc<dyn MemoryEventRepository>,
    pub app_state_repo: Arc<dyn AppStateRepository>,
    pub memory_archive_repo: Arc<dyn MemoryArchiveRepository>,
    pub memory_entry_repo: Arc<dyn MemoryEntryRepository>,
    pub execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
    pub agent_lane_settings_repo: Arc<dyn AgentLaneSettingsRepository>,
    pub agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
    pub ideation_effort_settings_repo: Arc<dyn IdeationEffortSettingsRepository>,
    pub ideation_model_settings_repo: Arc<dyn IdeationModelSettingsRepository>,
    pub interactive_process_registry: Arc<InteractiveProcessRegistry>,
    pub review_repo: Arc<dyn ReviewRepository>,
    pub external_events_repo: Arc<dyn ExternalEventsRepository>,
    pub github_service: Option<Arc<dyn crate::domain::services::GithubServiceTrait>>,
    pub pr_poller_registry: Arc<crate::application::PrPollerRegistry>,
    pub agent_clients: AgentClientBundle,
    pub webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    pub session_merge_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub app_handle: tauri::AppHandle,
    pub git_auth_recovery_state: Arc<StartupGitAuthRecoveryState>,
    pub mode: StartupPipelineMode,
    pub startup_coordinator: Option<Arc<crate::application::startup_status::StartupCoordinator>>,
    pub startup_attempt_id: Option<u64>,
    pub pr_fix_review_publish_resumer: Option<
        Arc<
            dyn crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
        >,
    >,
}

pub(crate) fn publish_runtime_boundary_for(
    coordinator: Option<&Arc<crate::application::startup_status::StartupCoordinator>>,
    attempt_id: Option<u64>,
) -> AppResult<()> {
    let (Some(coordinator), Some(attempt_id)) = (coordinator, attempt_id) else {
        return Ok(());
    };
    coordinator
        .complete_safety_barrier(attempt_id)
        .and_then(|()| coordinator.publish_runtime_ready(attempt_id))
        .and_then(|()| {
            coordinator.advance(
                attempt_id,
                crate::application::startup_status::StartupStage::BackgroundRecovery,
            )
        })
        .map_err(|error| AppError::Infrastructure(error.to_string()))
}

fn publish_runtime_boundary(deps: &StartupPipelineDeps) -> AppResult<()> {
    publish_runtime_boundary_for(deps.startup_coordinator.as_ref(), deps.startup_attempt_id)
}

pub(crate) fn startup_previous_session_cutoff() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

pub(crate) fn run_startup_orphan_mcp_cleanup(kill_orphans: impl FnOnce() -> u32) -> u32 {
    let mcp_killed = kill_orphans();
    if mcp_killed > 0 {
        info!(count = mcp_killed, "Killed orphaned MCP server processes");
    }
    mcp_killed
}

pub(crate) fn startup_phase_started(phase: &'static str) -> Instant {
    tracing::info!(phase, "Startup phase starting");
    Instant::now()
}

pub(crate) fn startup_phase_completed(phase: &'static str, started_at: Instant) {
    tracing::info!(
        phase,
        elapsed_ms = started_at.elapsed().as_millis(),
        "Startup phase completed"
    );
}

pub(crate) async fn run_startup_owned_phase<F>(phase: &'static str, delay: Duration, future: F)
where
    F: Future<Output = ()>,
{
    tracing::info!(
        phase,
        delay_ms = delay.as_millis(),
        "Startup background phase scheduled"
    );
    if delay > Duration::ZERO {
        tokio::time::sleep(delay).await;
    }
    let phase_started_at = startup_phase_started(phase);
    future.await;
    startup_phase_completed(phase, phase_started_at);
}

async fn run_external_mcp_startup(mode: StartupPipelineMode, app_handle: tauri::AppHandle) {
    if mode != StartupPipelineMode::Full {
        return;
    }
    startup_background::maybe_start_external_mcp(app_handle, |port, timeout| {
        Box::pin(crate::wait_for_backend_ready(port, timeout))
    })
    .await;
}

pub(crate) async fn run_startup_pipeline(deps: StartupPipelineDeps) -> AppResult<()> {
    let previous_session_cutoff = startup_previous_session_cutoff();

    let phase_started_at = startup_phase_started("legacy_claude_mcp_reconciliation");
    if deps
        .app_state
        .mcp_policy_service()
        .reconcile_reserved_claude_registration_best_effort()
        .await
        .is_err()
    {
        warn!(
            failure_code = "legacy_mcp_reconciliation_failed",
            "Legacy Claude MCP reconciliation did not complete; launch preflight remains authoritative"
        );
    }
    startup_phase_completed("legacy_claude_mcp_reconciliation", phase_started_at);

    // The host-owned driver for spawn-free remote conversation starts is a LIVE operational
    // driver, not recovery work: it polls a durable intent table and starts chat-mode
    // conversations on the host. It must run whenever the host is (or becomes) reachable, so it is
    // spawned here — in the always-run prefix, BEFORE the recovery-disabled early return below —
    // not in the recovery pipeline. Otherwise a host with `RALPHX_DISABLE_STARTUP_RECOVERY` set
    // (routine in dev) leaves every remote start intent stuck `pending` and the conversation blank.
    // `try_start_recurring_service` inside the spawn dedupes if the recovery path ever reaches it.
    {
        let phase_started_at = startup_phase_started("remote_conversation_start_dispatcher_spawn");
        startup_background::spawn_remote_conversation_start_dispatcher(
            deps.app_state.clone(),
            Arc::clone(&deps.execution_state),
            deps.app_handle.clone(),
        );
        startup_phase_completed(
            "remote_conversation_start_dispatcher_spawn",
            phase_started_at,
        );
    }

    // Same reasoning as the start dispatcher above, and the same placement for the same reason:
    // the continuation driver is a LIVE operational loop, not recovery work. A host running with
    // `RALPHX_DISABLE_STARTUP_RECOVERY` (routine in dev) would otherwise leave every remote
    // follow-up message stuck `pending`, which is exactly the ghost-message hazard WP1 exists to
    // remove.
    {
        let phase_started_at = startup_phase_started("remote_conversation_message_dispatcher_spawn");
        startup_background::spawn_remote_conversation_message_dispatcher(
            deps.app_state.clone(),
            Arc::clone(&deps.execution_state),
            deps.app_handle.clone(),
        );
        startup_phase_completed(
            "remote_conversation_message_dispatcher_spawn",
            phase_started_at,
        );
    }

    if startup_jobs::is_startup_recovery_disabled() {
        info!(
            env_var = startup_jobs::RALPHX_DISABLE_STARTUP_RECOVERY_ENV,
            "Startup recovery disabled via environment; skipping startup recovery pipeline"
        );
        publish_runtime_boundary(&deps)?;
        run_external_mcp_startup(deps.mode, deps.app_handle.clone()).await;
        return Ok(());
    }

    // Pattern-based MCP cleanup cannot reliably distinguish current-boot agent
    // servers after user actions begin, so run it before delayed PR/workspace recovery.
    let phase_started_at = startup_phase_started("orphan_mcp_cleanup");
    tokio::task::spawn_blocking(|| run_startup_orphan_mcp_cleanup(kill_orphaned_mcp_servers))
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("Startup orphan MCP cleanup worker failed: {error}"))
        })?;
    startup_phase_completed("orphan_mcp_cleanup", phase_started_at);

    let phase_started_at = startup_phase_started("startup_recovery_initial_delay");
    tokio::time::sleep(Duration::from_millis(500)).await;
    startup_phase_completed("startup_recovery_initial_delay", phase_started_at);

    info!("Starting startup job runner...");

    let StartupPipelineDeps {
        execution_state,
        active_project_state,
        task_repo,
        project_repo,
        task_dependency_repo,
        execution_plan_repo,
        plan_branch_repo,
        step_repo,
        chat_message_repo,
        chat_attachment_repo,
        artifact_repo,
        conversation_repo,
        agent_conversation_workspace_repo,
        agent_conversation_jira_issue_repo,
        agent_conversation_linear_issue_repo,
        agent_conversation_granola_note_repo,
        orphan_worktree_cleanup_marker_repo,
        automation_repo: _automation_repo,
        automation_run_repo,
        agent_run_repo,
        ideation_session_repo,
        activity_event_repo,
        message_queue,
        running_agent_registry,
        memory_event_repo,
        app_state_repo,
        memory_archive_repo,
        memory_entry_repo,
        execution_settings_repo,
        agent_lane_settings_repo,
        agent_provider_settings_repo,
        ideation_effort_settings_repo,
        ideation_model_settings_repo,
        interactive_process_registry,
        review_repo,
        external_events_repo,
        github_service,
        pr_poller_registry,
        agent_clients,
        webhook_publisher,
        session_merge_locks,
        app_handle,
        git_auth_recovery_state,
        mode,
        startup_coordinator,
        startup_attempt_id,
        app_state,
        pr_fix_review_publish_resumer,
    } = deps;

    let phase_started_at = startup_phase_started("deferred_plan_approval_reconciliation");
    if let Err(error) = crate::application::plan_approval_notification_service::reconcile_deferred_plan_approvals_on_startup(&app_state).await {
        tracing::warn!(error = %error, "Deferred plan approval reconciliation did not complete");
    }
    startup_phase_completed("deferred_plan_approval_reconciliation", phase_started_at);

    // Authority cleanup must precede the Tasks drain: a crash-persisted mutation claim
    // deliberately prevents branch-operation pause until its process and Git state are proven safe.
    let phase_started_at = startup_phase_started("git_mutation_authority_recovery");
    match crate::application::git_mutation_recovery::recover_in_flight_git_mutations_for_state(
        &app_state,
    )
    .await
    {
        Ok(outcomes) => {
            let repair_count = outcomes
                .iter()
                .filter(|outcome| {
                    matches!(
                        outcome,
                        crate::application::git_mutation_recovery::GitMutationRecoveryOutcome::NeedsRepair { .. }
                    )
                })
                .count();
            tracing::info!(
                recovered_claims = outcomes.len().saturating_sub(repair_count),
                fenced_for_repair = repair_count,
                "Startup Git mutation authority recovery completed"
            );
        }
        Err(error) => {
            tracing::error!(
                error = %error,
                "Startup Git mutation recovery failed; durable claims remain fenced"
            );
        }
    }
    startup_phase_completed("git_mutation_authority_recovery", phase_started_at);

    let tasks_settings = app_state
        .ideation_settings_repo
        .get_settings()
        .await
        .map_err(|error| AppError::Database(error.to_string()))?;
    if tasks_settings.tasks_feature_state != crate::domain::ideation::TasksFeatureState::Enabled {
        app_state
            .build_tasks_feature_toggle_service(
                Arc::clone(&execution_state),
                Some(app_handle.clone()),
            )
            .set_tasks_enabled(false)
            .await?;
    }

    let phase_started_at = startup_phase_started("branch_update_run_binding_recovery");
    match crate::application::git_mutation_recovery::recover_terminal_branch_update_run_bindings(
        Arc::clone(&app_state.branch_update_repo),
        Arc::clone(&agent_run_repo),
    )
    .await
    {
        Ok(recovered) => tracing::info!(
            recovered,
            "Startup terminal branch-update run binding recovery completed"
        ),
        Err(error) => tracing::error!(
            error = %error,
            "Startup branch-update run binding recovery failed closed"
        ),
    }
    startup_phase_completed("branch_update_run_binding_recovery", phase_started_at);

    let phase_started_at = startup_phase_started("git_auth_preflight");
    let startup_git_preflight =
        crate::application::startup_git_auth_preflight::run_startup_git_auth_preflight_with_notifications(
            Arc::clone(&project_repo),
            Arc::clone(&app_state_repo),
            Some(Arc::clone(&plan_branch_repo)),
            Some(Arc::clone(&agent_conversation_workspace_repo)),
            &app_handle,
            Some(app_state.notification_service()),
        )
        .await;
    startup_phase_completed("git_auth_preflight", phase_started_at);
    let git_startup_preflight_failed = startup_git_preflight.failure_code.is_some();
    let active_git_startup_blocked = startup_git_preflight.active_project_blocked();
    let has_git_startup_blocked_projects = startup_git_preflight.has_blocked_projects();
    let blocked_git_project_ids = Arc::new(startup_git_preflight.blocked_project_ids());
    if has_git_startup_blocked_projects {
        git_auth_recovery_state.mark_pending();
    } else if mode == StartupPipelineMode::Full {
        git_auth_recovery_state.clear_pending();
    }
    if active_git_startup_blocked {
        tracing::warn!(
            "Startup Git auth preflight blocked active-project Git/GitHub recovery until user repair"
        );
        if mode == StartupPipelineMode::DeferredGitResume {
            return Ok(());
        }
    }

    let phase_started_at = startup_phase_started("task_scheduler_build");
    let task_scheduler = build_startup_task_scheduler(StartupSchedulerDeps {
        execution_state: Arc::clone(&execution_state),
        project_repo: Arc::clone(&project_repo),
        task_repo: Arc::clone(&task_repo),
        task_dependency_repo: Arc::clone(&task_dependency_repo),
        artifact_repo: Arc::clone(&artifact_repo),
        execution_plan_repo: Arc::clone(&execution_plan_repo),
        chat_message_repo: Arc::clone(&chat_message_repo),
        chat_attachment_repo: Arc::clone(&chat_attachment_repo),
        conversation_repo: Arc::clone(&conversation_repo),
        agent_run_repo: Arc::clone(&agent_run_repo),
        ideation_session_repo: Arc::clone(&ideation_session_repo),
        activity_event_repo: Arc::clone(&activity_event_repo),
        message_queue: Arc::clone(&message_queue),
        running_agent_registry: Arc::clone(&running_agent_registry),
        memory_event_repo: Arc::clone(&memory_event_repo),
        agent_clients: agent_clients.clone(),
        agent_provider_settings_repo: Arc::clone(&agent_provider_settings_repo),
        plan_branch_repo: Arc::clone(&plan_branch_repo),
        github_service: github_service.as_ref().map(Arc::clone),
        pr_poller_registry: Arc::clone(&pr_poller_registry),
        interactive_process_registry: Arc::clone(&interactive_process_registry),
        _app_handle: app_handle.clone(),
    });
    startup_phase_completed("task_scheduler_build", phase_started_at);

    let phase_started_at = startup_phase_started("startup_transition_factory_build");
    let startup_transition_factory = StartupTransitionFactory {
        execution_state: Arc::clone(&execution_state),
        execution_settings_repo: Arc::clone(&execution_settings_repo),
        agent_lane_settings_repo: Arc::clone(&agent_lane_settings_repo),
        agent_provider_settings_repo: Arc::clone(&agent_provider_settings_repo),
        plan_branch_repo: Arc::clone(&plan_branch_repo),
        interactive_process_registry: Arc::clone(&interactive_process_registry),
        agent_clients: agent_clients.clone(),
        task_scheduler: Arc::clone(&task_scheduler),
        step_repo: Arc::clone(&step_repo),
        external_events_repo: Arc::clone(&external_events_repo),
        webhook_publisher: webhook_publisher.clone(),
        session_merge_locks: Arc::clone(&session_merge_locks),
        notification_service: app_state.notification_service(),
    };
    startup_phase_completed("startup_transition_factory_build", phase_started_at);

    let phase_started_at = startup_phase_started("core_runtime_deps_build");
    let core_runtime_deps = RuntimeFactoryDeps::from_core(
        Arc::clone(&task_repo),
        Arc::clone(&task_dependency_repo),
        Arc::clone(&project_repo),
        Arc::clone(&artifact_repo),
        Arc::clone(&chat_message_repo),
        Arc::clone(&chat_attachment_repo),
        Arc::clone(&conversation_repo),
        Arc::clone(&agent_run_repo),
        Arc::clone(&ideation_session_repo),
        Arc::clone(&activity_event_repo),
        Arc::clone(&message_queue),
        Arc::clone(&running_agent_registry),
        Arc::clone(&memory_event_repo),
    )
    .with_github_runtime_support(
        github_service.as_ref().map(Arc::clone),
        Some(Arc::clone(&pr_poller_registry)),
    );
    startup_phase_completed("core_runtime_deps_build", phase_started_at);

    let phase_started_at = startup_phase_started("transition_service_build");
    let transition_service = startup_transition_factory
        .build(core_runtime_deps, app_handle.clone())
        .into_arc();
    startup_phase_completed("transition_service_build", phase_started_at);

    let phase_started_at = startup_phase_started("recovery_chat_service_deps_build");
    let recovery_chat_service_deps = ChatRuntimeFactoryDeps::from_core(
        Arc::clone(&chat_message_repo),
        Arc::clone(&chat_attachment_repo),
        Arc::clone(&artifact_repo),
        Arc::clone(&conversation_repo),
        Arc::clone(&agent_run_repo),
        Arc::clone(&automation_run_repo),
        Arc::clone(&project_repo),
        Arc::clone(&task_repo),
        Arc::clone(&task_dependency_repo),
        Arc::clone(&ideation_session_repo),
        Arc::clone(&activity_event_repo),
        Arc::clone(&message_queue),
        Arc::clone(&running_agent_registry),
        Arc::clone(&memory_event_repo),
    )
    .with_agent_conversation_workspace_repo(Some(Arc::clone(&agent_conversation_workspace_repo)))
    .with_agent_conversation_jira_issue_repo(Some(Arc::clone(&agent_conversation_jira_issue_repo)))
    .with_agent_conversation_linear_issue_repo(Some(Arc::clone(
        &agent_conversation_linear_issue_repo,
    )))
    .with_agent_conversation_granola_note_repo(Some(Arc::clone(
        &agent_conversation_granola_note_repo,
    )))
    .with_runtime_support(
        Some(Arc::clone(&execution_settings_repo)),
        Some(Arc::clone(&agent_lane_settings_repo)),
        Some(Arc::clone(&agent_provider_settings_repo)),
        None,
        Some(Arc::clone(&interactive_process_registry)),
    )
    .with_ideation_runtime_support(
        Some(Arc::clone(&ideation_effort_settings_repo)),
        Some(Arc::clone(&ideation_model_settings_repo)),
    )
    .with_integration_reference_services_from_app_state(&app_state);
    startup_phase_completed("recovery_chat_service_deps_build", phase_started_at);

    let phase_started_at = startup_phase_started("recovery_chat_service_build");
    let recovery_chat_service = build_startup_recovery_chat_service(
        app_handle.clone(),
        Arc::clone(&execution_state),
        recovery_chat_service_deps.clone(),
    );
    startup_phase_completed("recovery_chat_service_build", phase_started_at);

    let runner = Arc::new(
        StartupJobRunner::new(
            Arc::clone(&task_repo),
            Arc::clone(&task_dependency_repo),
            Arc::clone(&project_repo),
            Arc::clone(&artifact_repo),
            Arc::clone(&conversation_repo),
            Arc::clone(&chat_message_repo),
            Arc::clone(&chat_attachment_repo),
            Arc::clone(&ideation_session_repo),
            Arc::clone(&activity_event_repo),
            Arc::clone(&message_queue),
            Arc::clone(&running_agent_registry),
            Arc::clone(&memory_event_repo),
            Arc::clone(&agent_run_repo),
            Arc::clone(&transition_service),
            Arc::clone(&execution_state),
            Arc::clone(&active_project_state),
            Arc::clone(&app_state_repo),
            Arc::clone(&execution_settings_repo),
            Some(Arc::clone(&plan_branch_repo)),
        )
        .with_task_scheduler(Arc::clone(&task_scheduler))
        .with_app_handle(app_handle.clone())
        .with_review_repo(Arc::clone(&review_repo))
        .with_chat_service(Arc::clone(&recovery_chat_service))
        .with_previous_session_cutoff(previous_session_cutoff)
        .with_git_startup_blocked_projects(Arc::clone(&blocked_git_project_ids))
        .with_notification_repo(Arc::clone(&app_state.notification_repo)),
    );

    let phase_started_at = startup_phase_started("startup_job_runner");
    let _startup_ideation_recovery_claims = runner.run().await;
    startup_phase_completed("startup_job_runner", phase_started_at);

    publish_runtime_boundary_for(startup_coordinator.as_ref(), startup_attempt_id)?;
    run_external_mcp_startup(mode, app_handle.clone()).await;
    if git_startup_preflight_failed {
        return Err(AppError::Infrastructure(
            "Startup Git recovery state could not be verified".to_string(),
        ));
    }

    if let Some(github_service) = github_service.as_ref() {
        tracing::info!("Running startup PR creation recovery...");
        let phase_started_at = Instant::now();
        let plan_pr_description_drafter =
            crate::application::plan_pr_description::build_app_state_plan_pr_description_drafter(
                Arc::clone(&agent_conversation_workspace_repo),
                Arc::clone(&conversation_repo),
                Arc::clone(&agent_provider_settings_repo),
                Arc::new(app_state.manual_role_default_service()),
                agent_clients.clone(),
            );
        crate::application::pr_startup_recovery::recover_missing_draft_prs(
            Arc::clone(&task_repo),
            Arc::clone(&plan_branch_repo),
            Arc::clone(&project_repo),
            Arc::clone(&execution_plan_repo),
            Arc::clone(&ideation_session_repo),
            Arc::clone(&artifact_repo),
            Arc::clone(github_service),
            plan_pr_description_drafter,
            Arc::clone(&blocked_git_project_ids),
        )
        .await;
        tracing::info!(
            elapsed_ms = phase_started_at.elapsed().as_millis(),
            "Startup phase completed: PR creation recovery"
        );
    }

    tracing::info!("Running PR startup recovery...");
    tracing::info!("Scheduling terminal PR local git cleanup...");
    {
        let plan_branch_repo = Arc::clone(&plan_branch_repo);
        let project_repo = Arc::clone(&project_repo);
        let github_service = github_service.as_ref().map(Arc::clone);
        let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
        let running_agent_registry = Arc::clone(&running_agent_registry);
        git_cmd::with_git_command_lane(GitCommandLane::Background, async move {
            crate::application::pr_startup_recovery::cleanup_terminal_plan_branch_local_artifacts_on_startup(
                plan_branch_repo,
                project_repo,
                github_service,
                blocked_git_project_ids,
                running_agent_registry,
            )
            .await;
        })
        .await;
    }

    // Reconcile durable repair attempts before any startup PR poller can inspect reviews or
    // construct a monitor. This preserves the attempt repository as the sole repair authority
    // across a restart instead of allowing the poller to replay legacy workspace state first.
    let phase_started_at = startup_phase_started("stale_workspace_publish_repair");
    recover_stale_agent_workspace_publish_repairs_on_startup_for_state(&app_state).await;
    startup_phase_completed("stale_workspace_publish_repair", phase_started_at);

    let phase_started_at = Instant::now();
    crate::application::pr_startup_recovery::recover_pr_pollers(
        Arc::clone(&task_repo),
        Arc::clone(&plan_branch_repo),
        Arc::clone(&pr_poller_registry),
        Arc::clone(&project_repo),
        Arc::clone(&transition_service),
        Arc::clone(&blocked_git_project_ids),
    )
    .await;
    tracing::info!(
        elapsed_ms = phase_started_at.elapsed().as_millis(),
        "Startup phase completed: PR poller recovery"
    );

    tracing::info!("Running agent workspace PR startup recovery...");
    tracing::info!("Scheduling terminal agent workspace local cleanup...");
    {
        let agent_conversation_workspace_repo = Arc::clone(&agent_conversation_workspace_repo);
        let cleanup_plan_branch_repo = Arc::clone(&plan_branch_repo);
        let project_repo = Arc::clone(&project_repo);
        let github_service = github_service.as_ref().map(Arc::clone);
        let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
        let running_agent_registry = Arc::clone(&running_agent_registry);
        git_cmd::with_git_command_lane(GitCommandLane::Background, async move {
            crate::application::pr_startup_recovery::cleanup_terminal_agent_workspace_local_artifacts_on_startup(
                agent_conversation_workspace_repo,
                cleanup_plan_branch_repo,
                project_repo,
                github_service,
                blocked_git_project_ids,
                running_agent_registry,
            )
            .await;
        })
        .await;
    }

    tracing::info!("Scheduling periodic terminal PR local cleanup...");
    if startup_background::try_start_recurring_service("terminal_pr_local_cleanup") {
        let plan_branch_repo = Arc::clone(&plan_branch_repo);
        let agent_conversation_workspace_repo = Arc::clone(&agent_conversation_workspace_repo);
        let project_repo = Arc::clone(&project_repo);
        let github_service = github_service.as_ref().map(Arc::clone);
        let running_agent_registry = Arc::clone(&running_agent_registry);
        tauri::async_runtime::spawn(async move {
            crate::application::pr_startup_recovery::run_periodic_terminal_pr_local_cleanup(
                plan_branch_repo,
                agent_conversation_workspace_repo,
                project_repo,
                github_service,
                running_agent_registry,
            )
            .await;
        });
    }

    tracing::info!("Scheduling orphan agent worktree cleanup...");
    if startup_background::try_start_recurring_service("orphan_agent_worktree_cleanup") {
        let project_repo = Arc::clone(&project_repo);
        let agent_conversation_workspace_repo = Arc::clone(&agent_conversation_workspace_repo);
        let orphan_worktree_cleanup_marker_repo = Arc::clone(&orphan_worktree_cleanup_marker_repo);
        let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
        let running_agent_registry = Arc::clone(&running_agent_registry);
        tauri::async_runtime::spawn(async move {
            crate::application::orphan_worktree_cleanup::run_periodic_orphan_agent_worktree_cleanup(
                project_repo,
                agent_conversation_workspace_repo,
                orphan_worktree_cleanup_marker_repo,
                blocked_git_project_ids,
                running_agent_registry,
            )
            .await;
        });
    }

    let phase_started_at = Instant::now();
    crate::application::pr_startup_recovery::recover_agent_workspace_pr_pollers_with_notifications(
        Arc::clone(&agent_conversation_workspace_repo),
        Arc::clone(&project_repo),
        Arc::clone(&plan_branch_repo),
        Arc::clone(&pr_poller_registry),
        Arc::clone(&agent_run_repo),
        Arc::clone(&recovery_chat_service),
        Some(app_state.notification_service()),
        Some(Arc::clone(&app_state.agent_workspace_repair_repo)),
        Arc::clone(&blocked_git_project_ids),
    )
    .await;
    tracing::info!(
        elapsed_ms = phase_started_at.elapsed().as_millis(),
        "Startup phase completed: agent workspace PR poller recovery"
    );
    if let Some(github_service) = github_service.as_ref().map(Arc::clone) {
        tracing::info!("Scheduling agent workspace external PR startup reconciliation...");
        let deps =
            crate::application::agent_workspace_external_pr_reconciliation::AgentWorkspaceExternalPrReconciliationDeps {
                workspace_repo: Arc::clone(&agent_conversation_workspace_repo),
                project_repo: Arc::clone(&project_repo),
                github: github_service,
                clickup_integration_service: Some(Arc::clone(
                    &app_state.clickup_integration_service,
                )),
                external_issue_link_service: Some(Arc::clone(
                    &app_state.external_issue_link_service,
                )),
                pr_poller_registry: Some(Arc::clone(&pr_poller_registry)),
                chat_service: Some(Arc::clone(&recovery_chat_service)),
                agent_run_repo: Arc::clone(&agent_run_repo),
                agent_workspace_repair_repo: Some(Arc::clone(&app_state.agent_workspace_repair_repo)),
                plan_branch_repo: Arc::clone(&plan_branch_repo),
                app_handle: Some(app_handle.clone()),
            };
        let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
        crate::application::agent_workspace_external_pr_reconciliation::reconcile_recent_agent_workspace_external_prs_on_startup(
            deps,
            blocked_git_project_ids,
        )
        .await;
    }

    let phase_started_at = startup_phase_started("agent_workflow_recovery_launch");
    match crate::application::server_boot::recover_agent_workflow_runs_for_startup(
        Arc::new(app_state.clone()),
        Arc::clone(&execution_state),
    )
    .await
    {
        Ok(count) if count > 0 => {
            tracing::info!(count, "Recovered Scripted Agent workflow runs")
        }
        Ok(_) => {}
        Err(error) => {
            tracing::error!(%error, "Workflow startup recovery failed closed");
            return Err(error);
        }
    }
    startup_phase_completed("agent_workflow_recovery_launch", phase_started_at);

    let phase_started_at = startup_phase_started("branch_update_post_process_recovery");
    match crate::application::git_mutation_recovery::recover_terminal_branch_update_run_bindings(
        Arc::clone(&app_state.branch_update_repo),
        Arc::clone(&agent_run_repo),
    )
    .await
    {
        Ok(recovered) => tracing::info!(
            recovered,
            "Post-process branch-update run binding recovery completed"
        ),
        Err(error) => tracing::error!(
            error = %error,
            "Post-process branch-update run binding recovery failed closed"
        ),
    }
    if !active_git_startup_blocked {
        match crate::application::git_mutation_recovery::recover_branch_update_continuations(
            Arc::clone(&app_state.branch_update_repo),
            Arc::clone(&task_repo),
            Arc::clone(&project_repo),
        )
        .await
        {
            Ok(outcomes) => {
                let needs_repair = outcomes
                    .iter()
                    .filter(|outcome| {
                        matches!(
                            outcome,
                            crate::application::git_mutation_recovery::BranchUpdateContinuationRecoveryOutcome::NeedsRepair { .. }
                        )
                    })
                    .count();
                tracing::info!(
                    completed = outcomes.len().saturating_sub(needs_repair),
                    needs_repair,
                    "Startup branch-update continuation recovery completed"
                );
            }
            Err(error) => tracing::error!(
                error = %error,
                "Startup branch-update continuation recovery failed closed"
            ),
        }
    }
    for operation in app_state
        .branch_update_repo
        .list_active_operations()
        .await?
    {
        if operation.phase != crate::domain::entities::BranchUpdatePhase::Resolving
            || operation.agent_run_id.is_some()
            || operation.conversation_id.is_some()
        {
            continue;
        }
        let Some(task) = task_repo.get_by_id(&operation.task_id).await? else {
            continue;
        };
        if matches!(
            task.internal_status,
            crate::domain::entities::InternalStatus::UpdatingPlanBranch
                | crate::domain::entities::InternalStatus::UpdatingTaskBranch
        ) {
            transition_service
                .execute_entry_actions(&task.id, &task, task.internal_status)
                .await;
        }
    }
    startup_phase_completed("branch_update_post_process_recovery", phase_started_at);

    let phase_started_at = startup_phase_started("workspace_review_startup_reconciliation");
    match crate::application::agent_workspace_review::reconcile_interrupted_agent_workspace_reviews_on_startup(
        Arc::clone(&agent_conversation_workspace_repo),
        Arc::clone(&agent_run_repo),
    )
    .await
    {
        Ok(count) if count > 0 => {
            info!(
                count,
                "Startup reconciled interrupted workspace Review monitors"
            );
        }
        Ok(_) => {}
        Err(error) => {
            warn!(
                error = %error,
                "Startup workspace Review monitor reconciliation failed"
            );
        }
    }
    startup_phase_completed("workspace_review_startup_reconciliation", phase_started_at);

    let phase_started_at = startup_phase_started("workspace_review_fixer_startup_reconciliation");
    match crate::application::agent_workspace_review::reconcile_interrupted_workspace_review_fixers_on_startup(
        Arc::clone(&agent_conversation_workspace_repo),
        Arc::clone(&agent_run_repo),
        Arc::clone(&app_state.queued_message_repo),
    )
    .await
    {
        Ok(count) if count > 0 => {
            info!(count, "Startup reconciled interrupted workspace Review fixers");
        }
        Ok(_) => {}
        Err(error) => {
            warn!(error = %error, "Startup workspace Review fixer reconciliation failed");
        }
    }
    startup_phase_completed(
        "workspace_review_fixer_startup_reconciliation",
        phase_started_at,
    );

    let phase_started_at =
        startup_phase_started("workspace_review_auto_merge_guard_reconciliation");
    match crate::application::agent_workspace_review_auto_merge::
        reconcile_workspace_review_auto_merge_guards(&app_state)
        .await
    {
        Ok(count) if count > 0 => {
            info!(
                count,
                "Startup reconciled workspace Review auto-merge guards"
            );
        }
        Ok(_) => {}
        Err(error) => {
            warn!(
                error = %error,
                "Startup workspace Review auto-merge guard reconciliation failed closed"
            );
        }
    }
    startup_phase_completed(
        "workspace_review_auto_merge_guard_reconciliation",
        phase_started_at,
    );

    if startup_background::try_start_recurring_service("workspace_publish_recovery") {
        let periodic_app_state = app_state.clone();
        tauri::async_runtime::spawn(async move {
            crate::application::agent_workspace_publish_recovery::run_periodic_workspace_publish_recovery(
                periodic_app_state,
            )
            .await;
        });
    }

    if let Some(deps) = crate::application::agent_workspace_pr_supervision_recovery::build_agent_workspace_pr_supervision_recovery_deps(
        &app_state,
        Some(app_handle.clone()),
        Some(Arc::clone(&transition_service)),
        Some(Arc::clone(&recovery_chat_service)),
        pr_fix_review_publish_resumer.clone(),
    ) {
        tracing::info!("Scheduling agent workspace PR supervision startup recovery...");
        let blocked_git_project_ids = Arc::clone(&blocked_git_project_ids);
        git_cmd::with_git_command_lane(GitCommandLane::Background, async move {
            crate::application::agent_workspace_pr_supervision_recovery::recover_recent_agent_workspace_pr_supervision_on_startup(
                deps,
                blocked_git_project_ids,
            )
            .await;
        })
        .await;
    }

    if mode == StartupPipelineMode::Full {
        let phase_started_at = startup_phase_started("memory_archive_recovery");
        startup_background::recover_memory_archive_jobs_on_startup(
            Arc::clone(&memory_archive_repo),
            Arc::clone(&memory_entry_repo),
            Arc::clone(&project_repo),
        )
        .await;
        startup_phase_completed("memory_archive_recovery", phase_started_at);
    }

    if active_git_startup_blocked {
        tracing::warn!(
            "Startup Git auth preflight blocked active-project chat resumption until user repair"
        );
    } else {
        info!("Starting chat resumption runner...");
        let chat_resumption = build_startup_chat_resumption_runner(StartupChatResumptionDeps {
            agent_run_repo: Arc::clone(&agent_run_repo),
            automation_run_repo: Arc::clone(&automation_run_repo),
            task_repo: Arc::clone(&task_repo),
            execution_state: Arc::clone(&execution_state),
            chat_runtime_deps: recovery_chat_service_deps.clone(),
            execution_settings_repo: Arc::clone(&execution_settings_repo),
            agent_lane_settings_repo: Arc::clone(&agent_lane_settings_repo),
            agent_provider_settings_repo: Arc::clone(&agent_provider_settings_repo),
            plan_branch_repo: Arc::clone(&plan_branch_repo),
            interactive_process_registry: Arc::clone(&interactive_process_registry),
            app_handle: app_handle.clone(),
        });
        run_startup_owned_phase("chat_resumption", STARTUP_BACKGROUND_DB_GRACE, async move {
            chat_resumption.run().await;
        })
        .await;
    }

    let phase_started_at = startup_phase_started("agent_task_assignment_recovery");
    let assignment_recovery =
        crate::application::agent_task_assignment_recovery::AgentTaskAssignmentRecoveryService::new(
            Arc::clone(&app_state.agent_task_repo),
            Arc::clone(&app_state.delegated_session_repo),
            Arc::clone(&conversation_repo),
            Arc::clone(&agent_run_repo),
            Arc::clone(&running_agent_registry),
        );
    match assignment_recovery.recover().await {
        Ok(report) => tracing::info!(
            inspected = report.inspected,
            settled = report.settled,
            retained_running = report.retained_running,
            "Startup delegate assignment recovery completed"
        ),
        Err(error) => tracing::error!(
            %error,
            "Startup delegate assignment recovery failed closed; unresolved tasks remain unavailable"
        ),
    }
    startup_phase_completed("agent_task_assignment_recovery", phase_started_at);

    let phase_started_at = startup_phase_started("reconcile_transition_service_reuse");
    let reconcile_transition_service = Arc::clone(&transition_service);
    startup_phase_completed("reconcile_transition_service_reuse", phase_started_at);

    let phase_started_at = startup_phase_started("reconciliation_runner_build");
    let reconcile_runner = Arc::new(build_startup_reconciliation_runner(
        StartupReconciliationDeps {
            task_repo: Arc::clone(&task_repo),
            task_dependency_repo: Arc::clone(&task_dependency_repo),
            project_repo: Arc::clone(&project_repo),
            artifact_repo: Arc::clone(&artifact_repo),
            conversation_repo: Arc::clone(&conversation_repo),
            chat_message_repo: Arc::clone(&chat_message_repo),
            chat_attachment_repo: Arc::clone(&chat_attachment_repo),
            ideation_session_repo: Arc::clone(&ideation_session_repo),
            activity_event_repo: Arc::clone(&activity_event_repo),
            message_queue: Arc::clone(&message_queue),
            running_agent_registry: Arc::clone(&running_agent_registry),
            memory_event_repo: Arc::clone(&memory_event_repo),
            agent_run_repo: Arc::clone(&agent_run_repo),
            transition_service: reconcile_transition_service,
            execution_state: Arc::clone(&execution_state),
            execution_settings_repo: Arc::clone(&execution_settings_repo),
            plan_branch_repo: Arc::clone(&plan_branch_repo),
            branch_update_repo: Arc::clone(&app_state.branch_update_repo),
            pr_poller_registry: Arc::clone(&pr_poller_registry),
            interactive_process_registry: Arc::clone(&interactive_process_registry),
            review_repo: Arc::clone(&review_repo),
            notification_service: app_state.notification_service(),
            app_handle: app_handle.clone(),
        },
    ));
    startup_phase_completed("reconciliation_runner_build", phase_started_at);

    if active_git_startup_blocked {
        tracing::warn!(
            "Startup Git auth preflight blocked active-project reconciliation and ready-task watchdog until user repair"
        );
    } else {
        let phase_started_at = startup_phase_started("timeout_failure_recovery");
        reconcile_runner.recover_timeout_failures().await;
        startup_phase_completed("timeout_failure_recovery", phase_started_at);

        let immediate_reconcile_runner = Arc::clone(&reconcile_runner);
        run_startup_owned_phase(
            "stuck_task_reconciliation",
            STARTUP_BACKGROUND_DB_GRACE,
            async move {
                immediate_reconcile_runner.reconcile_stuck_tasks().await;
            },
        )
        .await;

        let phase_started_at = startup_phase_started("stuck_task_reconciliation_loop_spawn");
        if startup_background::try_start_recurring_service("stuck_task_reconciliation_loop") {
            let loop_reconcile_runner = Arc::clone(&reconcile_runner);
            tauri::async_runtime::spawn(async move {
                let interval = Duration::from_secs(30);
                loop {
                    tokio::time::sleep(interval).await;
                    loop_reconcile_runner.reconcile_stuck_tasks().await;
                }
            });
        }
        startup_phase_completed("stuck_task_reconciliation_loop_spawn", phase_started_at);

        let phase_started_at = startup_phase_started("watchdog_spawn");
        startup_background::spawn_watchdog(
            Arc::clone(&task_scheduler),
            Arc::clone(&task_repo),
            Arc::clone(&project_repo),
        );
        startup_phase_completed("watchdog_spawn", phase_started_at);

        let phase_started_at = startup_phase_started("automation_scheduler_spawn");
        startup_background::spawn_automation_scheduler(
            app_state.clone(),
            Arc::clone(&execution_state),
            app_handle.clone(),
        );
        startup_phase_completed("automation_scheduler_spawn", phase_started_at);
    }

    if active_git_startup_blocked {
        tracing::warn!(
            "Startup Git auth preflight blocked agent workspace bridge dispatcher until user repair"
        );
    } else {
        let phase_started_at = startup_phase_started("agent_workspace_bridge_dispatcher_spawn");
        startup_background::spawn_agent_workspace_bridge_dispatcher(
            AgentWorkspaceBridgeDeps {
                project_repo: Arc::clone(&project_repo),
                chat_conversation_repo: Arc::clone(&conversation_repo),
                chat_message_repo: Arc::clone(&chat_message_repo),
                agent_conversation_workspace_repo: Arc::clone(&agent_conversation_workspace_repo),
                external_events_repo: Arc::clone(&external_events_repo),
                task_repo: Arc::clone(&task_repo),
                message_queue: Arc::clone(&message_queue),
            },
            recovery_chat_service_deps.clone(),
            Arc::clone(&execution_state),
            app_handle.clone(),
        );
        startup_phase_completed("agent_workspace_bridge_dispatcher_spawn", phase_started_at);
    }

    // The remote conversation-start dispatcher is spawned earlier, in the always-run startup
    // prefix (before the recovery-disabled early return), because it is a live driver rather than
    // recovery work — see the note above the `is_startup_recovery_disabled` check.

    if mode == StartupPipelineMode::Full {
        let phase_started_at = startup_phase_started("cleanup_loop_spawn");
        startup_background::spawn_cleanup_loops(
            Arc::clone(&external_events_repo),
            Arc::clone(&memory_archive_repo),
            Arc::clone(&memory_entry_repo),
            Arc::clone(&project_repo),
        );
        startup_phase_completed("cleanup_loop_spawn", phase_started_at);
    }

    runner.spawn_post_ready_safety_net(STARTUP_BACKGROUND_DB_GRACE);

    if mode == StartupPipelineMode::DeferredGitResume && !has_git_startup_blocked_projects {
        git_auth_recovery_state.clear_pending();
    }

    if mode == StartupPipelineMode::Full && has_git_startup_blocked_projects {
        return Err(AppError::Infrastructure(
            "Startup Git authentication preflight blocked background recovery".to_string(),
        ));
    }

    Ok(())
}
