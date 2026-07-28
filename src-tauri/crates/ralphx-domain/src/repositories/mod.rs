// Repository traits - domain layer abstractions for data persistence
// These traits define the contract; implementations live in infrastructure layer

pub mod active_plan_repository;
pub mod activity_event_repository;
pub mod agent_conversation_granola_note_repository;
pub mod agent_conversation_issue_repository;
pub mod agent_conversation_jira_issue_repository;
pub mod agent_conversation_linear_issue_repository;
pub mod agent_conversation_workspace_repository;
#[cfg(test)]
mod agent_conversation_workspace_repository_tests;
pub mod agent_lane_settings_repository;
pub mod manual_role_default_repository;
pub mod mcp_policy_repository;
pub mod agent_model_registry_repository;
pub mod agent_profile_repository;
pub mod agent_provider_settings_repository;
pub mod agent_run_repository;
pub mod agent_task_repository;
pub mod agent_workflow_repository;
pub mod api_key_repository;
pub mod app_state_repository;
pub mod artifact_bucket_repository;
pub mod artifact_flow_repository;
pub mod artifact_repository;
pub mod automation_repository;
pub mod branch_update_repository;
pub mod automation_run_repository;
pub mod chat_attachment_repository;
pub mod chat_conversation_repository;
pub mod conversation_folder_reference_repository;
pub mod chat_message_repository;
pub mod chat_timeline_repository;
pub mod delegated_session_repository;
pub mod execution_plan_repository;
pub mod execution_settings_repository;
pub mod external_events_repository;
pub mod ideation_effort_settings_repository;
pub mod ideation_model_settings_repository;
pub mod ideation_session_repository;
pub mod ideation_settings_repository;
pub mod memory_archive_job_repository;
pub mod memory_archive_repository;
pub mod memory_entry_repository;
pub mod memory_event_repository;
pub mod methodology_repo;
pub mod notification_repository;
pub mod notification_settings_repository;
pub mod plan_artifact_approval_repository;
pub mod plan_branch_repository;
pub mod plan_selection_stats_repository;
pub mod persona_repository;
pub mod process_repo;
pub mod project_memory_settings_repository;
pub mod project_skill_settings_repository;
pub mod project_skill_evidence_batch_repository;
pub mod project_skill_repository;
pub mod project_repository;
pub mod proposal_dependency_repository;
pub mod review_repository;
pub mod review_settings_repository;
pub mod session_link_repository;
pub mod skill_usage_event_repository;
pub mod status_transition;
pub mod task_dependency_repository;
pub mod task_outcome_repository;
pub mod task_proposal_repository;
pub mod task_qa_repository;
pub mod task_repository;
pub mod task_step_repository;
pub mod ticket_canonical_branch_repository;
pub mod validation_run_repository;
pub mod webhook_registration_repository;
pub mod workflow_repository;
pub mod workspace_review_runtime_settings_repository;

pub use active_plan_repository::ActivePlanRepository;
pub use activity_event_repository::{
    ActivityEventFilter, ActivityEventPage, ActivityEventRepository,
};
pub use agent_conversation_granola_note_repository::AgentConversationGranolaNoteRepository;
pub use agent_conversation_issue_repository::{
    is_open_issue_status, AgentConversationIssueRepository,
};
pub use agent_conversation_jira_issue_repository::AgentConversationJiraIssueRepository;
pub use agent_conversation_linear_issue_repository::AgentConversationLinearIssueRepository;
pub use agent_conversation_workspace_repository::{
    AgentConversationWorkspaceRepository, AgentWorkspaceLocalCleanupClaim,
    AgentWorkspacePrReviewActionMutation, AgentWorkspacePrReviewStateTransition,
    AgentWorkspacePrTerminalSettlement, AgentWorkspaceRepairStateGuard,
    AgentWorkspaceRepairStateTransition,
};
pub use agent_lane_settings_repository::AgentLaneSettingsRepository;
pub use manual_role_default_repository::ManualRoleDefaultRepository;
pub use mcp_policy_repository::McpPolicyRepository;
pub use agent_model_registry_repository::AgentModelRegistryRepository;
pub use agent_profile_repository::{AgentProfileId, AgentProfileRepository};
pub use agent_provider_settings_repository::AgentProviderSettingsRepository;
pub use agent_run_repository::{AgentRunRepository, ORPHANED_AGENT_RUN_ON_APP_RESTART};
pub use agent_task_repository::{AgentTaskListOptions, AgentTaskRepository};
pub use agent_workflow_repository::AgentWorkflowRepository;
pub use api_key_repository::{ApiKeyRepository, CreateKeyParams, RotateKeyParams};
pub use app_state_repository::AppStateRepository;
pub use artifact_bucket_repository::ArtifactBucketRepository;
pub use artifact_flow_repository::ArtifactFlowRepository;
pub use artifact_repository::{ArtifactRepository, ArtifactVersionSummary};
pub use automation_repository::{
    AutomationConfigPatch, AutomationRepository, AutomationSettingsPatch,
};
pub use branch_update_repository::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, BeginGitMutation, BindBranchUpdateRun,
    BlockBranchUpdate, BranchUpdateActivation, BranchUpdateActivationOutcome,
    BranchUpdateCasOutcome, BranchUpdateRepository, ClaimBranchUpdateContinuation,
    CompleteBranchUpdateContinuation, CompleteGitMutation, GitAuthorityCasOutcome,
    MarkBranchUpdateResolving, PauseBranchUpdate, ResumeBranchUpdate, RetryBranchUpdate,
    CheckpointBranchUpdateResult, SettleBranchUpdateProgrammatic, StopBranchUpdate,
    TransferBranchUpdateTargetLease,
    UnbindBranchUpdateRun,
};
pub use automation_run_repository::{AutomationRunPublicationMetadata, AutomationRunRepository};
pub use chat_attachment_repository::ChatAttachmentRepository;
pub use chat_conversation_repository::{ChatConversationPage, ChatConversationRepository};
pub use conversation_folder_reference_repository::ConversationFolderReferenceRepository;
pub use chat_message_repository::ChatMessageRepository;
pub use chat_timeline_repository::ChatTimelineRepository;
pub use delegated_session_repository::DelegatedSessionRepository;
pub use execution_plan_repository::ExecutionPlanRepository;
pub use execution_settings_repository::{
    ExecutionSettingsRepository, GlobalExecutionSettingsRepository,
};
pub use external_events_repository::{ExternalEventRecord, ExternalEventsRepository};
pub use ideation_effort_settings_repository::IdeationEffortSettingsRepository;
pub use ideation_model_settings_repository::IdeationModelSettingsRepository;
pub use ideation_session_repository::{
    IdeationSessionRepository, IdeationSessionWithProgress, SessionGroupCounts, SessionProgress,
};
pub use ideation_settings_repository::IdeationSettingsRepository;
pub use memory_archive_job_repository::MemoryArchiveJobRepository;
pub use memory_archive_repository::MemoryArchiveRepository;
pub use memory_entry_repository::MemoryEntryRepository;
pub use memory_event_repository::MemoryEventRepository;
pub use methodology_repo::MethodologyRepository;
pub use notification_repository::{NotificationPage, NotificationRepository};
pub use notification_settings_repository::NotificationSettingsRepository;
pub use plan_artifact_approval_repository::{
    PlanApprovalActor, PlanArtifactApproval, PlanArtifactApprovalRepository,
};
pub use plan_branch_repository::PlanBranchRepository;
pub use plan_selection_stats_repository::PlanSelectionStatsRepository;
pub use persona_repository::PersonaRepository;
pub use process_repo::ProcessRepository;
pub use project_memory_settings_repository::ProjectMemorySettingsRepository;
pub use project_skill_settings_repository::ProjectSkillSettingsRepository;
pub use project_skill_evidence_batch_repository::ProjectSkillEvidenceBatchRepository;
pub use project_skill_repository::{
    ProjectSkillListOptions, ProjectSkillMatchedMutation, ProjectSkillRepository,
    ProjectSkillResolutionCommand, ProjectSkillResolutionIdentity,
    ProjectSkillResolutionIdentityKind, ProjectSkillResolutionIntent,
    ProjectSkillResolutionOutcome, ProjectSkillResolutionResult, ProjectSkillStagingPolicy,
};
pub use project_repository::ProjectRepository;
pub use proposal_dependency_repository::ProposalDependencyRepository;
pub use review_repository::ReviewRepository;
pub use review_settings_repository::ReviewSettingsRepository;
pub use session_link_repository::SessionLinkRepository;
pub use skill_usage_event_repository::{SkillUsageEventRepository, SkillUsageListOptions};
pub use status_transition::StatusTransition;
pub use task_dependency_repository::TaskDependencyRepository;
pub use task_outcome_repository::{
    canonical_terminal_pr_source_ref_id, resolve_task_outcome_upsert,
    terminal_pr_status_for_class, ResolvedTaskOutcomeUpsert, TaskOutcomeListOptions,
    TaskOutcomeRepository, UpsertTaskOutcomeInput, AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
    TERMINAL_PR_SOURCE_REF_KIND, WORKSPACE_PR_CLOSED_CLASS, WORKSPACE_PR_FAILED_CLASS,
    WORKSPACE_PR_MERGED_CLASS, WORKSPACE_PR_MERGED_CLEAN_CLASS,
    WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS, WORKSPACE_PR_TERMINAL_CLASS,
    WORKSPACE_PUBLISH_FAILED_CLASS, WORKSPACE_SESSION_ABANDONED_CLASS,
};
pub use task_proposal_repository::TaskProposalRepository;
pub use task_qa_repository::TaskQARepository;
pub use task_repository::{StateHistoryMetadata, TaskRepository};
pub use task_step_repository::TaskStepRepository;
pub use ticket_canonical_branch_repository::TicketCanonicalBranchRepository;
pub use validation_run_repository::ValidationRunRepository;
pub use webhook_registration_repository::{WebhookRegistration, WebhookRegistrationRepository};
pub use workflow_repository::WorkflowRepository;
pub use workspace_review_runtime_settings_repository::WorkspaceReviewRuntimeSettingsRepository;
