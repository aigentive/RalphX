// SQLite infrastructure layer
// Database connection management, migrations, and repository implementations

pub mod connection;
pub mod db_connection;
pub mod database_maintenance;
#[cfg(test)]
mod database_maintenance_tests;
pub mod migrations;
pub mod sqlite_active_plan_repo;
pub mod sqlite_activity_event_repo;
pub mod sqlite_agent_conversation_granola_note_repo;
pub mod sqlite_agent_conversation_issue_repo;
pub mod sqlite_agent_conversation_jira_issue_repo;
pub mod sqlite_agent_conversation_linear_issue_repo;
pub mod sqlite_agent_conversation_mute_repo;
pub mod sqlite_agent_conversation_workspace_repo;
#[cfg(test)]
mod sqlite_agent_conversation_workspace_repo_tests;
pub mod sqlite_agent_lane_settings_repo;
pub mod sqlite_agent_model_registry_repo;
pub mod sqlite_agent_profile_repo;
pub mod sqlite_agent_provider_settings_repo;
pub mod sqlite_agent_remote_agent_stop_request_repo;
pub mod sqlite_agent_remote_conversation_message_request_repo;
pub mod sqlite_agent_remote_conversation_mode_switch_request_repo;
pub mod sqlite_agent_remote_conversation_start_request_repo;
pub mod sqlite_agent_run_repo;
#[cfg(test)]
mod sqlite_agent_task_assignment_repo_tests;
pub mod sqlite_agent_task_repo;
#[cfg(test)]
mod sqlite_agent_task_repo_tests;
pub mod sqlite_agent_workflow_repo;
pub mod sqlite_team_repo;
pub(crate) mod sqlite_team_support;
pub mod sqlite_team_coordination_transition_repo;
pub mod sqlite_team_run_binding_repo;
pub mod sqlite_team_message_repo;
pub mod sqlite_team_wake_batch_repo;
pub mod sqlite_team_workspace_reservation_repo;
#[cfg(test)]
mod sqlite_team_repo_tests;
#[cfg(test)]
mod sqlite_team_coordination_transition_repo_tests;
#[cfg(test)]
mod sqlite_team_run_binding_repo_tests;
#[cfg(test)]
mod sqlite_team_message_repo_tests;
#[cfg(test)]
mod sqlite_team_wake_batch_repo_tests;
#[cfg(test)]
mod sqlite_team_workspace_reservation_repo_tests;
#[cfg(test)]
mod sqlite_agent_workflow_repo_tests;
pub mod sqlite_api_key_repo;
pub mod sqlite_app_state_repo;
#[cfg(test)]
mod sqlite_app_state_repo_tests;
pub mod sqlite_artifact_bucket_repo;
pub mod sqlite_artifact_flow_repo;
pub mod sqlite_artifact_repo;
pub mod sqlite_atlassian_integration_settings_repo;
pub mod sqlite_automation_repo;
#[cfg(test)]
mod sqlite_automation_repo_tests;
pub mod sqlite_branch_update_repo;
#[cfg(test)]
mod sqlite_branch_update_repo_tests;
pub mod sqlite_chat_attachment_repo;
#[cfg(test)]
mod sqlite_chat_attachment_repo_tests;
pub mod sqlite_chat_conversation_repo;
#[cfg(test)]
mod sqlite_chat_conversation_repo_tests;
pub mod sqlite_chat_message_repo;
#[cfg(test)]
mod sqlite_chat_message_repo_tests;
pub mod sqlite_chat_payload_retention_repo;
#[cfg(test)]
mod sqlite_chat_payload_retention_repo_tests;
pub mod sqlite_chat_timeline_repo;
#[cfg(test)]
mod sqlite_chat_timeline_repo_tests;
pub mod sqlite_clickup_integration_settings_repo;
#[cfg(test)]
mod sqlite_clickup_integration_settings_repo_tests;
pub mod sqlite_conversation_folder_reference_repo;
#[cfg(test)]
mod sqlite_conversation_folder_reference_repo_tests;
pub mod sqlite_delegated_session_repo;
#[cfg(test)]
mod sqlite_delegated_session_repo_tests;
pub mod sqlite_delegation_park_repo;
#[cfg(test)]
mod sqlite_delegation_park_repo_tests;
pub mod sqlite_execution_plan_repo;
#[cfg(test)]
mod sqlite_execution_plan_repo_tests;
pub mod sqlite_execution_settings_repo;
pub mod sqlite_external_events_repo;
#[cfg(test)]
mod sqlite_external_events_repo_tests;
pub mod sqlite_external_issue_link_repo;
pub mod sqlite_granola_integration_settings_repo;
#[cfg(test)]
mod sqlite_granola_integration_settings_repo_tests;
pub mod sqlite_ideation_effort_settings_repo;
pub mod sqlite_ideation_model_settings_repo;
pub mod sqlite_ideation_session_repo;
pub mod sqlite_ideation_settings_repo;
pub mod sqlite_linear_integration_settings_repo;
pub mod sqlite_linear_webhook_store;
pub mod sqlite_manual_role_default_repo;
pub mod sqlite_mcp_policy_repo;
pub mod sqlite_memory_archive_job_repository;
#[cfg(test)]
mod sqlite_memory_archive_job_repository_tests;
pub mod sqlite_memory_archive_repo;
pub mod sqlite_memory_entry_repo;
pub mod sqlite_memory_event_repository;
#[cfg(test)]
mod sqlite_memory_event_repository_tests;
pub mod sqlite_methodology_repo;
pub mod sqlite_notification_repo;
#[cfg(test)]
mod sqlite_notification_repo_tests;
pub mod sqlite_notification_settings_repo;
pub mod sqlite_orphan_worktree_cleanup_marker_repo;
#[cfg(test)]
mod sqlite_orphan_worktree_cleanup_marker_repo_tests;
pub mod sqlite_permission_repo;
pub mod sqlite_persona_repo;
#[cfg(test)]
mod sqlite_persona_repo_tests;
pub mod sqlite_plan_artifact_approval_repo;
#[cfg(test)]
mod sqlite_plan_artifact_approval_repo_tests;
pub mod sqlite_plan_branch_repo;
pub mod sqlite_plan_selection_stats_repo;
pub mod sqlite_process_repo;
pub mod sqlite_project_repo;
pub mod sqlite_proposal_dependency_repo;
pub mod sqlite_question_repo;
pub mod sqlite_queued_message_repo;
pub mod sqlite_remote_access_repo;
pub mod sqlite_remote_environment_repo;
pub mod sqlite_remote_event_log_repo;
pub mod sqlite_remote_request_dedup_repo;
pub mod sqlite_remote_resume_request_repo;
pub mod sqlite_remote_plan_approval_request_repo;
pub mod sqlite_remote_finalize_decision_request_repo;
pub mod sqlite_remote_automation_run_request_repo;
pub mod sqlite_remote_automation_draft_request_repo;
pub mod sqlite_remote_plan_edit_request_repo;
pub mod sqlite_remote_queued_send_request_repo;
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
pub mod sqlite_ticket_canonical_branch_repo;
pub mod sqlite_ticketing_status_catalog_repo;
pub mod sqlite_ui_feature_flag_overrides_repo;
#[cfg(test)]
mod sqlite_ui_feature_flag_overrides_repo_tests;
pub mod sqlite_validation_run_repo;
#[cfg(test)]
mod sqlite_validation_run_repo_tests;
pub mod sqlite_webhook_registration_repo;
pub mod sqlite_workflow_repo;
pub mod sqlite_workspace_review_runtime_settings_repo;
pub mod state_machine_repository;

// Re-export commonly used items
pub use connection::{
    get_app_data_db_path, get_default_db_path, open_connection, open_memory_connection,
};
pub use db_connection::DbConnection;
pub use migrations::{run_migrations, SCHEMA_VERSION};
pub use sqlite_active_plan_repo::SqliteActivePlanRepository;
pub use sqlite_activity_event_repo::SqliteActivityEventRepository;
pub use sqlite_agent_conversation_granola_note_repo::SqliteAgentConversationGranolaNoteRepository;
pub use sqlite_agent_conversation_issue_repo::SqliteAgentConversationIssueRepository;
pub use sqlite_agent_conversation_jira_issue_repo::SqliteAgentConversationJiraIssueRepository;
pub use sqlite_agent_conversation_linear_issue_repo::SqliteAgentConversationLinearIssueRepository;
pub use sqlite_agent_conversation_mute_repo::SqliteAgentConversationMuteRepository;
pub use sqlite_agent_conversation_workspace_repo::SqliteAgentConversationWorkspaceRepository;
pub use sqlite_agent_lane_settings_repo::SqliteAgentLaneSettingsRepository;
pub use sqlite_agent_remote_agent_stop_request_repo::SqliteRemoteAgentStopRequestRepository;
pub use sqlite_agent_remote_conversation_message_request_repo::SqliteRemoteConversationMessageRequestRepository;
pub use sqlite_agent_remote_conversation_mode_switch_request_repo::SqliteRemoteConversationModeSwitchRequestRepository;
pub use sqlite_agent_remote_conversation_start_request_repo::SqliteRemoteConversationStartRequestRepository;
pub use sqlite_manual_role_default_repo::SqliteManualRoleDefaultRepository;
pub use sqlite_mcp_policy_repo::SqliteMcpPolicyRepository;

#[cfg(test)]
mod sqlite_mcp_policy_repo_tests;
pub use sqlite_agent_model_registry_repo::SqliteAgentModelRegistryRepository;
pub use sqlite_agent_profile_repo::SqliteAgentProfileRepository;
pub use sqlite_agent_provider_settings_repo::SqliteAgentProviderSettingsRepository;
pub use sqlite_agent_run_repo::SqliteAgentRunRepository;
pub use sqlite_agent_task_repo::SqliteAgentTaskRepository;
pub use sqlite_agent_workflow_repo::SqliteAgentWorkflowRepository;
pub use sqlite_team_repo::SqliteTeamRepository;
pub use sqlite_team_coordination_transition_repo::SqliteTeamCoordinationTransitionRepository;
pub use sqlite_team_run_binding_repo::SqliteTeamRunBindingRepository;
pub use sqlite_team_message_repo::SqliteTeamMessageRepository;
pub use sqlite_team_wake_batch_repo::SqliteTeamWakeBatchRepository;
pub use sqlite_team_workspace_reservation_repo::SqliteTeamWorkspaceReservationRepository;
pub use sqlite_api_key_repo::SqliteApiKeyRepository;
pub use sqlite_app_state_repo::SqliteAppStateRepository;
pub use sqlite_artifact_bucket_repo::SqliteArtifactBucketRepository;
pub use sqlite_artifact_flow_repo::SqliteArtifactFlowRepository;
pub use sqlite_artifact_repo::SqliteArtifactRepository;
pub use sqlite_atlassian_integration_settings_repo::SqliteAtlassianIntegrationSettingsRepository;
pub use sqlite_automation_repo::{SqliteAutomationRepository, SqliteAutomationRunRepository};
pub use sqlite_branch_update_repo::SqliteBranchUpdateRepository;
pub use sqlite_chat_attachment_repo::SqliteChatAttachmentRepository;
pub use sqlite_chat_conversation_repo::SqliteChatConversationRepository;
pub use sqlite_chat_message_repo::SqliteChatMessageRepository;
pub use sqlite_chat_timeline_repo::SqliteChatTimelineRepository;
pub use sqlite_clickup_integration_settings_repo::SqliteClickUpIntegrationSettingsRepository;
pub use sqlite_conversation_folder_reference_repo::SqliteConversationFolderReferenceRepository;
pub use sqlite_delegated_session_repo::SqliteDelegatedSessionRepository;
pub use sqlite_delegation_park_repo::SqliteDelegationParkRepo;
pub use sqlite_execution_plan_repo::SqliteExecutionPlanRepository;
pub use sqlite_execution_settings_repo::{
    SqliteExecutionSettingsRepository, SqliteGlobalExecutionSettingsRepository,
};
pub use sqlite_external_events_repo::SqliteExternalEventsRepository;
pub use sqlite_external_issue_link_repo::SqliteExternalIssueLinkRepository;
pub use sqlite_granola_integration_settings_repo::SqliteGranolaIntegrationSettingsRepository;
pub use sqlite_ideation_effort_settings_repo::SqliteIdeationEffortSettingsRepository;
pub use sqlite_ideation_model_settings_repo::SqliteIdeationModelSettingsRepository;
pub use sqlite_ideation_session_repo::SqliteIdeationSessionRepository;
pub use sqlite_ideation_settings_repo::SqliteIdeationSettingsRepository;
pub use sqlite_linear_integration_settings_repo::SqliteLinearIntegrationSettingsRepository;
pub use sqlite_linear_webhook_store::SqliteLinearWebhookStore;
pub use sqlite_memory_archive_job_repository::SqliteMemoryArchiveJobRepository;
pub use sqlite_memory_archive_repo::SqliteMemoryArchiveRepository;
pub use sqlite_memory_entry_repo::SqliteMemoryEntryRepository;
pub use sqlite_memory_event_repository::SqliteMemoryEventRepository;
pub use sqlite_methodology_repo::SqliteMethodologyRepository;
pub use sqlite_notification_repo::SqliteNotificationRepository;
pub use sqlite_notification_settings_repo::SqliteNotificationSettingsRepository;
pub use sqlite_orphan_worktree_cleanup_marker_repo::SqliteOrphanWorktreeCleanupMarkerRepository;
pub use sqlite_permission_repo::SqlitePermissionRepository;
pub use sqlite_persona_repo::SqlitePersonaRepository;
pub use sqlite_plan_artifact_approval_repo::SqlitePlanArtifactApprovalRepository;
pub use sqlite_plan_branch_repo::SqlitePlanBranchRepository;
pub use sqlite_plan_selection_stats_repo::SqlitePlanSelectionStatsRepository;
pub use sqlite_process_repo::SqliteProcessRepository;
pub use sqlite_project_repo::SqliteProjectRepository;
pub use sqlite_proposal_dependency_repo::SqliteProposalDependencyRepository;
pub use sqlite_question_repo::SqliteQuestionRepository;
pub use sqlite_queued_message_repo::SqliteQueuedMessageRepository;
pub use sqlite_remote_access_repo::SqliteRemoteAccessRepository;
pub use sqlite_remote_environment_repo::SqliteRemoteEnvironmentRepository;
pub use sqlite_remote_event_log_repo::{
    RemoteEventLogStore, RemoteEventRow, RemotePruneOutcome, RemotePruneRequest,
    SqliteRemoteEventLogRepository,
};
pub use sqlite_remote_request_dedup_repo::SqliteRemoteRequestDedupRepository;
pub use sqlite_remote_resume_request_repo::{
    SqliteRemoteExecutionResumeRequestRepository, SqliteRemoteTaskActionRequestRepository,
};
pub use sqlite_remote_plan_approval_request_repo::SqliteRemotePlanApprovalRequestRepository;
pub use sqlite_remote_finalize_decision_request_repo::SqliteRemoteFinalizeDecisionRequestRepository;
pub use sqlite_remote_automation_run_request_repo::SqliteRemoteAutomationRunRequestRepository;
pub use sqlite_remote_automation_draft_request_repo::SqliteRemoteAutomationDraftRequestRepository;
pub use sqlite_remote_plan_edit_request_repo::SqliteRemotePlanEditRequestRepository;
pub use sqlite_remote_queued_send_request_repo::SqliteRemoteQueuedSendRequestRepository;
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
pub use sqlite_ticket_canonical_branch_repo::SqliteTicketCanonicalBranchRepository;
pub use sqlite_ticketing_status_catalog_repo::SqliteTicketingStatusCatalogRepository;
pub use sqlite_ui_feature_flag_overrides_repo::SqliteUiFeatureFlagOverridesRepository;
pub use sqlite_validation_run_repo::SqliteValidationRunRepository;
pub use sqlite_webhook_registration_repo::SqliteWebhookRegistrationRepository;
pub use sqlite_workflow_repo::SqliteWorkflowRepository;
pub use sqlite_workspace_review_runtime_settings_repo::SqliteWorkspaceReviewRuntimeSettingsRepository;
pub use state_machine_repository::TaskStateMachineRepository;
