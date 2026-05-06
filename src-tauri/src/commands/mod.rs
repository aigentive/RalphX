// Tauri commands - thin layer bridging frontend to backend
// Commands should be minimal - delegate to domain/infrastructure

pub mod activity_commands;
pub mod agent_model_commands;
pub mod agent_terminal_commands;
pub mod branch_helpers;
pub mod api_key_commands;
pub mod agent_profile_commands;
pub mod artifact_commands;
pub mod chat_attachment_commands;
pub mod conversation_stats_commands;
pub mod chat_responses;
pub mod diagnostic_commands;
pub mod diff_commands;
pub mod execution_commands;
pub mod external_mcp_commands;
pub mod git_commands;
pub mod health;
pub mod ideation_commands;
pub mod merge_pipeline_commands;
pub mod metrics_commands;
pub(crate) mod metrics_queries;
pub(crate) mod metrics_trends;
pub mod metrics_types;
pub mod methodology_commands;
pub mod permission_commands;
pub mod plan_branch_commands;
pub mod plan_commands;
pub mod project_commands;
pub mod qa_commands;
pub mod question_commands;
pub mod research_commands;
pub mod registry;
pub mod review_commands;
pub mod review_commands_types;
pub mod review_helpers;
pub mod task_commands;
pub mod task_context_commands;
pub mod task_step_commands;
pub mod task_step_commands_types;
pub mod team_commands;
pub mod test_data_commands;
pub mod unified_chat_commands;
pub mod ui_commands;
pub mod workflow_commands;

// Re-export commands for registration
pub use activity_commands::{
    count_session_activity_events, count_task_activity_events, list_session_activity_events,
    list_task_activity_events, ActivityEventFilterInput, ActivityEventPageResponse,
    ActivityEventResponse,
};
pub use agent_model_commands::{
    delete_custom_agent_model, list_agent_models, upsert_custom_agent_model, AgentModelResponse,
    UpsertCustomAgentModelInput,
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
pub use chat_attachment_commands::{
    delete_chat_attachment, link_attachments_to_message, list_conversation_attachments,
    list_message_attachments, upload_chat_attachment, ChatAttachmentResponse, LinkAttachmentsInput,
    UploadChatAttachmentInput,
};
pub use conversation_stats_commands::{
    build_conversation_stats_response, build_scope_stats_response, get_agent_conversation_stats,
    get_project_chat_usage_stats, get_task_chat_usage_stats, ConversationAttributionCoverageResponse,
    ConversationStatsResponse, ConversationUsageCoverageResponse, ScopeStatsResponse,
    UsageBucketResponse, UsageTotalsResponse,
};
pub use chat_responses::ChatMessageResponse;
pub use diagnostic_commands::{
    get_agent_health, get_codex_cli_diagnostics, AgentHealthReport,
    CodexCliDiagnosticsResponse, IprEntryResponse, RunningAgentResponse,
};
pub use diff_commands::{
    detect_merge_conflicts, get_conflict_file_diff, get_file_diff, get_task_file_changes,
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
pub use health::health_check;
pub use ideation_commands::{
    analyze_dependencies, apply_proposals_to_kanban,
    archive_ideation_session, assess_all_priorities, assess_proposal_priority,
    count_session_messages, create_ideation_session, create_task_proposal, delete_chat_message,
    delete_ideation_session, delete_session_messages, delete_task_proposal, get_blocked_tasks,
    get_agent_harness_availability, get_agent_lane_settings,
    get_ideation_harness_availability, get_ideation_session, get_ideation_session_with_data,
    get_project_messages,
    get_proposal_dependencies, get_proposal_dependents, get_recent_session_messages,
    get_session_messages, get_task_blockers, get_task_messages, get_task_proposal,
    is_orchestrator_available, list_ideation_sessions, list_session_proposals,
    remove_proposal_dependency, reorder_proposals, send_chat_message, send_orchestrator_message,
    set_proposal_selection, toggle_proposal_selection, update_task_proposal,
    update_agent_lane_settings, AgentLaneHarnessAvailabilityResponse,
    ApplyProposalsResultResponse, DependencyGraphResponse,
    IdeationLaneHarnessAvailabilityResponse, LaneHarnessAvailabilityResponse,
    IdeationSessionResponse, OrchestratorMessageResponse,
    PriorityAssessmentResponse, SessionWithDataResponse, TaskProposalResponse,
    ToolCallResultResponse,
};
pub use merge_pipeline_commands::{
    get_merge_phase_list, get_merge_pipeline, get_merge_progress, MergePipelineResponse,
};
pub use metrics_commands::{
    compute_project_stats, get_column_metrics, get_metrics_config, get_project_stats,
    get_project_trends, get_task_metrics, save_metrics_config, MetricsConfig,
};
pub use methodology_commands::{
    activate_methodology, deactivate_methodology, get_active_methodology, get_methodologies,
    MethodologyActivationResponse, MethodologyPhaseResponse, MethodologyResponse,
    MethodologyTemplateResponse, WorkflowSchemaResponse,
};
pub use permission_commands::{
    get_pending_permissions, resolve_permission_request, ResolvePermissionArgs,
    ResolvePermissionResponse,
};
pub use project_commands::{
    archive_project, create_project, delete_project, get_project, list_projects, update_project,
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
    list_tasks, move_task, pause_task, restore_task, search_tasks, stop_task, update_task,
    StateTransitionResponse,
};
pub use task_context_commands::{
    get_artifact_full, get_artifact_version, get_related_artifacts, get_task_context,
    search_artifacts, ArtifactSearchResult, SearchArtifactsInput,
};
pub use task_step_commands::{
    create_task_step, get_step_progress, get_task_steps, reorder_task_steps, update_task_step,
};
pub use test_data_commands::{clear_test_data, seed_test_data, seed_visual_audit_data};
pub use workflow_commands::{
    create_workflow, delete_workflow, get_active_workflow_columns, get_builtin_workflows,
    get_workflow, get_workflows, seed_builtin_workflows, set_default_workflow, update_workflow,
    CreateWorkflowInput, UpdateWorkflowInput, WorkflowColumnInput, WorkflowColumnResponse,
    WorkflowResponse,
};
// Team commands (agent teams collaboration)
pub use team_commands::{
    create_team, disband_team, get_team_history, get_team_messages, get_team_status,
    get_teammate_cost, send_team_message, send_teammate_message, stop_team, stop_teammate,
    CreateTeamInput, GetTeamHistoryInput, SendTeamMessageInput, SendTeammateMessageInput,
    TeamHistoryResponse, TeamMessageRecordResponse, TeamSessionResponse, TeammateSnapshotResponse,
};
// Unified chat commands (consolidates context_chat + execution_chat)
pub use unified_chat_commands::{
    archive_agent_conversation, create_agent_conversation,
    delete_queued_agent_message, get_agent_conversation, get_agent_conversation_messages_page,
    get_agent_conversation_workspace, get_agent_conversation_workspace_freshness,
    get_agent_run_status_unified, get_queued_agent_messages, is_agent_running,
    is_chat_service_available, list_agent_conversations,
    list_agent_conversation_workspace_publication_events,
    list_agent_conversation_workspaces_by_project, list_agent_conversations_page,
    publish_agent_conversation_workspace, queue_agent_message, restore_agent_conversation,
    send_agent_message, start_agent_conversation, stop_agent, switch_agent_conversation_mode,
    update_agent_conversation_title, update_agent_conversation_workspace_from_base,
    AgentConversationWorkspaceFreshnessResponse,
    AgentConversationWorkspacePublicationEventResponse, AgentConversationWorkspaceResponse,
    PublishAgentConversationWorkspaceResponse, UpdateAgentConversationWorkspaceFromBaseResponse,
    AgentConversationListPageResponse, AgentConversationMessagesPageResponse,
    AgentConversationResponse,
    AgentConversationWithMessagesResponse, AgentMessageResponse, AgentRunStatusResponse,
    CreateAgentConversationInput, QueueAgentMessageInput,
    QueuedMessageResponse as UnifiedQueuedMessageResponse, SendAgentMessageInput,
    SendAgentMessageResponse, StartAgentConversationInput, StartAgentConversationResponse,
    SwitchAgentConversationModeInput, SwitchAgentConversationModeResponse,
    UpdateAgentConversationTitleInput,
};
// Plan branch commands (Phase 85 - Feature branch for plan groups)
pub use plan_branch_commands::{
    enable_feature_branch, get_plan_branch, get_plan_branch_by_task_id,
    get_project_plan_branches, EnableFeatureBranchInput,
    PlanBranchResponse,
};
// UI feature flag commands
pub use ui_commands::{get_ui_feature_flags, UiFeatureFlagsResponse};
// Plan commands (Active plan management)
pub use plan_commands::{
    clear_active_plan, get_active_plan, list_plan_selector_candidates, set_active_plan,
};
// Git commands (Phase 66 - Per-task branch isolation)
pub use git_commands::{
    change_project_git_mode, cleanup_task_branch, get_task_commits, get_task_diff_stats,
    resolve_merge_conflict, retry_merge, ChangeGitModeInput, CommitInfoResponse,
    TaskCommitsResponse, TaskDiffStatsResponse,
};
