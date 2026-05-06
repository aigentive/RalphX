// SQLite infrastructure layer
// Database connection management, migrations, and repository implementations

pub mod db_connection;
pub mod connection;
pub mod migrations;
pub mod sqlite_api_key_repo;
pub mod sqlite_active_plan_repo;
pub mod sqlite_activity_event_repo;
pub mod sqlite_agent_lane_settings_repo;
pub mod sqlite_agent_conversation_workspace_repo;
pub mod sqlite_agent_model_registry_repo;
pub mod sqlite_agent_profile_repo;
pub mod sqlite_agent_run_repo;
pub mod sqlite_app_state_repo;
pub mod sqlite_artifact_bucket_repo;
pub mod sqlite_artifact_flow_repo;
pub mod sqlite_artifact_repo;
pub mod sqlite_chat_attachment_repo;
#[cfg(test)]
mod sqlite_chat_attachment_repo_tests;
pub mod sqlite_chat_conversation_repo;
#[cfg(test)]
mod sqlite_chat_conversation_repo_tests;
pub mod sqlite_chat_message_repo;
#[cfg(test)]
mod sqlite_chat_message_repo_tests;
pub mod sqlite_delegated_session_repo;
#[cfg(test)]
mod sqlite_delegated_session_repo_tests;
pub mod sqlite_execution_plan_repo;
pub mod sqlite_external_events_repo;
#[cfg(test)]
mod sqlite_execution_plan_repo_tests;
pub mod sqlite_execution_settings_repo;
pub mod sqlite_ideation_effort_settings_repo;
pub mod sqlite_ideation_model_settings_repo;
pub mod sqlite_ideation_session_repo;
pub mod sqlite_ideation_settings_repo;
pub mod sqlite_memory_archive_job_repository;
#[cfg(test)]
mod sqlite_memory_archive_job_repository_tests;
pub mod sqlite_memory_archive_repo;
pub mod sqlite_memory_entry_repo;
pub mod sqlite_memory_event_repository;
#[cfg(test)]
mod sqlite_memory_event_repository_tests;
pub mod sqlite_methodology_repo;
pub mod sqlite_permission_repo;
pub mod sqlite_plan_branch_repo;
pub mod sqlite_plan_selection_stats_repo;
pub mod sqlite_process_repo;
pub mod sqlite_project_repo;
pub mod sqlite_proposal_dependency_repo;
pub mod sqlite_question_repo;
pub mod sqlite_review_issue_repo;
pub mod sqlite_review_repo;
pub mod sqlite_review_settings_repo;
pub mod sqlite_running_agent_registry;
pub mod sqlite_session_link_repo;
pub mod sqlite_task_dependency_repo;
pub mod sqlite_task_proposal_repo;
pub mod sqlite_task_qa_repo;
pub mod sqlite_task_repo;
pub mod sqlite_task_step_repo;
pub mod sqlite_team_message_repo;
#[cfg(test)]
mod sqlite_team_message_repo_tests;
pub mod sqlite_team_session_repo;
#[cfg(test)]
mod sqlite_team_session_repo_tests;
pub mod sqlite_webhook_registration_repo;
pub mod sqlite_workflow_repo;
pub mod state_machine_repository;

// Re-export commonly used items
pub use connection::{
    get_app_data_db_path, get_default_db_path, open_connection, open_memory_connection,
};
pub use db_connection::DbConnection;
pub use migrations::{run_migrations, SCHEMA_VERSION};
pub use sqlite_api_key_repo::SqliteApiKeyRepository;
pub use sqlite_active_plan_repo::SqliteActivePlanRepository;
pub use sqlite_activity_event_repo::SqliteActivityEventRepository;
pub use sqlite_agent_lane_settings_repo::SqliteAgentLaneSettingsRepository;
pub use sqlite_agent_conversation_workspace_repo::SqliteAgentConversationWorkspaceRepository;
pub use sqlite_agent_model_registry_repo::SqliteAgentModelRegistryRepository;
pub use sqlite_agent_profile_repo::SqliteAgentProfileRepository;
pub use sqlite_agent_run_repo::SqliteAgentRunRepository;
pub use sqlite_app_state_repo::SqliteAppStateRepository;
pub use sqlite_artifact_bucket_repo::SqliteArtifactBucketRepository;
pub use sqlite_artifact_flow_repo::SqliteArtifactFlowRepository;
pub use sqlite_artifact_repo::SqliteArtifactRepository;
pub use sqlite_chat_attachment_repo::SqliteChatAttachmentRepository;
pub use sqlite_chat_conversation_repo::SqliteChatConversationRepository;
pub use sqlite_chat_message_repo::SqliteChatMessageRepository;
pub use sqlite_delegated_session_repo::SqliteDelegatedSessionRepository;
pub use sqlite_execution_plan_repo::SqliteExecutionPlanRepository;
pub use sqlite_external_events_repo::SqliteExternalEventsRepository;
pub use sqlite_execution_settings_repo::{
    SqliteExecutionSettingsRepository, SqliteGlobalExecutionSettingsRepository,
};
pub use sqlite_ideation_effort_settings_repo::SqliteIdeationEffortSettingsRepository;
pub use sqlite_ideation_model_settings_repo::SqliteIdeationModelSettingsRepository;
pub use sqlite_ideation_session_repo::SqliteIdeationSessionRepository;
pub use sqlite_ideation_settings_repo::SqliteIdeationSettingsRepository;
pub use sqlite_memory_archive_job_repository::SqliteMemoryArchiveJobRepository;
pub use sqlite_memory_archive_repo::SqliteMemoryArchiveRepository;
pub use sqlite_memory_entry_repo::SqliteMemoryEntryRepository;
pub use sqlite_memory_event_repository::SqliteMemoryEventRepository;
pub use sqlite_methodology_repo::SqliteMethodologyRepository;
pub use sqlite_permission_repo::SqlitePermissionRepository;
pub use sqlite_plan_branch_repo::SqlitePlanBranchRepository;
pub use sqlite_plan_selection_stats_repo::SqlitePlanSelectionStatsRepository;
pub use sqlite_process_repo::SqliteProcessRepository;
pub use sqlite_project_repo::SqliteProjectRepository;
pub use sqlite_proposal_dependency_repo::SqliteProposalDependencyRepository;
pub use sqlite_question_repo::SqliteQuestionRepository;
pub use sqlite_review_issue_repo::{ReviewIssueRepository, SqliteReviewIssueRepository};
pub use sqlite_review_repo::SqliteReviewRepository;
pub use sqlite_review_settings_repo::SqliteReviewSettingsRepository;
pub use sqlite_running_agent_registry::SqliteRunningAgentRegistry;
pub use sqlite_session_link_repo::SqliteSessionLinkRepository;
pub use sqlite_task_dependency_repo::SqliteTaskDependencyRepository;
pub use sqlite_task_proposal_repo::SqliteTaskProposalRepository;
pub use sqlite_task_qa_repo::SqliteTaskQARepository;
pub use sqlite_task_repo::SqliteTaskRepository;
pub use sqlite_task_step_repo::SqliteTaskStepRepository;
pub use sqlite_team_message_repo::SqliteTeamMessageRepository;
pub use sqlite_team_session_repo::SqliteTeamSessionRepository;
pub use sqlite_webhook_registration_repo::SqliteWebhookRegistrationRepository;
pub use sqlite_workflow_repo::SqliteWorkflowRepository;
pub use state_machine_repository::TaskStateMachineRepository;
