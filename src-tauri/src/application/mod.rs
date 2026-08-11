// Application layer - dependency injection and service orchestration
// This layer bridges the domain and infrastructure layers

pub mod agent_client_bundle;
pub mod delegation_park;
pub mod agent_conversation_archive;
pub mod agents;
#[cfg(test)]
mod agent_conversation_archive_tests;
pub mod agent_conversation_fork;
pub mod agent_conversation_granola_note;
pub mod agent_conversation_jira_issue;
pub mod agent_conversation_linear_issue;
pub(crate) mod agent_conversation_mode_switch;
pub mod agent_conversation_start_service;
pub mod agent_conversation_workspace;
pub mod agent_conversation_workspace_base;
pub(crate) mod agent_conversation_workspace_restart;
pub mod agent_issue_report;
pub mod agent_lane_resolution;
pub(crate) mod agent_plan_context;
#[cfg(test)]
mod agent_plan_context_tests;
pub mod manual_role_default_service;
pub mod manual_router_config;
pub mod mcp_policy_config;
pub mod mcp_policy_agent_client;
pub mod mcp_policy_service;
#[cfg(test)]
mod mcp_policy_agent_client_tests;
#[cfg(test)]
mod mcp_policy_config_tests;
#[cfg(test)]
mod mcp_policy_service_tests;
pub mod agent_lane_settings_bootstrap;
pub(crate) mod agent_runtime_context;
#[cfg(test)]
mod agent_runtime_context_tests;
#[cfg(test)]
mod agent_runtime_context_branch_status_tests;
#[cfg(test)]
mod agent_runtime_context_linked_plan_tests;
#[cfg(test)]
mod agent_runtime_context_team_tests;
pub(crate) mod agent_planning_session_titles;
pub mod agent_task_assignment_recovery;
pub(crate) mod agent_task_pipeline_service;
pub mod agent_task_service;
pub mod agent_terminal;
pub mod agent_workspace_bridge;
pub mod agent_workspace_local_commit;
#[cfg(test)]
mod agent_workspace_local_commit_tests;
pub mod agent_workspace_continuation;
pub mod agent_workspace_external_pr_reconciliation;
pub(crate) mod agent_workspace_pr_reopen;
pub(crate) mod agent_workspace_pr_reopen_restore;
#[cfg(test)]
mod agent_workspace_pr_reopen_tests;
pub mod agent_workspace_publication_reconciliation;
pub(crate) mod agent_workspace_pr_autofix_attempt;
#[cfg(test)]
mod agent_workspace_pr_autofix_attempt_tests;
pub mod agent_workspace_pr_description;
#[cfg(test)]
pub(crate) mod agent_workspace_pr_metadata_reconciliation;
pub(crate) mod agent_workspace_terminal_cleanup;
#[cfg(test)]
mod agent_workspace_terminal_cleanup_tests;
pub(crate) mod agent_workspace_pr_supervision_recovery;
pub(crate) mod agent_workspace_publish_lease;
#[cfg(test)]
mod agent_workspace_publish_lease_tests;
pub mod agent_workspace_publish_recovery;
pub(crate) mod agent_workspace_base_staleness;
#[cfg(test)]
mod agent_workspace_base_staleness_tests;
pub(crate) mod agent_workspace_ci_rerun;
pub(crate) mod agent_workspace_publish_repair_state;
pub mod agent_workspace_review;
pub(crate) mod agent_workspace_review_approval;
pub mod agent_workspace_review_auto_merge;
#[cfg(test)]
mod agent_workspace_review_auto_merge_tests;
pub mod agent_workspace_review_base;
#[cfg(test)]
mod agent_workspace_review_base_tests;
pub mod agent_workspace_review_context;
#[cfg(test)]
mod agent_workspace_review_context_tests;
pub mod agent_workspace_review_diff;
mod agent_workspace_review_diff_cursor;
mod agent_workspace_review_diff_inventory;
#[cfg(test)]
mod agent_workspace_review_diff_scope_tests;
#[cfg(test)]
mod agent_workspace_review_diff_tests;
#[cfg(test)]
mod agent_workspace_review_mode_guard_tests;
pub(crate) mod agent_workspace_review_publish_handoff;
#[cfg(test)]
mod agent_workspace_review_run_guard_tests;
#[cfg(test)]
mod agent_workspace_review_unfinished_git_recovery_tests;
#[cfg(test)]
mod agent_workspace_review_unfinished_git_tests;
pub mod app_paths;
#[cfg(test)]
mod app_paths_tests;
pub mod app_state;
pub mod apply_service;
pub mod atlassian_integration_service;
pub mod atlassian_mcp_access;
#[cfg(test)]
mod atlassian_mcp_access_tests;
pub mod atlassian_mcp_service;
pub mod attention_service;
pub mod automation;
pub mod branch_update_executor;
#[cfg(test)]
mod branch_update_executor_tests;
pub mod branch_update_workflow;
pub mod builder_attachment_materializer;
pub mod chat_attachment_service;
pub(crate) mod conversation_reference_inheritance;
#[cfg(test)]
mod conversation_reference_inheritance_tests;
pub mod chat_attachment_storage;
pub mod chat_resumption;
pub mod completion_correlation;
pub mod chat_service;
pub mod clickup_git_association;
pub mod clickup_integration_service;
pub mod conversation_folder_reference_service;
#[cfg(test)]
mod conversation_folder_reference_service_tests;
pub mod data_retention_service;
#[cfg(test)]
mod data_retention_service_tests;
pub mod dependency_service;
#[cfg(target_os = "macos")]
pub(crate) mod desktop_notification;
pub mod diff_service;
pub mod event_cleanup_service;
pub mod execution_settings_bootstrap;
pub mod execution_state;
pub mod external_issue_link_service;
pub(crate) mod git_artifact_cleanup;
pub mod git_mutation_recovery;
#[cfg(test)]
mod git_mutation_recovery_tests;
pub mod git_service;
#[cfg(test)]
mod git_service_strict_worktree_tests;
pub mod granola_integration_service;
pub mod harness_runtime_registry;
pub mod http_shutdown;
#[cfg(test)]
mod http_shutdown_tests;
pub mod ideation_apply_service;
pub mod ideation_effort_bootstrap;
pub mod ideation_harness_availability;
pub mod ideation_model_bootstrap;
pub mod ideation_service;
pub mod ideation_workspace;
pub mod integration_reference_expansion;
pub mod interactive_notification_producer;
#[cfg(test)]
mod interactive_notification_producer_tests;
pub mod interactive_process_registry;
#[cfg(test)]
mod interactive_process_registry_tests;
pub mod linear_integration_service;
pub mod linear_webhook_reconciliation_service;
pub(crate) mod managed_provider_cli;
pub mod managed_team;
pub mod memory_archive_service;
pub mod memory_orchestration;
pub(crate) mod merge_pipeline_visibility;
pub mod notification_context_resolver;
pub mod notification_service;
#[cfg(test)]
mod notification_service_tests;
pub(crate) mod orphan_worktree_cleanup;
pub mod pending_session_drain;
pub mod permission_state;
pub mod persona_ingest;
pub mod persona_prompt;
#[cfg(test)]
mod persona_prompt_tests;
pub mod persona_resolver;
#[cfg(test)]
mod persona_resolver_tests;
pub mod personas;
pub mod plan_approval_notification_service;
#[cfg(test)]
mod plan_approval_notification_service_tests;
pub(crate) mod plan_artifact_approval;
pub(crate) mod plan_complexity_assessment;
pub(crate) mod plan_pr_description;
pub mod plan_ranking;
pub(crate) mod plan_reference_import;
pub mod plan_verification_service;
#[cfg(test)]
mod plan_verification_service_tests;
pub mod pr_startup_recovery;
pub mod priority_service;
pub mod project_pr_template;
pub(crate) mod provider_env_file;
pub(crate) mod provider_management_eligibility;
#[cfg(test)]
mod provider_management_eligibility_tests;
pub(crate) mod provider_onboarding_gate;
#[cfg(test)]
mod provider_onboarding_gate_tests;
pub mod provider_session_fork;
pub mod prune_engine;
pub mod publish_resilience;
pub mod pull_request_detail;
pub mod qa_service;
pub mod question_state;
pub mod ready_task_scheduler;
pub mod reconciliation;
pub mod recovery_queue;
pub mod resume_validator;
pub mod review_issue_service;
pub mod review_service;
pub mod runtime_factory;
pub mod seeded_agent_conversation_abort;
pub mod services;
pub mod session_export_service;
pub(crate) mod session_namer_agent;
pub mod session_namer_prompt;
pub mod session_reopen_service;
pub mod standalone_workspace;
#[cfg(test)]
mod standalone_workspace_path_safety_tests;
#[cfg(test)]
mod standalone_workspace_tests;
pub mod startup_background;
pub mod startup_failure_classification;
pub mod startup_git_auth_preflight;
pub mod startup_jobs;
pub mod startup_status;
pub mod supervisor_service;
pub mod task_cleanup_service;
pub mod task_context_service;
pub(crate) mod task_diff_base;
#[cfg(test)]
mod task_diff_base_tests;
pub mod task_notification_producer;
pub mod task_restart;
pub mod task_scheduler_service;
pub mod task_transition_service;
pub(crate) mod tasks_feature_policy;
#[cfg(test)]
mod tasks_feature_policy_tests;
pub(crate) mod tasks_feature_toggle_service;
#[cfg(test)]
mod tasks_feature_toggle_service_tests;
pub mod throttled_emitter;
pub mod ticket_attachment;
pub mod ticket_attachment_runtime_store;
#[cfg(test)]
mod ticket_attachment_runtime_store_tests;
#[cfg(test)]
mod ticket_attachment_tests;
pub mod ticket_canonical_branch;
pub mod ticketing_cache_invalidator;
pub mod ticketing_pr_summary;
pub mod ticketing_service;
pub mod ticketing_status_catalog_service;
pub(crate) mod validation_events;
pub mod validation_service;
pub mod verification_child_lifecycle;
pub mod verification_event_emitters;
pub mod webhook_service;
pub(crate) mod workspace_capacity;

// Re-export commonly used items
pub(crate) use agent_client_bundle::AgentClientBundle;
pub use agent_issue_report::{
    build_agent_issue_report_draft, submit_agent_issue_report, AgentIssueReportDestination,
    AgentIssueReportDestinationSource, AgentIssueReportDraft, AgentIssueReportEnvironment,
    AgentIssueReportSource, AgentIssueReportSubmitResponse, BuildAgentIssueReportInput,
    SubmitAgentIssueReportInput,
};
pub use agent_lane_settings_bootstrap::{
    load_or_seed_agent_lane_settings_defaults, AgentLaneSettingsBootstrapResult,
};
pub use agent_task_service::AgentTaskService;
pub use agent_terminal::AgentTerminalService;
pub use app_paths::AppPaths;
pub use app_state::AppState;
pub use apply_service::{
    ApplyProposalsOptions, ApplyProposalsResult, ApplyService, SelectionValidation, TargetColumn,
};
pub use crate::domain::integrations::atlassian_api_error::AtlassianApiError;
pub use atlassian_mcp_access::{
    atlassian_mcp_tools_for_resumed_run, atlassian_mcp_tools_for_spawn,
    effective_atlassian_mcp_access,
};
pub use crate::domain::integrations::atlassian_mcp_ops::{
    validate_atlassian_raw_path, AtlassianRawMethod, AtlassianRawResponse, ConfluencePageContent, ConfluencePageCreateRequest,
    ConfluencePageUpdateRequest, JiraIssueCreateRequest, JiraIssueCreated, JiraIssueUpdateRequest,
    ATLASSIAN_RAW_PATH_PREFIXES, ATLASSIAN_RAW_RESPONSE_MAX_BYTES,
};
pub use atlassian_integration_service::{
    AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity, AtlassianCredential,
    AtlassianIntegrationService, AtlassianJiraAttachment, AtlassianJiraComment,
    AtlassianJiraTransition, AtlassianOAuthAuthorization, AtlassianOAuthResource,
    AtlassianOAuthTokenResponse, AtlassianResourceContent, AtlassianResourceKind,
    AtlassianResourceSummary, AtlassianResourceUrlResolution, EmptyAtlassianApiClient,
    JiraIssueDetail, JiraProjectSummary, JiraStatusSummary, UnavailableAtlassianApiClient,
};
pub use chat_attachment_service::ChatAttachmentService;
pub use chat_resumption::ChatResumptionRunner;
pub use clickup_integration_service::{
    ClickUpApiClient, ClickUpAuthContext, ClickUpComment, ClickUpIntegrationService, ClickUpSpace,
    ClickUpStatus, ClickUpTag, ClickUpTaskContent, ClickUpTaskSummary, ClickUpUser,
    ClickUpWorkspace, EmptyClickUpApiClient, UnavailableClickUpApiClient,
};
pub use dependency_service::{DependencyAnalysis, DependencyService, ValidationResult};
pub use diff_service::{
    ConflictDiff, DiffHunk, DiffLine, DiffLineKind, DiffPageRow, DiffRefKind, DiffService,
    DiffSide, FileChange, FileChangeStatus, FileDiff, FileDiffPage, RangeLine,
};
pub use event_cleanup_service::EventCleanupService;
pub use execution_settings_bootstrap::{
    load_or_seed_execution_settings_defaults, ExecutionSettingsBootstrapResult,
};
pub use external_issue_link_service::ExternalIssueLinkService;
pub use git_service::{
    checkout_free::CheckoutFreeMergeResult, CommitInfo, DiffStats, GitService, MergeAttemptResult,
    MergeResult, RebaseResult,
};
pub use granola_integration_service::{
    EmptyGranolaApiClient, GranolaApiClient, GranolaApiError, GranolaAuthContext,
    GranolaIntegrationService, GranolaNoteDetail, GranolaNoteListPage, GranolaNoteSummary,
    GranolaTranscriptEntry, UnavailableGranolaApiClient,
};
pub use http_shutdown::HttpShutdownHandle;
pub(crate) use ideation_harness_availability::{
    build_lane_harness_availability, refreshed_provider_aware_runtime_probes,
    resolve_lane_harness_config, resolve_primary_ideation_harness_availability_for_state,
    validate_chat_runtime_for_context, validate_chat_runtime_for_context_with_override,
    AGENT_LANES, IDEATION_LANES,
};
pub use ideation_service::{
    CreateProposalOptions, IdeationService, SessionStats, SessionWithData, UpdateProposalOptions,
    UpdateSource,
};
pub use interactive_process_registry::{InteractiveProcessKey, InteractiveProcessRegistry};
pub use crate::domain::integrations::jira_agile_types::{
    JiraBoardColumn, JiraBoardConfiguration, JiraBoardSummary, JiraSprintSummary,
};
pub use linear_integration_service::{
    resolve_linear_label_ids, EmptyLinearApiClient, LinearApiClient, LinearAuthContext,
    LinearComment, LinearIntegrationService, LinearIntegrationSettings,
    LinearIntegrationSettingsRepository, LinearIssueContent, LinearIssueSummary, LinearLabel,
    LinearProject, LinearUser, LinearWorkflowState, UnavailableLinearApiClient,
};
pub use linear_webhook_reconciliation_service::{
    ExternalIssueLink, LinearWebhookAction, LinearWebhookError, LinearWebhookHeaders,
    LinearWebhookOutcome, LinearWebhookReconciliationService, LinearWebhookRequest,
    LinearWebhookStore, MemoryLinearWebhookStore,
};
pub use memory_archive_service::MemoryArchiveService;
pub use notification_context_resolver::NotificationContextResolver;
pub use notification_service::NotificationService;
pub use permission_state::{
    PendingPermissionInfo, PermissionDecision, PermissionState, PERMISSION_REQUEST_TTL,
    PERMISSION_RESOLVED_EVENT,
};
pub use plan_ranking::{
    compute_activity_score, compute_final_score, compute_final_score_with_breakdown,
    compute_interaction_score, compute_recency_score, ScoreBreakdown,
};
pub use priority_service::PriorityService;
pub(crate) use provider_onboarding_gate::{
    ensure_provider_spawn_enabled, resolve_enabled_default_provider,
};
pub use prune_engine::PruneEngine;
pub use qa_service::{QAPrepStatus, QAService, TaskQAState};
pub use question_state::{PendingQuestionInfo, QuestionAnswer, QuestionOption, QuestionState};
pub use ready_task_scheduler::spawn_ready_task_scheduler_if_needed;
pub use reconciliation::ReconciliationRunner;
pub use recovery_queue::{ProcessSummary, RecoveryItem, RecoveryPriority, RecoveryQueue};
pub use resume_validator::{ResumeValidationResult, ResumeValidator};
pub use review_issue_service::{CreateIssueInput, ReviewIssueService};
pub use review_service::ReviewService;
pub use services::PrPollerRegistry;
pub use session_export_service::{
    DependencyData, ImportedSession, PlanVersionData, PriorityFactorsData, ProposalData,
    SessionData, SessionExport, SessionExportService, SourceInstance,
};
pub use session_reopen_service::SessionReopenService;
pub use startup_jobs::StartupJobRunner;
pub use supervisor_service::{SupervisorConfig, SupervisorService, TaskMonitorState};
pub use task_cleanup_service::{
    CleanupReport, StopMode, TaskCleanupService, TaskGroup, TaskStopper,
};
pub use task_context_service::TaskContextService;
pub use task_scheduler_service::{ReadyWatchdog, TaskSchedulerService};
pub use task_transition_service::TaskTransitionService;
pub use throttled_emitter::ThrottledEmitter;
pub use ticketing_cache_invalidator::{
    TicketingCacheInvalidatedEvent, TicketingCacheInvalidator, TICKETING_CACHE_INVALIDATED_EVENT,
};
pub use ticketing_service::{
    TauriTicketingEventSink, TicketAssignRequest, TicketCommentRequest, TicketSetLabelsRequest,
    TicketTransitionRequest, TicketingCommentResult, TicketingEventSink, TicketingLabelResult,
    TicketingMutationResult, TicketingOperationEvent, TicketingPersonResult, TicketingService,
    TicketingTicketIdentity, TicketingTransitionOption, TICKETING_OPERATION_EVENT,
};
pub use ticketing_status_catalog_service::TicketingStatusCatalogService;
pub use validation_service::{
    RunTaskValidationRequest, TaskValidationService, TaskValidationSummary,
    ValidationCommandRequest, ValidationCommandSummary, ValidationRunSummary,
};
pub use webhook_service::WebhookService;

#[cfg(test)]
mod agent_conversation_archive_restart_tests;
#[cfg(test)]
mod agent_conversation_mode_switch_tests;
#[cfg(test)]
mod agent_conversation_workspace_base_tests;
#[cfg(test)]
mod agent_conversation_workspace_restart_tests;
#[cfg(test)]
mod agent_conversation_workspace_tests;
#[cfg(test)]
mod agent_issue_report_tests;
#[cfg(test)]
mod agent_lane_resolution_tests;
#[cfg(test)]
mod agent_planning_session_titles_tests;
#[cfg(test)]
mod agent_terminal_tests;
#[cfg(test)]
mod agent_workspace_continuation_tests;
#[cfg(test)]
mod agent_workspace_external_pr_reconciliation_tests;
#[cfg(test)]
mod agent_workspace_publication_reconciliation_tests;
#[cfg(test)]
mod agent_workspace_pr_metadata_reconciliation_tests;
#[cfg(test)]
mod agent_workspace_pr_supervision_recovery_tests;
#[cfg(test)]
mod agent_workspace_publish_recovery_tests;
#[cfg(test)]
#[path = "agent_workspace_ci_rerun_tests.rs"]
mod agent_workspace_ci_rerun_tests;
#[cfg(test)]
mod agent_workspace_publish_repair_state_tests;
#[cfg(test)]
mod agent_workspace_review_publish_handoff_tests;
#[cfg(test)]
mod app_state_shared_state_tests;
#[cfg(test)]
mod chat_service_output_tests;
#[cfg(test)]
mod clickup_integration_service_tests;
#[cfg(test)]
mod git_artifact_cleanup_tests;
#[cfg(test)]
mod granola_integration_prompt_edge_tests;
#[cfg(test)]
mod granola_integration_prompt_tests;
#[cfg(test)]
mod granola_integration_service_tests;
#[cfg(test)]
pub(crate) mod harness_runtime_test_support;
#[cfg(test)]
mod harness_runtime_registry_tests;
#[cfg(test)]
mod ideation_harness_availability_tests;
#[cfg(test)]
mod ideation_workspace_tests;
#[cfg(test)]
mod integration_reference_expansion_tests;
#[cfg(test)]
mod manual_role_default_service_tests;
#[cfg(test)]
mod manual_router_config_tests;
#[cfg(test)]
mod integration_reference_expansion_edge_tests;
#[cfg(test)]
mod orphan_worktree_cleanup_tests;
#[cfg(test)]
mod pending_session_drain_tests;
#[cfg(test)]
mod plan_complexity_assessment_tests;
#[cfg(test)]
mod plan_pr_description_tests;
#[cfg(test)]
mod pr_startup_recovery_tests;
#[cfg(test)]
mod project_pr_template_tests;
#[cfg(test)]
mod provider_env_file_tests;
#[cfg(test)]
mod prune_engine_tests;
#[cfg(test)]
mod publish_resilience_git_safety_tests;
#[cfg(test)]
mod publish_resilience_legacy_tests;
#[cfg(test)]
mod publish_resilience_tests;
#[cfg(test)]
mod pull_request_detail_tests;
#[cfg(test)]
mod recovery_queue_tests;
#[cfg(test)]
mod session_export_service_tests;
#[cfg(test)]
mod session_namer_agent_tests;
#[cfg(test)]
mod session_namer_prompt_tests;
#[cfg(test)]
mod startup_background_tests;
#[cfg(test)]
mod startup_failure_classification_tests;
#[cfg(test)]
mod startup_status_guard_tests;
#[cfg(test)]
mod startup_status_tests;
#[cfg(test)]
mod task_cleanup_service_tests;
#[cfg(test)]
mod task_transition_service_tests;
#[cfg(test)]
mod throttled_emitter_tests;
#[cfg(test)]
mod ticketing_cache_invalidator_tests;
#[cfg(test)]
mod ticketing_pr_summary_tests;
#[cfg(test)]
mod validation_service_tests;
#[cfg(test)]
mod verification_event_emitters_tests;
#[cfg(test)]
mod webhook_service_tests;

// Unified chat service (handles all chat contexts: ideation, task, project, task_execution)
pub use chat_service::{
    AgentChunkPayload, AgentErrorPayload, AgentMessageCreatedPayload, AgentMessageQueuedPayload,
    AgentQueueSentPayload, AgentRunCompletedPayload, AgentRunStartedPayload, AgentToolCallPayload,
    AppChatService, ChatConversationWithMessages, ChatService, ChatServiceError, MockChatResponse,
    MockChatService, SendResult, AGENT_MESSAGE_QUEUED,
};
pub mod agent_capability_gate;
#[cfg(test)]
mod agent_capability_gate_tests;
pub mod agent_capability_validation;
#[cfg(test)]
mod agent_capability_validation_tests;
pub mod agent_workflow_runner;
