use ralphx_events::EventSink;
use std::sync::Arc;

use crate::application::runtime_factory::{build_chat_service_from_deps, ChatRuntimeFactoryDeps};
use crate::application::{
    AgentClientBundle, ChatResumptionRunner, ChatService, InteractiveProcessRegistry,
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

pub(crate) struct StartupSchedulerDeps {
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
    pub agent_lane_settings_repo: Arc<dyn AgentLaneSettingsRepository>,
    pub agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pub execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    pub interactive_process_registry: Arc<InteractiveProcessRegistry>,
    pub github_service: Option<Arc<dyn GithubServiceTrait>>,
    pub pr_poller_registry: Arc<PrPollerRegistry>,
}

pub(crate) fn build_startup_task_scheduler(deps: StartupSchedulerDeps) -> Arc<dyn TaskScheduler> {
    build_startup_task_scheduler_concrete(deps) as Arc<dyn TaskScheduler>
}

pub(super) fn build_startup_task_scheduler_concrete(
    deps: StartupSchedulerDeps,
) -> Arc<TaskSchedulerService> {
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
    .with_agent_lane_settings_repo(deps.agent_lane_settings_repo)
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
    execution_state: Arc<ExecutionState>,
    deps: ChatRuntimeFactoryDeps,
) -> Arc<dyn ChatService> {
    Arc::new(build_chat_service_from_deps(Some(execution_state), &deps))
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
    pub events: Arc<dyn EventSink>,
    pub chat_runtime_deps: ChatRuntimeFactoryDeps,
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
        None,
    )
    .with_events(deps.events)
    .with_chat_runtime_deps(deps.chat_runtime_deps)
    .with_notification_service(deps.notification_service)
    .with_execution_settings_repo(deps.execution_settings_repo)
    .with_plan_branch_repo(deps.plan_branch_repo)
    .with_branch_update_repo(deps.branch_update_repo)
    .with_pr_poller_registry(deps.pr_poller_registry)
    .with_interactive_process_registry(deps.interactive_process_registry)
    .with_review_repo(deps.review_repo)
}
