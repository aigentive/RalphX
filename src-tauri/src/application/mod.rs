// Application layer - dependency injection and service orchestration
// This layer bridges the domain and infrastructure layers

pub mod agent_lane_settings_bootstrap;
pub mod agent_lane_resolution;
pub mod agent_conversation_workspace;
pub mod agent_conversation_workspace_base;
pub mod agent_workspace_pr_description;
pub mod agent_workspace_publish_recovery;
pub mod agent_workspace_bridge;
pub mod agent_terminal;
pub mod agent_client_bundle;
pub mod app_setup;
pub mod app_state;
pub mod ideation_harness_availability;
pub mod ideation_workspace;
pub mod apply_service;
pub mod chat_attachment_service;
pub mod chat_resumption;
pub mod chat_service;
pub mod dependency_service;
pub mod event_cleanup_service;
pub mod execution_settings_bootstrap;
pub mod ideation_effort_bootstrap;
pub mod ideation_model_bootstrap;
pub mod diff_service;
pub mod git_service;
pub(crate) mod git_artifact_cleanup;
pub mod harness_runtime_registry;
pub mod ideation_service;
pub mod interactive_process_registry;
pub mod memory_archive_service;
pub mod memory_orchestration;
pub mod pending_session_drain;
pub mod permission_state;
pub mod plan_ranking;
pub mod priority_service;
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
pub mod webhook_service;

// Re-export commonly used items
pub use app_state::AppState;
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
pub use chat_attachment_service::ChatAttachmentService;
pub use agent_terminal::AgentTerminalService;
pub use chat_resumption::ChatResumptionRunner;
pub use dependency_service::{DependencyAnalysis, DependencyService, ValidationResult};
pub use event_cleanup_service::EventCleanupService;
pub use execution_settings_bootstrap::{
    load_or_seed_execution_settings_defaults, ExecutionSettingsBootstrapResult,
};
pub use diff_service::{ConflictDiff, DiffService, FileChange, FileChangeStatus, FileDiff};
pub use git_service::{
    checkout_free::CheckoutFreeMergeResult, CommitInfo, DiffStats, GitService, MergeAttemptResult,
    MergeResult, RebaseResult,
};
pub use interactive_process_registry::{InteractiveProcessKey, InteractiveProcessRegistry};
pub use ideation_service::{
    CreateProposalOptions, IdeationService, SessionStats, SessionWithData, UpdateProposalOptions,
    UpdateSource,
};
pub use memory_archive_service::MemoryArchiveService;
pub use permission_state::{PendingPermissionInfo, PermissionDecision, PermissionState};
pub use plan_ranking::{
    compute_activity_score, compute_final_score, compute_final_score_with_breakdown,
    compute_interaction_score, compute_recency_score, ScoreBreakdown,
};
pub use priority_service::PriorityService;
pub use recovery_queue::{ProcessSummary, RecoveryItem, RecoveryPriority, RecoveryQueue};
pub use ready_task_scheduler::spawn_ready_task_scheduler_if_needed;
pub use prune_engine::PruneEngine;
pub use qa_service::{QAPrepStatus, QAService, TaskQAState};
pub use question_state::{PendingQuestionInfo, QuestionAnswer, QuestionOption, QuestionState};
pub use reconciliation::ReconciliationRunner;
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
mod app_state_shared_state_tests;
#[cfg(test)]
mod agent_lane_resolution_tests;
#[cfg(test)]
mod agent_terminal_tests;
#[cfg(test)]
mod chat_service_output_tests;
#[cfg(test)]
mod git_artifact_cleanup_tests;
#[cfg(test)]
mod ideation_harness_availability_tests;
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
mod session_export_service_tests;
#[cfg(test)]
mod session_namer_agent_tests;
#[cfg(test)]
mod session_namer_prompt_tests;
#[cfg(test)]
mod throttled_emitter_tests;
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
