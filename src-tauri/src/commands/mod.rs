// Tauri commands - thin layer bridging frontend to backend
// Commands should be minimal - delegate to domain/infrastructure

pub mod activity_commands;
pub mod agent_composer_commands;
pub mod agent_conversation_mute_commands;
#[cfg(test)]
mod agent_conversation_mute_commands_tests;
#[cfg(test)]
mod agent_workspace_dispatch_contract_tests;
pub mod agent_issue_report_commands;
#[cfg(test)]
mod agent_issue_report_commands_tests;
pub mod agent_model_commands;
pub mod agent_plan_commands;
#[cfg(test)]
mod agent_plan_commands_tests;
pub mod agent_profile_commands;
pub mod agent_sidebar_commands;
pub mod agent_terminal_commands;
pub(crate) mod agent_workspace_auto_publish;
#[cfg(test)]
mod agent_workspace_auto_publish_tests;
pub(crate) mod agent_workspace_auto_review;
#[cfg(test)]
mod agent_workspace_auto_review_tests;
pub mod api_key_commands;
pub mod artifact_commands;
pub mod atlassian_commands;
pub mod automation_commands;
#[cfg(test)]
mod automation_commands_tests;
pub mod branch_helpers;
pub mod chat_attachment_commands;
pub mod chat_responses;
pub mod clickup_commands;
#[cfg(test)]
mod clickup_commands_tests;
pub mod conversation_folder_reference_commands;
pub mod conversation_stats_commands;
pub mod database_maintenance_commands;
pub mod diagnostic_commands;
#[cfg(test)]
mod diagnostic_commands_tests;
pub mod diff_commands;
pub mod execution_commands;
pub(crate) mod execution_task_navigation;
pub mod external_mcp_commands;
pub mod git_commands;
pub mod github_commands;
#[cfg(test)]
mod github_commands_tests;
pub mod granola_commands;
#[cfg(test)]
mod granola_commands_tests;
pub mod harness_provider_commands;
pub mod health;
pub mod ideation_commands;
pub mod linear_commands;
pub mod manual_role_default_commands;
pub mod mcp_policy_commands;
#[cfg(test)]
mod mcp_policy_commands_tests;
pub mod merge_pipeline_commands;
pub mod methodology_commands;
pub mod metrics_commands;
pub(crate) mod metrics_pr_insights;
pub(crate) mod metrics_queries;
pub(crate) mod metrics_scope;
pub(crate) mod metrics_trends;
pub mod metrics_types;
pub mod notification_commands;
#[cfg(test)]
mod notification_commands_tests;
pub mod permission_commands;
pub mod persona_commands;
pub mod plan_branch_commands;
pub mod plan_commands;
pub mod project_commands;
#[cfg(test)]
mod project_commands_tests;
pub mod provider_cli_management_commands;
pub mod qa_commands;
pub mod question_commands;
#[cfg(test)]
mod question_commands_tests;
pub mod registry;
pub mod release_notes_commands;
pub mod remote_agent_stop_commands;
pub mod remote_chat_commands;
#[cfg(test)]
mod remote_chat_commands_tests;
pub mod remote_conversation_message_commands;
pub mod remote_conversation_mode_switch_commands;
pub mod remote_conversation_start_commands;
pub mod remote_device_commands;
pub mod remote_environment_commands;
pub mod remote_execution_settings_commands;
pub mod remote_execution_status_commands;
#[cfg(test)]
mod remote_execution_status_commands_tests;
pub mod remote_host_commands;
pub mod remote_question_commands;
pub mod remote_queue_commands;
#[cfg(test)]
mod remote_queue_commands_tests;
pub mod remote_resume_commands;
pub mod remote_plan_commands;
pub mod remote_ideation_commands;
#[cfg(test)]
mod remote_plan_commands_tests;
#[cfg(test)]
mod remote_ideation_commands_tests;
#[cfg(test)]
mod remote_resume_commands_tests;
#[cfg(test)]
mod remote_question_commands_tests;
pub mod remote_transcript_commands;
#[cfg(test)]
mod remote_transcript_commands_tests;
#[cfg(debug_assertions)]
pub mod remote_transport_spike_commands;
pub mod remote_workspace_commands;
pub mod repository_settings_commands;
#[cfg(test)]
mod repository_settings_commands_tests;
pub mod research_commands;
pub mod review_commands;
#[cfg(test)]
mod review_commands_tests;
pub mod review_commands_types;
#[cfg(test)]
mod review_commands_types_tests;
pub mod review_helpers;
pub mod startup_commands;
#[cfg(test)]
mod startup_commands_tests;
pub mod task_commands;
pub mod task_context_commands;
pub mod task_step_commands;
pub mod task_step_commands_types;
pub mod test_data_commands;
pub mod ticketing_commands;
pub mod ui_commands;
pub mod unified_chat_commands;
pub mod update_channel_commands;
#[cfg(test)]
mod update_channel_commands_tests;
pub mod validation_commands;
pub mod workflow_commands;
pub mod workspace_open_commands;
pub mod workspace_review_settings_commands;

// Re-export commands for registration
pub use crate::application::automation::api::{
    AutomationDetailResponse, AutomationResponse, AutomationRunResponse,
    CreateAutomationDraftResponse,
};
pub use activity_commands::{
    count_session_activity_events, count_task_activity_events, list_session_activity_events,
    list_task_activity_events, ActivityEventFilterInput, ActivityEventPageResponse,
    ActivityEventResponse,
};
pub use agent_composer_commands::{
    list_agent_composer_skills, search_agent_composer_entries,
    search_agent_composer_plan_references, AgentComposerEntryResponse,
    AgentComposerPlanReferenceResponse, AgentComposerSkillResponse, ListAgentComposerSkillsInput,
    ListAgentComposerSkillsResponse, SearchAgentComposerEntriesInput,
    SearchAgentComposerEntriesResponse, SearchAgentComposerPlanReferencesInput,
    SearchAgentComposerPlanReferencesResponse,
};
pub use agent_conversation_mute_commands::{
    set_agent_conversation_muted, SetAgentConversationMutedInput,
};
pub use agent_issue_report_commands::{build_agent_issue_report, submit_agent_issue_report};
pub use agent_model_commands::{
    delete_custom_agent_model, list_agent_models, upsert_custom_agent_model, AgentModelResponse,
    UpsertCustomAgentModelInput,
};
pub use agent_plan_commands::{
    activate_agent_plan_direct_implementation, activate_agent_task_pipeline,
    copy_agent_conversation_plan, import_agent_conversation_plan, start_agent_task_pipeline,
    ActivateAgentPlanDirectImplementationInput, ActivateAgentTaskPipelineInput,
    AgentConversationPlanSeedResponse, CopyAgentConversationPlanInput,
    ImportAgentConversationPlanInput, StartAgentTaskPipelineInput,
};
pub use agent_profile_commands::{
    get_agent_profile, get_agent_profiles_by_role, get_builtin_agent_profiles,
    get_custom_agent_profiles, list_agent_profiles, seed_builtin_profiles,
};
pub use agent_terminal_commands::{
    clear_agent_terminal, close_agent_terminal, open_agent_terminal, resize_agent_terminal,
    restart_agent_terminal, write_agent_terminal, AgentTerminalCloseInput, AgentTerminalOpenInput,
    AgentTerminalResizeInput, AgentTerminalWriteInput,
};
pub use artifact_commands::{
    add_artifact_relation, archive_artifact, create_artifact, create_bucket, delete_artifact,
    get_artifact, get_artifact_relations, get_artifacts, get_artifacts_by_bucket,
    get_artifacts_by_task, get_buckets, get_system_buckets, get_team_artifacts_by_session,
    update_artifact, AddRelationInput, ArtifactRelationResponse, ArtifactResponse, BucketResponse,
    CreateArtifactInput, CreateBucketInput, GetTeamArtifactsResponse, TeamArtifactSummaryResponse,
    UpdateArtifactInput,
};
pub use atlassian_commands::{
    assign_agent_conversation_jira_issue, assign_agent_conversation_jira_issue_to_me,
    build_atlassian_oauth_authorization_url, clear_agent_conversation_jira_issue,
    complete_atlassian_oauth_local_callback, exchange_atlassian_oauth_code,
    get_agent_conversation_jira_issue, get_atlassian_integration_settings,
    refresh_agent_conversation_jira_issue, save_atlassian_integration_settings,
    search_atlassian_resources, start_atlassian_oauth_local_callback,
    validate_atlassian_integration, AgentConversationJiraIssueLinkResponse,
    AgentConversationJiraIssueResponse, AssignAgentConversationJiraIssueInput,
    AssignAgentConversationJiraIssueToMeInput, AtlassianIntegrationSettingsResponse,
    ClearAgentConversationJiraIssueInput, CompleteAtlassianOAuthLocalCallbackInput,
    ExchangeAtlassianOAuthCodeInput, GetAgentConversationJiraIssueInput,
    RefreshAgentConversationJiraIssueInput, SaveAtlassianIntegrationSettingsInput,
    SearchAtlassianResourcesInput, SearchAtlassianResourcesResponse,
};
pub use automation_commands::{
    create_automation_draft, get_automation, list_automations, pause_automation,
    restart_automation, resume_automation, retry_automation_judge, retry_automation_plan_judge,
    stop_automation, update_automation_config, update_automation_settings, AutomationIdInput,
    CreateAutomationDraftInput, ListAutomationsInput, PauseAutomationInput,
    UpdateAutomationConfigInput, UpdateAutomationSettingsInput,
};
pub use chat_attachment_commands::{
    delete_chat_attachment, link_attachments_to_message, list_conversation_attachments,
    list_message_attachments, upload_chat_attachment, ChatAttachmentResponse, LinkAttachmentsInput,
    UploadChatAttachmentInput,
};
pub use chat_responses::ChatMessageResponse;
pub use clickup_commands::{
    disconnect_clickup_integration, get_clickup_integration_settings, list_clickup_workspaces,
    save_clickup_integration_settings, search_clickup_tasks, validate_clickup_integration,
    ClickUpIntegrationSettingsResponse, ListClickUpWorkspacesResponse,
    SaveClickUpIntegrationSettingsInput, SearchClickUpTasksInput, SearchClickUpTasksResponse,
};
pub use conversation_stats_commands::{
    build_conversation_stats_response, build_scope_stats_response, get_agent_conversation_stats,
    get_insights_chat_usage_stats, get_project_chat_usage_stats, get_task_chat_usage_stats,
    ConversationAttributionCoverageResponse, ConversationStatsResponse,
    ConversationUsageCoverageResponse, ScopeStatsResponse, UsageBucketResponse,
    UsageTotalsResponse,
};
pub use diagnostic_commands::{
    get_agent_health, get_codex_cli_diagnostics, AgentHealthReport, CodexCliDiagnosticsResponse,
    IprEntryResponse, RunningAgentResponse,
};
pub use diff_commands::{
    detect_merge_conflicts, get_conflict_file_diff, get_file_diff, get_task_file_changes,
};
pub use granola_commands::{
    assign_agent_conversation_granola_note, clear_agent_conversation_granola_note,
    get_agent_conversation_granola_note, get_granola_integration_settings, get_granola_note_detail,
    list_granola_notes, refresh_agent_conversation_granola_note, save_granola_integration_settings,
    validate_granola_integration_settings, AgentConversationGranolaNoteLinkResponse,
    AgentConversationGranolaNoteResponse, AssignAgentConversationGranolaNoteInput,
    ClearAgentConversationGranolaNoteInput, GetAgentConversationGranolaNoteInput,
    GetGranolaNoteDetailInput, GranolaIntegrationSettingsResponse, GranolaNoteDetailResponse,
    GranolaNotePullRequestLinkResponse, GranolaNoteRxConversationResponse,
    GranolaNoteSummaryResponse, GranolaNoteTicketLinkResponse, ListGranolaNotesInput,
    ListGranolaNotesResponse, RefreshAgentConversationGranolaNoteInput,
    SaveGranolaIntegrationSettingsInput,
};
// Re-export ConflictDiff from application for convenience
#[allow(unused_imports)]
pub use crate::application::ConflictDiff;
pub use execution_commands::{
    get_active_project, get_execution_status, get_global_execution_settings, get_running_processes,
    pause_execution, recover_task_execution, resolve_recovery_prompt, restart_task,
    resume_execution, set_active_project, stop_execution, update_global_execution_settings,
    ActiveProjectState, ExecutionState, RestartResult, ResumeCategory, RunningProcessesResponse,
};
pub use harness_provider_commands::{
    get_agent_provider_settings, update_agent_provider_settings, AgentProviderSettingsResponse,
    AgentProvidersSettingsResponse,
};
pub use health::health_check;
pub use ideation_commands::{
    analyze_dependencies, apply_proposals_to_kanban, archive_ideation_session,
    assess_all_priorities, assess_proposal_priority, count_session_messages,
    create_ideation_session, create_task_proposal, delete_chat_message, delete_ideation_session,
    delete_session_messages, delete_task_proposal, get_agent_harness_availability,
    get_agent_lane_settings, get_blocked_tasks, get_ideation_harness_availability,
    get_ideation_session, get_ideation_session_with_data, get_project_messages,
    get_proposal_dependencies, get_proposal_dependents, get_recent_session_messages,
    get_session_messages, get_task_blockers, get_task_messages, get_task_proposal,
    is_orchestrator_available, list_ideation_sessions, list_session_proposals,
    remove_proposal_dependency, reorder_proposals, restart_ideation_implementation,
    send_chat_message, send_orchestrator_message, set_proposal_selection,
    toggle_proposal_selection, update_agent_lane_settings, update_task_proposal,
    AgentLaneHarnessAvailabilityResponse, ApplyProposalsResultResponse, DependencyGraphResponse,
    IdeationLaneHarnessAvailabilityResponse, IdeationSessionResponse,
    LaneHarnessAvailabilityResponse, OrchestratorMessageResponse, PriorityAssessmentResponse,
    RestartImplementationResultResponse, SessionWithDataResponse, TaskProposalResponse,
    ToolCallResultResponse,
};
pub use linear_commands::{
    assign_agent_conversation_linear_issue, clear_agent_conversation_linear_issue,
    get_agent_conversation_linear_issue, get_linear_integration_settings,
    get_linear_webhook_config, refresh_agent_conversation_linear_issue,
    save_linear_integration_settings, save_linear_webhook_signing_secret, search_linear_issues,
    validate_linear_integration, AgentConversationLinearIssueLinkResponse,
    AgentConversationLinearIssueResponse, AssignAgentConversationLinearIssueInput,
    ClearAgentConversationLinearIssueInput, GetAgentConversationLinearIssueInput,
    LinearIntegrationSettingsResponse, LinearWebhookConfigResponse,
    RefreshAgentConversationLinearIssueInput, SaveLinearIntegrationSettingsInput,
    SaveLinearWebhookSigningSecretInput, SearchLinearIssuesInput, SearchLinearIssuesResponse,
};
pub use manual_role_default_commands::{
    clear_manual_role_default, get_agent_conversation_role_default, get_manual_role_defaults,
    get_start_composer_role_default, reset_agent_conversation_role_default,
    update_manual_role_default, ManualRoleCatalogResponse, ManualRoleDefaultInput,
};
pub use merge_pipeline_commands::{
    get_merge_phase_list, get_merge_pipeline, get_merge_progress, MergePipelineResponse,
};
pub use methodology_commands::{
    activate_methodology, deactivate_methodology, get_active_methodology, get_methodologies,
    MethodologyActivationResponse, MethodologyPhaseResponse, MethodologyResponse,
    MethodologyTemplateResponse, WorkflowSchemaResponse,
};
pub use remote_execution_status_commands::get_remote_execution_status;
pub use remote_question_commands::resolve_remote_user_question;
pub use remote_transcript_commands::get_remote_agent_conversation_workspace;

#[cfg(test)]
mod manual_role_default_commands_tests;
pub use metrics_commands::{
    compute_insights_stats, compute_project_stats, get_column_metrics, get_insights_pr_insights,
    get_insights_stats, get_insights_trends, get_metrics_config, get_project_pr_insights,
    get_project_stats, get_project_trends, get_task_metrics, save_metrics_config, MetricsConfig,
};
pub use permission_commands::{
    get_pending_permissions, resolve_permission_request, ResolvePermissionArgs,
    ResolvePermissionResponse,
};
pub use project_commands::{
    archive_project, create_project, delete_project, get_project, list_projects, update_project,
};
pub use provider_cli_management_commands::{
    auto_update_managed_provider_clis, get_managed_provider_cli_status,
    install_or_update_managed_provider_cli, ManagedProviderCliActionInput,
    ManagedProviderCliActionResponse, ManagedProviderCliAutoUpdateResponse,
    ManagedProviderCliStatusResponse, ManagedProviderCliStatusesResponse,
};
pub use qa_commands::{
    get_qa_results, get_qa_settings, get_task_qa, retry_qa, skip_qa, update_qa_settings,
};
pub use question_commands::{
    get_pending_questions, resolve_user_question, ResolveQuestionArgs, ResolveQuestionResponse,
};
pub use research_commands::{
    get_research_presets, get_research_process, get_research_processes, pause_research,
    resume_research, start_research, stop_research, CustomDepthInput, ResearchPresetResponse,
    ResearchProcessResponse, StartResearchInput,
};
pub use review_commands::{
    approve_fix_task, approve_review, approve_task_for_review, get_fix_task_attempts,
    get_pending_reviews, get_review_by_id, get_reviews_by_task_id, get_task_state_history,
    reject_fix_task, reject_review, request_changes, request_task_changes_for_review,
    request_task_changes_from_reviewing,
};
pub use task_commands::{
    answer_user_question, archive_task, cancel_tasks_in_group, create_task, emit_queue_changed,
    get_archived_count, get_task, get_task_state_transitions, get_valid_transitions, inject_task,
    list_tasks, move_task, pause_task, restore_task, retry_branch_update, search_tasks, stop_task,
    update_task, StateTransitionResponse,
};
pub use task_context_commands::{
    get_artifact_full, get_artifact_version, get_related_artifacts, get_task_context,
    search_artifacts, ArtifactSearchResult, SearchArtifactsInput,
};
pub use task_step_commands::{
    create_task_step, get_step_progress, get_task_steps, reorder_task_steps, update_task_step,
};
pub use test_data_commands::{clear_test_data, seed_test_data, seed_visual_audit_data};
pub use ticketing_commands::{
    add_ticket_comment, assign_ticket, clear_ticket_assignee, get_conversation_ticket,
    get_ticket_associations, get_ticket_detail, list_ticket_filter_options, list_ticket_labels,
    list_ticket_transitions, list_ticketing_columns, list_ticketing_containers,
    list_ticketing_providers, list_ticketing_status_catalog, list_tickets,
    refresh_ticketing_status_catalog, refresh_tickets, set_ticket_labels,
    start_ralphx_work_from_ticket, transition_ticket_status, update_ticketing_status_presentation,
    AddTicketCommentInput, AssignTicketInput, ConversationTicketResponse,
    ListTicketFilterOptionsQuery, ListTicketsQuery, RefreshTicketsResponse, SetTicketLabelsInput,
    StartRalphxWorkFromTicketInput, TicketAssociationsResponse, TicketDetailResponse,
    TicketFilterOptionsResponse, TicketLabelOptionResponse, TicketLabelsResponse,
    TicketMutationResponse, TicketOperationResponse, TicketPageResponse, TicketRefInput,
    TicketSummaryResponse, TicketingCapabilitiesResponse, TicketingColumnResponse,
    TicketingContainerResponse, TicketingProviderSummaryResponse,
    TicketingStatusCatalogEntryResponse, TransitionTicketStatusInput,
    UpdateTicketingStatusPresentationInput,
};
pub use workflow_commands::{
    create_workflow, delete_workflow, get_active_workflow_columns, get_builtin_workflows,
    get_workflow, get_workflows, seed_builtin_workflows, set_default_workflow, update_workflow,
    CreateWorkflowInput, UpdateWorkflowInput, WorkflowColumnInput, WorkflowColumnResponse,
    WorkflowResponse,
};
pub use workspace_review_settings_commands::{
    get_workspace_review_runtime_settings, update_workspace_review_runtime_settings,
    UpdateWorkspaceReviewRuntimeSettingsInput, WorkspaceReviewRuntimeSettingsResponse,
};
// Unified chat commands (consolidates context_chat + execution_chat)
pub use agent_sidebar_commands::{
    get_bulk_workspace_publication_states, BulkPublicationStateResponse,
};
pub use unified_chat_commands::{
    archive_agent_conversation, commit_agent_conversation_workspace_locally,
    create_agent_conversation, delete_queued_agent_message, fork_agent_conversation,
    get_agent_conversation, get_agent_conversation_messages_page,
    get_agent_conversation_runtime_index, get_agent_conversation_runtime_statuses,
    get_agent_conversation_summary, get_agent_conversation_timeline_page,
    get_agent_conversation_workspace, get_agent_conversation_workspace_freshness,
    get_agent_message_tool_call_detail, get_agent_run_attribution, get_agent_run_attributions,
    get_agent_run_status_unified,
    get_agent_running_states, get_agent_timeline_item_tool_call_detail, get_queued_agent_messages,
    is_agent_running, is_chat_service_available,
    list_agent_conversation_workspace_publication_events,
    list_agent_conversation_workspaces_by_project, list_agent_conversations,
    list_agent_conversations_page, precompute_agent_conversation_workspace_pr_description,
    publish_agent_conversation_workspace, queue_agent_message,
    reconcile_agent_conversation_workspace_publication, restore_agent_conversation,
    send_agent_message, set_agent_conversation_workspace_auto_publish,
    set_agent_conversation_workspace_pr_supervision, start_agent_conversation, stop_agent,
    switch_agent_conversation_mode, switch_agent_conversation_persona,
    update_agent_conversation_coordination_mode, update_agent_conversation_title,
    update_agent_conversation_workspace_from_base, AgentConversationListPageResponse,
    AgentConversationMessagesPageResponse, AgentConversationResponse,
    AgentConversationRuntimeIndexResponse, AgentConversationTimelinePageResponse,
    AgentConversationWithMessagesResponse, AgentConversationWorkspaceAutoPublishInput,
    AgentConversationWorkspaceFreshnessResponse, AgentConversationWorkspacePrSupervisionInput,
    AgentConversationWorkspacePublicationEventResponse, AgentConversationWorkspaceResponse,
    AgentMessageResponse, AgentRunStatusResponse, AgentTimelineItemResponse,
    AgentToolCallDetailResponse, CreateAgentConversationInput, ForkAgentConversationInput,
    ForkAgentConversationResponse, PrecomputeAgentConversationWorkspacePrDescriptionResponse,
    PublishAgentConversationWorkspaceResponse, QueueAgentMessageInput,
    QueuedMessageResponse as UnifiedQueuedMessageResponse, SendAgentMessageInput,
    SendAgentMessageResponse, StartAgentConversationInput, StartAgentConversationResponse,
    SwitchAgentConversationModeInput, SwitchAgentConversationModeResponse,
    SwitchAgentConversationPersonaInput, SwitchAgentConversationPersonaResponse,
    UpdateAgentConversationCoordinationModeInput, UpdateAgentConversationTitleInput,
    UpdateAgentConversationWorkspaceFromBaseResponse,
};
// Plan branch commands (Phase 85 - Feature branch for plan groups)
pub use plan_branch_commands::{
    enable_feature_branch, get_plan_branch, get_plan_branch_by_task_id, get_project_plan_branches,
    EnableFeatureBranchInput, PlanBranchResponse,
};
// UI feature flag commands
pub use ui_commands::{
    get_ui_feature_flags, update_ui_feature_flags, UiFeatureFlagsResponse,
    UpdateUiFeatureFlagsInput,
};
pub use workspace_open_commands::{
    list_workspace_open_targets, open_agent_conversation_workspace,
    open_agent_conversation_workspace_path, WorkspaceOpenTargetKind, WorkspaceOpenTargetResponse,
};
// Plan commands (Active plan management)
pub use plan_commands::{
    clear_active_plan, get_active_plan, list_plan_selector_candidates, set_active_plan,
};
pub use repository_settings_commands::{
    get_repository_settings, update_repository_settings, RepositorySettingsResponse,
    UpdateRepositorySettingsInput,
};
pub use update_channel_commands::{get_update_channel, set_update_channel};
// Git commands (Phase 66 - Per-task branch isolation)
pub use git_commands::{
    change_project_git_mode, cleanup_task_branch, get_task_commits, get_task_diff_stats,
    resolve_merge_conflict, retry_merge, ChangeGitModeInput, CommitInfoResponse,
    TaskCommitsResponse, TaskDiffStatsResponse,
};
// GitHub commands (PR visibility — connection status)
pub use github_commands::{
    get_github_branch_overview, get_github_connection_status, GithubBranchOverviewResponse,
    GithubConnectionStatusResponse,
};
