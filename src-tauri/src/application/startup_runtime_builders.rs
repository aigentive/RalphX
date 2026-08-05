use std::sync::Arc;

use crate::application::runtime_factory::{
    build_chat_service_with_fallback, ChatRuntimeFactoryDeps,
};
use crate::application::{
    AgentClientBundle, AppState, ChatResumptionRunner, ChatService, InteractiveProcessRegistry,
    NotificationService, PrPollerRegistry, ReconciliationRunner, TaskSchedulerService,
};
use crate::commands::ExecutionState;
use crate::domain::repositories::{
    ActivityEventRepository, AgentLaneSettingsRepository, AgentProviderSettingsRepository,
    AgentRunRepository, ArtifactRepository, AutomationRunRepository, BranchUpdateRepository,
    ChatAttachmentRepository, ChatConversationRepository, ChatMessageRepository,
    ExecutionPlanRepository, ExecutionSettingsRepository, IdeationSessionRepository,
    MemoryEventRepository, PlanBranchRepository, ProjectRepository, ReviewRepository,
    TaskDependencyRepository, TaskRepository,
};
use crate::domain::services::{GithubServiceTrait, MessageQueue, RunningAgentRegistry};
use crate::domain::state_machine::services::TaskScheduler;
use tauri::{Manager, Runtime};

pub(crate) struct StartupSchedulerDeps<R: Runtime = tauri::Wry> {
    pub execution_state: Arc<ExecutionState>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    pub artifact_repo: Arc<dyn ArtifactRepository>,
    pub chat_message_repo: Arc<dyn ChatMessageRepository>,
    pub chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    pub conversation_repo: Arc<dyn ChatConversationRepository>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    pub activity_event_repo: Arc<dyn ActivityEventRepository>,
    pub message_queue: Arc<MessageQueue>,
    pub running_agent_registry: Arc<dyn RunningAgentRegistry>,
    pub memory_event_repo: Arc<dyn MemoryEventRepository>,
    pub agent_clients: AgentClientBundle,
    pub agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pub execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    pub interactive_process_registry: Arc<InteractiveProcessRegistry>,
    pub github_service: Option<Arc<dyn GithubServiceTrait>>,
    pub pr_poller_registry: Arc<PrPollerRegistry>,
    pub _app_handle: tauri::AppHandle<R>,
}

pub(crate) fn build_startup_task_scheduler<R: Runtime>(
    deps: StartupSchedulerDeps<R>,
) -> Arc<dyn TaskScheduler> {
    build_startup_task_scheduler_concrete(deps) as Arc<dyn TaskScheduler>
}

fn build_startup_task_scheduler_concrete<R: Runtime>(
    deps: StartupSchedulerDeps<R>,
) -> Arc<TaskSchedulerService> {
    let agent_lane_settings_repo = {
        let app_state = deps._app_handle.state::<AppState>();
        Arc::clone(&app_state.agent_lane_settings_repo)
    };

    let mut scheduler = TaskSchedulerService::new(
        Arc::clone(&deps.execution_state),
        deps.project_repo,
        deps.task_repo,
        deps.task_dependency_repo,
        deps.artifact_repo,
        deps.chat_message_repo,
        deps.chat_attachment_repo,
        deps.conversation_repo,
        deps.agent_run_repo,
        deps.ideation_session_repo,
        deps.activity_event_repo,
        deps.message_queue,
        deps.running_agent_registry,
        deps.memory_event_repo,
        None,
    )
    .with_agent_clients(deps.agent_clients)
    .with_agent_lane_settings_repo(agent_lane_settings_repo)
    .with_agent_provider_settings_repo(deps.agent_provider_settings_repo)
    .with_plan_branch_repo(deps.plan_branch_repo)
    .with_execution_plan_repo(deps.execution_plan_repo)
    .with_interactive_process_registry(deps.interactive_process_registry)
    .with_pr_poller_registry(deps.pr_poller_registry);

    if let Some(github_service) = deps.github_service {
        scheduler = scheduler.with_github_service(github_service);
    }

    let scheduler_concrete = Arc::new(scheduler);
    scheduler_concrete.set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
    scheduler_concrete
}

pub(crate) fn build_startup_recovery_chat_service(
    app_handle: tauri::AppHandle,
    execution_state: Arc<ExecutionState>,
    deps: ChatRuntimeFactoryDeps,
) -> Arc<dyn ChatService> {
    Arc::new(build_chat_service_with_fallback(
        &Some(app_handle),
        Some(execution_state),
        &deps,
    ))
}

pub(crate) struct StartupChatResumptionDeps {
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub automation_run_repo: Arc<dyn AutomationRunRepository>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub execution_state: Arc<ExecutionState>,
    pub chat_runtime_deps: ChatRuntimeFactoryDeps,
    pub execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
    pub agent_lane_settings_repo: Arc<dyn AgentLaneSettingsRepository>,
    pub agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pub interactive_process_registry: Arc<InteractiveProcessRegistry>,
    pub app_handle: tauri::AppHandle,
    pub managed_team_barrier: Arc<crate::application::managed_team::ManagedTeamStartupBarrier>,
}

pub(crate) fn build_startup_chat_resumption_runner(
    deps: StartupChatResumptionDeps,
) -> ChatResumptionRunner {
    ChatResumptionRunner::new(
        deps.agent_run_repo,
        deps.automation_run_repo,
        deps.task_repo,
        deps.execution_state,
        deps.chat_runtime_deps,
    )
    .with_app_handle(deps.app_handle)
    .with_execution_settings_repo(deps.execution_settings_repo)
    .with_agent_lane_settings_repo(deps.agent_lane_settings_repo)
    .with_agent_provider_settings_repo(deps.agent_provider_settings_repo)
    .with_plan_branch_repo(deps.plan_branch_repo)
    .with_interactive_process_registry(deps.interactive_process_registry)
    .with_managed_team_barrier(deps.managed_team_barrier)
}

pub(crate) struct StartupReconciliationDeps {
    pub task_repo: Arc<dyn TaskRepository>,
    pub task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub artifact_repo: Arc<dyn ArtifactRepository>,
    pub conversation_repo: Arc<dyn ChatConversationRepository>,
    pub chat_message_repo: Arc<dyn ChatMessageRepository>,
    pub chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    pub ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    pub activity_event_repo: Arc<dyn ActivityEventRepository>,
    pub message_queue: Arc<MessageQueue>,
    pub running_agent_registry: Arc<dyn RunningAgentRegistry>,
    pub memory_event_repo: Arc<dyn MemoryEventRepository>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub transition_service: Arc<crate::application::TaskTransitionService>,
    pub execution_state: Arc<ExecutionState>,
    pub execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pub branch_update_repo: Arc<dyn BranchUpdateRepository>,
    pub pr_poller_registry: Arc<PrPollerRegistry>,
    pub interactive_process_registry: Arc<InteractiveProcessRegistry>,
    pub review_repo: Arc<dyn ReviewRepository>,
    pub notification_service: Arc<NotificationService>,
    pub recovery_prompt_tracker: Arc<crate::application::reconciliation::RecoveryPromptTracker>,
    pub app_handle: tauri::AppHandle,
}

pub(crate) fn build_startup_reconciliation_runner(
    deps: StartupReconciliationDeps,
) -> ReconciliationRunner {
    ReconciliationRunner::new(
        deps.task_repo,
        deps.task_dependency_repo,
        deps.project_repo,
        deps.artifact_repo,
        deps.conversation_repo,
        deps.chat_message_repo,
        deps.chat_attachment_repo,
        deps.ideation_session_repo,
        deps.activity_event_repo,
        deps.message_queue,
        deps.running_agent_registry,
        deps.memory_event_repo,
        deps.agent_run_repo,
        deps.transition_service,
        deps.execution_state,
        Some(deps.app_handle),
    )
    .with_prompt_tracker(deps.recovery_prompt_tracker)
    .with_notification_service(deps.notification_service)
    .with_execution_settings_repo(deps.execution_settings_repo)
    .with_plan_branch_repo(deps.plan_branch_repo)
    .with_branch_update_repo(deps.branch_update_repo)
    .with_pr_poller_registry(deps.pr_poller_registry)
    .with_interactive_process_registry(deps.interactive_process_registry)
    .with_review_repo(deps.review_repo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::AppState;
    use crate::domain::entities::{ExecutionPlan, IdeationSession, InternalStatus, Project, Task};
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[tokio::test]
    async fn startup_scheduler_skips_ready_task_from_superseded_execution_plan() {
        let app_state = AppState::new_test();
        let execution_state = Arc::new(ExecutionState::new());
        let app = mock_builder()
            .manage(app_state.clone())
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let project = Project::new(
            "Startup Scheduler Project".into(),
            "/tmp/startup-scheduler".into(),
        );
        let project_id = project.id.clone();
        app_state.project_repo.create(project).await.unwrap();

        let session = IdeationSession::new(project_id.clone());
        let session = app_state
            .ideation_session_repo
            .create(session)
            .await
            .unwrap();

        let stale_plan = app_state
            .execution_plan_repo
            .create(ExecutionPlan::new(session.id.clone()))
            .await
            .unwrap();
        app_state
            .execution_plan_repo
            .mark_superseded(&stale_plan.id)
            .await
            .unwrap();

        let mut stale_task = Task::new(project_id, "stale ready task".into());
        stale_task.internal_status = InternalStatus::Ready;
        stale_task.execution_plan_id = Some(stale_plan.id.clone());
        let stale_task_id = stale_task.id.clone();
        app_state.task_repo.create(stale_task).await.unwrap();

        let scheduler = build_startup_task_scheduler(StartupSchedulerDeps {
            execution_state,
            project_repo: Arc::clone(&app_state.project_repo),
            task_repo: Arc::clone(&app_state.task_repo),
            task_dependency_repo: Arc::clone(&app_state.task_dependency_repo),
            artifact_repo: Arc::clone(&app_state.artifact_repo),
            chat_message_repo: Arc::clone(&app_state.chat_message_repo),
            chat_attachment_repo: Arc::clone(&app_state.chat_attachment_repo),
            conversation_repo: Arc::clone(&app_state.chat_conversation_repo),
            agent_run_repo: Arc::clone(&app_state.agent_run_repo),
            ideation_session_repo: Arc::clone(&app_state.ideation_session_repo),
            activity_event_repo: Arc::clone(&app_state.activity_event_repo),
            message_queue: Arc::clone(&app_state.message_queue),
            running_agent_registry: Arc::clone(&app_state.running_agent_registry),
            memory_event_repo: Arc::clone(&app_state.memory_event_repo),
            agent_clients: app_state.agent_client_bundle(),
            agent_provider_settings_repo: Arc::clone(&app_state.agent_provider_settings_repo),
            plan_branch_repo: Arc::clone(&app_state.plan_branch_repo),
            execution_plan_repo: Arc::clone(&app_state.execution_plan_repo),
            interactive_process_registry: Arc::clone(&app_state.interactive_process_registry),
            github_service: None,
            pr_poller_registry: Arc::clone(&app_state.pr_poller_registry),
            _app_handle: app.handle().clone(),
        });

        scheduler.try_schedule_ready_tasks().await;

        let stored = app_state
            .task_repo
            .get_by_id(&stale_task_id)
            .await
            .unwrap()
            .expect("stale task should remain persisted");
        assert_eq!(
            stored.internal_status,
            InternalStatus::Ready,
            "startup-built scheduler must not admit superseded-plan ready tasks"
        );
    }

    #[test]
    fn startup_scheduler_carries_lane_and_provider_settings() {
        let app_state = AppState::new_test();
        let execution_state = Arc::new(ExecutionState::new());
        let app = mock_builder()
            .manage(app_state.clone())
            .build(mock_context(noop_assets()))
            .expect("mock app");

        let scheduler = build_startup_task_scheduler_concrete(StartupSchedulerDeps {
            execution_state,
            project_repo: Arc::clone(&app_state.project_repo),
            task_repo: Arc::clone(&app_state.task_repo),
            task_dependency_repo: Arc::clone(&app_state.task_dependency_repo),
            artifact_repo: Arc::clone(&app_state.artifact_repo),
            chat_message_repo: Arc::clone(&app_state.chat_message_repo),
            chat_attachment_repo: Arc::clone(&app_state.chat_attachment_repo),
            conversation_repo: Arc::clone(&app_state.chat_conversation_repo),
            agent_run_repo: Arc::clone(&app_state.agent_run_repo),
            ideation_session_repo: Arc::clone(&app_state.ideation_session_repo),
            activity_event_repo: Arc::clone(&app_state.activity_event_repo),
            message_queue: Arc::clone(&app_state.message_queue),
            running_agent_registry: Arc::clone(&app_state.running_agent_registry),
            memory_event_repo: Arc::clone(&app_state.memory_event_repo),
            agent_clients: app_state.agent_client_bundle(),
            agent_provider_settings_repo: Arc::clone(&app_state.agent_provider_settings_repo),
            plan_branch_repo: Arc::clone(&app_state.plan_branch_repo),
            execution_plan_repo: Arc::clone(&app_state.execution_plan_repo),
            interactive_process_registry: Arc::clone(&app_state.interactive_process_registry),
            github_service: None,
            pr_poller_registry: Arc::clone(&app_state.pr_poller_registry),
            _app_handle: app.handle().clone(),
        });

        assert!(scheduler.agent_lane_settings_repo.is_some());
        assert!(scheduler.agent_provider_settings_repo.is_some());
    }
}
