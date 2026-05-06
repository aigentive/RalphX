// Repository traits - domain layer abstractions for data persistence
// These traits define the contract; implementations live in infrastructure layer

pub mod active_plan_repository;
pub mod activity_event_repository;
pub mod agent_lane_settings_repository;
pub mod agent_conversation_workspace_repository;
pub mod agent_profile_repository;
pub mod agent_model_registry_repository;
pub mod agent_run_repository;
pub mod api_key_repository;
pub mod app_state_repository;
pub mod artifact_bucket_repository;
pub mod artifact_flow_repository;
pub mod artifact_repository;
pub mod chat_attachment_repository;
pub mod chat_conversation_repository;
pub mod chat_message_repository;
pub mod delegated_session_repository;
pub mod execution_plan_repository;
pub mod external_events_repository;
pub mod execution_settings_repository;
pub mod ideation_session_repository;
pub mod ideation_effort_settings_repository;
pub mod ideation_model_settings_repository;
pub mod ideation_settings_repository;
pub mod memory_archive_job_repository;
pub mod memory_archive_repository;
pub mod memory_entry_repository;
pub mod memory_event_repository;
pub mod methodology_repo;
pub mod plan_branch_repository;
pub mod plan_selection_stats_repository;
pub mod process_repo;
pub mod project_repository;
pub mod proposal_dependency_repository;
pub mod review_repository;
pub mod review_settings_repository;
pub mod session_link_repository;
pub mod status_transition;
pub mod task_dependency_repository;
pub mod task_proposal_repository;
pub mod task_qa_repository;
pub mod task_repository;
pub mod task_step_repository;
pub mod team_message_repository;
pub mod team_session_repository;
pub mod webhook_registration_repository;
pub mod workflow_repository;

pub use active_plan_repository::ActivePlanRepository;
pub use activity_event_repository::{
    ActivityEventFilter, ActivityEventPage, ActivityEventRepository,
};
pub use agent_lane_settings_repository::AgentLaneSettingsRepository;
pub use agent_conversation_workspace_repository::AgentConversationWorkspaceRepository;
pub use agent_model_registry_repository::AgentModelRegistryRepository;
pub use agent_profile_repository::{AgentProfileId, AgentProfileRepository};
pub use agent_run_repository::{AgentRunRepository, ORPHANED_AGENT_RUN_ON_APP_RESTART};
pub use api_key_repository::{ApiKeyRepository, CreateKeyParams, RotateKeyParams};
pub use app_state_repository::AppStateRepository;
pub use artifact_bucket_repository::ArtifactBucketRepository;
pub use artifact_flow_repository::ArtifactFlowRepository;
pub use artifact_repository::{ArtifactRepository, ArtifactVersionSummary};
pub use chat_attachment_repository::ChatAttachmentRepository;
pub use chat_conversation_repository::{ChatConversationPage, ChatConversationRepository};
pub use chat_message_repository::ChatMessageRepository;
pub use delegated_session_repository::DelegatedSessionRepository;
pub use execution_plan_repository::ExecutionPlanRepository;
pub use execution_settings_repository::{
    ExecutionSettingsRepository, GlobalExecutionSettingsRepository,
};
pub use external_events_repository::{ExternalEventRecord, ExternalEventsRepository};
pub use ideation_session_repository::{
    IdeationSessionRepository, IdeationSessionWithProgress, SessionGroupCounts, SessionProgress,
};
pub use ideation_effort_settings_repository::IdeationEffortSettingsRepository;
pub use ideation_model_settings_repository::IdeationModelSettingsRepository;
pub use ideation_settings_repository::IdeationSettingsRepository;
pub use memory_archive_job_repository::MemoryArchiveJobRepository;
pub use memory_archive_repository::MemoryArchiveRepository;
pub use memory_entry_repository::MemoryEntryRepository;
pub use memory_event_repository::MemoryEventRepository;
pub use methodology_repo::MethodologyRepository;
pub use plan_branch_repository::PlanBranchRepository;
pub use plan_selection_stats_repository::PlanSelectionStatsRepository;
pub use process_repo::ProcessRepository;
pub use project_repository::ProjectRepository;
pub use proposal_dependency_repository::ProposalDependencyRepository;
pub use review_repository::ReviewRepository;
pub use review_settings_repository::ReviewSettingsRepository;
pub use session_link_repository::SessionLinkRepository;
pub use status_transition::StatusTransition;
pub use task_dependency_repository::TaskDependencyRepository;
pub use task_proposal_repository::TaskProposalRepository;
pub use task_qa_repository::TaskQARepository;
pub use task_repository::{StateHistoryMetadata, TaskRepository};
pub use task_step_repository::TaskStepRepository;
pub use team_message_repository::TeamMessageRepository;
pub use team_session_repository::TeamSessionRepository;
pub use webhook_registration_repository::{WebhookRegistration, WebhookRegistrationRepository};
pub use workflow_repository::WorkflowRepository;
