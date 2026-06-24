// Application layer - dependency injection and service orchestration
// This layer bridges the domain and infrastructure layers

pub mod agent_lane_settings_bootstrap;
pub mod agent_lane_resolution;
pub mod agent_conversation_fork;
pub mod agent_conversation_jira_issue;
pub mod agent_conversation_linear_issue;
pub mod agent_conversation_start_service;
pub mod agent_conversation_workspace;
pub mod agent_conversation_workspace_base;
pub mod agent_issue_report;
pub(crate) mod agent_planning_session_titles;
pub mod agent_workspace_pr_description;
pub mod agent_workspace_external_pr_reconciliation;
pub(crate) mod plan_pr_description;
pub(crate) mod agent_workspace_pr_supervision_recovery;
pub mod agent_workspace_publish_recovery;
pub mod agent_workspace_bridge;
pub mod agent_terminal;
pub mod agent_client_bundle;
pub mod agent_task_service;
pub mod app_setup;
pub mod app_state;
pub mod chat_attachment_storage;
pub mod http_shutdown;
#[cfg(test)]
mod http_shutdown_tests;
#[cfg(test)]
mod shutdown_tests;
pub mod ideation_harness_availability;
pub mod ideation_workspace;
pub mod apply_service;
pub mod atlassian_integration_service;
pub mod chat_attachment_service;
pub mod chat_resumption;
pub mod chat_service;
pub mod clickup_integration_service;
pub mod dependency_service;
pub mod event_cleanup_service;
pub mod execution_settings_bootstrap;
pub mod external_issue_link_service;
pub mod ideation_effort_bootstrap;
pub mod ideation_model_bootstrap;
pub mod diff_service;
pub mod git_service;
pub(crate) mod git_artifact_cleanup;
pub mod harness_runtime_registry;
pub mod ideation_service;
pub mod interactive_process_registry;
pub mod linear_integration_service;
pub mod linear_webhook_reconciliation_service;
pub(crate) mod managed_provider_cli;
pub mod memory_archive_service;
pub mod memory_orchestration;
#[cfg(all(dev, target_os = "macos"))]
pub(crate) mod dev_dock_icon;
pub(crate) mod native_menu;
pub(crate) mod orphan_worktree_cleanup;
pub mod pending_session_drain;
pub mod permission_state;
pub(crate) mod plan_complexity_assessment;
pub mod plan_ranking;
pub mod priority_service;
pub(crate) mod provider_onboarding_gate;
pub mod provider_session_fork;
pub mod pr_startup_recovery;
pub mod prune_engine;
pub mod publish_resilience;
pub mod qa_service;
pub mod question_state;
pub mod ready_task_scheduler;
pub mod reconciliation;
pub mod recovery_queue;
pub mod runtime_factory;
pub mod runtime_wiring;
pub mod server_boot;
pub mod resume_validator;
pub mod services;
pub mod review_issue_service;
pub mod review_service;
pub mod session_export_service;
pub(crate) mod session_namer_agent;
pub mod session_namer_prompt;
pub mod session_reopen_service;
pub mod setup_settings;
pub mod shutdown;
pub mod startup_jobs;
pub mod startup_background;
pub mod startup_bootstrap;
pub mod startup_cleanup;
pub mod startup_git_auth_preflight;
pub mod startup_pipeline;
pub mod startup_pipeline_launch;
pub mod startup_runtime_builders;
pub mod supervisor_service;
pub mod startup_transition_factory;
pub mod task_cleanup_service;
pub mod task_context_service;
pub mod task_scheduler_service;
pub mod task_transition_service;
pub mod throttled_emitter;
pub mod team_events;
pub mod team_service;
pub mod team_state_tracker;
pub mod team_stream_processor;
pub mod ticket_canonical_branch;
pub mod ticketing_cache_invalidator;
pub mod ticketing_pr_summary;
pub mod ticketing_service;
pub mod webhook_service;

// Re-export commonly used items
pub use app_state::AppState;
pub use http_shutdown::HttpShutdownHandle;
pub(crate) use agent_client_bundle::AgentClientBundle;
pub(crate) use harness_runtime_registry::probe_supported_harnesses;
pub use agent_lane_settings_bootstrap::{
    load_or_seed_agent_lane_settings_defaults, AgentLaneSettingsBootstrapResult,
};
pub(crate) use ideation_harness_availability::{
    build_lane_harness_availability, resolve_lane_harness_config,
    resolve_primary_ideation_harness_availability, team_mode_supported_for_context,
    validate_chat_runtime_for_context, validate_chat_runtime_for_context_with_override, AGENT_LANES,
    IDEATION_LANES,
};
pub use apply_service::{
    ApplyProposalsOptions, ApplyProposalsResult, ApplyService, SelectionValidation, TargetColumn,
};
pub use atlassian_integration_service::{
    AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity, AtlassianCredential,
    AtlassianIntegrationService, AtlassianJiraAttachment, AtlassianJiraComment,
    AtlassianJiraTransition, AtlassianOAuthAuthorization, AtlassianOAuthResource,
    AtlassianOAuthTokenResponse,
    AtlassianResourceContent, AtlassianResourceKind, AtlassianResourceSummary,
    EmptyAtlassianApiClient, JiraIssueDetail, JiraProjectSummary, JiraStatusSummary,
    UnavailableAtlassianApiClient,
};
pub use clickup_integration_service::{
    ClickUpApiClient, ClickUpAuthContext, ClickUpComment, ClickUpIntegrationService, ClickUpSpace,
    ClickUpStatus, ClickUpTag, ClickUpTaskContent, ClickUpTaskSummary, ClickUpUser,
    ClickUpWorkspace, EmptyClickUpApiClient, UnavailableClickUpApiClient,
};
pub use chat_attachment_service::ChatAttachmentService;
pub use agent_terminal::AgentTerminalService;
pub use agent_task_service::AgentTaskService;
pub use agent_issue_report::{
    build_agent_issue_report_draft, submit_agent_issue_report, AgentIssueReportDestination,
    AgentIssueReportDestinationSource, AgentIssueReportDraft, AgentIssueReportEnvironment,
    AgentIssueReportSource, AgentIssueReportSubmitResponse, BuildAgentIssueReportInput,
    SubmitAgentIssueReportInput,
};
pub use chat_resumption::ChatResumptionRunner;
pub use dependency_service::{DependencyAnalysis, DependencyService, ValidationResult};
pub use event_cleanup_service::EventCleanupService;
pub use execution_settings_bootstrap::{
    load_or_seed_execution_settings_defaults, ExecutionSettingsBootstrapResult,
};
pub use external_issue_link_service::ExternalIssueLinkService;
pub use diff_service::{
    ConflictDiff, DiffHunk, DiffLine, DiffLineKind, DiffPageRow, DiffRefKind, DiffService,
    DiffSide, FileChange, FileChangeStatus, FileDiff, FileDiffPage, RangeLine,
};
pub use git_service::{
    checkout_free::CheckoutFreeMergeResult, CommitInfo, DiffStats, GitService, MergeAttemptResult,
    MergeResult, RebaseResult,
};
pub use interactive_process_registry::{InteractiveProcessKey, InteractiveProcessRegistry};
pub use linear_integration_service::{
    resolve_linear_label_ids, EmptyLinearApiClient, LinearApiClient, LinearAuthContext,
    LinearIntegrationService, LinearComment, LinearIntegrationSettings,
    LinearIntegrationSettingsRepository, LinearIssueContent, LinearIssueSummary, LinearLabel,
    LinearProject, LinearUser, LinearWorkflowState, UnavailableLinearApiClient,
};
pub use ideation_service::{
    CreateProposalOptions, IdeationService, SessionStats, SessionWithData, UpdateProposalOptions,
    UpdateSource,
};
pub use linear_webhook_reconciliation_service::{
    ExternalIssueLink, LinearWebhookAction, LinearWebhookError, LinearWebhookHeaders,
    LinearWebhookOutcome, LinearWebhookReconciliationService, LinearWebhookRequest,
    LinearWebhookStore, MemoryLinearWebhookStore,
};
pub use memory_archive_service::MemoryArchiveService;
pub use permission_state::{PendingPermissionInfo, PermissionDecision, PermissionState};
pub use plan_ranking::{
    compute_activity_score, compute_final_score, compute_final_score_with_breakdown,
    compute_interaction_score, compute_recency_score, ScoreBreakdown,
};
pub use priority_service::PriorityService;
pub(crate) use provider_onboarding_gate::{
    ensure_provider_spawn_enabled, resolve_enabled_default_provider,
};
pub use recovery_queue::{ProcessSummary, RecoveryItem, RecoveryPriority, RecoveryQueue};
pub use ready_task_scheduler::spawn_ready_task_scheduler_if_needed;
pub use prune_engine::PruneEngine;
pub use qa_service::{QAPrepStatus, QAService, TaskQAState};
pub use question_state::{PendingQuestionInfo, QuestionAnswer, QuestionOption, QuestionState};
pub use reconciliation::ReconciliationRunner;
pub use ticketing_cache_invalidator::{
    TicketingCacheInvalidatedEvent, TicketingCacheInvalidator,
    TICKETING_CACHE_INVALIDATED_EVENT,
};
pub use ticketing_service::{
    TauriTicketingEventSink, TicketAssignRequest, TicketCommentRequest, TicketSetLabelsRequest,
    TicketTransitionRequest, TicketingCommentResult, TicketingEventSink, TicketingLabelResult,
    TicketingMutationResult, TicketingPersonResult, TicketingOperationEvent, TicketingService,
    TicketingTicketIdentity, TicketingTransitionOption, TICKETING_OPERATION_EVENT,
};
pub use services::PrPollerRegistry;
pub use resume_validator::{ResumeValidationResult, ResumeValidator};
pub use review_issue_service::{CreateIssueInput, ReviewIssueService};
pub use review_service::ReviewService;
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
pub use team_service::TeamService;
pub use team_state_tracker::TeamStateTracker;
pub use webhook_service::WebhookService;

#[cfg(test)]
mod agent_conversation_workspace_base_tests;
#[cfg(test)]
mod agent_issue_report_tests;
#[cfg(test)]
mod agent_workspace_external_pr_reconciliation_tests;
#[cfg(test)]
mod agent_workspace_pr_supervision_recovery_tests;
#[cfg(test)]
mod app_state_shared_state_tests;
#[cfg(test)]
mod agent_lane_resolution_tests;
#[cfg(test)]
mod agent_terminal_tests;
#[cfg(test)]
mod chat_service_output_tests;
#[cfg(test)]
mod clickup_integration_service_tests;
#[cfg(test)]
mod git_artifact_cleanup_tests;
#[cfg(test)]
mod harness_runtime_registry_tests;
#[cfg(test)]
mod ideation_harness_availability_tests;
#[cfg(test)]
mod ideation_workspace_tests;
#[cfg(test)]
mod orphan_worktree_cleanup_tests;
#[cfg(test)]
mod pr_startup_recovery_tests;
#[cfg(test)]
mod recovery_queue_tests;
#[cfg(test)]
mod runtime_wiring_tests;
#[cfg(test)]
mod webhook_service_tests;
#[cfg(test)]
mod prune_engine_tests;
#[cfg(test)]
mod publish_resilience_tests;
#[cfg(test)]
mod plan_complexity_assessment_tests;
#[cfg(test)]
mod plan_pr_description_tests;
#[cfg(test)]
mod session_export_service_tests;
#[cfg(test)]
mod session_namer_agent_tests;
#[cfg(test)]
mod agent_planning_session_titles_tests;
#[cfg(test)]
mod session_namer_prompt_tests;
#[cfg(test)]
mod throttled_emitter_tests;
#[cfg(test)]
mod ticketing_cache_invalidator_tests;
#[cfg(test)]
mod ticketing_pr_summary_tests;
#[cfg(test)]
mod task_transition_service_tests;

// Unified chat service (handles all chat contexts: ideation, task, project, task_execution)
pub use chat_service::{
    AppChatService,
    AgentChunkPayload, AgentErrorPayload, AgentMessageCreatedPayload, AgentMessageQueuedPayload,
    AgentQueueSentPayload, AgentRunCompletedPayload, AgentRunStartedPayload, AgentToolCallPayload,
    ChatConversationWithMessages, ChatService, ChatServiceError,
    MockChatResponse, MockChatService, SendResult, TeamCostUpdatePayload, TeamCreatedPayload,
    TeamDisbandedPayload, TeamMessagePayload, TeamTeammateIdlePayload, TeamTeammateShutdownPayload,
    TeamTeammateSpawnedPayload, AGENT_MESSAGE_QUEUED,
};
