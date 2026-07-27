use std::sync::Arc;
use std::time::Instant;

use tauri::{AppHandle, Manager, Runtime};

use crate::application::chat_service::{AppChatService, ChatService, StreamingStateCache};
use crate::application::manual_role_default_service::ManualRoleDefaultService;
use crate::application::notification_service::NotificationService;
use crate::application::{
    AgentClientBundle, AppState, AtlassianIntegrationService, ClickUpIntegrationService,
    GranolaIntegrationService, InteractiveProcessRegistry, LinearIntegrationService,
    PrPollerRegistry, TaskSchedulerService, TaskTransitionService,
};
use crate::commands::ExecutionState;
use crate::domain::repositories::{
    ActivityEventRepository, AgentConversationGranolaNoteRepository,
    AgentConversationJiraIssueRepository, AgentConversationLinearIssueRepository,
    AgentConversationWorkspaceRepository, AgentLaneSettingsRepository,
    AgentProviderSettingsRepository, AgentRunRepository, ArtifactRepository,
    AutomationRunRepository, BranchUpdateRepository, ChatAttachmentRepository,
    ChatConversationRepository, ChatMessageRepository, ChatTimelineRepository,
    ConversationFolderReferenceRepository, DelegatedSessionRepository, ExecutionPlanRepository,
    ExecutionSettingsRepository, ExternalEventsRepository, IdeationEffortSettingsRepository,
    IdeationModelSettingsRepository, IdeationSessionRepository, MemoryEventRepository,
    PersonaRepository, PlanBranchRepository, ProjectRepository, QueuedMessageRepository,
    ReviewRepository, TaskDependencyRepository, TaskProposalRepository, TaskRepository,
    TaskStepRepository, ValidationRunRepository,
};
use crate::domain::services::{
    GithubServiceTrait, MessageQueue, PlanPrDescriptionDrafter, RunningAgentRegistry,
};
use crate::domain::state_machine::services::WebhookPublisher;
use crate::infrastructure::memory::MemoryDelegatedSessionRepository;

#[derive(Clone)]
pub(crate) struct RuntimeFactoryDeps {
    pub task_repo: Arc<dyn TaskRepository>,
    pub task_step_repo: Option<Arc<dyn TaskStepRepository>>,
    pub validation_run_repo: Option<Arc<dyn ValidationRunRepository>>,
    pub external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,
    pub webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    pub branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
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
    pub manual_role_default_service: Option<Arc<ManualRoleDefaultService>>,
    pub review_repo: Option<Arc<dyn ReviewRepository>>,
    pub plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    pub agent_conversation_workspace_repo: Option<Arc<dyn AgentConversationWorkspaceRepository>>,
    pub interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    pub github_service: Option<Arc<dyn GithubServiceTrait>>,
    pub pr_poller_registry: Option<Arc<PrPollerRegistry>>,
    pub plan_pr_description_drafter: Option<Arc<dyn PlanPrDescriptionDrafter>>,
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
            task_step_repo: None,
            validation_run_repo: None,
            external_events_repo: None,
            webhook_publisher: None,
            branch_update_repo: None,
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
            manual_role_default_service: None,
            review_repo: None,
            plan_branch_repo: None,
            agent_conversation_workspace_repo: None,
            interactive_process_registry: None,
            github_service: None,
            pr_poller_registry: None,
            plan_pr_description_drafter: None,
        }
    }

    pub(crate) fn with_agent_clients(mut self, agent_clients: Option<AgentClientBundle>) -> Self {
        self.agent_clients = agent_clients;
        self
    }

    pub(crate) fn with_branch_update_repo(
        mut self,
        repository: Arc<dyn BranchUpdateRepository>,
    ) -> Self {
        self.branch_update_repo = Some(repository);
        self
    }

    pub(crate) fn with_execution_plan_repo(
        mut self,
        execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    ) -> Self {
        self.execution_plan_repo = Some(execution_plan_repo);
        self
    }

    pub(crate) fn with_completion_authority_repositories(
        mut self,
        task_step_repo: Option<Arc<dyn TaskStepRepository>>,
        validation_run_repo: Option<Arc<dyn ValidationRunRepository>>,
    ) -> Self {
        self.task_step_repo = task_step_repo;
        self.validation_run_repo = validation_run_repo;
        self
    }

    pub(crate) fn with_completion_event_delivery(
        mut self,
        external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,
        webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    ) -> Self {
        self.external_events_repo = external_events_repo;
        self.webhook_publisher = webhook_publisher;
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

    pub(crate) fn with_manual_role_default_service(
        mut self,
        service: Arc<ManualRoleDefaultService>,
    ) -> Self {
        self.manual_role_default_service = Some(service);
        self
    }

    pub(crate) fn with_plan_pr_description_drafter(
        mut self,
        drafter: Arc<dyn PlanPrDescriptionDrafter>,
    ) -> Self {
        self.plan_pr_description_drafter = Some(drafter);
        self
    }

    pub(crate) fn with_agent_conversation_workspace_repo(
        mut self,
        repo: Option<Arc<dyn AgentConversationWorkspaceRepository>>,
    ) -> Self {
        self.agent_conversation_workspace_repo = repo;
        self
    }

    pub(crate) fn from_app_state(state: &AppState) -> Self {
        let started_at = Instant::now();
        let deps = Self::from_core(
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
        .with_branch_update_repo(Arc::clone(&state.branch_update_repo))
        .with_execution_plan_repo(Arc::clone(&state.execution_plan_repo))
        .with_completion_authority_repositories(
            Some(Arc::clone(&state.task_step_repo)),
            Some(Arc::clone(&state.validation_run_repo)),
        )
        .with_completion_event_delivery(
            Some(Arc::clone(&state.external_events_repo)),
            state.webhook_publisher.as_ref().map(Arc::clone),
        )
        .with_review_repo(Arc::clone(&state.review_repo))
        .with_manual_role_default_service(Arc::new(state.manual_role_default_service()))
        .with_runtime_support(
            Some(Arc::clone(&state.execution_settings_repo)),
            Some(Arc::clone(&state.agent_lane_settings_repo)),
            Some(Arc::clone(&state.agent_provider_settings_repo)),
            Some(Arc::clone(&state.plan_branch_repo)),
            Some(Arc::clone(&state.interactive_process_registry)),
        )
        .with_agent_conversation_workspace_repo(Some(Arc::clone(
            &state.agent_conversation_workspace_repo,
        )))
        .with_github_runtime_support(
            state.github_service.as_ref().map(Arc::clone),
            Some(Arc::clone(&state.pr_poller_registry)),
        )
        .with_plan_pr_description_drafter(
            crate::application::plan_pr_description::build_app_state_plan_pr_description_drafter(
                Arc::clone(&state.agent_conversation_workspace_repo),
                Arc::clone(&state.chat_conversation_repo),
                Arc::clone(&state.agent_provider_settings_repo),
                Arc::new(state.manual_role_default_service()),
                state.agent_clients.clone(),
            ),
        );
        tracing::info!(
            elapsed_ms = started_at.elapsed().as_millis(),
            "Runtime factory deps loaded from AppState"
        );
        deps
    }
}

#[derive(Clone)]
pub(crate) struct ChatRuntimeFactoryDeps {
    pub chat_message_repo: Arc<dyn ChatMessageRepository>,
    pub chat_timeline_repo: Option<Arc<dyn ChatTimelineRepository>>,
    pub chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    pub conversation_folder_reference_repo: Option<Arc<dyn ConversationFolderReferenceRepository>>,
    pub folder_reference_app_data_dir: Option<std::path::PathBuf>,
    pub artifact_repo: Arc<dyn ArtifactRepository>,
    pub conversation_repo: Arc<dyn ChatConversationRepository>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub automation_run_repo: Arc<dyn AutomationRunRepository>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    pub ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    pub persona_repo: Option<Arc<dyn PersonaRepository>>,
    pub delegated_session_repo: Option<Arc<dyn DelegatedSessionRepository>>,
    pub activity_event_repo: Arc<dyn ActivityEventRepository>,
    pub message_queue: Arc<MessageQueue>,
    pub queued_message_repo: Option<Arc<dyn QueuedMessageRepository>>,
    pub running_agent_registry: Arc<dyn RunningAgentRegistry>,
    pub memory_event_repo: Arc<dyn MemoryEventRepository>,
    pub notification_service: Option<Arc<NotificationService>>,
    pub execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    pub agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    pub agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    pub manual_role_default_service: Option<Arc<ManualRoleDefaultService>>,
    pub ideation_effort_settings_repo: Option<Arc<dyn IdeationEffortSettingsRepository>>,
    pub ideation_model_settings_repo: Option<Arc<dyn IdeationModelSettingsRepository>>,
    pub agent_conversation_workspace_repo: Option<Arc<dyn AgentConversationWorkspaceRepository>>,
    pub agent_conversation_jira_issue_repo: Option<Arc<dyn AgentConversationJiraIssueRepository>>,
    pub agent_conversation_linear_issue_repo:
        Option<Arc<dyn AgentConversationLinearIssueRepository>>,
    pub agent_conversation_granola_note_repo:
        Option<Arc<dyn AgentConversationGranolaNoteRepository>>,
    pub plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    pub branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
    pub task_proposal_repo: Option<Arc<dyn TaskProposalRepository>>,
    pub task_step_repo: Option<Arc<dyn TaskStepRepository>>,
    pub validation_run_repo: Option<Arc<dyn ValidationRunRepository>>,
    pub external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,
    pub webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    pub review_repo: Option<Arc<dyn ReviewRepository>>,
    pub interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    pub streaming_state_cache: Option<StreamingStateCache>,
    pub atlassian_integration_service: Option<Arc<AtlassianIntegrationService>>,
    pub linear_integration_service: Option<Arc<LinearIntegrationService>>,
    pub granola_integration_service: Option<Arc<GranolaIntegrationService>>,
    pub clickup_integration_service: Option<Arc<ClickUpIntegrationService>>,
    pub mcp_policy_service: Option<crate::application::mcp_policy_service::McpPolicyService>,
}

impl ChatRuntimeFactoryDeps {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_core(
        chat_message_repo: Arc<dyn ChatMessageRepository>,
        chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
        artifact_repo: Arc<dyn ArtifactRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        automation_run_repo: Arc<dyn AutomationRunRepository>,
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
            chat_timeline_repo: None,
            chat_attachment_repo,
            conversation_folder_reference_repo: None,
            folder_reference_app_data_dir: None,
            artifact_repo,
            conversation_repo,
            agent_run_repo,
            automation_run_repo,
            project_repo,
            task_repo,
            task_dependency_repo,
            ideation_session_repo,
            persona_repo: None,
            delegated_session_repo: None,
            activity_event_repo,
            message_queue,
            queued_message_repo: None,
            running_agent_registry,
            memory_event_repo,
            notification_service: None,
            execution_settings_repo: None,
            agent_lane_settings_repo: None,
            agent_provider_settings_repo: None,
            manual_role_default_service: None,
            ideation_effort_settings_repo: None,
            ideation_model_settings_repo: None,
            agent_conversation_workspace_repo: None,
            agent_conversation_jira_issue_repo: None,
            agent_conversation_linear_issue_repo: None,
            agent_conversation_granola_note_repo: None,
            plan_branch_repo: None,
            branch_update_repo: None,
            task_proposal_repo: None,
            task_step_repo: None,
            validation_run_repo: None,
            external_events_repo: None,
            webhook_publisher: None,
            review_repo: None,
            interactive_process_registry: None,
            streaming_state_cache: None,
            atlassian_integration_service: None,
            linear_integration_service: None,
            granola_integration_service: None,
            clickup_integration_service: None,
            mcp_policy_service: None,
        }
    }

    pub(crate) fn with_persona_repo(mut self, repo: Arc<dyn PersonaRepository>) -> Self {
        self.persona_repo = Some(repo);
        self
    }

    pub(crate) fn with_conversation_folder_reference_context(
        mut self,
        repo: Arc<dyn ConversationFolderReferenceRepository>,
        app_data_dir: std::path::PathBuf,
    ) -> Self {
        self.conversation_folder_reference_repo = Some(repo);
        self.folder_reference_app_data_dir = Some(app_data_dir);
        self
    }

    pub(crate) fn with_execution_settings_repo(
        mut self,
        repo: Arc<dyn ExecutionSettingsRepository>,
    ) -> Self {
        self.execution_settings_repo = Some(repo);
        self
    }

    pub(crate) fn with_chat_timeline_repo(mut self, repo: Arc<dyn ChatTimelineRepository>) -> Self {
        self.chat_timeline_repo = Some(repo);
        self
    }

    pub(crate) fn with_queued_message_repo(
        mut self,
        repo: Arc<dyn QueuedMessageRepository>,
    ) -> Self {
        self.queued_message_repo = Some(repo);
        self
    }

    pub(crate) fn with_notification_service(mut self, service: Arc<NotificationService>) -> Self {
        self.notification_service = Some(service);
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

    pub(crate) fn with_manual_role_default_service(
        mut self,
        service: Arc<ManualRoleDefaultService>,
    ) -> Self {
        self.manual_role_default_service = Some(service);
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

    pub(crate) fn with_agent_conversation_jira_issue_repo(
        mut self,
        repo: Option<Arc<dyn AgentConversationJiraIssueRepository>>,
    ) -> Self {
        self.agent_conversation_jira_issue_repo = repo;
        self
    }

    pub(crate) fn with_agent_conversation_linear_issue_repo(
        mut self,
        repo: Option<Arc<dyn AgentConversationLinearIssueRepository>>,
    ) -> Self {
        self.agent_conversation_linear_issue_repo = repo;
        self
    }

    pub(crate) fn with_agent_conversation_granola_note_repo(
        mut self,
        repo: Option<Arc<dyn AgentConversationGranolaNoteRepository>>,
    ) -> Self {
        self.agent_conversation_granola_note_repo = repo;
        self
    }

    pub(crate) fn with_plan_branch_repo(mut self, repo: Arc<dyn PlanBranchRepository>) -> Self {
        self.plan_branch_repo = Some(repo);
        self
    }

    pub(crate) fn with_branch_update_repo(mut self, repo: Arc<dyn BranchUpdateRepository>) -> Self {
        self.branch_update_repo = Some(repo);
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

    pub(crate) fn with_validation_run_repo(
        mut self,
        repo: Arc<dyn ValidationRunRepository>,
    ) -> Self {
        self.validation_run_repo = Some(repo);
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

    pub(crate) fn with_atlassian_integration_service(
        mut self,
        service: Arc<AtlassianIntegrationService>,
    ) -> Self {
        self.atlassian_integration_service = Some(service);
        self
    }

    pub(crate) fn with_linear_integration_service(
        mut self,
        service: Arc<LinearIntegrationService>,
    ) -> Self {
        self.linear_integration_service = Some(service);
        self
    }

    pub(crate) fn with_granola_integration_service(
        mut self,
        service: Arc<GranolaIntegrationService>,
    ) -> Self {
        self.granola_integration_service = Some(service);
        self
    }

    pub(crate) fn with_clickup_integration_service(
        mut self,
        service: Arc<ClickUpIntegrationService>,
    ) -> Self {
        self.clickup_integration_service = Some(service);
        self
    }

    pub(crate) fn with_integration_reference_services_from_app_state(
        self,
        state: &AppState,
    ) -> Self {
        self.with_atlassian_integration_service(Arc::clone(&state.atlassian_integration_service))
            .with_linear_integration_service(Arc::clone(&state.linear_integration_service))
            .with_granola_integration_service(Arc::clone(&state.granola_integration_service))
            .with_clickup_integration_service(Arc::clone(&state.clickup_integration_service))
    }

    pub(crate) fn with_mcp_policy_service(
        mut self,
        service: crate::application::mcp_policy_service::McpPolicyService,
    ) -> Self {
        self.mcp_policy_service = Some(service);
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
        validation_run_repo: Option<Arc<dyn ValidationRunRepository>>,
        review_repo: Option<Arc<dyn ReviewRepository>>,
        streaming_state_cache: Option<StreamingStateCache>,
    ) -> Self {
        if let Some(repo) = task_proposal_repo {
            self = self.with_task_proposal_repo(repo);
        }
        if let Some(repo) = task_step_repo {
            self = self.with_task_step_repo(repo);
        }
        if let Some(repo) = validation_run_repo {
            self = self.with_validation_run_repo(repo);
        }
        if let Some(repo) = review_repo {
            self = self.with_review_repo(repo);
        }
        if let Some(cache) = streaming_state_cache {
            self = self.with_streaming_state_cache(cache);
        }
        self
    }

    pub(crate) fn with_completion_event_delivery(
        mut self,
        external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,
        webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    ) -> Self {
        self.external_events_repo = external_events_repo;
        self.webhook_publisher = webhook_publisher;
        self
    }

    pub(crate) fn from_app_state(state: &AppState) -> Self {
        Self::from_core(
            Arc::clone(&state.chat_message_repo),
            Arc::clone(&state.chat_attachment_repo),
            Arc::clone(&state.artifact_repo),
            Arc::clone(&state.chat_conversation_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.automation_run_repo),
            Arc::clone(&state.project_repo),
            Arc::clone(&state.task_repo),
            Arc::clone(&state.task_dependency_repo),
            Arc::clone(&state.ideation_session_repo),
            Arc::clone(&state.activity_event_repo),
            Arc::clone(&state.message_queue),
            Arc::clone(&state.running_agent_registry),
            Arc::clone(&state.memory_event_repo),
        )
        .with_chat_timeline_repo(Arc::clone(&state.chat_timeline_repo))
        .with_queued_message_repo(Arc::clone(&state.queued_message_repo))
        .with_notification_service(state.notification_service())
        .with_delegated_session_repo(Arc::clone(&state.delegated_session_repo))
        .with_persona_repo(Arc::clone(&state.persona_repo))
        .with_conversation_folder_reference_context(
            Arc::clone(&state.conversation_folder_reference_repo),
            state.app_paths.app_data_dir().to_path_buf(),
        )
        .with_manual_role_default_service(Arc::new(state.manual_role_default_service()))
        .with_branch_update_repo(Arc::clone(&state.branch_update_repo))
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
        .with_agent_conversation_jira_issue_repo(Some(Arc::clone(
            &state.agent_conversation_jira_issue_repo,
        )))
        .with_agent_conversation_linear_issue_repo(Some(Arc::clone(
            &state.agent_conversation_linear_issue_repo,
        )))
        .with_agent_conversation_granola_note_repo(Some(Arc::clone(
            &state.agent_conversation_granola_note_repo,
        )))
        .with_chat_context_support(
            Some(Arc::clone(&state.task_proposal_repo)),
            Some(Arc::clone(&state.task_step_repo)),
            Some(Arc::clone(&state.validation_run_repo)),
            Some(Arc::clone(&state.review_repo)),
            Some(state.streaming_state_cache.clone()),
        )
        .with_completion_event_delivery(
            Some(Arc::clone(&state.external_events_repo)),
            state.webhook_publisher.as_ref().map(Arc::clone),
        )
        .with_integration_reference_services_from_app_state(state)
        .with_mcp_policy_service(state.mcp_policy_service())
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

    if let Some(repo) = deps.persona_repo.as_ref() {
        service = service.with_persona_repo(Arc::clone(repo));
    }
    if let (Some(repo), Some(app_data_dir)) = (
        deps.conversation_folder_reference_repo.as_ref(),
        deps.folder_reference_app_data_dir.as_ref(),
    ) {
        service = service
            .with_conversation_folder_reference_context(Arc::clone(repo), app_data_dir.clone());
    }
    if let Some(state) = execution_state {
        service = service.with_execution_state(state);
    }
    if let Some(repo) = deps.chat_timeline_repo.as_ref() {
        service = service.with_chat_timeline_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.queued_message_repo.as_ref() {
        service = service.with_queued_message_repo(Arc::clone(repo));
    }
    if let Some(notification_service) = deps.notification_service.as_ref() {
        service = service.with_notification_service(Arc::clone(notification_service));
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
    if let Some(defaults) = deps.manual_role_default_service.as_ref() {
        service = service.with_manual_role_default_service(Arc::clone(defaults));
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
    if let Some(repo) = deps.agent_conversation_jira_issue_repo.as_ref() {
        service = service.with_agent_conversation_jira_issue_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.agent_conversation_linear_issue_repo.as_ref() {
        service = service.with_agent_conversation_linear_issue_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.agent_conversation_granola_note_repo.as_ref() {
        service = service.with_agent_conversation_granola_note_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.plan_branch_repo.as_ref() {
        service = service.with_plan_branch_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.branch_update_repo.as_ref() {
        service.set_branch_update_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.task_proposal_repo.as_ref() {
        service = service.with_task_proposal_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.task_step_repo.as_ref() {
        service = service.with_task_step_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.validation_run_repo.as_ref() {
        service = service.with_validation_run_repo(Arc::clone(repo));
    }
    service = service.with_completion_event_delivery(
        deps.external_events_repo.as_ref().map(Arc::clone),
        deps.webhook_publisher.as_ref().map(Arc::clone),
    );
    if let Some(repo) = deps.review_repo.as_ref() {
        service = service.with_review_repo(Arc::clone(repo));
    }
    if let Some(ipr) = deps.interactive_process_registry.as_ref() {
        service = service.with_interactive_process_registry(Arc::clone(ipr));
    }
    if let Some(cache) = deps.streaming_state_cache.as_ref() {
        service = service.with_streaming_state_cache(cache.clone());
    }
    if let Some(atlassian) = deps.atlassian_integration_service.as_ref() {
        service = service.with_atlassian_integration_service(Arc::clone(atlassian));
    }
    if let Some(linear) = deps.linear_integration_service.as_ref() {
        service = service.with_linear_integration_service(Arc::clone(linear));
    }
    if let Some(granola) = deps.granola_integration_service.as_ref() {
        service = service.with_granola_integration_service(Arc::clone(granola));
    }
    if let Some(clickup) = deps.clickup_integration_service.as_ref() {
        service = service.with_clickup_integration_service(Arc::clone(clickup));
    }
    if let Some(policy_service) = deps.mcp_policy_service.as_ref() {
        service = service.with_mcp_policy_service(policy_service.clone());
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
) -> TaskTransitionService {
    let total_started_at = Instant::now();
    if let Some(handle) = app_handle {
        let lookup_started_at = Instant::now();
        let app_state = handle.try_state::<AppState>();
        tracing::info!(
            elapsed_ms = lookup_started_at.elapsed().as_millis(),
            app_state_available = app_state.is_some(),
            "Transition service AppState lookup completed"
        );
        if let Some(app_state) = app_state {
            let build_started_at = Instant::now();
            let service = app_state.build_transition_service_for_runtime(execution_state, None);
            tracing::info!(
                elapsed_ms = build_started_at.elapsed().as_millis(),
                total_elapsed_ms = total_started_at.elapsed().as_millis(),
                source = "app_state",
                "Transition service built with fallback"
            );
            return service;
        }
    }

    let build_started_at = Instant::now();
    let service = build_transition_service_from_deps(None, execution_state, deps);
    tracing::info!(
        elapsed_ms = build_started_at.elapsed().as_millis(),
        total_elapsed_ms = total_started_at.elapsed().as_millis(),
        source = "deps",
        "Transition service built with fallback"
    );
    service
}

#[cfg(test)]
#[path = "runtime_factory_tests.rs"]
mod runtime_factory_tests;

pub(crate) fn build_transition_service_from_deps(
    app_handle: Option<AppHandle>,
    execution_state: Arc<ExecutionState>,
    deps: &RuntimeFactoryDeps,
) -> TaskTransitionService {
    let new_started_at = Instant::now();
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
    tracing::info!(
        elapsed_ms = new_started_at.elapsed().as_millis(),
        "Transition service core constructor completed"
    );

    let runtime_started_at = Instant::now();
    service = service.with_runtime_resolution_context(
        deps.agent_clients.clone(),
        deps.execution_settings_repo.as_ref().map(Arc::clone),
        deps.agent_lane_settings_repo.as_ref().map(Arc::clone),
        deps.agent_provider_settings_repo.as_ref().map(Arc::clone),
        deps.manual_role_default_service.as_ref().map(Arc::clone),
        deps.plan_branch_repo.as_ref().map(Arc::clone),
        deps.interactive_process_registry.as_ref().map(Arc::clone),
    );
    tracing::info!(
        elapsed_ms = runtime_started_at.elapsed().as_millis(),
        "Transition service runtime context wiring completed"
    );

    if let Some(repo) = deps.review_repo.as_ref() {
        service = service.with_review_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.branch_update_repo.as_ref() {
        service = service.with_branch_update_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.agent_conversation_workspace_repo.as_ref() {
        service = service.with_agent_conversation_workspace_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.task_step_repo.as_ref() {
        service = service.with_step_repo(Arc::clone(repo));
    }
    if let Some(repo) = deps.validation_run_repo.as_ref() {
        service = service.with_validation_run_repo(Arc::clone(repo));
    }
    if let Some(publisher) = deps.webhook_publisher.as_ref() {
        service = service.with_webhook_publisher_for_emitter(Arc::clone(publisher));
    }
    if let Some(repo) = deps.external_events_repo.as_ref() {
        service = service.with_external_events_repo(Arc::clone(repo));
    }
    service = service.with_artifact_repo(Arc::clone(&deps.artifact_repo));
    if let Some(registry) = deps.pr_poller_registry.as_ref() {
        service = service.with_pr_poller_registry(Arc::clone(registry));
    }
    if let Some(github) = deps.github_service.as_ref() {
        service = service.with_github_service(Arc::clone(github));
    }
    if let Some(drafter) = deps.plan_pr_description_drafter.as_ref() {
        service = service.with_plan_pr_description_drafter(Arc::clone(drafter));
    }
    service
}

pub(crate) fn build_task_scheduler_with_fallback<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
    execution_state: Arc<ExecutionState>,
    deps: &RuntimeFactoryDeps,
) -> TaskSchedulerService {
    if let Some(handle) = app_handle {
        if let Some(app_state) = handle.try_state::<AppState>() {
            return app_state.build_task_scheduler_for_runtime(execution_state, None);
        }
    }

    build_task_scheduler_from_deps(None, execution_state, deps)
}

pub(crate) fn build_task_scheduler_from_deps(
    app_handle: Option<AppHandle>,
    execution_state: Arc<ExecutionState>,
    deps: &RuntimeFactoryDeps,
) -> TaskSchedulerService {
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
    scheduler = scheduler.with_completion_authority_repositories(
        deps.task_step_repo.as_ref().map(Arc::clone),
        deps.validation_run_repo.as_ref().map(Arc::clone),
    );
    scheduler = scheduler.with_completion_event_delivery(
        deps.external_events_repo.as_ref().map(Arc::clone),
        deps.webhook_publisher.as_ref().map(Arc::clone),
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
    if let Some(drafter) = deps.plan_pr_description_drafter.as_ref() {
        scheduler = scheduler.with_plan_pr_description_drafter(Arc::clone(drafter));
    }
    scheduler
}
