use std::sync::Arc;

use tauri::{AppHandle, Manager, Runtime};

use crate::application::chat_service::{AppChatService, StreamingStateCache};
use crate::application::{
    AgentClientBundle, AppState, InteractiveProcessRegistry, PrPollerRegistry,
    TaskSchedulerService, TaskTransitionService,
};
use crate::commands::ExecutionState;
use crate::domain::repositories::{
    ActivityEventRepository, AgentConversationWorkspaceRepository, AgentLaneSettingsRepository,
    AgentProviderSettingsRepository, AgentRunRepository, ArtifactRepository,
    ChatAttachmentRepository, ChatConversationRepository, ChatMessageRepository,
    DelegatedSessionRepository, ExecutionPlanRepository, ExecutionSettingsRepository,
    IdeationEffortSettingsRepository, IdeationModelSettingsRepository, IdeationSessionRepository,
    MemoryEventRepository, PlanBranchRepository, ProjectRepository, ReviewRepository,
    TaskDependencyRepository, TaskProposalRepository, TaskRepository, TaskStepRepository,
};
use crate::domain::services::{GithubServiceTrait, MessageQueue, RunningAgentRegistry};
use crate::infrastructure::memory::MemoryDelegatedSessionRepository;

#[derive(Clone)]
pub(crate) struct RuntimeFactoryDeps {
    pub task_repo: Arc<dyn TaskRepository>,
    pub task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    pub project_repo: Arc<dyn ProjectRepository>,
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
    pub agent_clients: Option<AgentClientBundle>,
    pub execution_plan_repo: Option<Arc<dyn ExecutionPlanRepository>>,
    pub execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    pub agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    pub agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    pub review_repo: Option<Arc<dyn ReviewRepository>>,
    pub plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    pub interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    pub github_service: Option<Arc<dyn GithubServiceTrait>>,
    pub pr_poller_registry: Option<Arc<PrPollerRegistry>>,
}

impl RuntimeFactoryDeps {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_core(
        task_repo: Arc<dyn TaskRepository>,
        task_dependency_repo: Arc<dyn TaskDependencyRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        artifact_repo: Arc<dyn ArtifactRepository>,
        chat_message_repo: Arc<dyn ChatMessageRepository>,
        chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        activity_event_repo: Arc<dyn ActivityEventRepository>,
        message_queue: Arc<MessageQueue>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        memory_event_repo: Arc<dyn MemoryEventRepository>,
    ) -> Self {
        Self {
            task_repo,
            task_dependency_repo,
            project_repo,
            artifact_repo,
            chat_message_repo,
            chat_attachment_repo,
            conversation_repo,
            agent_run_repo,
            ideation_session_repo,
            activity_event_repo,
            message_queue,
            running_agent_registry,
            memory_event_repo,
            agent_clients: None,
            execution_plan_repo: None,
            execution_settings_repo: None,
            agent_lane_settings_repo: None,
            agent_provider_settings_repo: None,
            review_repo: None,
            plan_branch_repo: None,
            interactive_process_registry: None,
            github_service: None,
            pr_poller_registry: None,
        }
    }

    pub(crate) fn with_agent_clients(
        mut self,
        agent_clients: Option<AgentClientBundle>,
    ) -> Self {
        self.agent_clients = agent_clients;
        self
    }

    pub(crate) fn with_execution_plan_repo(
        mut self,
        execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    ) -> Self {
        self.execution_plan_repo = Some(execution_plan_repo);
        self
    }

    pub(crate) fn with_review_repo(mut self, review_repo: Arc<dyn ReviewRepository>) -> Self {
        self.review_repo = Some(review_repo);
        self
    }

    pub(crate) fn with_runtime_support(
        mut self,
        execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
        agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
        agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
        plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
        interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    ) -> Self {
        self.execution_settings_repo = execution_settings_repo;
        self.agent_lane_settings_repo = agent_lane_settings_repo;
        self.agent_provider_settings_repo = agent_provider_settings_repo;
        self.plan_branch_repo = plan_branch_repo;
        self.interactive_process_registry = interactive_process_registry;
        self
    }

    pub(crate) fn with_github_runtime_support(
        mut self,
        github_service: Option<Arc<dyn GithubServiceTrait>>,
        pr_poller_registry: Option<Arc<PrPollerRegistry>>,
    ) -> Self {
        self.github_service = github_service;
        self.pr_poller_registry = pr_poller_registry;
        self
    }

    pub(crate) fn from_app_state(state: &AppState) -> Self {
        Self::from_core(
            Arc::clone(&state.task_repo),
            Arc::clone(&state.task_dependency_repo),
            Arc::clone(&state.project_repo),
            Arc::clone(&state.artifact_repo),
            Arc::clone(&state.chat_message_repo),
            Arc::clone(&state.chat_attachment_repo),
            Arc::clone(&state.chat_conversation_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.ideation_session_repo),
            Arc::clone(&state.activity_event_repo),
            Arc::clone(&state.message_queue),
            Arc::clone(&state.running_agent_registry),
            Arc::clone(&state.memory_event_repo),
        )
        .with_agent_clients(Some(state.agent_client_bundle()))
        .with_execution_plan_repo(Arc::clone(&state.execution_plan_repo))
        .with_review_repo(Arc::clone(&state.review_repo))
        .with_runtime_support(
            Some(Arc::clone(&state.execution_settings_repo)),
            Some(Arc::clone(&state.agent_lane_settings_repo)),
            Some(Arc::clone(&state.agent_provider_settings_repo)),
            Some(Arc::clone(&state.plan_branch_repo)),
            Some(Arc::clone(&state.interactive_process_registry)),
        )
        .with_github_runtime_support(
            state.github_service.as_ref().map(Arc::clone),
            Some(Arc::clone(&state.pr_poller_registry)),
        )
    }
}

#[derive(Clone)]
pub(crate) struct ChatRuntimeFactoryDeps {
    pub chat_message_repo: Arc<dyn ChatMessageRepository>,
    pub chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    pub artifact_repo: Arc<dyn ArtifactRepository>,
    pub conversation_repo: Arc<dyn ChatConversationRepository>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    pub ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    pub delegated_session_repo: Option<Arc<dyn DelegatedSessionRepository>>,
    pub activity_event_repo: Arc<dyn ActivityEventRepository>,
    pub message_queue: Arc<MessageQueue>,
    pub running_agent_registry: Arc<dyn RunningAgentRegistry>,
    pub memory_event_repo: Arc<dyn MemoryEventRepository>,
    pub execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    pub agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    pub agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    pub ideation_effort_settings_repo: Option<Arc<dyn IdeationEffortSettingsRepository>>,
    pub ideation_model_settings_repo: Option<Arc<dyn IdeationModelSettingsRepository>>,
    pub agent_conversation_workspace_repo: Option<Arc<dyn AgentConversationWorkspaceRepository>>,
    pub plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    pub task_proposal_repo: Option<Arc<dyn TaskProposalRepository>>,
    pub task_step_repo: Option<Arc<dyn TaskStepRepository>>,
    pub review_repo: Option<Arc<dyn ReviewRepository>>,
    pub interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    pub streaming_state_cache: Option<StreamingStateCache>,
}

impl ChatRuntimeFactoryDeps {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_core(
        chat_message_repo: Arc<dyn ChatMessageRepository>,
        chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
        artifact_repo: Arc<dyn ArtifactRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        task_repo: Arc<dyn TaskRepository>,
        task_dependency_repo: Arc<dyn TaskDependencyRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        activity_event_repo: Arc<dyn ActivityEventRepository>,
        message_queue: Arc<MessageQueue>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        memory_event_repo: Arc<dyn MemoryEventRepository>,
    ) -> Self {
        Self {
            chat_message_repo,
            chat_attachment_repo,
            artifact_repo,
            conversation_repo,
            agent_run_repo,
            project_repo,
            task_repo,
            task_dependency_repo,
            ideation_session_repo,
            delegated_session_repo: None,
            activity_event_repo,
            message_queue,
            running_agent_registry,
            memory_event_repo,
            execution_settings_repo: None,
            agent_lane_settings_repo: None,
            agent_provider_settings_repo: None,
            ideation_effort_settings_repo: None,
            ideation_model_settings_repo: None,
            agent_conversation_workspace_repo: None,
            plan_branch_repo: None,
            task_proposal_repo: None,
            task_step_repo: None,
            review_repo: None,
            interactive_process_registry: None,
            streaming_state_cache: None,
        }
    }

    pub(crate) fn with_execution_settings_repo(
        mut self,
        repo: Arc<dyn ExecutionSettingsRepository>,
    ) -> Self {
        self.execution_settings_repo = Some(repo);
        self
    }

    pub(crate) fn with_agent_lane_settings_repo(
        mut self,
        repo: Arc<dyn AgentLaneSettingsRepository>,
    ) -> Self {
        self.agent_lane_settings_repo = Some(repo);
        self
    }

    pub(crate) fn with_agent_provider_settings_repo(
        mut self,
        repo: Arc<dyn AgentProviderSettingsRepository>,
    ) -> Self {
        self.agent_provider_settings_repo = Some(repo);
        self
    }

    pub(crate) fn with_ideation_effort_settings_repo(
        mut self,
        repo: Arc<dyn IdeationEffortSettingsRepository>,
    ) -> Self {
        self.ideation_effort_settings_repo = Some(repo);
        self
    }

    pub(crate) fn with_ideation_model_settings_repo(
        mut self,
        repo: Arc<dyn IdeationModelSettingsRepository>,
    ) -> Self {
        self.ideation_model_settings_repo = Some(repo);
        self
    }

    pub(crate) fn with_agent_conversation_workspace_repo(
        mut self,
        repo: Option<Arc<dyn AgentConversationWorkspaceRepository>>,
    ) -> Self {
        self.agent_conversation_workspace_repo = repo;
        self
    }

    pub(crate) fn with_plan_branch_repo(mut self, repo: Arc<dyn PlanBranchRepository>) -> Self {
        self.plan_branch_repo = Some(repo);
        self
    }

    pub(crate) fn with_task_proposal_repo(mut self, repo: Arc<dyn TaskProposalRepository>) -> Self {
        self.task_proposal_repo = Some(repo);
        self
    }

    pub(crate) fn with_task_step_repo(mut self, repo: Arc<dyn TaskStepRepository>) -> Self {
        self.task_step_repo = Some(repo);
        self
    }

    pub(crate) fn with_review_repo(mut self, repo: Arc<dyn ReviewRepository>) -> Self {
        self.review_repo = Some(repo);
        self
    }

    pub(crate) fn with_interactive_process_registry(
        mut self,
        registry: Arc<InteractiveProcessRegistry>,
    ) -> Self {
        self.interactive_process_registry = Some(registry);
        self
    }

    pub(crate) fn with_streaming_state_cache(mut self, cache: StreamingStateCache) -> Self {
        self.streaming_state_cache = Some(cache);
        self
    }

    pub(crate) fn with_delegated_session_repo(
        mut self,
        repo: Arc<dyn DelegatedSessionRepository>,
    ) -> Self {
        self.delegated_session_repo = Some(repo);
        self
    }

    pub(crate) fn with_runtime_support(
        mut self,
        execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
        agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
        agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
        plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
        interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    ) -> Self {
        if let Some(repo) = execution_settings_repo {
            self = self.with_execution_settings_repo(repo);
        }
        if let Some(repo) = agent_lane_settings_repo {
            self = self.with_agent_lane_settings_repo(repo);
        }
        if let Some(repo) = agent_provider_settings_repo {
            self = self.with_agent_provider_settings_repo(repo);
        }
        if let Some(repo) = plan_branch_repo {
            self = self.with_plan_branch_repo(repo);
        }
        if let Some(registry) = interactive_process_registry {
            self = self.with_interactive_process_registry(registry);
        }
        self
    }

    pub(crate) fn with_ideation_runtime_support(
        mut self,
        ideation_effort_settings_repo: Option<Arc<dyn IdeationEffortSettingsRepository>>,
        ideation_model_settings_repo: Option<Arc<dyn IdeationModelSettingsRepository>>,
    ) -> Self {
        if let Some(repo) = ideation_effort_settings_repo {
            self = self.with_ideation_effort_settings_repo(repo);
        }
        if let Some(repo) = ideation_model_settings_repo {
            self = self.with_ideation_model_settings_repo(repo);
        }
        self
    }

    pub(crate) fn with_chat_context_support(
        mut self,
        task_proposal_repo: Option<Arc<dyn TaskProposalRepository>>,
        task_step_repo: Option<Arc<dyn TaskStepRepository>>,
        review_repo: Option<Arc<dyn ReviewRepository>>,
        streaming_state_cache: Option<StreamingStateCache>,
    ) -> Self {
        if let Some(repo) = task_proposal_repo {
            self = self.with_task_proposal_repo(repo);
        }
        if let Some(repo) = task_step_repo {
            self = self.with_task_step_repo(repo);
        }
        if let Some(repo) = review_repo {
            self = self.with_review_repo(repo);
        }
        if let Some(cache) = streaming_state_cache {
            self = self.with_streaming_state_cache(cache);
        }
        self
    }

    pub(crate) fn from_app_state(state: &AppState) -> Self {
        Self::from_core(
            Arc::clone(&state.chat_message_repo),
            Arc::clone(&state.chat_attachment_repo),
            Arc::clone(&state.artifact_repo),
            Arc::clone(&state.chat_conversation_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.project_repo),
            Arc::clone(&state.task_repo),
            Arc::clone(&state.task_dependency_repo),
            Arc::clone(&state.ideation_session_repo),
            Arc::clone(&state.activity_event_repo),
            Arc::clone(&state.message_queue),
            Arc::clone(&state.running_agent_registry),
            Arc::clone(&state.memory_event_repo),
        )
        .with_delegated_session_repo(Arc::clone(&state.delegated_session_repo))
        .with_runtime_support(
            Some(Arc::clone(&state.execution_settings_repo)),
            Some(Arc::clone(&state.agent_lane_settings_repo)),
            Some(Arc::clone(&state.agent_provider_settings_repo)),
            Some(Arc::clone(&state.plan_branch_repo)),
            Some(Arc::clone(&state.interactive_process_registry)),
        )
        .with_ideation_runtime_support(
            Some(Arc::clone(&state.ideation_effort_settings_repo)),
            Some(Arc::clone(&state.ideation_model_settings_repo)),
        )
        .with_agent_conversation_workspace_repo(Some(Arc::clone(
            &state.agent_conversation_workspace_repo,
        )))
        .with_chat_context_support(
            Some(Arc::clone(&state.task_proposal_repo)),
            Some(Arc::clone(&state.task_step_repo)),
            Some(Arc::clone(&state.review_repo)),
            Some(state.streaming_state_cache.clone()),
        )
    }
}

pub(crate) fn build_chat_service_from_deps<R: Runtime>(
    app_handle: Option<AppHandle<R>>,
    execution_state: Option<Arc<ExecutionState>>,
    deps: &ChatRuntimeFactoryDeps,
) -> AppChatService<R> {
    let mut service = AppChatService::new(
        Arc::clone(&deps.chat_message_repo),
        Arc::clone(&deps.chat_attachment_repo),
        Arc::clone(&deps.artifact_repo),
        Arc::clone(&deps.conversation_repo),
        Arc::clone(&deps.agent_run_repo),
        Arc::clone(&deps.project_repo),
        Arc::clone(&deps.task_repo),
        Arc::clone(&deps.task_dependency_repo),
        Arc::clone(&deps.ideation_session_repo),
        deps.delegated_session_repo
            .as_ref()
            .map(Arc::clone)
            .unwrap_or_else(|| Arc::new(MemoryDelegatedSessionRepository::new())),
        Arc::clone(&deps.activity_event_repo),
        Arc::clone(&deps.message_queue),
        Arc::clone(&deps.running_agent_registry),
        Arc::clone(&deps.memory_event_repo),
    );

    if let Some(state) = execution_state {
        service = service.with_execution_state(state);
    }
    if let Some(handle) = app_handle {
        service = service.with_app_handle(handle);
    }
    if let Some(repo) = deps.execution_settings_repo.as_ref() {
        service = service.with_execution_settings_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.agent_lane_settings_repo.as_ref() {
        service = service.with_agent_lane_settings_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.agent_provider_settings_repo.as_ref() {
        service = service.with_agent_provider_settings_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.ideation_effort_settings_repo.as_ref() {
        service = service.with_ideation_effort_settings_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.ideation_model_settings_repo.as_ref() {
        service = service.with_ideation_model_settings_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.agent_conversation_workspace_repo.as_ref() {
        service = service.with_agent_conversation_workspace_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.plan_branch_repo.as_ref() {
        service = service.with_plan_branch_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.task_proposal_repo.as_ref() {
        service = service.with_task_proposal_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.task_step_repo.as_ref() {
        service = service.with_task_step_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.review_repo.as_ref() {
        service = service.with_review_repo(Arc::clone(repo));
    }
    if let Some(ipr) = deps.interactive_process_registry.as_ref() {
        service = service.with_interactive_process_registry(Arc::clone(ipr));
    }
    if let Some(cache) = deps.streaming_state_cache.as_ref() {
        service = service.with_streaming_state_cache(cache.clone());
    }

    service
}

pub(crate) fn build_chat_service_with_fallback<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
    execution_state: Option<Arc<ExecutionState>>,
    deps: &ChatRuntimeFactoryDeps,
) -> AppChatService<R> {
    if let Some(handle) = app_handle {
        if let Some(app_state) = handle.try_state::<AppState>() {
            return app_state.build_chat_service_for_runtime(execution_state, app_handle.clone());
        }
    }

    build_chat_service_from_deps(app_handle.clone(), execution_state, deps)
}

pub(crate) fn build_transition_service_with_fallback<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
    execution_state: Arc<ExecutionState>,
    deps: &RuntimeFactoryDeps,
) -> TaskTransitionService<R> {
    if let Some(handle) = app_handle {
        if let Some(app_state) = handle.try_state::<AppState>() {
            return app_state
                .build_transition_service_for_runtime(execution_state, app_handle.clone());
        }
    }

    build_transition_service_from_deps(app_handle.clone(), execution_state, deps)
}

pub(crate) fn build_transition_service_from_deps<R: Runtime>(
    app_handle: Option<AppHandle<R>>,
    execution_state: Arc<ExecutionState>,
    deps: &RuntimeFactoryDeps,
) -> TaskTransitionService<R> {
    let mut service = TaskTransitionService::new(
        Arc::clone(&deps.task_repo),
        Arc::clone(&deps.task_dependency_repo),
        Arc::clone(&deps.project_repo),
        Arc::clone(&deps.chat_message_repo),
        Arc::clone(&deps.chat_attachment_repo),
        Arc::clone(&deps.conversation_repo),
        Arc::clone(&deps.agent_run_repo),
        Arc::clone(&deps.ideation_session_repo),
        Arc::clone(&deps.activity_event_repo),
        Arc::clone(&deps.message_queue),
        Arc::clone(&deps.running_agent_registry),
        execution_state,
        app_handle,
        Arc::clone(&deps.memory_event_repo),
    );
    if let Some(repo) = deps.execution_settings_repo.as_ref() {
        service = service.with_execution_settings_repo(Arc::clone(repo));
    }
    if let Some(agent_clients) = deps.agent_clients.as_ref() {
        service = service.with_agent_clients(agent_clients.clone());
    }
    if let Some(repo) = deps.agent_lane_settings_repo.as_ref() {
        service = service.with_agent_lane_settings_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.review_repo.as_ref() {
        service = service.with_review_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.plan_branch_repo.as_ref() {
        service = service.with_plan_branch_repo(Arc::clone(repo));
    }
    service = service.with_artifact_repo(Arc::clone(&deps.artifact_repo));
    if let Some(ipr) = deps.interactive_process_registry.as_ref() {
        service = service.with_interactive_process_registry(Arc::clone(ipr));
    }
    if let Some(registry) = deps.pr_poller_registry.as_ref() {
        service = service.with_pr_poller_registry(Arc::clone(registry));
    }
    if let Some(github) = deps.github_service.as_ref() {
        service = service.with_github_service(Arc::clone(github));
    }
    service
}

pub(crate) fn build_task_scheduler_with_fallback<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
    execution_state: Arc<ExecutionState>,
    deps: &RuntimeFactoryDeps,
) -> TaskSchedulerService<R> {
    if let Some(handle) = app_handle {
        if let Some(app_state) = handle.try_state::<AppState>() {
            return app_state.build_task_scheduler_for_runtime(execution_state, app_handle.clone());
        }
    }

    build_task_scheduler_from_deps(app_handle.clone(), execution_state, deps)
}

pub(crate) fn build_task_scheduler_from_deps<R: Runtime>(
    app_handle: Option<AppHandle<R>>,
    execution_state: Arc<ExecutionState>,
    deps: &RuntimeFactoryDeps,
) -> TaskSchedulerService<R> {
    let mut scheduler = TaskSchedulerService::new(
        execution_state,
        Arc::clone(&deps.project_repo),
        Arc::clone(&deps.task_repo),
        Arc::clone(&deps.task_dependency_repo),
        Arc::clone(&deps.artifact_repo),
        Arc::clone(&deps.chat_message_repo),
        Arc::clone(&deps.chat_attachment_repo),
        Arc::clone(&deps.conversation_repo),
        Arc::clone(&deps.agent_run_repo),
        Arc::clone(&deps.ideation_session_repo),
        Arc::clone(&deps.activity_event_repo),
        Arc::clone(&deps.message_queue),
        Arc::clone(&deps.running_agent_registry),
        Arc::clone(&deps.memory_event_repo),
        app_handle,
    );
    if let Some(repo) = deps.execution_settings_repo.as_ref() {
        scheduler = scheduler.with_execution_settings_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.execution_plan_repo.as_ref() {
        scheduler = scheduler.with_execution_plan_repo(Arc::clone(repo));
    }
    if let Some(agent_clients) = deps.agent_clients.as_ref() {
        scheduler = scheduler.with_agent_clients(agent_clients.clone());
    }
    if let Some(repo) = deps.agent_lane_settings_repo.as_ref() {
        scheduler = scheduler.with_agent_lane_settings_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.agent_provider_settings_repo.as_ref() {
        scheduler = scheduler.with_agent_provider_settings_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.plan_branch_repo.as_ref() {
        scheduler = scheduler.with_plan_branch_repo(Arc::clone(repo));
    }
    if let Some(ipr) = deps.interactive_process_registry.as_ref() {
        scheduler = scheduler.with_interactive_process_registry(Arc::clone(ipr));
    }
    if let Some(registry) = deps.pr_poller_registry.as_ref() {
        scheduler = scheduler.with_pr_poller_registry(Arc::clone(registry));
    }
    if let Some(github) = deps.github_service.as_ref() {
        scheduler = scheduler.with_github_service(Arc::clone(github));
    }
    scheduler
}
