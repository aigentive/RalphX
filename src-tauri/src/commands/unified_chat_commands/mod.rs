// Unified Tauri commands for all chat contexts
//
// These commands use the unified ChatService that consolidates
// OrchestratorService and ExecutionChatService functionality.
//
// Event namespace: agent:* (instead of chat:*/execution:*)
// - agent:run_started - Agent begins processing
// - agent:chunk - Streaming text chunk
// - agent:tool_call - Tool invocation
// - agent:message_created - Message persisted
// - agent:run_completed - Agent finished successfully (or agent:turn_completed in interactive mode)
// - agent:error - Agent failed
// - agent:queue_sent - Queued message sent
// - agent:startup_progress - Project agent startup phase label for chat typing indicator

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use tauri::{Emitter, Manager, Runtime, State};

use crate::application::agent_conversation_archive::{
    archive_agent_conversation_for_state, close_agent_workspace_pr_for_state,
};
use crate::application::agent_conversation_fork::{
    fork_agent_conversation as fork_agent_conversation_in_state, AgentConversationForkResult,
};
#[doc(hidden)]
pub use crate::application::agent_conversation_mode_switch::AUTOMATION_RUN_MODE_LOCKED_ERROR_CODE;
use crate::application::agent_conversation_mode_switch::{
    automation_run_mode_locked_error_message, is_automation_run_mode_switch_locked,
};
use crate::application::agent_conversation_start_service::{
    AgentConversationStartDeps, AgentConversationStartService,
};
pub use crate::application::agent_conversation_start_service::{
    AgentWorkspaceSourcePullRequestInput, StartAgentConversationInput,
};
use crate::application::agent_conversation_workspace::{
    ensure_linked_plan_branch_agent_worktree, is_terminal_agent_conversation_publication_status,
    prepare_agent_conversation_workspace_with_setup_mode_and_defaults,
    reject_persona_builder_workspace_mode, resolve_agent_conversation_workspace_path_for_send,
    resolve_valid_agent_conversation_workspace_path, AgentConversationWorkspaceBaseSelection,
    AgentConversationWorkspacePrAutomationDefaults, AgentConversationWorkspaceSetupMode,
};
use crate::application::agent_conversation_workspace_base::{
    apply_workspace_base_resolution, resolve_workspace_base,
    resolve_workspace_base_from_local_snapshot, BaseResolutionResult, BaseStatus,
};
use crate::application::agent_plan_context::{
    admit_linked_edit_plan_references, linked_workspace_planning_session_is_reusable,
};
use crate::application::agent_planning_session_titles::{
    hydrate_agent_conversation_planning_session_title,
    sync_linked_planning_session_title_from_conversation,
};
use crate::application::agent_workspace_bridge::{
    wake_agent_workspace_for_bridge_events,
    wake_agent_workspace_for_bridge_events_with_service_factory,
};
use crate::application::agent_workspace_external_pr_reconciliation::{
    external_pr_reconciliation_skip_reason,
    schedule_agent_workspace_external_pr_reconciliation_with_lazy_deps,
    AgentWorkspaceExternalPrReconciliationDeps, AgentWorkspaceExternalPrReconciliationTrigger,
};
use crate::application::agent_workspace_local_commit::{
    commit_agent_workspace_locally, AgentWorkspaceLocalCommitRequest,
};
use crate::application::agent_workspace_pr_autofix_attempt::{
    load_pr_autofix_completion_authority, PrAutofixCompletionAuthority,
};
use crate::application::agent_workspace_pr_description::{
    draft_agent_workspace_pr_metadata_decision, get_or_draft_agent_workspace_pr_metadata_decision,
    invalidate_agent_workspace_pr_description_cache, AgentWorkspacePrDescriptionCacheKey,
    ExistingPrMetadataSnapshot, ResolvedAgentWorkspacePrTarget,
};
use crate::application::agent_workspace_pr_supervision_recovery::{
    build_agent_workspace_pr_supervision_recovery_deps,
    schedule_agent_workspace_pr_supervision_recovery_with_lazy_deps,
    AgentWorkspacePrFixReviewPublishResumer, AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::agent_workspace_publish_recovery::recover_stale_publish_repair_for_workspace_in_state;
use crate::application::agent_workspace_publish_repair_state::{
    classify_agent_workspace_repair_delivery, reserve_agent_workspace_repair_dispatch,
    resume_current_agent_workspace_repair_publish, resume_ready_agent_workspace_repair_for_publish,
    settle_agent_workspace_repair_dispatch_outcome, start_or_join_agent_workspace_repair,
    AgentWorkspaceRepairDispatchOutcome, AgentWorkspaceRepairDispatchSettlement,
    AgentWorkspaceRepairPublishResumeOutcome, AgentWorkspaceRepairStartOutcome,
    AgentWorkspaceRepairStartRequest, AgentWorkspaceRepairTransitionOutcome,
    DEFERRED_REPAIR_WAIT_TIMEOUT_SECS,
};
use crate::application::agent_workspace_review::{
    load_workspace_review_publish_blocker, lock_workspace_review_lifecycle,
};
use crate::application::agent_workspace_review_base::resolve_agent_workspace_review_base;
use crate::application::agent_workspace_terminal_cleanup::TerminalAgentWorkspaceOutcome;
use crate::application::chat_service::tool_result_preview::{
    preview_tool_arguments_object, preview_tool_result_object, tool_detail_ref,
};
use crate::application::chat_service::{
    message_metadata_hidden_from_ui, running_state_from_run_status_and_idle,
    AgentConversationCreatedPayload, AgentRunningState, AgentRuntimeStatus, ChatServiceError,
    SendMessageOptions, SendQueuePolicy,
};
use crate::application::git_service::{
    git_cmd::{self, GitCommandLane},
    GitService,
};
use crate::application::ideation_workspace::prepare_ideation_analysis_state_from_agent_workspace;
use crate::application::personas::{PERSONA_FEATURE_DISABLED_PREFIX, PERSONA_UNAVAILABLE_PREFIX};
use crate::application::publish_resilience::{
    classify_publish_failure, continue_agent_workspace_repair_publish,
    count_publish_reviewable_commits, count_publishable_commits_with_base_fallback,
    count_unpublished_publish_commits, ensure_plan_publish_branch_fresh,
    ensure_publish_base_pushed, ensure_publish_branch_fresh,
    inspect_publish_branch_freshness_for_source,
    inspect_publish_branch_freshness_for_source_after_fetch, push_publish_branch,
    remote_tracking_ref_for_publish, review_base_for_publish,
    verify_agent_workspace_repair_pr_handoff, AgentWorkspaceRepairPrHandoff,
    AgentWorkspaceRepairPrHandoffResult, AgentWorkspaceRepairPublishContinuation,
    AgentWorkspaceRepairPushOutcome, PublishAfterRepairPushError, PublishBranchFreshnessOutcome,
    PublishBranchFreshnessStatus, PublishFailureClass,
};
use crate::application::services::pr_auto_merge_status::{
    auto_merge_disable_failure_summary, auto_merge_enable_failure_summary,
    AUTO_MERGE_SUPERVISION_STATUS_WAITING,
};
use crate::application::services::pr_merge_poller::sync_agent_workspace_auto_merge_preference_for_workspace;
use crate::application::session_namer_agent::{spawn_session_namer_agent, SessionNamerTarget};
use crate::application::{AppChatService, AppState, ChatService, SendResult};
use crate::commands::agent_model_commands::load_agent_model_registry;
use crate::commands::ExecutionState;
use crate::domain::agents::{
    default_effort_for_provider, default_efforts_for_provider, AgentHarnessKind, LogicalEffort,
    RoutingRole, DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::plan_branch::PrPushStatus;
use crate::domain::entities::task_step::StepProgressSummary;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentConversationWorkspacePublicationEvent, AgentRun,
    AgentRunId, AgentRunStatus, AgentWorkspacePrDescription, AgentWorkspacePrMetadataDecision,
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairContinuation, AgentWorkspaceRepairPhase,
    AgentWorkspaceRepairSource, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceSourcePullRequest,
    ArtifactContent, ChatAttachmentId, ChatContextType, ChatConversation, ChatConversationId,
    ChatMessage, ChatMessageId, ChatTimelineItem, CoordinationMode, DelegatedSessionId,
    ExecutionPlanStatus, GitTargetIdentity, IdeationAnalysisBaseRefKind, IdeationSession,
    IdeationSessionFlow, IdeationSessionId, InternalStatus, PersonaId, PlanBranch,
    PlanBranchStatus, Project, ProjectId, Task, TaskCategory, TeamIntent, TeamMessageTarget,
    DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::execution::{
    build_running_ideation_session, build_running_process, context_matches_running_status,
    elapsed_seconds_for_status, RunningIdeationSession, RunningProcess,
};
use crate::domain::services::github_service::GithubServiceTrait;
use crate::domain::services::pr_publish_service::AgentWorkspacePrPublishOutcome;
use crate::domain::services::{
    normalize_title_with_jira_key, primary_jira_key_from_composer_metadata,
    AgentWorkspacePrPublisher, ComposerArtifactReference, ComposerExcerptReference,
    ComposerIntegrationReference, ComposerProjectReference, ComposerSelectionSnapshot,
    QueuedMessage, RunningAgentKey, RunningAgentRegistry,
};
use crate::domain::state_machine::transition_handler::get_trigger_origin;
use crate::error::AppError;
use crate::infrastructure::agents::agent_personas_enabled;
use crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_REPAIR;
use crate::infrastructure::agents::claude::{git_runtime_config, ui_feature_flags_config};
use crate::infrastructure::git_auth::{inspect_repository_capability, RepositoryCapability};

const AGENT_WORKSPACE_REPAIR_REQUESTED_STEP: &str = "repair_requested";
const AGENT_WORKSPACE_REPAIR_ACTION_PREFIX: &str = "agent_fixable:";
const AGENT_WORKSPACE_REPAIR_ACTION_PUBLISH: &str = "publish";
const AGENT_WORKSPACE_REPAIR_ACTION_UPDATE_ONLY: &str = "update_only";
pub const AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE: &str =
    "Agent workspace publish is already in progress";
pub(crate) const PERSONA_SWITCH_AGENT_RUNNING_ERROR: &str =
    "Cannot change persona while the agent is running";

fn agent_workspace_interactive_slot_key(conversation_id: &ChatConversationId) -> String {
    format!("{}/{}", ChatContextType::Project, conversation_id.as_str())
}

// ============================================================================
// Request/Response types
// ============================================================================

/// Input for send_agent_message command
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAgentMessageInput {
    pub context_type: String,
    pub context_id: String,
    pub content: String,
    /// Optional existing conversation to continue.
    pub conversation_id: Option<String>,
    /// Optional provider harness selected for this send. Existing conversations switch
    /// provider by starting a fresh provider-native session when the harness changes.
    pub provider_harness: Option<String>,
    /// Optional explicit model override for the spawned agent.
    pub model_override: Option<String>,
    /// Optional provider-neutral reasoning effort override for the spawned agent.
    pub logical_effort: Option<LogicalEffort>,
    /// Optional Codex Fast Mode override for this send.
    pub codex_fast_mode: Option<bool>,
    /// Complete permission-free runtime tuple for a backend-derived role launch.
    pub runtime_override: Option<crate::domain::agents::ManualRoleRuntimeOverride>,
    /// Internal handoff messages should reach the runtime without rendering as user chat.
    #[serde(default)]
    pub suppress_user_message: bool,
    /// Require the linked Edit workspace's current plan bundle to retain exact user approval.
    #[serde(default)]
    pub require_approved_linked_plan: bool,
    /// Opaque backend activation receipt that pins a direct implementation send to one plan pair.
    #[serde(default)]
    pub expected_linked_plan_fingerprint: Option<String>,
    /// Structured composer project references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_project_references: Vec<ComposerProjectReference>,
    /// Structured external integration references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_integration_references: Vec<ComposerIntegrationReference>,
    /// Structured artifact references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_artifact_references: Vec<ComposerArtifactReference>,
    /// Immutable whole-line artifact or ticket excerpt selected for this turn.
    pub composer_selection_snapshot: Option<ComposerSelectionSnapshot>,
    /// Bounded plain-text excerpts selected from the artifact pane.
    #[serde(default)]
    pub composer_excerpt_references: Vec<ComposerExcerptReference>,
    /// Optional native team-mode overlay request for this send.
    #[serde(alias = "capabilityIntent")]
    pub team_intent: Option<TeamIntent>,
    /// Optional native team mailbox target.
    pub team_message_target: Option<TeamMessageTarget>,
    /// Attachment IDs selected by the composer for this message.
    #[serde(default)]
    pub attachment_ids: Vec<String>,
}

fn hidden_user_message_metadata() -> String {
    serde_json::json!({
        "source": "hidden_user_message",
        "resume_in_place": true,
        "persist_hidden_marker": true,
        "hidden_from_ui": true,
        "recovery_context": true,
    })
    .to_string()
}

#[doc(hidden)]
pub fn validate_persona_builder_team_intent_for_send(
    context_type: ChatContextType,
    persisted_conversation: Option<&ChatConversation>,
    requested_capability: CoordinationMode,
) -> Result<(), String> {
    let persona_builder_conversation = persisted_conversation.is_some_and(|conversation| {
        conversation.agent_mode == Some(AgentConversationWorkspaceMode::PersonaBuilder)
    });
    if (context_type == ChatContextType::Standalone || persona_builder_conversation)
        && requested_capability != CoordinationMode::Solo
    {
        return Err(if persona_builder_conversation {
            "Team mode is not supported for persona builder conversations".to_string()
        } else {
            STANDALONE_TEAM_INTENT_REJECTED_ERROR.to_string()
        });
    }
    Ok(())
}

/// Response from send_agent_message command
#[derive(Debug, Serialize)]
pub struct SendAgentMessageResponse {
    pub conversation_id: String,
    pub agent_run_id: String,
    pub is_new_conversation: bool,
    #[serde(default)]
    pub was_queued: bool,
    #[serde(default)]
    pub queued_as_pending: bool,
    #[serde(default)]
    pub queued_message_id: Option<String>,
}

fn parse_chat_attachment_ids(raw_ids: &[String]) -> Result<Vec<ChatAttachmentId>, String> {
    raw_ids
        .iter()
        .map(|id| {
            id.parse::<ChatAttachmentId>()
                .map_err(|_| format!("Invalid attachment id: {}", id))
        })
        .collect()
}

#[cfg(test)]
mod chat_attachment_id_parser_tests {
    use super::{
        parse_chat_attachment_ids, visible_queued_message_responses, ComposerSelectionSnapshot,
        QueuedMessageResponse,
    };
    use crate::domain::entities::ChatAttachmentId;
    use crate::domain::services::QueuedMessage;

    #[test]
    fn parses_chat_attachment_ids_and_reports_invalid_values() {
        let first = ChatAttachmentId::new();
        let second = ChatAttachmentId::new();

        let parsed = parse_chat_attachment_ids(&[first.as_str(), second.as_str()])
            .expect("valid ids should parse");

        assert_eq!(parsed, vec![first, second]);
        assert_eq!(
            parse_chat_attachment_ids(&["not-a-uuid".to_string()]).unwrap_err(),
            "Invalid attachment id: not-a-uuid"
        );
    }

    #[test]
    fn queued_message_response_includes_attachment_ids() {
        let attachment_id = ChatAttachmentId::new();
        let mut queued = QueuedMessage::new("queued with file".to_string());
        queued.attachment_ids = vec![attachment_id];
        queued.composer_selection_snapshot = Some(ComposerSelectionSnapshot {
            source_type: "ticket".to_string(),
            source_kind: "jira".to_string(),
            source_id: "10042".to_string(),
            source_title: Some("Queue recovery".to_string()),
            source_key: Some("RX-42".to_string()),
            provider: Some("atlassian".to_string()),
            artifact_version: None,
            source_revision: None,
            start_line: 2,
            end_line: 2,
            content: "selected".to_string(),
        });

        let response = QueuedMessageResponse::from(queued);

        assert_eq!(response.attachment_ids, vec![attachment_id.to_string()]);
        assert_eq!(
            response
                .composer_selection_snapshot
                .as_ref()
                .map(|snapshot| snapshot.source_key.as_deref()),
            Some(Some("RX-42"))
        );
    }

    #[test]
    fn visible_queued_message_responses_omits_hidden_messages() {
        let visible = QueuedMessage::new("visible follow-up".to_string());
        let mut hidden = QueuedMessage::new("internal handoff".to_string());
        hidden.metadata_override = Some(r#"{"hidden_from_ui":true}"#.to_string());

        let responses = visible_queued_message_responses(vec![visible, hidden]);

        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].content, "visible follow-up");
    }
}

impl From<SendResult> for SendAgentMessageResponse {
    fn from(result: SendResult) -> Self {
        Self {
            conversation_id: result.conversation_id,
            agent_run_id: result.agent_run_id,
            is_new_conversation: result.is_new_conversation,
            was_queued: result.was_queued,
            queued_as_pending: result.queued_as_pending,
            queued_message_id: result.queued_message_id,
        }
    }
}

/// Response for an agent conversation workspace.
#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceSourcePullRequestResponse {
    pub number: i64,
    pub url: Option<String>,
    pub title: Option<String>,
    pub head_ref_name: String,
    pub base_ref_name: Option<String>,
    pub head_ref_oid: Option<String>,
}

impl From<AgentWorkspaceSourcePullRequest> for AgentWorkspaceSourcePullRequestResponse {
    fn from(pull_request: AgentWorkspaceSourcePullRequest) -> Self {
        Self {
            number: pull_request.number,
            url: pull_request.url,
            title: pull_request.title,
            head_ref_name: pull_request.head_ref_name,
            base_ref_name: pull_request.base_ref_name,
            head_ref_oid: pull_request.head_ref_oid,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentConversationWorkspaceResponse {
    pub conversation_id: String,
    pub project_id: String,
    pub mode: String,
    pub branch_mode: String,
    pub base_ref_kind: String,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub base_commit: Option<String>,
    pub branch_name: String,
    pub worktree_path: String,
    pub linked_ideation_session_id: Option<String>,
    pub task_pipeline_session_id: Option<String>,
    pub task_pipeline_available: bool,
    pub linked_plan_branch_id: Option<String>,
    pub source_pull_request: Option<AgentWorkspaceSourcePullRequestResponse>,
    pub publication_pr_number: Option<i64>,
    pub publication_pr_url: Option<String>,
    pub publication_pr_status: Option<String>,
    pub publication_push_status: Option<String>,
    pub auto_publish_enabled: bool,
    pub auto_publish_initial_pr_enabled: bool,
    pub auto_publish_paused_pr_autofix_enabled: Option<bool>,
    pub auto_publish_paused_pr_auto_merge_desired: Option<bool>,
    pub pr_autofix_enabled: bool,
    pub pr_auto_merge_desired: bool,
    pub pr_auto_merge_method: String,
    pub pr_auto_merge_current: Option<bool>,
    pub pr_supervision_status: Option<String>,
    pub pr_supervision_summary: Option<String>,
    pub pr_supervision_updated_at: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub mode_switch_locked: bool,
    pub mode_switch_lock_reason: Option<String>,
    pub maintenance_operation:
        Option<crate::domain::entities::AgentWorkspaceRepairOperationSnapshot>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationWorkspacePrSupervisionInput {
    pub auto_fix_enabled: bool,
    pub auto_merge_desired: bool,
    pub auto_merge_method: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationWorkspaceAutoPublishInput {
    pub auto_publish_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkAgentConversationInput {
    pub conversation_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForkAgentConversationResponse {
    pub parent_conversation: AgentConversationResponse,
    pub conversation: AgentConversationResponse,
    pub workspace: Option<AgentConversationWorkspaceResponse>,
    pub provider_session_forked: bool,
    pub copied_message_count: usize,
    pub copied_timeline_item_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentConversationForkedPayload {
    pub parent_conversation_id: String,
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
}

impl From<AgentConversationWorkspace> for AgentConversationWorkspaceResponse {
    fn from(workspace: AgentConversationWorkspace) -> Self {
        let task_pipeline_available = workspace.task_pipeline_session_id.is_some();
        Self {
            conversation_id: workspace.conversation_id.as_str(),
            project_id: workspace.project_id.as_str().to_string(),
            mode: workspace.mode.to_string(),
            branch_mode: workspace.branch_mode.to_string(),
            base_ref_kind: workspace.base_ref_kind.to_string(),
            base_ref: workspace.base_ref,
            base_display_name: workspace.base_display_name,
            base_commit: workspace.base_commit,
            branch_name: workspace.branch_name,
            worktree_path: workspace.worktree_path,
            linked_ideation_session_id: workspace
                .linked_ideation_session_id
                .map(|id| id.as_str().to_string()),
            task_pipeline_session_id: workspace
                .task_pipeline_session_id
                .map(|id| id.as_str().to_string()),
            task_pipeline_available,
            linked_plan_branch_id: workspace
                .linked_plan_branch_id
                .map(|id| id.as_str().to_string()),
            source_pull_request: workspace
                .source_pull_request
                .map(AgentWorkspaceSourcePullRequestResponse::from),
            publication_pr_number: workspace.publication_pr_number,
            publication_pr_url: workspace.publication_pr_url,
            publication_pr_status: workspace.publication_pr_status,
            publication_push_status: workspace.publication_push_status,
            auto_publish_enabled: workspace.auto_publish_enabled,
            auto_publish_initial_pr_enabled: workspace.auto_publish_initial_pr_enabled,
            auto_publish_paused_pr_autofix_enabled: workspace
                .auto_publish_paused_pr_autofix_enabled,
            auto_publish_paused_pr_auto_merge_desired: workspace
                .auto_publish_paused_pr_auto_merge_desired,
            pr_autofix_enabled: workspace.pr_autofix_enabled,
            pr_auto_merge_desired: workspace.pr_auto_merge_desired,
            pr_auto_merge_method: workspace.pr_auto_merge_method,
            pr_auto_merge_current: workspace.pr_auto_merge_current,
            pr_supervision_status: workspace.pr_supervision_status,
            pr_supervision_summary: workspace.pr_supervision_summary,
            pr_supervision_updated_at: workspace
                .pr_supervision_updated_at
                .map(|value| value.to_rfc3339()),
            status: workspace.status.to_string(),
            created_at: workspace.created_at.to_rfc3339(),
            updated_at: workspace.updated_at.to_rfc3339(),
            mode_switch_locked: false,
            mode_switch_lock_reason: None,
            maintenance_operation: None,
        }
    }
}

fn project_plan_branch_publication_into_workspace_response(
    response: &mut AgentConversationWorkspaceResponse,
    plan_branch: &PlanBranch,
) {
    response.publication_pr_number = plan_branch.pr_number;
    response.publication_pr_url = plan_branch.pr_url.clone();
    response.publication_pr_status = if plan_branch.status == PlanBranchStatus::Merged {
        Some("merged".to_string())
    } else {
        plan_branch
            .pr_status
            .as_ref()
            .map(|status| status.to_db_string().to_ascii_lowercase())
    };
    response.publication_push_status = Some(plan_branch.pr_push_status.to_db_string().to_string());
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentConversationWorkspaceModeLock {
    locked: bool,
    reason: Option<String>,
}

impl AgentConversationWorkspaceModeLock {
    fn unlocked() -> Self {
        Self {
            locked: false,
            reason: None,
        }
    }

    fn locked(reason: impl Into<String>) -> Self {
        Self {
            locked: true,
            reason: Some(reason.into()),
        }
    }
}

async fn resolve_agent_conversation_workspace_mode_lock(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> Result<AgentConversationWorkspaceModeLock, String> {
    if let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() {
        let Some(plan_branch) = state
            .plan_branch_repo
            .get_by_id(plan_branch_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(AgentConversationWorkspaceModeLock::unlocked());
        };

        if plan_branch.status != PlanBranchStatus::Active {
            return Ok(AgentConversationWorkspaceModeLock::unlocked());
        }

        if let Some(execution_plan_id) = plan_branch.execution_plan_id.as_ref() {
            let execution_plan = state
                .execution_plan_repo
                .get_by_id(execution_plan_id)
                .await
                .map_err(|error| error.to_string())?;
            if execution_plan
                .as_ref()
                .is_some_and(|plan| plan.status != ExecutionPlanStatus::Active)
            {
                return Ok(AgentConversationWorkspaceModeLock::unlocked());
            }
        }

        return Ok(AgentConversationWorkspaceModeLock::locked(
            "Plan execution is still active",
        ));
    }

    if let Some(session_id) = workspace.linked_ideation_session_id.as_ref() {
        let Some(session) = state
            .ideation_session_repo
            .get_by_id(session_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(AgentConversationWorkspaceModeLock::unlocked());
        };

        if session.session_flow == IdeationSessionFlow::Planning {
            return Ok(AgentConversationWorkspaceModeLock::unlocked());
        }

        if session.is_active() && session.archived_at.is_none() && session.converted_at.is_none() {
            return Ok(AgentConversationWorkspaceModeLock::locked(
                "Ideation session is still active",
            ));
        }
    }

    Ok(AgentConversationWorkspaceModeLock::unlocked())
}

async fn ensure_plan_workspace_planning_session_link(
    state: &AppState,
    project: &Project,
    workspace: &mut AgentConversationWorkspace,
) -> Result<bool, String> {
    if workspace.mode != AgentConversationWorkspaceMode::Plan {
        return Ok(false);
    }

    if linked_workspace_planning_session_is_reusable(state, workspace).await? {
        return Ok(false);
    }

    let analysis = prepare_ideation_analysis_state_from_agent_workspace(project, workspace)
        .await
        .map_err(|error| error.to_string())?;
    let session = IdeationSession::builder()
        .project_id(workspace.project_id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(workspace.conversation_id.as_str())
        .spawn_reason("agent_plan_mode")
        .analysis(analysis)
        .build();
    let session = hydrate_agent_conversation_planning_session_title(state, session)
        .await
        .map_err(|error| error.to_string())?;
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .map_err(|error| error.to_string())?;

    workspace.linked_ideation_session_id = Some(session.id);
    workspace.linked_plan_branch_id = None;
    workspace.updated_at = chrono::Utc::now();
    Ok(true)
}

pub(crate) async fn ensure_plan_workspace_planning_session_link_for_send(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<bool, String> {
    let Some(mut workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };

    if workspace.mode != AgentConversationWorkspaceMode::Plan {
        return Ok(false);
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;

    if !ensure_plan_workspace_planning_session_link(state, &project, &mut workspace).await? {
        return Ok(false);
    }

    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .map_err(|error| error.to_string())?;
    Ok(true)
}

fn plan_branch_base_ref(plan_branch: &PlanBranch, project: &Project) -> String {
    plan_branch
        .base_branch_override
        .as_deref()
        .filter(|branch| !branch.is_empty())
        .or_else(|| {
            (!plan_branch.source_branch.is_empty()).then_some(plan_branch.source_branch.as_str())
        })
        .or(project.base_branch.as_deref())
        .unwrap_or("main")
        .to_string()
}

fn plan_branch_base_display_name(base_ref: &str) -> Option<String> {
    Some(format!("Current branch ({base_ref})"))
}

#[doc(hidden)]
#[derive(Debug, Clone)]
pub(crate) struct AgentConversationWorkspacePublishTarget {
    pub(crate) worktree_path: PathBuf,
    pub(crate) branch_name: String,
    pub(crate) base_ref: String,
    pub(crate) base_display_name: Option<String>,
    pub(crate) plan_branch: Option<PlanBranch>,
}

impl AgentConversationWorkspacePublishTarget {
    pub(crate) fn repair_target(&self) -> AgentConversationWorkspaceRepairTarget {
        AgentConversationWorkspaceRepairTarget {
            branch_name: self.branch_name.clone(),
            base_ref: self.base_ref.clone(),
            base_display_name: self.base_display_name.clone(),
            worktree_path: Some(self.worktree_path.clone()),
        }
    }
}

#[doc(hidden)]
pub(crate) async fn resolve_agent_workspace_publish_target(
    state: &AppState,
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> Result<AgentConversationWorkspacePublishTarget, String> {
    if workspace.mode == AgentConversationWorkspaceMode::Ideation {
        let plan_branch_id = workspace.linked_plan_branch_id.as_ref().ok_or_else(|| {
            "Ideation workspace without a linked plan branch cannot use publish actions".to_string()
        })?;
        let plan_branch = state
            .plan_branch_repo
            .get_by_id(plan_branch_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Plan branch not found: {}", plan_branch_id))?;
        let base_ref = plan_branch_base_ref(&plan_branch, project);
        let worktree_path = ensure_linked_plan_branch_agent_worktree(project, &plan_branch)
            .await
            .map_err(|error| error.to_string())?;
        return Ok(AgentConversationWorkspacePublishTarget {
            worktree_path,
            branch_name: plan_branch.branch_name.clone(),
            base_display_name: plan_branch_base_display_name(&base_ref),
            base_ref,
            plan_branch: Some(plan_branch),
        });
    }

    if workspace.is_execution_owned() {
        return Err(
            "This agent conversation workspace is owned by an execution plan and cannot be directly updated"
                .to_string(),
        );
    }

    let worktree_path = resolve_valid_agent_conversation_workspace_path(project, workspace)
        .await
        .map_err(|e| e.to_string())?;
    Ok(AgentConversationWorkspacePublishTarget {
        worktree_path,
        branch_name: workspace.branch_name.clone(),
        base_ref: workspace.base_ref.clone(),
        base_display_name: workspace.base_display_name.clone(),
        plan_branch: None,
    })
}

fn apply_base_resolution_to_publish_target(
    target: &mut AgentConversationWorkspacePublishTarget,
    resolution: &BaseResolutionResult,
) -> Result<(), String> {
    if matches!(resolution.status, BaseStatus::Blocked) {
        return Err(resolution
            .block_reason
            .clone()
            .unwrap_or_else(|| "Agent workspace base is blocked".to_string()));
    }

    if let Some(effective_base_ref) = resolution.effective_base_ref.clone() {
        target.base_ref = effective_base_ref;
    }
    if resolution.status == BaseStatus::Retargeted {
        target.base_display_name = resolution.display_name.clone();
    }
    Ok(())
}

async fn persist_workspace_base_resolution_if_retargeted(
    state: &AppState,
    workspace: &mut AgentConversationWorkspace,
    resolution: &BaseResolutionResult,
) -> Result<(), String> {
    if apply_workspace_base_resolution(workspace, resolution).map_err(|e| e.to_string())? {
        *workspace = state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn retarget_existing_workspace_pr_base_if_needed(
    state: &AppState,
    target: &AgentConversationWorkspacePublishTarget,
    workspace: &AgentConversationWorkspace,
    resolution: &BaseResolutionResult,
) -> Result<(), String> {
    if resolution.status != BaseStatus::Retargeted {
        return Ok(());
    }

    let pr_number = target
        .plan_branch
        .as_ref()
        .and_then(|branch| branch.pr_number)
        .or(workspace.publication_pr_number);
    let Some(pr_number) = pr_number else {
        return Ok(());
    };
    let Some(github) = state.github_service.as_ref() else {
        return Err(existing_pr_retarget_block_reason(pr_number, resolution));
    };
    let effective_base = resolution
        .effective_base_ref
        .as_deref()
        .ok_or_else(|| existing_pr_retarget_block_reason(pr_number, resolution))?;

    AgentWorkspacePrPublisher::new(github)
        .update_pr_base(&target.worktree_path, pr_number, effective_base)
        .await
        .map_err(|_| existing_pr_retarget_block_reason(pr_number, resolution))
}

fn existing_pr_retarget_block_reason(pr_number: i64, resolution: &BaseResolutionResult) -> String {
    format!(
        "Existing PR #{} targets the deleted branch '{}'. Close and recreate the PR, or manually retarget it on GitHub.",
        pr_number, resolution.old_base_ref
    )
}

#[derive(Debug, Clone)]
struct ExplicitPublishBaseSelection {
    kind: IdeationAnalysisBaseRefKind,
    base_ref: String,
    display_name: String,
    source_pull_request: Option<AgentWorkspaceSourcePullRequest>,
}

fn normalize_explicit_publish_base_selection(
    selection: AgentConversationWorkspaceBaseSelection,
) -> Result<Option<ExplicitPublishBaseSelection>, String> {
    let Some(base_ref) = selection
        .base_ref
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let kind = selection
        .kind
        .unwrap_or(IdeationAnalysisBaseRefKind::LocalBranch);
    if kind == IdeationAnalysisBaseRefKind::PullRequest {
        return Err(
            "Pull-request base refs are not supported for agent workspace base recovery"
                .to_string(),
        );
    }
    if let Some(source_pull_request) = selection.source_pull_request.as_ref() {
        if kind != IdeationAnalysisBaseRefKind::LocalBranch {
            return Err(
                "Source pull request metadata requires a local_branch base ref".to_string(),
            );
        }
        let head_ref_name = source_pull_request.head_ref_name.trim();
        if head_ref_name.is_empty() {
            return Err("Source pull request head branch is required".to_string());
        }
        if head_ref_name != base_ref {
            return Err(
                "Source pull request head branch must match the selected base ref".to_string(),
            );
        }
    }
    let display_name = selection
        .display_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| match kind {
            IdeationAnalysisBaseRefKind::ProjectDefault => {
                format!("Project default ({base_ref})")
            }
            IdeationAnalysisBaseRefKind::CurrentBranch => {
                format!("Current branch ({base_ref})")
            }
            IdeationAnalysisBaseRefKind::LocalBranch => base_ref.clone(),
            IdeationAnalysisBaseRefKind::PullRequest => unreachable!("handled above"),
        });

    Ok(Some(ExplicitPublishBaseSelection {
        kind,
        base_ref,
        display_name,
        source_pull_request: selection.source_pull_request,
    }))
}

async fn validate_explicit_publish_base_ref(
    repo_path: &Path,
    base_ref: &str,
) -> Result<(), String> {
    let base_ref = base_ref.trim();
    if base_ref.is_empty() {
        return Err("Selected base branch is empty".to_string());
    }

    let selected_ref_exists = GitService::ref_exists(repo_path, base_ref)
        .await
        .map_err(|e| e.to_string())?;
    let remote_ref = remote_tracking_ref_for_publish(base_ref);
    let remote_ref_exists = remote_ref != base_ref
        && GitService::ref_exists(repo_path, &remote_ref)
            .await
            .map_err(|e| e.to_string())?;
    if !selected_ref_exists && !remote_ref_exists {
        return Err(format!(
            "Selected base branch '{}' does not exist in the project repository",
            base_ref
        ));
    }

    Ok(())
}

struct WorkspaceChangedEventGuard {
    app: tauri::AppHandle,
    conversation_id: String,
}

impl Drop for WorkspaceChangedEventGuard {
    fn drop(&mut self) {
        let _ = self.app.emit(
            "agent:workspace_changed",
            serde_json::json!({ "conversation_id": self.conversation_id }),
        );
    }
}

fn emit_workspace_changed_when_done(
    app: &tauri::AppHandle,
    conversation_id: &ChatConversationId,
) -> WorkspaceChangedEventGuard {
    WorkspaceChangedEventGuard {
        app: app.clone(),
        conversation_id: conversation_id.as_str(),
    }
}

#[derive(Clone)]
pub(crate) struct AgentWorkspacePrFixReviewPublishCommandResumer {
    pub app_state: AppState,
    pub execution_state: Arc<ExecutionState>,
}

#[async_trait::async_trait]
impl AgentWorkspacePrFixReviewPublishResumer for AgentWorkspacePrFixReviewPublishCommandResumer {
    async fn publish_pr_fix_after_workspace_review(
        &self,
        conversation_id: ChatConversationId,
    ) -> Result<Option<bool>, String> {
        publish_agent_conversation_workspace_for_app_state(
            &self.app_state,
            &self.execution_state,
            conversation_id,
            false,
        )
        .await
        .map(|result| result.workspace.pr_auto_merge_current)
    }
}

/// Command composition for the normal publisher after the repair coordinator has already
/// observed the exact repair-owned branch push. Application callers receive only this neutral
/// contract, so their durable attempt/lease authority never depends outward on commands.
pub(crate) struct AgentWorkspaceRepairPublishCommandContinuation;

#[async_trait::async_trait]
impl AgentWorkspaceRepairPublishContinuation for AgentWorkspaceRepairPublishCommandContinuation {
    async fn publish_after_repair_push(
        &self,
        state: &AppState,
        conversation_id: ChatConversationId,
        repair_handoff: AgentWorkspaceRepairPrHandoff,
    ) -> Result<AgentWorkspaceRepairPrHandoffResult, PublishAfterRepairPushError> {
        let result = publish_agent_conversation_workspace_after_repair_push(
            state,
            conversation_id,
            repair_handoff,
        )
        .await
        .map_err(|error| {
            if error == AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE {
                PublishAfterRepairPushError::Busy
            } else {
                PublishAfterRepairPushError::Failed(error)
            }
        })?;
        let pr_number = result
            .pr_number
            .ok_or_else(|| {
                PublishAfterRepairPushError::Failed(
                    "normal publish completed without a pull-request number".to_string(),
                )
            })?;
        Ok(AgentWorkspaceRepairPrHandoffResult {
            pr_number,
            pr_url: result.pr_url,
        })
    }
}

/// Installs the command-owned publisher at the runtime composition boundary. The shared Arc in
/// `AppState` makes the same callback visible to the paired HTTP state.
#[doc(hidden)]
pub fn install_agent_workspace_repair_publish_continuation(state: &AppState) {
    state.install_agent_workspace_repair_publish_continuation(Arc::new(
        AgentWorkspaceRepairPublishCommandContinuation,
    ));
}

#[doc(hidden)]
pub async fn agent_workspace_response_for_state(
    state: &AppState,
    workspace: AgentConversationWorkspace,
) -> Result<AgentConversationWorkspaceResponse, String> {
    // A workspace response is a read boundary. Durable repair reconciliation can fetch, enqueue
    // an agent, or continue publication, so it is owned by the established background PR
    // supervision scheduler rather than by the response request.
    schedule_pr_supervision_recovery_for_workspace(
        state,
        &workspace,
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        false,
    );

    agent_workspace_response_without_repair_recovery_for_state(state, workspace).await
}

/// Returns the persisted workspace and durable repair projection without starting recovery work.
/// Preference no-ops use this read-only form so an already-enabled Auto Publish toggle cannot
/// become a second producer for an in-flight repair continuation.
async fn agent_workspace_response_without_repair_recovery_for_state(
    state: &AppState,
    workspace: AgentConversationWorkspace,
) -> Result<AgentConversationWorkspaceResponse, String> {
    let mode_lock = resolve_agent_conversation_workspace_mode_lock(state, &workspace).await?;
    let linked_plan_branch_id = workspace.linked_plan_branch_id.clone();
    let mut response = AgentConversationWorkspaceResponse::from(workspace);
    response.mode_switch_locked = mode_lock.locked;
    response.mode_switch_lock_reason = mode_lock.reason;
    response.maintenance_operation = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&ChatConversationId::from_string(
            response.conversation_id.clone(),
        ))
        .await
        .map_err(|error| error.to_string())?
        .filter(|attempt| attempt.is_unsettled())
        .map(|attempt| attempt.operation_snapshot());

    if let Some(plan_branch_id) = linked_plan_branch_id {
        if let Some(plan_branch) = state
            .plan_branch_repo
            .get_by_id(&plan_branch_id)
            .await
            .map_err(|e| e.to_string())?
        {
            project_plan_branch_publication_into_workspace_response(&mut response, &plan_branch);
        }
    }

    Ok(response)
}

fn schedule_external_pr_reconciliation_for_workspace(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    trigger: AgentWorkspaceExternalPrReconciliationTrigger,
    force: bool,
) {
    if external_pr_reconciliation_skip_reason(workspace).is_some() {
        return;
    }
    let Some(github) = state.github_service.as_ref().map(Arc::clone) else {
        return;
    };
    let recovery_state = state.clone();
    schedule_agent_workspace_external_pr_reconciliation_with_lazy_deps(
        move || {
            let chat_service: Arc<dyn ChatService> = Arc::new(recovery_state.build_chat_service());
            AgentWorkspaceExternalPrReconciliationDeps {
                workspace_repo: Arc::clone(&recovery_state.agent_conversation_workspace_repo),
                project_repo: Arc::clone(&recovery_state.project_repo),
                github,
                clickup_integration_service: Some(Arc::clone(
                    &recovery_state.clickup_integration_service,
                )),
                external_issue_link_service: Some(Arc::clone(
                    &recovery_state.external_issue_link_service,
                )),
                pr_poller_registry: Some(Arc::clone(&recovery_state.pr_poller_registry)),
                chat_service: Some(chat_service),
                agent_run_repo: Arc::clone(&recovery_state.agent_run_repo),
                agent_workspace_repair_repo: Some(Arc::clone(
                    &recovery_state.agent_workspace_repair_repo,
                )),
                plan_branch_repo: Arc::clone(&recovery_state.plan_branch_repo),
                app_handle: recovery_state.app_handle.clone(),
            }
        },
        workspace.conversation_id.clone(),
        trigger,
        force,
    );
}

fn schedule_pr_supervision_recovery_for_workspace(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    trigger: AgentWorkspacePrSupervisionRecoveryTrigger,
    force: bool,
) {
    if state.github_service.is_none() {
        return;
    }
    let recovery_state = state.clone();
    schedule_agent_workspace_pr_supervision_recovery_with_lazy_deps(
        move || {
            let runtime_app_handle = recovery_state.app_handle.clone();
            let execution_state = runtime_app_handle
                .as_ref()
                .and_then(|handle| handle.try_state::<Arc<ExecutionState>>())
                .map(|state| state.inner().clone());
            let transition_service = execution_state.as_ref().map(|execution_state| {
                Arc::new(recovery_state.build_transition_service_for_runtime(
                    Arc::clone(execution_state),
                    runtime_app_handle.clone(),
                ))
            });
            let chat_service: Arc<dyn ChatService> = Arc::new(
                recovery_state
                    .build_chat_service_for_runtime(execution_state, runtime_app_handle.clone()),
            );
            build_agent_workspace_pr_supervision_recovery_deps(
                &recovery_state,
                runtime_app_handle,
                transition_service,
                Some(chat_service),
                None,
            )
            .expect("github service was checked before scheduling PR supervision recovery")
        },
        workspace.conversation_id.clone(),
        trigger,
        force,
    );
}

async fn schedule_external_pr_reconciliation_for_conversation_id(
    state: &AppState,
    conversation_id: ChatConversationId,
    trigger: AgentWorkspaceExternalPrReconciliationTrigger,
    force: bool,
) -> Result<(), String> {
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };

    schedule_external_pr_reconciliation_for_workspace(state, &workspace, trigger, force);
    Ok(())
}

async fn schedule_pr_supervision_recovery_for_conversation_id(
    state: &AppState,
    conversation_id: ChatConversationId,
    trigger: AgentWorkspacePrSupervisionRecoveryTrigger,
    force: bool,
) -> Result<(), String> {
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };

    schedule_pr_supervision_recovery_for_workspace(state, &workspace, trigger, force);
    Ok(())
}

/// Response from start_agent_conversation command.
#[derive(Debug, Serialize)]
pub struct StartAgentConversationResponse {
    pub conversation: AgentConversationResponse,
    pub workspace: Option<AgentConversationWorkspaceResponse>,
    pub send_result: SendAgentMessageResponse,
}

/// Input for changing the active mode of an existing project-backed agent conversation.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchAgentConversationModeInput {
    pub conversation_id: String,
    pub mode: String,
    /// Complete permission-free runtime for a user-confirmed Plan → Edit handoff.
    pub runtime_override: Option<crate::domain::agents::ManualRoleRuntimeOverride>,
    /// Optional base ref kind used when upgrading a branchless chat into edit/ideation mode.
    pub base_ref_kind: Option<String>,
    /// Optional branch work policy: isolated creates a new RalphX branch; linked uses the selected branch.
    pub base_branch_mode: Option<String>,
    /// Optional selected branch/ref name for the base.
    pub base_ref: Option<String>,
    /// Optional user-facing base ref label.
    pub base_display_name: Option<String>,
    /// Optional source pull request metadata when the selected base came from a PR head branch.
    pub base_source_pull_request: Option<AgentWorkspaceSourcePullRequestInput>,
}

/// Input for changing the persona binding of an existing project-backed agent conversation.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchAgentConversationPersonaInput {
    pub conversation_id: String,
    pub persona_id: Option<String>,
}

/// Response from switch_agent_conversation_mode command.
#[derive(Debug, Serialize)]
pub struct SwitchAgentConversationModeResponse {
    pub conversation: AgentConversationResponse,
    pub workspace: Option<AgentConversationWorkspaceResponse>,
}

/// Response from switch_agent_conversation_persona command.
#[derive(Debug, Serialize)]
pub struct SwitchAgentConversationPersonaResponse {
    pub conversation: AgentConversationResponse,
}

/// Response from publishing a project-backed agent conversation workspace.
#[derive(Debug, Serialize)]
pub struct PublishAgentConversationWorkspaceResponse {
    pub workspace: AgentConversationWorkspaceResponse,
    pub commit_sha: Option<String>,
    pub pushed: bool,
    pub created_pr: bool,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
}

/// Input for an explicit local commit of an Agent workspace branch.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitAgentConversationWorkspaceLocallyInput {
    pub conversation_id: String,
    pub expected_head_sha: String,
    pub review_artifact_id: Option<String>,
    pub review_artifact_version: Option<u32>,
    pub reviewed_head_sha: Option<String>,
    pub reviewed_diff_fingerprint: Option<String>,
    pub attempt_token: String,
}

/// Result of an explicit local Agent workspace commit.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitAgentConversationWorkspaceLocallyResponse {
    pub workspace: AgentConversationWorkspaceResponse,
    pub outcome: String,
    pub branch_name: String,
    pub previous_head_sha: String,
    pub commit_sha: String,
    pub had_changes: bool,
    pub attempt_token: String,
}

#[derive(Debug, Serialize)]
pub struct PrecomputeAgentConversationWorkspacePrDescriptionResponse {
    pub conversation_id: String,
    pub status: String,
    pub cache_status: Option<String>,
    pub reason: Option<String>,
}

/// Read-only freshness state for an edit-agent workspace base branch.
#[derive(Debug, Clone, Serialize)]
pub struct AgentConversationWorkspaceFreshnessResponse {
    pub conversation_id: String,
    pub freshness_scope: String,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub target_ref: String,
    pub captured_base_commit: Option<String>,
    pub target_base_commit: String,
    pub is_base_ahead: bool,
    pub has_uncommitted_changes: bool,
    pub unpublished_commit_count: Option<u32>,
    pub remote_refreshed: bool,
    pub worktree_status_checked: bool,
    pub base_status: String,
    pub effective_base_ref: Option<String>,
    pub effective_base_display_name: Option<String>,
    pub base_block_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AgentWorkspaceFreshnessScope {
    Local,
    Full,
}

impl AgentWorkspaceFreshnessScope {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("full")
        {
            "local" => Ok(Self::Local),
            "full" => Ok(Self::Full),
            other => Err(format!(
                "Unsupported agent workspace freshness scope '{}'",
                other
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Full => "full",
        }
    }
}

impl AgentConversationWorkspaceFreshnessResponse {
    fn from_target_status(
        conversation_id: String,
        freshness_scope: AgentWorkspaceFreshnessScope,
        base_ref: String,
        base_display_name: Option<String>,
        base_resolution: Option<&BaseResolutionResult>,
        status: PublishBranchFreshnessStatus,
        has_uncommitted_changes: bool,
        unpublished_commit_count: Option<u32>,
        remote_refreshed: bool,
        worktree_status_checked: bool,
    ) -> Self {
        let base_status = base_resolution
            .map(|resolution| resolution.status.as_str())
            .unwrap_or(BaseStatus::Valid.as_str())
            .to_string();
        let effective_base_ref = base_resolution
            .and_then(|resolution| resolution.effective_base_ref.clone())
            .or_else(|| Some(base_ref.clone()));
        let effective_base_display_name = base_resolution
            .and_then(|resolution| resolution.display_name.clone())
            .or_else(|| base_display_name.clone());
        let base_block_reason =
            base_resolution.and_then(|resolution| resolution.block_reason.clone());
        Self {
            conversation_id,
            freshness_scope: freshness_scope.as_str().to_string(),
            base_ref,
            base_display_name,
            target_ref: status.target_ref,
            captured_base_commit: status.captured_base_commit,
            target_base_commit: status.target_base_commit,
            is_base_ahead: status.is_base_ahead,
            has_uncommitted_changes,
            unpublished_commit_count,
            remote_refreshed,
            worktree_status_checked,
            base_status,
            effective_base_ref,
            effective_base_display_name,
            base_block_reason,
        }
    }

    fn blocked(
        conversation_id: String,
        freshness_scope: AgentWorkspaceFreshnessScope,
        workspace: &AgentConversationWorkspace,
        base_resolution: &BaseResolutionResult,
        has_uncommitted_changes: bool,
        unpublished_commit_count: Option<u32>,
        remote_refreshed: bool,
        worktree_status_checked: bool,
    ) -> Self {
        Self {
            conversation_id,
            freshness_scope: freshness_scope.as_str().to_string(),
            base_ref: workspace.base_ref.clone(),
            base_display_name: workspace.base_display_name.clone(),
            target_ref: String::new(),
            captured_base_commit: workspace.base_commit.clone(),
            target_base_commit: String::new(),
            is_base_ahead: false,
            has_uncommitted_changes,
            unpublished_commit_count,
            remote_refreshed,
            worktree_status_checked,
            base_status: BaseStatus::Blocked.as_str().to_string(),
            effective_base_ref: None,
            effective_base_display_name: None,
            base_block_reason: base_resolution.block_reason.clone(),
        }
    }

    fn from_local_summary(
        conversation_id: String,
        base_ref: String,
        base_display_name: Option<String>,
        target_ref: String,
        captured_base_commit: Option<String>,
    ) -> Self {
        let target_base_commit = captured_base_commit.clone().unwrap_or_default();
        Self {
            conversation_id,
            freshness_scope: AgentWorkspaceFreshnessScope::Local.as_str().to_string(),
            base_ref: base_ref.clone(),
            base_display_name: base_display_name.clone(),
            target_ref,
            captured_base_commit,
            target_base_commit,
            is_base_ahead: false,
            has_uncommitted_changes: false,
            unpublished_commit_count: None,
            remote_refreshed: false,
            worktree_status_checked: false,
            base_status: BaseStatus::Valid.as_str().to_string(),
            effective_base_ref: Some(base_ref),
            effective_base_display_name: base_display_name,
            base_block_reason: None,
        }
    }

    fn from_terminal_publication(
        conversation_id: String,
        freshness_scope: AgentWorkspaceFreshnessScope,
        workspace: &AgentConversationWorkspace,
    ) -> Self {
        let target_base_commit = workspace.base_commit.clone().unwrap_or_default();
        Self {
            conversation_id,
            freshness_scope: freshness_scope.as_str().to_string(),
            base_ref: workspace.base_ref.clone(),
            base_display_name: workspace.base_display_name.clone(),
            target_ref: workspace.branch_name.clone(),
            captured_base_commit: workspace.base_commit.clone(),
            target_base_commit,
            is_base_ahead: false,
            has_uncommitted_changes: false,
            unpublished_commit_count: Some(0),
            remote_refreshed: false,
            worktree_status_checked: false,
            base_status: BaseStatus::Valid.as_str().to_string(),
            effective_base_ref: Some(workspace.base_ref.clone()),
            effective_base_display_name: workspace.base_display_name.clone(),
            base_block_reason: None,
        }
    }
}

#[derive(Debug, Clone)]
struct AgentWorkspaceFreshnessCacheEntry {
    inserted_at: Instant,
    response: AgentConversationWorkspaceFreshnessResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentWorkspaceFreshnessCacheStatus {
    Hit,
    Coalesced,
    Miss,
}

impl AgentWorkspaceFreshnessCacheStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Coalesced => "coalesced",
            Self::Miss => "miss",
        }
    }
}

fn log_agent_workspace_freshness_phase(
    conversation_id: &ChatConversationId,
    freshness_scope: AgentWorkspaceFreshnessScope,
    phase: &'static str,
    started_at: Instant,
) {
    tracing::info!(
        target: "ralphx_lib::commands::agent_workspace_freshness",
        conversation_id = %conversation_id,
        freshness_scope = freshness_scope.as_str(),
        phase,
        elapsed_ms = started_at.elapsed().as_millis(),
        "Agent workspace freshness phase completed"
    );
}

fn agent_workspace_freshness_cache() -> &'static DashMap<String, AgentWorkspaceFreshnessCacheEntry>
{
    static CACHE: OnceLock<DashMap<String, AgentWorkspaceFreshnessCacheEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_freshness_locks() -> &'static DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: OnceLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = OnceLock::new();
    LOCKS.get_or_init(DashMap::new)
}

fn agent_workspace_publish_locks() -> &'static DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: OnceLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = OnceLock::new();
    LOCKS.get_or_init(DashMap::new)
}

fn try_acquire_agent_workspace_publish_guard(
    conversation_id: &ChatConversationId,
) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    let lock = agent_workspace_publish_locks()
        .entry(conversation_id.as_str())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    lock.try_lock_owned()
        .map_err(|_| AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE.to_string())
}

fn agent_workspace_freshness_cache_ttl() -> Duration {
    Duration::from_millis(git_runtime_config().workspace_freshness_cache_ttl_ms)
}

fn agent_workspace_freshness_cache_key(
    conversation_id: &ChatConversationId,
    freshness_scope: AgentWorkspaceFreshnessScope,
) -> Option<String> {
    if conversation_id.as_uuid().is_nil() {
        return None;
    }
    Some(format!(
        "{}:{}",
        conversation_id.as_str(),
        freshness_scope.as_str()
    ))
}

fn cached_agent_workspace_freshness(
    conversation_id: &ChatConversationId,
    freshness_scope: AgentWorkspaceFreshnessScope,
) -> Option<AgentConversationWorkspaceFreshnessResponse> {
    let ttl = agent_workspace_freshness_cache_ttl();
    if ttl.is_zero() {
        return None;
    }
    let key = agent_workspace_freshness_cache_key(conversation_id, freshness_scope)?;
    let entry = agent_workspace_freshness_cache().get(&key)?;
    if entry.inserted_at.elapsed() <= ttl {
        return Some(entry.response.clone());
    }
    drop(entry);
    agent_workspace_freshness_cache().remove(&key);
    None
}

fn store_agent_workspace_freshness(
    conversation_id: &ChatConversationId,
    freshness_scope: AgentWorkspaceFreshnessScope,
    response: &AgentConversationWorkspaceFreshnessResponse,
) {
    if agent_workspace_freshness_cache_ttl().is_zero() {
        return;
    }
    let Some(key) = agent_workspace_freshness_cache_key(conversation_id, freshness_scope) else {
        return;
    };
    agent_workspace_freshness_cache().insert(
        key,
        AgentWorkspaceFreshnessCacheEntry {
            inserted_at: Instant::now(),
            response: response.clone(),
        },
    );
}

pub(crate) fn invalidate_agent_workspace_freshness_cache(conversation_id: &ChatConversationId) {
    if conversation_id.as_uuid().is_nil() {
        return;
    }
    for freshness_scope in [
        AgentWorkspaceFreshnessScope::Local,
        AgentWorkspaceFreshnessScope::Full,
    ] {
        if let Some(key) = agent_workspace_freshness_cache_key(conversation_id, freshness_scope) {
            if let Some(cache) = agent_workspace_freshness_cache().remove(&key) {
                drop(cache);
            }
        }
    }
}

struct AgentWorkspaceFreshnessInvalidationGuard {
    conversation_id: ChatConversationId,
}

impl AgentWorkspaceFreshnessInvalidationGuard {
    fn new(conversation_id: &ChatConversationId) -> Self {
        invalidate_agent_workspace_freshness_cache(conversation_id);
        crate::commands::diff_commands::invalidate_agent_workspace_diff_caches(conversation_id);
        Self {
            conversation_id: conversation_id.clone(),
        }
    }
}

impl Drop for AgentWorkspaceFreshnessInvalidationGuard {
    fn drop(&mut self) {
        invalidate_agent_workspace_freshness_cache(&self.conversation_id);
        crate::commands::diff_commands::invalidate_agent_workspace_diff_caches(
            &self.conversation_id,
        );
    }
}

struct AgentWorkspacePrDescriptionInvalidationGuard {
    conversation_id: ChatConversationId,
}

impl AgentWorkspacePrDescriptionInvalidationGuard {
    fn new(conversation_id: &ChatConversationId, invalidate_now: bool) -> Self {
        if invalidate_now {
            invalidate_agent_workspace_pr_description_cache(conversation_id);
        }
        Self {
            conversation_id: conversation_id.clone(),
        }
    }
}

impl Drop for AgentWorkspacePrDescriptionInvalidationGuard {
    fn drop(&mut self) {
        invalidate_agent_workspace_pr_description_cache(&self.conversation_id);
    }
}

/// Result of explicitly updating an edit-agent workspace branch from its base.
#[derive(Debug, Serialize)]
pub struct UpdateAgentConversationWorkspaceFromBaseResponse {
    pub workspace: AgentConversationWorkspaceResponse,
    pub updated: bool,
    pub repair_started: bool,
    pub target_ref: String,
    pub base_commit: String,
    pub base_status: String,
    pub effective_base_display_name: Option<String>,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspacePostRepairAction {
    Publish,
    UpdateOnly,
}

impl AgentWorkspacePostRepairAction {
    #[cfg(test)]
    fn as_str(self) -> &'static str {
        match self {
            Self::Publish => AGENT_WORKSPACE_REPAIR_ACTION_PUBLISH,
            Self::UpdateOnly => AGENT_WORKSPACE_REPAIR_ACTION_UPDATE_ONLY,
        }
    }

    #[cfg(test)]
    fn classification(self) -> String {
        format!("{AGENT_WORKSPACE_REPAIR_ACTION_PREFIX}{}", self.as_str())
    }

    fn failure_title(self) -> &'static str {
        match self {
            Self::Publish => "Commit & Publish failed for this agent workspace.",
            Self::UpdateOnly => "Update from base failed for this agent workspace.",
        }
    }

    fn repair_instruction(self) -> &'static str {
        match self {
            Self::Publish => "Please fix the workspace so publishing can be retried.",
            Self::UpdateOnly => "Please fix the workspace so the base update can be completed.",
        }
    }

    fn repair_requested_summary(self) -> &'static str {
        match self {
            Self::Publish => "Workspace agent repair requested before publishing can continue",
            Self::UpdateOnly => {
                "Workspace agent repair requested before the base update can complete"
            }
        }
    }

    fn repair_sent_summary(self) -> &'static str {
        match self {
            Self::Publish => "Sent publish failure to workspace agent",
            Self::UpdateOnly => "Sent base update failure to workspace agent",
        }
    }

    fn deferred_repair_sent_summary(self) -> &'static str {
        match self {
            Self::Publish => "Sent publish failure to workspace agent after active turn completed",
            Self::UpdateOnly => {
                "Sent base update failure to workspace agent after active turn completed"
            }
        }
    }

    fn repair_send_failed_summary(self, repair_error: &str) -> String {
        match self {
            Self::Publish => {
                format!("Failed to send publish failure to workspace agent: {repair_error}")
            }
            Self::UpdateOnly => {
                format!("Failed to send base update failure to workspace agent: {repair_error}")
            }
        }
    }

    fn from_classification(value: Option<&str>) -> Option<Self> {
        let action = value?.strip_prefix(AGENT_WORKSPACE_REPAIR_ACTION_PREFIX)?;
        match action {
            AGENT_WORKSPACE_REPAIR_ACTION_PUBLISH => Some(Self::Publish),
            AGENT_WORKSPACE_REPAIR_ACTION_UPDATE_ONLY => Some(Self::UpdateOnly),
            _ => None,
        }
    }
}

fn repair_dispatch_authority_error(
    result: &SendResult,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
) -> Option<String> {
    if result.was_queued
        || result.queued_as_pending
        || result.conversation_id != conversation_id.as_str()
        || result.agent_run_id != run_id.to_string()
    {
        return Some(
            "Workspace repair launch did not preserve its reserved immediate-start authority"
                .to_string(),
        );
    }
    None
}

#[doc(hidden)]
pub fn agent_workspace_post_repair_action_from_events(
    events: &[AgentConversationWorkspacePublicationEvent],
) -> AgentWorkspacePostRepairAction {
    events
        .iter()
        .rev()
        .find(|event| event.step == AGENT_WORKSPACE_REPAIR_REQUESTED_STEP)
        .and_then(|event| {
            AgentWorkspacePostRepairAction::from_classification(event.classification.as_deref())
        })
        .unwrap_or(AgentWorkspacePostRepairAction::Publish)
}

/// Durable publish operation event for an agent conversation workspace.
#[derive(Debug, Serialize)]
pub struct AgentConversationWorkspacePublicationEventResponse {
    pub id: String,
    pub conversation_id: String,
    pub step: String,
    pub status: String,
    pub summary: String,
    pub classification: Option<String>,
    pub created_at: String,
}

impl From<AgentConversationWorkspacePublicationEvent>
    for AgentConversationWorkspacePublicationEventResponse
{
    fn from(event: AgentConversationWorkspacePublicationEvent) -> Self {
        Self {
            id: event.id,
            conversation_id: event.conversation_id.as_str(),
            step: event.step,
            status: event.status,
            summary: event.summary,
            classification: event.classification,
            created_at: event.created_at.to_rfc3339(),
        }
    }
}

/// Input for queue_agent_message command
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueAgentMessageInput {
    pub context_type: String,
    pub context_id: String,
    pub content: String,
    /// Client-provided ID for tracking (optional, allows frontend/backend to use same ID)
    pub client_id: Option<String>,
}

/// Response for queued message
#[derive(Debug, Serialize)]
pub struct QueuedMessageResponse {
    pub id: String,
    pub content: String,
    pub created_at: String,
    pub is_editing: bool,
    pub composer_selection_snapshot: Option<ComposerSelectionSnapshot>,
    pub composer_excerpt_references: Vec<ComposerExcerptReference>,
    pub attachment_ids: Vec<String>,
}

impl From<QueuedMessage> for QueuedMessageResponse {
    fn from(msg: QueuedMessage) -> Self {
        Self {
            id: msg.id,
            content: msg.content,
            created_at: msg.created_at,
            is_editing: msg.is_editing,
            composer_selection_snapshot: msg.composer_selection_snapshot,
            composer_excerpt_references: msg.composer_excerpt_references,
            attachment_ids: msg
                .attachment_ids
                .into_iter()
                .map(|attachment_id| attachment_id.to_string())
                .collect(),
        }
    }
}

fn visible_queued_message_responses(msgs: Vec<QueuedMessage>) -> Vec<QueuedMessageResponse> {
    msgs.into_iter()
        .filter(|msg| !message_metadata_hidden_from_ui(msg.metadata_override.as_deref()))
        .map(QueuedMessageResponse::from)
        .collect()
}

/// Response for conversation listing
#[derive(Debug, Clone, Serialize)]
pub struct AgentConversationResponse {
    pub id: String,
    pub context_type: String,
    pub context_id: String,
    pub claude_session_id: Option<String>,
    pub provider_session_id: Option<String>,
    pub provider_harness: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub logical_model: Option<String>,
    pub effective_model_id: Option<String>,
    pub logical_effort: Option<String>,
    pub effective_effort: Option<String>,
    pub service_tier: Option<String>,
    pub agent_mode: Option<String>,
    pub bound_agent_name: Option<String>,
    pub persona_id: Option<String>,
    pub builder_draft_id: Option<String>,
    pub builder_result_persona_id: Option<String>,
    pub last_run_persona_run_id: Option<String>,
    pub last_run_persona_id: Option<String>,
    pub last_run_persona_slug: Option<String>,
    pub last_run_persona_version: Option<i64>,
    pub last_run_persona_content_hash: Option<String>,
    pub last_run_persona_injected: Option<bool>,
    pub last_run_persona_skipped_reason: Option<String>,
    pub persona_runs: Vec<PersonaRunAttributionResponse>,
    pub coordination_mode: String,
    pub automation_id: Option<String>,
    pub automation_run_id: Option<String>,
    pub parent_conversation_id: Option<String>,
    pub title: Option<String>,
    pub message_count: i64,
    pub last_message_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveAgentConversationResponse {
    pub conversation: AgentConversationResponse,
    pub cleanup: TerminalAgentWorkspaceOutcome,
}

impl From<ChatConversation> for AgentConversationResponse {
    fn from(c: ChatConversation) -> Self {
        let (claude_session_id, provider_session_id, provider_harness) =
            c.compatible_provider_session_fields();

        Self {
            id: c.id.as_str(),
            context_type: c.context_type.to_string(),
            context_id: c.context_id,
            claude_session_id,
            provider_session_id,
            provider_harness: provider_harness.map(|harness| harness.to_string()),
            upstream_provider: c.upstream_provider,
            provider_profile: c.provider_profile,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
            service_tier: None,
            agent_mode: c.agent_mode.map(|mode| mode.to_string()),
            bound_agent_name: c.bound_agent_name,
            persona_id: c.persona_id,
            builder_draft_id: c.builder_draft_id,
            builder_result_persona_id: c.builder_result_persona_id,
            last_run_persona_run_id: None,
            last_run_persona_id: None,
            last_run_persona_slug: None,
            last_run_persona_version: None,
            last_run_persona_content_hash: None,
            last_run_persona_injected: None,
            last_run_persona_skipped_reason: None,
            persona_runs: Vec::new(),
            coordination_mode: c.coordination_mode.to_string(),
            automation_id: c.automation_id.map(|id| id.as_str().to_string()),
            automation_run_id: c.automation_run_id.map(|id| id.as_str().to_string()),
            parent_conversation_id: c.parent_conversation_id,
            title: c.title,
            message_count: c.message_count,
            last_message_at: c.last_message_at.map(|dt| dt.to_rfc3339()),
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
            archived_at: c.archived_at.map(|dt| dt.to_rfc3339()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonaRunAttributionResponse {
    pub run_id: String,
    pub persona_id: String,
    pub persona_slug: String,
    pub persona_version: i64,
    pub persona_content_hash: String,
    pub injected: bool,
    pub skipped_reason: Option<String>,
}

impl AgentConversationResponse {
    fn apply_runtime_attribution(&mut self, attribution: ConversationRuntimeAttribution) {
        self.logical_model = attribution.logical_model;
        self.effective_model_id = attribution.effective_model_id;
        self.logical_effort = attribution.logical_effort.map(|value| value.to_string());
        self.effective_effort = attribution.effective_effort;
        self.service_tier = attribution.service_tier;
        self.last_run_persona_run_id = attribution.persona_run_id;
        self.last_run_persona_id = attribution.persona_id;
        self.last_run_persona_slug = attribution.persona_slug;
        self.last_run_persona_version = attribution.persona_version;
        self.last_run_persona_content_hash = attribution.persona_content_hash;
        self.last_run_persona_injected = attribution.persona_injected;
        self.last_run_persona_skipped_reason = attribution.persona_skipped_reason;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ConversationRuntimeAttribution {
    logical_model: Option<String>,
    effective_model_id: Option<String>,
    logical_effort: Option<LogicalEffort>,
    effective_effort: Option<String>,
    service_tier: Option<String>,
    persona_run_id: Option<String>,
    persona_id: Option<String>,
    persona_slug: Option<String>,
    persona_version: Option<i64>,
    persona_content_hash: Option<String>,
    persona_injected: Option<bool>,
    persona_skipped_reason: Option<String>,
}

impl ConversationRuntimeAttribution {
    fn is_empty(&self) -> bool {
        self.logical_model.is_none()
            && self.effective_model_id.is_none()
            && self.logical_effort.is_none()
            && self.effective_effort.is_none()
            && self.service_tier.is_none()
            && self.persona_run_id.is_none()
    }

    fn apply_persona_from(&mut self, attribution: Self) {
        self.persona_run_id = attribution.persona_run_id;
        self.persona_id = attribution.persona_id;
        self.persona_slug = attribution.persona_slug;
        self.persona_version = attribution.persona_version;
        self.persona_content_hash = attribution.persona_content_hash;
        self.persona_injected = attribution.persona_injected;
        self.persona_skipped_reason = attribution.persona_skipped_reason;
    }
}

fn runtime_attribution_from_run(run: &AgentRun) -> Option<ConversationRuntimeAttribution> {
    let proven_skipped_reason = run
        .persona_skipped_reason
        .as_deref()
        .filter(|reason| !reason.trim().is_empty());
    let persona_injected = match run.persona_injected {
        Some(true) => Some(true),
        Some(false) if proven_skipped_reason.is_some() => Some(false),
        Some(false) | None => None,
    };
    let persona_skipped_reason = match persona_injected {
        Some(false) => proven_skipped_reason.map(str::to_string),
        Some(true) | None => None,
    };
    let attribution = ConversationRuntimeAttribution {
        logical_model: run.logical_model.clone(),
        effective_model_id: run.effective_model_id.clone(),
        logical_effort: run.logical_effort,
        effective_effort: run.effective_effort.clone(),
        service_tier: run.service_tier.clone(),
        persona_run_id: run.persona_id.as_ref().map(|_| run.id.as_str()),
        persona_id: run.persona_id.clone(),
        persona_slug: run.persona_slug.clone(),
        persona_version: run.persona_version,
        persona_content_hash: run.persona_content_hash.clone(),
        persona_injected,
        persona_skipped_reason,
    };
    (!attribution.is_empty()).then_some(attribution)
}

fn run_executed_for_runtime_attribution(run: &AgentRun) -> bool {
    match run.status {
        AgentRunStatus::Running | AgentRunStatus::Completed | AgentRunStatus::Failed => true,
        AgentRunStatus::Cancelled => false,
    }
}

fn runtime_attribution_from_message(
    message: &ChatMessage,
) -> Option<ConversationRuntimeAttribution> {
    let attribution = ConversationRuntimeAttribution {
        logical_model: message.logical_model.clone(),
        effective_model_id: message.effective_model_id.clone(),
        logical_effort: message.logical_effort,
        effective_effort: message.effective_effort.clone(),
        service_tier: None,
        persona_run_id: None,
        persona_id: None,
        persona_slug: None,
        persona_version: None,
        persona_content_hash: None,
        persona_injected: None,
        persona_skipped_reason: None,
    };
    (!attribution.is_empty()).then_some(attribution)
}

async fn latest_conversation_runtime_attribution(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<
    (
        Option<ConversationRuntimeAttribution>,
        Vec<PersonaRunAttributionResponse>,
    ),
    String,
> {
    let runs = state
        .agent_run_repo
        .get_by_conversation(conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let persona_runs = runs
        .iter()
        .filter_map(|run| {
            Some(PersonaRunAttributionResponse {
                run_id: run.id.as_str(),
                persona_id: run.persona_id.clone()?,
                persona_slug: run.persona_slug.clone()?,
                persona_version: run.persona_version?,
                persona_content_hash: run.persona_content_hash.clone()?,
                injected: run.persona_injected?,
                skipped_reason: run.persona_skipped_reason.clone(),
            })
        })
        .collect();
    let mut attribution = runs
        .iter()
        .filter(|run| run_executed_for_runtime_attribution(run))
        .find_map(runtime_attribution_from_run);
    let persona_attribution = runs
        .iter()
        .filter(|run| run_executed_for_runtime_attribution(run))
        .filter(|run| run.persona_id.is_some())
        .find_map(runtime_attribution_from_run);
    if let Some(persona_attribution) = persona_attribution {
        match attribution.as_mut() {
            Some(attribution) => attribution.apply_persona_from(persona_attribution),
            None => attribution = Some(persona_attribution),
        }
    }
    if attribution.is_some() {
        return Ok((attribution, persona_runs));
    }

    let messages = state
        .chat_message_repo
        .get_recent_by_conversation_paginated(conversation_id, 200, 0)
        .await
        .map_err(|error| error.to_string())?;
    Ok((
        messages.iter().find_map(runtime_attribution_from_message),
        persona_runs,
    ))
}

pub(crate) async fn agent_conversation_response_for_state(
    state: &AppState,
    conversation: ChatConversation,
) -> Result<AgentConversationResponse, String> {
    let conversation_id = conversation.id;
    let mut response = AgentConversationResponse::from(conversation);
    let (attribution, persona_runs) =
        latest_conversation_runtime_attribution(state, &conversation_id).await?;
    response.persona_runs = persona_runs;
    if let Some(attribution) = attribution {
        response.apply_runtime_attribution(attribution);
    }
    Ok(response)
}

async fn agent_conversation_responses_for_state(
    state: &AppState,
    conversations: Vec<ChatConversation>,
) -> Result<Vec<AgentConversationResponse>, String> {
    let mut responses = Vec::with_capacity(conversations.len());
    for conversation in conversations {
        responses.push(agent_conversation_response_for_state(state, conversation).await?);
    }
    Ok(responses)
}

async fn fork_agent_conversation_response_for_state(
    state: &AppState,
    result: AgentConversationForkResult,
) -> Result<ForkAgentConversationResponse, String> {
    Ok(ForkAgentConversationResponse {
        parent_conversation: agent_conversation_response_for_state(
            state,
            result.parent_conversation,
        )
        .await?,
        conversation: agent_conversation_response_for_state(state, result.conversation).await?,
        workspace: result
            .workspace
            .map(AgentConversationWorkspaceResponse::from),
        provider_session_forked: result.provider_session.is_some(),
        copied_message_count: result.copied_message_count,
        copied_timeline_item_count: result.copied_timeline_item_count,
    })
}

fn emit_agent_conversation_fork_events<R: Runtime>(
    app: &tauri::AppHandle<R>,
    response: &ForkAgentConversationResponse,
) {
    let _ = app.emit(
        "agent:conversation_created",
        AgentConversationCreatedPayload {
            conversation_id: response.conversation.id.clone(),
            context_type: response.conversation.context_type.clone(),
            context_id: response.conversation.context_id.clone(),
        },
    );
    let _ = app.emit(
        "agent:conversation_forked",
        AgentConversationForkedPayload {
            parent_conversation_id: response.parent_conversation.id.clone(),
            conversation_id: response.conversation.id.clone(),
            context_type: response.conversation.context_type.clone(),
            context_id: response.conversation.context_id.clone(),
        },
    );
}

/// Response for paginated conversation listing
#[derive(Debug, Serialize)]
pub struct AgentConversationListPageResponse {
    pub conversations: Vec<AgentConversationResponse>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
    pub has_more: bool,
}

/// Response for conversation with messages
#[derive(Debug, Serialize)]
pub struct AgentConversationWithMessagesResponse {
    pub conversation: AgentConversationResponse,
    pub messages: Vec<AgentMessageResponse>,
}

/// Response for a paginated conversation message window
#[derive(Debug, Serialize)]
pub struct AgentConversationMessagesPageResponse {
    pub conversation: AgentConversationResponse,
    pub messages: Vec<AgentMessageResponse>,
    pub limit: u32,
    pub offset: u32,
    pub total_message_count: i64,
    pub has_older: bool,
}

/// Response for a paginated visible conversation timeline window.
#[derive(Debug, Serialize)]
pub struct AgentConversationTimelinePageResponse {
    pub conversation: AgentConversationResponse,
    pub items: Vec<AgentTimelineItemResponse>,
    pub limit: u32,
    pub before_sequence: Option<i64>,
    pub total_item_count: u32,
    pub has_older: bool,
    pub oldest_loaded_sequence: Option<i64>,
    pub newest_loaded_sequence: Option<i64>,
}

/// Response for one normalized visible chat timeline item.
#[derive(Debug, Serialize)]
pub struct AgentTimelineItemResponse {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub run_id: Option<String>,
    pub sequence: i64,
    pub block_index: i64,
    pub role: String,
    pub kind: String,
    pub status: String,
    pub content: String,
    pub content_blocks: serde_json::Value,
    pub tool_call: Option<serde_json::Value>,
    pub metadata: Option<String>,
    pub provider_harness: Option<String>,
    pub provider_session_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finalized_at: Option<String>,
}

/// Response for a single message
#[derive(Debug, Serialize)]
pub struct AgentMessageResponse {
    pub id: String,
    pub conversation_id: Option<String>,
    pub role: String,
    pub content: String,
    pub metadata: Option<String>,
    pub tool_calls: Option<serde_json::Value>,
    pub content_blocks: Option<serde_json::Value>,
    pub attribution_source: Option<String>,
    pub provider_harness: Option<String>,
    pub provider_session_id: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub logical_model: Option<String>,
    pub effective_model_id: Option<String>,
    pub logical_effort: Option<String>,
    pub effective_effort: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub estimated_usd: Option<f64>,
    pub usage_provenance: Option<String>,
    pub created_at: String,
}

/// Response for a lazily loaded full tool-call detail.
#[derive(Debug, Serialize)]
pub struct AgentToolCallDetailResponse {
    pub tool_call: serde_json::Value,
}

impl From<ChatTimelineItem> for AgentTimelineItemResponse {
    fn from(item: ChatTimelineItem) -> Self {
        let message_id = item.message_id.as_ref().map(|id| id.as_str().to_string());
        let conversation_id = item.conversation_id.as_str();
        let content = item.text.clone().unwrap_or_default();
        let content_block =
            timeline_item_content_block(&item, &conversation_id, message_id.as_deref(), true);
        let content_blocks = serde_json::Value::Array(vec![content_block.clone()]);
        let tool_call = if item.kind.to_string() == "tool_use" {
            Some(content_block)
        } else {
            None
        };

        Self {
            id: item.id.to_string(),
            conversation_id,
            message_id,
            run_id: item.run_id.map(|id| id.as_str()),
            sequence: item.sequence,
            block_index: item.block_index,
            role: item.role.to_string(),
            kind: item.kind.to_string(),
            status: item.status.to_string(),
            content,
            content_blocks,
            tool_call,
            metadata: item.metadata,
            provider_harness: item.provider_harness.map(|value| value.to_string()),
            provider_session_id: item.provider_session_id,
            created_at: item.created_at.to_rfc3339(),
            updated_at: item.updated_at.to_rfc3339(),
            finalized_at: item.finalized_at.map(|value| value.to_rfc3339()),
        }
    }
}

fn timeline_item_content_block(
    item: &ChatTimelineItem,
    conversation_id: &str,
    message_id: Option<&str>,
    preview_arguments: bool,
) -> serde_json::Value {
    if item.kind.to_string() == "text" {
        return serde_json::json!({
            "type": "text",
            "text": item.text.clone().unwrap_or_default(),
        });
    }

    let arguments = item
        .input_json
        .as_deref()
        .or(item.tool_input_preview.as_deref())
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let result = item
        .result_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .or_else(|| {
            item.tool_result_preview
                .clone()
                .map(serde_json::Value::String)
        });
    let mut block = serde_json::json!({
        "type": "tool_use",
        "id": item.tool_call_id.clone().unwrap_or_else(|| item.id.to_string()),
        "name": item.tool_name.clone().unwrap_or_else(|| "unknown".to_string()),
        "arguments": arguments,
        "result": result,
        "detail_ref": {
            "conversation_id": conversation_id,
            "message_id": message_id.unwrap_or(item.id.as_str()),
            "tool_call_id": item.tool_call_id.clone(),
            "content_block_index": item.block_index,
            "timeline_item_id": item.id.to_string(),
        }
    });

    if let Some(raw) = item
        .raw_block_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
    {
        if let Some(diff_context) = raw.get("diff_context").cloned() {
            block["diff_context"] = diff_context;
        }
    }

    if preview_arguments {
        let detail_ref = block.get("detail_ref").cloned();
        if let Some(object) = block.as_object_mut() {
            preview_tool_arguments_object(object, detail_ref);
        }
    }

    block
}

async fn reconcile_delegated_timeline_item_result(
    state: &AppState,
    item: &mut ChatTimelineItem,
    snapshot_cache: &mut HashMap<String, DelegatedToolRuntimeSnapshot>,
) {
    let Some(tool_name) = item.tool_name.as_deref() else {
        return;
    };
    if !is_delegate_start_tool_name(tool_name) {
        return;
    }
    let Some(mut result) = item
        .result_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<JsonValue>(raw).ok())
    else {
        return;
    };

    reconcile_delegated_result_value(state, &mut result, snapshot_cache).await;
    item.result_json = Some(result.to_string());
}

/// Response for agent run status
#[derive(Debug, Serialize)]
pub struct AgentRunStatusResponse {
    pub id: String,
    pub conversation_id: String,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub model_id: Option<String>,
    pub model_label: Option<String>,
    pub persona_id: Option<String>,
    pub persona_slug: Option<String>,
    pub persona_version: Option<i64>,
    pub persona_content_hash: Option<String>,
    pub persona_injected: Option<bool>,
    pub persona_skipped_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct DelegatedToolRuntimeSnapshot {
    session_id: String,
    conversation_id: Option<String>,
    agent_run_id: Option<String>,
    agent_name: String,
    title: Option<String>,
    harness: String,
    provider_session_id: Option<String>,
    session_status: String,
    session_error: Option<String>,
    created_at: String,
    updated_at: String,
    completed_at: Option<String>,
    latest_run: Option<JsonValue>,
    recent_messages: Vec<JsonValue>,
}

fn is_delegate_start_tool_name(name: &str) -> bool {
    name == "delegate_start"
        || name.ends_with("::delegate_start")
        || name.ends_with("__delegate_start")
}

fn parse_wrapped_mcp_result_object(result: &JsonValue) -> Option<JsonMap<String, JsonValue>> {
    if let Some(object) = result.as_object() {
        if let Some(content) = object.get("content").and_then(JsonValue::as_array) {
            if let Some(inner_text) = content
                .iter()
                .find_map(|entry| entry.get("text").and_then(JsonValue::as_str))
            {
                if let Ok(JsonValue::Object(inner)) = serde_json::from_str::<JsonValue>(inner_text)
                {
                    return Some(inner);
                }
            }
        }
        return Some(object.clone());
    }

    result
        .as_str()
        .and_then(|raw| serde_json::from_str::<JsonValue>(raw).ok())
        .and_then(|parsed| parsed.as_object().cloned())
}

fn get_string_field<'a>(object: &'a JsonMap<String, JsonValue>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(JsonValue::as_str)
}

fn provider_chat_message_recent_payload(content: &str, created_at: &str) -> JsonValue {
    serde_json::json!({
        "role": "assistant",
        "content": content,
        "created_at": created_at,
    })
}

fn delegated_agent_state_label(status: &str) -> &'static str {
    if status == AgentRunStatus::Running.to_string() {
        "likely_generating"
    } else {
        "idle"
    }
}

fn delegated_total_tokens_from_run(run: &crate::domain::entities::AgentRun) -> Option<u64> {
    run.processed_tokens()
}

async fn load_delegated_tool_runtime_snapshot(
    state: &AppState,
    delegated_session_id: &str,
    delegated_conversation_id: Option<&str>,
    delegated_agent_run_id: Option<&str>,
) -> Option<DelegatedToolRuntimeSnapshot> {
    let session = state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(delegated_session_id))
        .await
        .ok()
        .flatten()?;

    let conversation = if let Some(conversation_id) = delegated_conversation_id {
        state
            .chat_conversation_repo
            .get_by_id(&ChatConversationId::from_string(conversation_id))
            .await
            .ok()
            .flatten()
    } else {
        state
            .chat_conversation_repo
            .get_active_for_context(ChatContextType::Delegation, delegated_session_id)
            .await
            .ok()
            .flatten()
    }?;
    if conversation.context_type != ChatContextType::Delegation
        || conversation.context_id != delegated_session_id
    {
        return None;
    }
    let conversation_id = conversation.id.as_str();
    let latest_run = if let Some(run_id) = delegated_agent_run_id {
        let run = state
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(run_id))
            .await
            .ok()
            .flatten()?;
        if run.conversation_id != conversation.id {
            return None;
        }
        Some(run)
    } else {
        state
            .agent_run_repo
            .get_latest_for_conversation(&conversation.id)
            .await
            .ok()
            .flatten()
    };

    let recent_messages = state
        .chat_message_repo
        .get_by_conversation(&conversation.id)
        .await
        .ok()
        .map(|messages| {
            messages
                .into_iter()
                .filter(|message| {
                    matches!(
                        message.role.to_string().as_str(),
                        "assistant" | "orchestrator"
                    )
                })
                .rev()
                .find_map(|message| {
                    let content = message.content.trim();
                    if content.is_empty() {
                        None
                    } else {
                        Some(provider_chat_message_recent_payload(
                            content,
                            &message.created_at.to_rfc3339(),
                        ))
                    }
                })
                .into_iter()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let latest_run_json = latest_run.as_ref().map(|run| {
        serde_json::json!({
            "agent_run_id": run.id.as_str(),
            "status": run.status.to_string(),
            "started_at": run.started_at.to_rfc3339(),
            "completed_at": run.completed_at.map(|timestamp| timestamp.to_rfc3339()),
            "error_message": run.error_message,
            "harness": run.harness.map(|value| value.to_string()),
            "provider_session_id": run.provider_session_id,
            "upstream_provider": run.upstream_provider,
            "provider_profile": run.provider_profile,
            "logical_model": run.logical_model,
            "effective_model_id": run.effective_model_id,
            "logical_effort": run.logical_effort.map(|value| value.to_string()),
            "effective_effort": run.effective_effort,
            "approval_policy": run.approval_policy,
            "sandbox_mode": run.sandbox_mode,
            "input_tokens": run.input_tokens,
            "output_tokens": run.output_tokens,
            "cache_creation_tokens": run.cache_creation_tokens,
            "cache_read_tokens": run.cache_read_tokens,
            "estimated_usd": run.estimated_usd,
            "total_tokens": delegated_total_tokens_from_run(run),
        })
    });

    Some(DelegatedToolRuntimeSnapshot {
        session_id: session.id.as_str().to_string(),
        conversation_id: Some(conversation_id),
        agent_run_id: latest_run.as_ref().map(|run| run.id.as_str()),
        agent_name: session.agent_name,
        title: session.title,
        harness: session.harness.to_string(),
        provider_session_id: session.provider_session_id,
        session_status: latest_run
            .as_ref()
            .map(|run| run.status.to_string())
            .unwrap_or_else(|| session.status.clone()),
        session_error: latest_run
            .as_ref()
            .and_then(|run| run.error_message.clone())
            .or(session.error),
        created_at: session.created_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
        completed_at: latest_run
            .as_ref()
            .and_then(|run| run.completed_at.map(|timestamp| timestamp.to_rfc3339()))
            .or_else(|| session.completed_at.map(|timestamp| timestamp.to_rfc3339())),
        latest_run: latest_run_json,
        recent_messages,
    })
}

fn merge_delegated_snapshot_wrapped_fields(
    result_object: &mut JsonMap<String, JsonValue>,
    snapshot: &DelegatedToolRuntimeSnapshot,
    merge_fields: fn(&mut JsonMap<String, JsonValue>, &DelegatedToolRuntimeSnapshot),
) {
    let structured_content_key = if result_object.contains_key("structured_content") {
        Some("structured_content")
    } else if result_object.contains_key("structuredContent") {
        Some("structuredContent")
    } else {
        None
    };
    if let Some(JsonValue::Object(structured_content)) =
        structured_content_key.and_then(|key| result_object.get_mut(key))
    {
        merge_fields(structured_content, snapshot);
    }

    if let Some(content) = result_object
        .get_mut("content")
        .and_then(JsonValue::as_array_mut)
    {
        for entry in content {
            let Some(text) = entry.get_mut("text") else {
                continue;
            };
            let Some(raw) = text.as_str() else {
                continue;
            };
            let Ok(JsonValue::Object(mut nested)) = serde_json::from_str::<JsonValue>(raw) else {
                continue;
            };
            merge_fields(&mut nested, snapshot);
            *text = JsonValue::String(JsonValue::Object(nested).to_string());
            break;
        }
    }
}

fn merge_delegated_snapshot_into_result(
    result: &mut JsonValue,
    snapshot: &DelegatedToolRuntimeSnapshot,
) {
    fn merge_fields(
        result_object: &mut JsonMap<String, JsonValue>,
        snapshot: &DelegatedToolRuntimeSnapshot,
    ) {
        result_object.insert(
            "job_status".to_string(),
            JsonValue::String(snapshot.session_status.clone()),
        );
        result_object.insert(
            "status".to_string(),
            JsonValue::String(snapshot.session_status.clone()),
        );
        result_object.insert(
            "agent_name".to_string(),
            JsonValue::String(snapshot.agent_name.clone()),
        );
        result_object.insert(
            "delegated_session_id".to_string(),
            JsonValue::String(snapshot.session_id.clone()),
        );
        result_object.insert(
            "harness".to_string(),
            JsonValue::String(snapshot.harness.clone()),
        );
        if let Some(conversation_id) = snapshot.conversation_id.as_ref() {
            result_object.insert(
                "delegated_conversation_id".to_string(),
                JsonValue::String(conversation_id.clone()),
            );
        }
        if let Some(agent_run_id) = snapshot.agent_run_id.as_ref() {
            result_object.insert(
                "delegated_agent_run_id".to_string(),
                JsonValue::String(agent_run_id.clone()),
            );
        }
        if let Some(provider_session_id) = snapshot.provider_session_id.as_ref() {
            result_object.insert(
                "provider_session_id".to_string(),
                JsonValue::String(provider_session_id.clone()),
            );
        }
        if let Some(error) = snapshot.session_error.as_ref() {
            result_object.insert("error".to_string(), JsonValue::String(error.clone()));
        }
        if let Some(completed_at) = snapshot.completed_at.as_ref() {
            result_object.insert(
                "completed_at".to_string(),
                JsonValue::String(completed_at.clone()),
            );
        }

        result_object.insert(
            "delegated_status".to_string(),
            serde_json::json!({
                "session": {
                    "id": snapshot.session_id,
                    "title": snapshot.title,
                    "status": snapshot.session_status,
                    "parent_context_type": "ideation",
                    "parent_context_id": JsonValue::Null,
                    "agent_name": snapshot.agent_name,
                    "harness": snapshot.harness,
                    "provider_session_id": snapshot.provider_session_id,
                    "created_at": snapshot.created_at,
                    "updated_at": snapshot.updated_at,
                    "completed_at": snapshot.completed_at,
                },
                "agent_state": {
                    "estimated_status": delegated_agent_state_label(&snapshot.session_status),
                },
                "conversation_id": snapshot.conversation_id,
                "latest_run": snapshot.latest_run,
                "recent_messages": if snapshot.recent_messages.is_empty() {
                    JsonValue::Null
                } else {
                    JsonValue::Array(snapshot.recent_messages.clone())
                },
            }),
        );
    }

    let JsonValue::Object(result_object) = result else {
        return;
    };

    merge_delegated_snapshot_wrapped_fields(result_object, snapshot, merge_fields);
    merge_fields(result_object, snapshot);
}

async fn reconcile_delegated_result_value(
    state: &AppState,
    result: &mut JsonValue,
    snapshot_cache: &mut HashMap<String, DelegatedToolRuntimeSnapshot>,
) {
    let Some(parsed_result) = parse_wrapped_mcp_result_object(result) else {
        return;
    };

    let delegated_session_id = get_string_field(&parsed_result, "delegated_session_id")
        .or_else(|| get_string_field(&parsed_result, "delegatedSessionId"));
    let Some(delegated_session_id) = delegated_session_id else {
        return;
    };
    let delegated_conversation_id = get_string_field(&parsed_result, "delegated_conversation_id")
        .or_else(|| get_string_field(&parsed_result, "delegatedConversationId"));
    let delegated_agent_run_id = get_string_field(&parsed_result, "delegated_agent_run_id")
        .or_else(|| get_string_field(&parsed_result, "delegatedAgentRunId"));

    let snapshot = if let Some(snapshot) = snapshot_cache.get(delegated_session_id) {
        snapshot.clone()
    } else {
        let Some(snapshot) = load_delegated_tool_runtime_snapshot(
            state,
            delegated_session_id,
            delegated_conversation_id,
            delegated_agent_run_id,
        )
        .await
        else {
            return;
        };
        snapshot_cache.insert(delegated_session_id.to_string(), snapshot.clone());
        snapshot
    };

    merge_delegated_snapshot_into_result(result, &snapshot);
}

async fn reconcile_delegated_result_payloads(
    state: &AppState,
    tool_calls: Option<String>,
    content_blocks: Option<String>,
) -> (Option<JsonValue>, Option<JsonValue>) {
    let mut snapshot_cache = HashMap::<String, DelegatedToolRuntimeSnapshot>::new();

    async fn reconcile_value_array(
        state: &AppState,
        raw: Option<String>,
        snapshot_cache: &mut HashMap<String, DelegatedToolRuntimeSnapshot>,
    ) -> Option<JsonValue> {
        let mut parsed = serde_json::from_str::<JsonValue>(&raw?).ok()?;
        let items = parsed.as_array_mut()?;

        for item in items.iter_mut() {
            let Some(item_object) = item.as_object_mut() else {
                continue;
            };
            let Some(name) = item_object.get("name").and_then(JsonValue::as_str) else {
                continue;
            };
            if !is_delegate_start_tool_name(name) {
                continue;
            }

            let Some(result) = item_object.get_mut("result") else {
                continue;
            };
            reconcile_delegated_result_value(state, result, snapshot_cache).await;
        }

        Some(parsed)
    }

    let tool_calls = reconcile_value_array(state, tool_calls, &mut snapshot_cache).await;
    let content_blocks = reconcile_value_array(state, content_blocks, &mut snapshot_cache).await;
    (tool_calls, content_blocks)
}

fn maybe_preview_tool_result(
    object: &mut JsonMap<String, JsonValue>,
    conversation_id: &str,
    message_id: &str,
    content_block_index: Option<usize>,
) {
    let tool_call_id = object
        .get("id")
        .and_then(JsonValue::as_str)
        .map(str::to_string);
    let detail_ref = tool_detail_ref(
        conversation_id,
        message_id,
        tool_call_id.as_deref(),
        content_block_index,
    );
    preview_tool_result_object(object, Some(detail_ref.clone()));
    preview_tool_arguments_object(object, Some(detail_ref));
}

fn preview_tool_call_array(value: &mut JsonValue, conversation_id: &str, message_id: &str) {
    let Some(items) = value.as_array_mut() else {
        return;
    };
    for item in items.iter_mut() {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        maybe_preview_tool_result(object, conversation_id, message_id, None);
    }
}

fn preview_content_block_array(value: &mut JsonValue, conversation_id: &str, message_id: &str) {
    let Some(items) = value.as_array_mut() else {
        return;
    };
    for (index, item) in items.iter_mut().enumerate() {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        if object.get("type").and_then(JsonValue::as_str) != Some("tool_use") {
            continue;
        }
        maybe_preview_tool_result(object, conversation_id, message_id, Some(index));
    }
}

pub(crate) fn preview_tool_payloads_for_message(
    conversation_id: &str,
    message_id: &str,
    mut tool_calls: Option<JsonValue>,
    mut content_blocks: Option<JsonValue>,
) -> (Option<JsonValue>, Option<JsonValue>) {
    if let Some(value) = tool_calls.as_mut() {
        preview_tool_call_array(value, conversation_id, message_id);
    }
    if let Some(value) = content_blocks.as_mut() {
        preview_content_block_array(value, conversation_id, message_id);
    }
    (tool_calls, content_blocks)
}

fn find_tool_call_by_id(value: &JsonValue, tool_call_id: &str) -> Option<JsonValue> {
    value.as_array()?.iter().find_map(|item| {
        let object = item.as_object()?;
        if object.get("id").and_then(JsonValue::as_str) == Some(tool_call_id) {
            Some(item.clone())
        } else {
            None
        }
    })
}

fn find_content_block_by_index(value: &JsonValue, content_block_index: usize) -> Option<JsonValue> {
    let item = value.as_array()?.get(content_block_index)?;
    let object = item.as_object()?;
    if object.get("type").and_then(JsonValue::as_str) == Some("tool_use") {
        Some(item.clone())
    } else {
        None
    }
}

fn find_tool_call_detail(
    tool_calls: Option<&JsonValue>,
    content_blocks: Option<&JsonValue>,
    tool_call_id: Option<&str>,
    content_block_index: Option<usize>,
) -> Option<JsonValue> {
    if let (Some(content_blocks), Some(index)) = (content_blocks, content_block_index) {
        return find_content_block_by_index(content_blocks, index);
    }

    if let Some(tool_call_id) = tool_call_id {
        if let Some(tool_call) =
            tool_calls.and_then(|value| find_tool_call_by_id(value, tool_call_id))
        {
            return Some(tool_call);
        }
        if let Some(tool_call) =
            content_blocks.and_then(|value| find_tool_call_by_id(value, tool_call_id))
        {
            return Some(tool_call);
        }
    }

    None
}

// ============================================================================
// Helper to create ChatService
// ============================================================================

pub(crate) fn create_chat_service<R: Runtime + 'static>(
    state: &AppState,
    app_handle: tauri::AppHandle<R>,
    execution_state: &Arc<ExecutionState>,
) -> AppChatService<R> {
    state.build_chat_service_for_runtime(Some(Arc::clone(execution_state)), Some(app_handle))
}

/// Parse context type string to enum
#[doc(hidden)]
pub fn parse_context_type(context_type: &str) -> Result<ChatContextType, String> {
    context_type
        .parse()
        .map_err(|e: String| format!("Invalid context type '{}': {}", context_type, e))
}

fn parse_agent_workspace_mode(
    mode: Option<&str>,
) -> Result<AgentConversationWorkspaceMode, String> {
    let mode = mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("edit");
    reject_persona_builder_workspace_mode(mode)?;
    mode.parse::<AgentConversationWorkspaceMode>()
}

fn parse_agent_workspace_mode_for_creation(
    mode: Option<&str>,
) -> Result<Option<AgentConversationWorkspaceMode>, String> {
    let Some(mode) = mode.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let mode = mode.parse::<AgentConversationWorkspaceMode>()?;
    if mode == AgentConversationWorkspaceMode::PersonaBuilder
        && !crate::infrastructure::agents::agent_personas_enabled()
    {
        return Err("PersonaBuilder mode requires the agent_personas feature flag".to_string());
    }
    Ok(Some(mode))
}

fn parse_agent_workspace_base_kind(
    kind: Option<&str>,
) -> Result<Option<IdeationAnalysisBaseRefKind>, String> {
    kind.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<IdeationAnalysisBaseRefKind>)
        .transpose()
}

fn parse_agent_workspace_branch_mode(
    branch_mode: Option<&str>,
) -> Result<Option<AgentConversationWorkspaceBranchMode>, String> {
    branch_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<AgentConversationWorkspaceBranchMode>)
        .transpose()
}

fn parse_agent_coordination_mode(mode: &str) -> Result<CoordinationMode, String> {
    let trimmed = mode.trim();
    if trimmed.is_empty() {
        return Err("Coordination mode cannot be empty".to_string());
    }
    let mode = trimmed.parse::<CoordinationMode>()?;
    normalize_new_agent_coordination_mode(mode)
}

fn normalize_new_agent_coordination_mode(
    mode: CoordinationMode,
) -> Result<CoordinationMode, String> {
    Ok(mode)
}

fn coordination_mode_from_team_intent(
    team_intent: Option<&TeamIntent>,
) -> Result<CoordinationMode, String> {
    team_intent
        .map(|intent| normalize_new_agent_coordination_mode(intent.coordination_mode))
        .unwrap_or(Ok(CoordinationMode::Solo))
}

fn trim_optional_input(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_agent_workspace_source_pull_request(
    input: Option<AgentWorkspaceSourcePullRequestInput>,
    base_ref_kind: Option<IdeationAnalysisBaseRefKind>,
    base_ref: Option<&str>,
) -> Result<Option<AgentWorkspaceSourcePullRequest>, String> {
    let Some(input) = input else {
        return Ok(None);
    };

    if input.number <= 0 {
        return Err("Source pull request number must be positive".to_string());
    }
    if base_ref_kind != Some(IdeationAnalysisBaseRefKind::LocalBranch) {
        return Err("Source pull request metadata requires a local_branch base ref".to_string());
    }

    let head_ref_name = input.head_ref_name.trim().to_string();
    if head_ref_name.is_empty() {
        return Err("Source pull request head branch is required".to_string());
    }
    if let Some(base_ref) = base_ref.map(str::trim).filter(|value| !value.is_empty()) {
        if base_ref != head_ref_name {
            return Err(
                "Source pull request head branch must match the selected base ref".to_string(),
            );
        }
    }

    Ok(Some(AgentWorkspaceSourcePullRequest {
        number: input.number,
        url: trim_optional_input(input.url),
        title: trim_optional_input(input.title),
        head_ref_name,
        base_ref_name: trim_optional_input(input.base_ref_name),
        head_ref_oid: trim_optional_input(input.head_ref_oid),
    }))
}

fn agent_mode_requires_workspace(mode: AgentConversationWorkspaceMode) -> bool {
    matches!(
        mode,
        AgentConversationWorkspaceMode::Edit
            | AgentConversationWorkspaceMode::Plan
            | AgentConversationWorkspaceMode::Tasks
            | AgentConversationWorkspaceMode::Autopilot
            | AgentConversationWorkspaceMode::Ideation
            | AgentConversationWorkspaceMode::ReviewPr
    )
}

fn agent_mode_should_create_workspace(
    mode: AgentConversationWorkspaceMode,
    source_pull_request: Option<&AgentWorkspaceSourcePullRequest>,
) -> bool {
    agent_mode_requires_workspace(mode)
        || (mode == AgentConversationWorkspaceMode::Chat && source_pull_request.is_some())
}

async fn ensure_linked_branch_workspace_available(
    state: &AppState,
    project_id: &ProjectId,
    current_conversation_id: Option<&ChatConversationId>,
    branch_mode: Option<AgentConversationWorkspaceBranchMode>,
    base_ref: Option<&str>,
    source_pull_request: Option<&AgentWorkspaceSourcePullRequest>,
) -> Result<(), String> {
    if branch_mode != Some(AgentConversationWorkspaceBranchMode::Linked) {
        return Ok(());
    }
    let branch_name = source_pull_request
        .map(|pull_request| pull_request.head_ref_name.as_str())
        .or(base_ref)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(branch_name) = branch_name else {
        return Ok(());
    };
    let active_workspaces = state
        .agent_conversation_workspace_repo
        .find_active_by_project_and_branch_name(project_id, branch_name)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(conflict) = active_workspaces
        .into_iter()
        .find(|workspace| current_conversation_id != Some(&workspace.conversation_id))
    {
        return Err(format!(
            "Selected branch '{}' is already linked to active conversation {}; choose isolated branch mode or continue in that conversation",
            branch_name, conflict.conversation_id
        ));
    }

    Ok(())
}

async fn hydrate_linked_branch_source_pull_request(
    state: &AppState,
    project: &Project,
    branch_mode: Option<AgentConversationWorkspaceBranchMode>,
    base_ref: Option<&str>,
    source_pull_request: Option<AgentWorkspaceSourcePullRequest>,
) -> Result<Option<AgentWorkspaceSourcePullRequest>, String> {
    if source_pull_request.is_some()
        || branch_mode != Some(AgentConversationWorkspaceBranchMode::Linked)
    {
        return Ok(source_pull_request);
    }
    let Some(branch_name) = base_ref.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some(github) = state.github_service.as_ref() else {
        return Ok(None);
    };
    let matches = match github
        .search_pull_requests(Path::new(&project.working_directory), Some(branch_name), 20)
        .await
    {
        Ok(matches) => matches,
        Err(error) => {
            tracing::warn!(
                project_id = %project.id,
                branch_name,
                error = %error,
                "Linked branch PR lookup failed during mode switch; continuing without PR linkage"
            );
            return Ok(None);
        }
    };

    Ok(matches
        .into_iter()
        .find(|pull_request| {
            !pull_request.is_cross_repository && pull_request.head_ref_name == branch_name
        })
        .map(|pull_request| AgentWorkspaceSourcePullRequest {
            number: pull_request.number,
            url: Some(pull_request.url),
            title: Some(pull_request.title),
            head_ref_name: pull_request.head_ref_name,
            base_ref_name: Some(pull_request.base_ref_name),
            head_ref_oid: pull_request.head_ref_oid,
        }))
}

async fn agent_workspace_pr_automation_defaults_for_project(
    state: &AppState,
    project_id: &ProjectId,
) -> Result<AgentConversationWorkspacePrAutomationDefaults, String> {
    let settings = state
        .execution_settings_repo
        .get_settings(Some(project_id))
        .await
        .map_err(|error| error.to_string())?;
    Ok(AgentConversationWorkspacePrAutomationDefaults::from(
        &settings,
    ))
}

fn validate_agent_conversation_mode_transition(
    current_mode: AgentConversationWorkspaceMode,
    target_mode: AgentConversationWorkspaceMode,
    workspace_mode_lock: &AgentConversationWorkspaceModeLock,
) -> Result<(), String> {
    if matches!(
        current_mode,
        AgentConversationWorkspaceMode::Automation | AgentConversationWorkspaceMode::PersonaBuilder
    ) {
        return Err("Automation and PersonaBuilder conversations cannot change mode".to_string());
    }
    reject_persona_builder_workspace_mode(&target_mode.to_string())?;
    if workspace_mode_lock.locked && target_mode != current_mode {
        return Err(workspace_mode_lock.reason.clone().unwrap_or_else(|| {
            format!(
                "This workspace is owned by active planning or execution state and cannot leave {current_mode} mode"
            )
        }));
    }

    Ok(())
}

#[cfg(test)]
mod agent_mode_workspace_tests;

fn build_agent_workspace_commit_message(conversation: &ChatConversation) -> String {
    let title = conversation
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "Untitled agent")
        .unwrap_or("agent conversation work");
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("feat: {title}")
}

fn normalized_effort_for_supported(
    requested: Option<LogicalEffort>,
    supported_efforts: &[LogicalEffort],
    default_effort: LogicalEffort,
) -> LogicalEffort {
    requested
        .filter(|effort| supported_efforts.contains(effort))
        .unwrap_or(default_effort)
}

async fn normalize_agent_runtime_selection(
    state: &AppState,
    provider: Option<AgentHarnessKind>,
    model_override: Option<String>,
    effort_override: Option<LogicalEffort>,
) -> Result<(Option<String>, Option<LogicalEffort>), String> {
    let Some(provider) = provider else {
        return Ok((model_override, effort_override));
    };

    let snapshot = load_agent_model_registry(state).await?;
    if let Some(model_id) = model_override {
        if let Some(model) = snapshot.find_enabled(provider, &model_id) {
            let effort = normalized_effort_for_supported(
                effort_override,
                &model.supported_efforts,
                model.default_effort,
            );
            return Ok((Some(model_id), Some(effort)));
        }

        let effort = normalized_effort_for_supported(
            effort_override,
            default_efforts_for_provider(provider),
            default_effort_for_provider(provider),
        );
        return Ok((Some(model_id), Some(effort)));
    }

    let effort = if let Some(default_model) = snapshot.default_for_provider(provider) {
        normalized_effort_for_supported(
            effort_override,
            &default_model.supported_efforts,
            default_model.default_effort,
        )
    } else {
        normalized_effort_for_supported(
            effort_override,
            default_efforts_for_provider(provider),
            default_effort_for_provider(provider),
        )
    };

    Ok((None, Some(effort)))
}

// ============================================================================
// Commands
// ============================================================================

/// Start a project-backed agent conversation in an isolated feature worktree.
#[tauri::command]
pub async fn start_agent_conversation<R: Runtime + 'static>(
    input: StartAgentConversationInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle<R>,
) -> Result<StartAgentConversationResponse, String> {
    start_agent_conversation_for_state(input, state.inner(), execution_state.inner(), app).await
}

#[tauri::command]
pub async fn abort_seeded_agent_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    crate::application::seeded_agent_conversation_abort::abort_seeded_agent_conversation(
        state.inner(),
        &ChatConversationId::from_string(conversation_id),
    )
    .await
}

#[doc(hidden)]
pub(crate) async fn start_agent_conversation_for_state<R: Runtime + 'static>(
    input: StartAgentConversationInput,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app: tauri::AppHandle<R>,
) -> Result<StartAgentConversationResponse, String> {
    let result = AgentConversationStartService::new(AgentConversationStartDeps {
        state,
        execution_state,
        app_handle: app,
    })
    .start(input)
    .await?;

    let workspace_response = match result.workspace {
        Some(workspace) => Some(agent_workspace_response_for_state(state, workspace).await?),
        None => None,
    };

    Ok(StartAgentConversationResponse {
        conversation: agent_conversation_response_for_state(state, result.conversation).await?,
        workspace: workspace_response,
        send_result: SendAgentMessageResponse::from(result.send_result),
    })
}

/// Fork a project-backed agent conversation into a new conversation/workspace branch.
#[tauri::command]
pub async fn fork_agent_conversation<R: Runtime + 'static>(
    input: ForkAgentConversationInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle<R>,
) -> Result<ForkAgentConversationResponse, String> {
    let parent_conversation_id = ChatConversationId::from_string(input.conversation_id);
    let result = fork_agent_conversation_in_state(state.inner(), &parent_conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let response = fork_agent_conversation_response_for_state(state.inner(), result).await?;
    emit_agent_conversation_fork_events(&app, &response);
    invalidate_agent_workspace_pr_description_cache(&parent_conversation_id);
    invalidate_agent_workspace_pr_description_cache(&ChatConversationId::from_string(
        response.conversation.id.clone(),
    ));
    Ok(response)
}

/// Switch a project-backed agent conversation between chat/edit/ideation modes.
#[tauri::command]
pub async fn switch_agent_conversation_mode<R: Runtime + 'static>(
    input: SwitchAgentConversationModeInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle<R>,
) -> Result<SwitchAgentConversationModeResponse, String> {
    let service = create_chat_service(&state, app, &execution_state);
    switch_agent_conversation_mode_for_state_stopping_running_agent(input, state.inner(), &service)
        .await
}

/// Switch the persona binding for a project-backed agent conversation.
#[tauri::command]
pub async fn switch_agent_conversation_persona<R: Runtime + 'static>(
    input: SwitchAgentConversationPersonaInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle<R>,
) -> Result<SwitchAgentConversationPersonaResponse, String> {
    let service = create_chat_service(&state, app, &execution_state);
    switch_agent_conversation_persona_for_state_stopping_running_agent(
        input,
        state.inner(),
        &service,
    )
    .await
}

#[doc(hidden)]
pub async fn switch_agent_conversation_persona_for_state_stopping_running_agent(
    input: SwitchAgentConversationPersonaInput,
    state: &AppState,
    chat_service: &dyn ChatService,
) -> Result<SwitchAgentConversationPersonaResponse, String> {
    switch_agent_conversation_persona_for_state_with_provider_session_reset(
        input,
        state,
        chat_service,
        ui_feature_flags_config().persona_switch_forces_fresh_provider_session,
    )
    .await
}

#[doc(hidden)]
pub async fn switch_agent_conversation_persona_for_state_with_provider_session_reset(
    input: SwitchAgentConversationPersonaInput,
    state: &AppState,
    chat_service: &dyn ChatService,
    force_fresh_provider_session: bool,
) -> Result<SwitchAgentConversationPersonaResponse, String> {
    if !agent_personas_enabled() {
        return Err(crate::error::AppError::FeatureDisabled(format!(
            "{PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]"
        ))
        .to_string());
    }

    let conversation_id = ChatConversationId::from_string(input.conversation_id);
    let mut conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Conversation not found: {conversation_id}"))?;
    if conversation.context_type != ChatContextType::Project {
        return Err("Only project agent conversations can change persona".to_string());
    }

    let persona_id = input.persona_id.map(PersonaId::from_string);
    if let Some(persona_id) = persona_id.as_ref() {
        let conversation_project_id = ProjectId::from_string(conversation.context_id.clone());
        let persona = state
            .persona_repo
            .get_by_id(persona_id)
            .await
            .map_err(|error| error.to_string())?;
        if !persona.is_some_and(|persona| persona.is_bindable_to_project(&conversation_project_id))
        {
            return Err(format!(
                "{PERSONA_UNAVAILABLE_PREFIX} persona {persona_id} is not active]"
            ));
        }
    }

    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    if state.running_agent_registry.is_running(&running_key).await {
        let stopped = chat_service
            .stop_agent(ChatContextType::Project, &conversation.id.as_str())
            .await
            .map_err(|error| error.to_string())?;
        tracing::info!(
            conversation_id = %conversation.id,
            stopped,
            "Stopped running project agent before switching conversation persona"
        );
        if state.running_agent_registry.is_running(&running_key).await {
            return Err(PERSONA_SWITCH_AGENT_RUNNING_ERROR.to_string());
        }
    }

    state
        .chat_conversation_repo
        .update_persona_binding(
            &conversation.id,
            persona_id.as_ref().map(|persona_id| persona_id.as_str()),
        )
        .await
        .map_err(|error| error.to_string())?;
    if force_fresh_provider_session {
        state
            .chat_conversation_repo
            .clear_provider_session_ref(&conversation.id)
            .await
            .map_err(|error| error.to_string())?;
    }
    conversation.persona_id = persona_id.map(|persona_id| persona_id.to_string());
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or(conversation);

    Ok(SwitchAgentConversationPersonaResponse {
        conversation: agent_conversation_response_for_state(state, conversation).await?,
    })
}

#[doc(hidden)]
pub async fn switch_agent_conversation_mode_for_state(
    input: SwitchAgentConversationModeInput,
    state: &AppState,
) -> Result<SwitchAgentConversationModeResponse, String> {
    switch_agent_conversation_mode_for_state_with_running_policy(
        input,
        state,
        ModeSwitchRunningAgentPolicy::Reject,
        ModeSwitchInitiator::User,
    )
    .await
}

#[doc(hidden)]
pub async fn switch_agent_conversation_mode_for_state_allowing_running(
    input: SwitchAgentConversationModeInput,
    state: &AppState,
    initiator: ModeSwitchInitiator,
) -> Result<SwitchAgentConversationModeResponse, String> {
    switch_agent_conversation_mode_for_state_with_running_policy(
        input,
        state,
        ModeSwitchRunningAgentPolicy::Allow,
        initiator,
    )
    .await
}

#[doc(hidden)]
pub async fn switch_agent_conversation_mode_for_state_stopping_running_agent(
    input: SwitchAgentConversationModeInput,
    state: &AppState,
    chat_service: &dyn ChatService,
) -> Result<SwitchAgentConversationModeResponse, String> {
    switch_agent_conversation_mode_for_state_with_running_policy(
        input,
        state,
        ModeSwitchRunningAgentPolicy::StopWithService(chat_service),
        ModeSwitchInitiator::User,
    )
    .await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeSwitchInitiator {
    User,
    System,
}

#[derive(Clone, Copy)]
enum ModeSwitchRunningAgentPolicy<'a> {
    Reject,
    Allow,
    StopWithService(&'a dyn ChatService),
}

async fn switch_agent_conversation_mode_for_state_with_running_policy(
    input: SwitchAgentConversationModeInput,
    state: &AppState,
    running_agent_policy: ModeSwitchRunningAgentPolicy<'_>,
    initiator: ModeSwitchInitiator,
) -> Result<SwitchAgentConversationModeResponse, String> {
    let conversation_id = ChatConversationId::from_string(input.conversation_id.clone());
    let target_mode = parse_agent_workspace_mode(Some(input.mode.as_str()))?;
    let base_ref_kind = parse_agent_workspace_base_kind(input.base_ref_kind.as_deref())?;
    let base_branch_mode = parse_agent_workspace_branch_mode(input.base_branch_mode.as_deref())?;
    let base_ref = trim_optional_input(input.base_ref);
    let base_display_name = trim_optional_input(input.base_display_name);
    let mut source_pull_request = normalize_agent_workspace_source_pull_request(
        input.base_source_pull_request,
        base_ref_kind,
        base_ref.as_deref(),
    )?;
    let should_create_workspace =
        agent_mode_should_create_workspace(target_mode, source_pull_request.as_ref());

    let mut conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;
    if conversation.context_type != ChatContextType::Project {
        return Err("Only project agent conversations can change mode".to_string());
    }
    if initiator == ModeSwitchInitiator::User && is_automation_run_mode_switch_locked(&conversation)
    {
        return Err(automation_run_mode_locked_error_message());
    }
    if input.runtime_override.is_some() && target_mode != AgentConversationWorkspaceMode::Edit {
        return Err("A mode runtime override is supported only for Edit handoffs".to_string());
    }
    if let Some(runtime_override) = input.runtime_override.as_ref() {
        let project = state
            .project_repo
            .get_by_id(&ProjectId::from_string(conversation.context_id.clone()))
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Project not found: {}", conversation.context_id))?;
        crate::application::agent_lane_resolution::resolve_manual_role_spawn_settings(
            crate::infrastructure::agents::claude::agent_names::AGENT_GENERAL_WORKER,
            Some(project.id.as_str()),
            Some(std::path::Path::new(&project.working_directory)),
            RoutingRole::WorkspaceEdit,
            Some(runtime_override),
            None,
            None,
            &state.manual_role_default_service(),
        )
        .await
        .map_err(|error| error.to_string())?;
    }

    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    let agent_is_running = state.running_agent_registry.is_running(&running_key).await;
    if agent_is_running {
        match running_agent_policy {
            ModeSwitchRunningAgentPolicy::Reject => {
                return Err("Cannot change mode while the agent is running".to_string());
            }
            ModeSwitchRunningAgentPolicy::Allow => {
                tracing::info!(
                    conversation_id = %conversation.id,
                    target_mode = %target_mode,
                    "Switching project agent conversation mode while its current run is still registered"
                );
            }
            ModeSwitchRunningAgentPolicy::StopWithService(_) => {}
        }
    }

    let mut existing_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .map_err(|error| error.to_string())?;
    let current_mode = conversation
        .agent_mode
        .or_else(|| existing_workspace.as_ref().map(|workspace| workspace.mode))
        .unwrap_or(AgentConversationWorkspaceMode::Chat);
    if target_mode == AgentConversationWorkspaceMode::Autopilot
        && current_mode != AgentConversationWorkspaceMode::Autopilot
        && !state.agent_capability_gate.autopilot_enabled()
    {
        return Err("Autopilot is disabled in Agent conversation capabilities".to_string());
    }
    if target_mode == AgentConversationWorkspaceMode::Tasks
        && existing_workspace
            .as_ref()
            .is_none_or(|workspace| workspace.task_pipeline_session_id.is_none())
    {
        return Err(
            "Tasks mode is available only for this conversation's attached task pipeline"
                .to_string(),
        );
    }
    let workspace_mode_lock = match existing_workspace.as_ref() {
        Some(workspace) => resolve_agent_conversation_workspace_mode_lock(state, workspace).await?,
        None => AgentConversationWorkspaceModeLock::unlocked(),
    };

    validate_agent_conversation_mode_transition(current_mode, target_mode, &workspace_mode_lock)?;
    let plan_to_edit_handoff = current_mode == AgentConversationWorkspaceMode::Plan
        && target_mode == AgentConversationWorkspaceMode::Edit;
    let entering_plan_workspace = target_mode == AgentConversationWorkspaceMode::Plan
        && existing_workspace
            .as_ref()
            .is_some_and(|workspace| workspace.mode != AgentConversationWorkspaceMode::Plan);
    let leaving_plan_for_review_eligible_workspace = current_mode
        == AgentConversationWorkspaceMode::Plan
        && crate::application::agent_workspace_review::workspace_review_mode_is_eligible(
            target_mode,
        )
        && existing_workspace.is_some();
    let crossing_plan_review_boundary =
        entering_plan_workspace || leaving_plan_for_review_eligible_workspace;
    let _workspace_review_lifecycle_guard = if crossing_plan_review_boundary {
        Some(
            crate::application::agent_workspace_review::lock_workspace_review_lifecycle(
                &conversation.id,
            )
            .await,
        )
    } else {
        None
    };

    if agent_is_running {
        if let ModeSwitchRunningAgentPolicy::StopWithService(chat_service) = running_agent_policy {
            let stop_context_id = conversation.id.as_str();
            let stopped = chat_service
                .stop_agent(ChatContextType::Project, &stop_context_id)
                .await
                .map_err(|error| error.to_string())?;
            tracing::info!(
                conversation_id = %conversation.id,
                target_mode = %target_mode,
                stopped,
                "Stopped running project agent before switching conversation mode"
            );
            if state.running_agent_registry.is_running(&running_key).await {
                return Err("Cannot change mode while the agent is running".to_string());
            }
        }
    }

    if crossing_plan_review_boundary {
        let cleanup_chat_service = match running_agent_policy {
            ModeSwitchRunningAgentPolicy::StopWithService(chat_service) => Some(chat_service),
            ModeSwitchRunningAgentPolicy::Reject | ModeSwitchRunningAgentPolicy::Allow => None,
        };
        let workspace = existing_workspace.as_ref().ok_or_else(|| {
            "Cannot change mode because the Agent Workspace no longer exists".to_string()
        })?;
        crate::application::agent_workspace_review::cleanup_workspace_review_for_plan_boundary(
            state,
            workspace,
            cleanup_chat_service,
        )
        .await
        .map_err(|error| error.to_string())?;
        existing_workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation.id)
            .await
            .map_err(|error| error.to_string())?;
    }

    let workspace = match existing_workspace {
        Some(mut workspace) => {
            let preserve_planning_session_link = if !matches!(
                target_mode,
                AgentConversationWorkspaceMode::Ideation | AgentConversationWorkspaceMode::Tasks
            ) && workspace.linked_plan_branch_id.is_none()
            {
                linked_workspace_planning_session_is_reusable(state, &workspace).await?
            } else {
                false
            };
            let linked_plan_handoff_changed = if target_mode == AgentConversationWorkspaceMode::Edit
                && !workspace_mode_lock.locked
                && workspace.linked_plan_branch_id.is_some()
            {
                apply_linked_plan_branch_edit_handoff(state, &mut workspace).await?
            } else {
                false
            };
            let should_detach_inactive_owner = !matches!(
                target_mode,
                AgentConversationWorkspaceMode::Ideation | AgentConversationWorkspaceMode::Tasks
            ) && !workspace_mode_lock.locked
                && (workspace.linked_ideation_session_id.is_some()
                    || workspace.linked_plan_branch_id.is_some())
                && !preserve_planning_session_link;
            let changed = workspace.mode != target_mode
                || should_detach_inactive_owner
                || linked_plan_handoff_changed;
            if workspace.mode != target_mode {
                workspace.mode = target_mode;
            }
            if should_detach_inactive_owner {
                workspace.linked_ideation_session_id = None;
                workspace.linked_plan_branch_id = None;
            }
            if changed {
                workspace.updated_at = chrono::Utc::now();
                Some(
                    state
                        .agent_conversation_workspace_repo
                        .create_or_update(workspace)
                        .await
                        .map_err(|error| error.to_string())?,
                )
            } else {
                Some(workspace)
            }
        }
        None => {
            if should_create_workspace {
                let project_id = ProjectId::from_string(conversation.context_id.clone());
                let project = state
                    .project_repo
                    .get_by_id(&project_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Project not found: {}", conversation.context_id))?;
                ensure_linked_branch_workspace_available(
                    state,
                    &project_id,
                    Some(&conversation.id),
                    base_branch_mode,
                    base_ref.as_deref(),
                    source_pull_request.as_ref(),
                )
                .await?;
                source_pull_request = hydrate_linked_branch_source_pull_request(
                    state,
                    &project,
                    base_branch_mode,
                    base_ref.as_deref(),
                    source_pull_request,
                )
                .await?;
                let pr_automation_defaults =
                    agent_workspace_pr_automation_defaults_for_project(state, &project.id).await?;
                let workspace = prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
                    &project,
                    &conversation.id,
                    target_mode,
                    AgentConversationWorkspaceBaseSelection {
                        kind: base_ref_kind,
                        branch_mode: base_branch_mode,
                        base_ref,
                        display_name: base_display_name,
                        source_pull_request,
                    },
                    AgentConversationWorkspaceSetupMode::Blocking,
                    pr_automation_defaults,
                    false,
                )
                .await
                .map_err(|error| error.to_string())?;
                Some(
                    state
                        .agent_conversation_workspace_repo
                        .create_or_update(workspace)
                        .await
                        .map_err(|error| error.to_string())?,
                )
            } else {
                None
            }
        }
    };

    if current_mode != target_mode {
        crate::application::agent_workspace_review_context::
            invalidate_workspace_review_presentation_context(&conversation.id);
    }

    if plan_to_edit_handoff && conversation.provider_session_ref().is_some() {
        state
            .chat_conversation_repo
            .clear_provider_session_ref(&conversation.id)
            .await
            .map_err(|error| error.to_string())?;
        conversation.clear_provider_session_ref();
        tracing::info!(
            conversation_id = %conversation.id,
            "Cleared planning provider session before Plan-to-Edit implementation handoff"
        );
    }

    if let Some(runtime_override) = input.runtime_override.as_ref() {
        let coordination_mode = runtime_override.coordination_mode.unwrap_or_default();
        state
            .chat_conversation_repo
            .update_agent_mode_and_role_default_bindings(
                &conversation.id,
                target_mode,
                coordination_mode,
                runtime_override
                    .persona_id
                    .as_ref()
                    .map(|persona_id| persona_id.as_str()),
                false,
            )
            .await
            .map_err(|error| error.to_string())?;
        conversation.coordination_mode = coordination_mode;
        conversation.persona_id = runtime_override
            .persona_id
            .as_ref()
            .map(|persona_id| persona_id.to_string());
        conversation.set_agent_mode(Some(target_mode));
    } else {
        state
            .chat_conversation_repo
            .update_agent_mode(&conversation.id, Some(target_mode))
            .await
            .map_err(|error| error.to_string())?;
        conversation.set_agent_mode(Some(target_mode));
    }

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or(conversation);

    let workspace_response = match workspace {
        Some(workspace) => Some(agent_workspace_response_for_state(state, workspace).await?),
        None => None,
    };

    Ok(SwitchAgentConversationModeResponse {
        conversation: agent_conversation_response_for_state(state, conversation).await?,
        workspace: workspace_response,
    })
}

/// Send a message to an agent in any context
///
/// Returns immediately with conversation_id and agent_run_id.
/// Processing happens in background with events emitted via Tauri.
///
/// Events emitted:
/// - agent:run_started - When agent begins
/// - agent:chunk - Streaming text chunks
/// - agent:tool_call - Tool invocations
/// - agent:message_created - When messages are persisted
/// - agent:run_completed or agent:turn_completed (interactive) - When agent finishes
/// - agent:error - On failure
async fn fork_terminal_agent_conversation_for_send<R: Runtime>(
    state: &AppState,
    app: &tauri::AppHandle<R>,
    conversation_id: Option<&ChatConversationId>,
    new_user_message: &str,
    requested_harness: Option<AgentHarnessKind>,
    service_tier_override: Option<String>,
) -> Result<Option<ChatConversationId>, String> {
    let Some(parent_conversation_id) = conversation_id else {
        return Ok(None);
    };
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(parent_conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if !workspace.has_terminal_publication_pr_status() {
        return Ok(None);
    }

    let result = fork_agent_conversation_in_state(state, parent_conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let response = fork_agent_conversation_response_for_state(state, result).await?;
    emit_agent_conversation_fork_events(app, &response);
    invalidate_agent_workspace_pr_description_cache(parent_conversation_id);
    let child_conversation_id = ChatConversationId::from_string(response.conversation.id.clone());
    invalidate_agent_workspace_pr_description_cache(&child_conversation_id);
    spawn_session_namer_for_continuity_fork(
        state,
        &child_conversation_id,
        new_user_message,
        requested_harness,
        service_tier_override,
    )
    .await;
    Ok(Some(child_conversation_id))
}

async fn spawn_session_namer_for_continuity_fork(
    state: &AppState,
    conversation_id: &ChatConversationId,
    new_user_message: &str,
    requested_harness: Option<AgentHarnessKind>,
    service_tier_override: Option<String>,
) {
    let new_user_message = new_user_message.trim();
    if new_user_message.is_empty() {
        return;
    }

    let target = match SessionNamerTarget::from_initial_request(
        None,
        Some(conversation_id.as_str().to_string()),
        new_user_message.to_string(),
        requested_harness,
        service_tier_override,
    ) {
        Ok(target) => target,
        Err(error) => {
            tracing::warn!(
                conversation_id = %conversation_id,
                error,
                "Failed to build continuity fork session namer target"
            );
            return;
        }
    };

    if let Err(error) = spawn_session_namer_agent(state, target).await {
        tracing::warn!(
            conversation_id = %conversation_id,
            error = %error,
            "Failed to spawn continuity fork session namer"
        );
    }
}

#[tauri::command]
pub async fn send_agent_message(
    input: SendAgentMessageInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<SendAgentMessageResponse, String> {
    send_agent_message_for_state(input, state.inner(), execution_state.inner(), app).await
}

#[doc(hidden)]
pub async fn send_agent_message_for_state<R: Runtime + 'static>(
    input: SendAgentMessageInput,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app: tauri::AppHandle<R>,
) -> Result<SendAgentMessageResponse, String> {
    tracing::info!(
        context_type = %input.context_type,
        context_id = %input.context_id,
        content_len = input.content.len(),
        "[SEND_MSG] send_agent_message command invoked"
    );
    let context_type = parse_context_type(&input.context_type)?;
    if input.runtime_override.is_some()
        && (input.provider_harness.is_some()
            || input.model_override.is_some()
            || input.logical_effort.is_some()
            || input.codex_fast_mode.is_some()
            || input.team_intent.is_some())
    {
        return Err(
            "runtimeOverride cannot be combined with legacy provider, model, effort, speed, or capability fields"
                .to_string(),
        );
    }
    let legacy_harness_override = input
        .provider_harness
        .as_deref()
        .map(str::parse::<AgentHarnessKind>)
        .transpose()?;
    let persisted_conversation = if let Some(conversation_id) = input.conversation_id.as_deref() {
        state
            .chat_conversation_repo
            .get_by_id(&ChatConversationId::from_string(conversation_id))
            .await
            .map_err(|error| error.to_string())?
    } else {
        None
    };
    let requested_harness = input
        .runtime_override
        .as_ref()
        .map(|runtime| runtime.harness)
        .or(legacy_harness_override)
        .or_else(|| {
            persisted_conversation
                .as_ref()
                .and_then(|conversation| conversation.provider_harness)
        })
        .unwrap_or(DEFAULT_AGENT_HARNESS);
    let requested_capability = input
        .runtime_override
        .as_ref()
        .and_then(|runtime| runtime.coordination_mode)
        .or_else(|| {
            input
                .team_intent
                .as_ref()
                .map(|intent| intent.coordination_mode)
        })
        .or_else(|| {
            persisted_conversation
                .as_ref()
                .map(|conversation| conversation.coordination_mode)
        })
        .unwrap_or_default();
    validate_persona_builder_team_intent_for_send(
        context_type,
        persisted_conversation.as_ref(),
        requested_capability,
    )?;
    let codex_ultra_supported = (requested_capability == CoordinationMode::CodexNativeUltra)
        .then(|| {
            crate::application::agent_capability_validation::codex_ultra_support_for_model(
                requested_harness,
                input
                    .runtime_override
                    .as_ref()
                    .and_then(|runtime| runtime.model.as_deref())
                    .or(input.model_override.as_deref()),
            )
        })
        .flatten();
    crate::application::agent_capability_validation::validate_agent_capability(
        requested_capability,
        requested_harness,
        &state.agent_capability_gate,
        codex_ultra_supported,
    )
    .map_err(|error| error.to_string())?;
    crate::application::managed_team::validate_native_team_intent(
        input.team_intent.as_ref(),
        requested_harness,
    )
    .map_err(|error| error.to_string())?;
    if input.team_message_target.is_some() {
        let native_message_intent = TeamIntent::rx_native(None);
        crate::application::managed_team::validate_native_team_intent(
            Some(&native_message_intent),
            requested_harness,
        )
        .map_err(|error| error.to_string())?;
    }

    let service = create_chat_service(state, app.clone(), execution_state);

    crate::application::validate_chat_runtime_for_context_with_override(
        state,
        context_type,
        &input.context_id,
        "send_agent_message",
        input
            .runtime_override
            .as_ref()
            .map(|runtime| runtime.harness)
            .or(legacy_harness_override),
    )
    .await?;

    let model_override = input
        .model_override
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    let (model_override, logical_effort_override) = normalize_agent_runtime_selection(
        state,
        legacy_harness_override,
        model_override,
        input.logical_effort,
    )
    .await?;
    let service_tier_override =
        crate::application::chat_service::codex_fast_mode_service_tier_override(
            input.codex_fast_mode,
        );
    let mut conversation_id_override = input
        .conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|conversation_id| !conversation_id.is_empty())
        .map(ChatConversationId::from_string);
    let mut auto_forked_terminal_conversation = false;
    if context_type == ChatContextType::Project {
        let parent_conversation_id = conversation_id_override.clone();
        if let Some(forked_conversation_id) = fork_terminal_agent_conversation_for_send(
            state,
            &app,
            parent_conversation_id.as_ref(),
            &input.content,
            legacy_harness_override,
            service_tier_override.clone(),
        )
        .await?
        {
            if let Some(parent_id) = parent_conversation_id.as_ref() {
                let reparented = state
                    .chat_attachment_repo
                    .reparent_pending_attachments(parent_id, &forked_conversation_id)
                    .await;
                if let Err(error) = &reparented {
                    tracing::warn!(
                        parent_conversation_id = %parent_id,
                        child_conversation_id = %forked_conversation_id,
                        %error,
                        "Failed to reparent pending attachments during terminal fork"
                    );
                }
            }
            conversation_id_override = Some(forked_conversation_id);
            auto_forked_terminal_conversation = true;
        }
    }
    if let Some(conversation_id) = conversation_id_override.as_ref() {
        invalidate_agent_workspace_pr_description_cache(conversation_id);
        if context_type == ChatContextType::Project
            && ensure_plan_workspace_planning_session_link_for_send(state, conversation_id).await?
        {
            let _ = app.emit(
                "agent:workspace_changed",
                serde_json::json!({ "conversation_id": conversation_id.as_str() }),
            );
        }
    }
    let composer_artifact_references = if context_type == ChatContextType::Project {
        if let Some(conversation_id) = conversation_id_override.as_ref() {
            admit_linked_edit_plan_references(
                state,
                conversation_id,
                input.composer_artifact_references,
                input.require_approved_linked_plan,
                input.expected_linked_plan_fingerprint.as_deref(),
            )
            .await?
        } else {
            input.composer_artifact_references
        }
    } else {
        input.composer_artifact_references
    };
    let attachment_ids = parse_chat_attachment_ids(&input.attachment_ids)?;

    let mut response = service
        .send_message(
            context_type,
            &input.context_id,
            &input.content,
            SendMessageOptions {
                metadata: input
                    .suppress_user_message
                    .then(hidden_user_message_metadata),
                harness_override: legacy_harness_override,
                model_override,
                logical_effort_override,
                service_tier_override,
                manual_role_runtime_override: input.runtime_override,
                conversation_id_override,
                composer_project_references: input.composer_project_references,
                composer_integration_references: input.composer_integration_references,
                composer_artifact_references,
                composer_selection_snapshot: input.composer_selection_snapshot,
                composer_excerpt_references: input.composer_excerpt_references,
                team_intent: input.team_intent,
                team_message_target: input.team_message_target,
                attachment_ids,
                ..Default::default()
            },
        )
        .await
        .map(SendAgentMessageResponse::from)
        .map_err(|e| e.to_string())?;
    if auto_forked_terminal_conversation {
        response.is_new_conversation = true;
    }
    Ok(response)
}

/// Queue a message to be sent when the current agent run completes
///
/// The message is held in the backend queue and automatically sent
/// via --resume when the current run finishes.
///
/// If `client_id` is provided, that ID will be used for the message,
/// allowing frontend and backend to use the same ID for tracking.
#[tauri::command]
pub async fn queue_agent_message(
    input: QueueAgentMessageInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<QueuedMessageResponse, String> {
    tracing::info!(
        context_type = %input.context_type,
        context_id = %input.context_id,
        content_len = input.content.len(),
        "[QUEUE_MSG] queue_agent_message command invoked"
    );
    let context_type = parse_context_type(&input.context_type)?;

    let service = create_chat_service(&state, app, &execution_state);

    service
        .queue_message(
            context_type,
            &input.context_id,
            &input.content,
            input.client_id.as_deref(),
        )
        .await
        .map(QueuedMessageResponse::from)
        .map_err(|e| e.to_string())
}

/// Get all queued messages for a context
#[tauri::command]
pub async fn get_queued_agent_messages(
    context_type: String,
    context_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<Vec<QueuedMessageResponse>, String> {
    let context_type = parse_context_type(&context_type)?;

    let service = create_chat_service(&state, app, &execution_state);

    service
        .get_queued_messages(context_type, &context_id)
        .await
        .map(visible_queued_message_responses)
        .map_err(|e| e.to_string())
}

/// Delete a queued message before it's sent
#[tauri::command]
pub async fn delete_queued_agent_message(
    context_type: String,
    context_id: String,
    message_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let context_type = parse_context_type(&context_type)?;

    let service = create_chat_service(&state, app, &execution_state);

    service
        .delete_queued_message(context_type, &context_id, &message_id)
        .await
        .map_err(|e| e.to_string())
}

/// Send a queued message immediately, interrupting the active provider process.
async fn send_queued_agent_message_now_for_state<R: Runtime + 'static>(
    context_type: String,
    context_id: String,
    message_id: String,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app: tauri::AppHandle<R>,
) -> Result<SendAgentMessageResponse, String> {
    let context_type = parse_context_type(&context_type)?;
    let service = create_chat_service(state, app, execution_state);

    service
        .send_queued_message_now(context_type, &context_id, &message_id)
        .await
        .map(SendAgentMessageResponse::from)
        .map_err(|e| e.to_string())
}

/// Send a queued message immediately, interrupting the active provider process.
#[tauri::command]
pub async fn send_queued_agent_message_now(
    context_type: String,
    context_id: String,
    message_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<SendAgentMessageResponse, String> {
    send_queued_agent_message_now_for_state(
        context_type,
        context_id,
        message_id,
        &state,
        &execution_state,
        app,
    )
    .await
}

/// List all conversations for a context
#[tauri::command]
pub async fn list_agent_conversations(
    context_type: String,
    context_id: String,
    include_archived: Option<bool>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<Vec<AgentConversationResponse>, String> {
    let context_type_enum = parse_context_type(&context_type)?;

    let include_archived = include_archived.unwrap_or(false);
    let conversations = if include_archived {
        state
            .chat_conversation_repo
            .get_by_context_filtered(context_type_enum, &context_id, true)
            .await
            .map_err(|e| e.to_string())?
    } else {
        let service = create_chat_service(&state, app, &execution_state);
        service
            .list_conversations(context_type_enum, &context_id)
            .await
            .map_err(|e| e.to_string())?
    };

    let conversations =
        filter_agent_list_visible_conversations(state.inner(), conversations).await?;
    agent_conversation_responses_for_state(state.inner(), conversations).await
}

/// List a page of conversations for a context with optional title search.
#[tauri::command]
pub async fn list_agent_conversations_page(
    context_type: String,
    context_id: String,
    include_archived: Option<bool>,
    archived_only: Option<bool>,
    offset: Option<u32>,
    limit: Option<u32>,
    search: Option<String>,
    state: State<'_, AppState>,
) -> Result<AgentConversationListPageResponse, String> {
    let context_type_enum = parse_context_type(&context_type)?;
    let archived_only = archived_only.unwrap_or(false);
    let include_archived = include_archived.unwrap_or(false) || archived_only;
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(6);

    let mut conversations = state
        .chat_conversation_repo
        .get_by_context_filtered(context_type_enum, &context_id, include_archived)
        .await
        .map_err(|e| e.to_string())?;
    conversations = filter_agent_list_visible_conversations(state.inner(), conversations)
        .await?
        .into_iter()
        .filter(|conversation| {
            if archived_only && !conversation.is_archived() {
                return false;
            }
            conversation_matches_agent_list_search(conversation, search.as_deref())
        })
        .collect();
    let total = i64::try_from(conversations.len()).unwrap_or(i64::MAX);
    let offset_usize = usize::try_from(offset).unwrap_or(usize::MAX);
    let limit_usize = usize::try_from(limit).unwrap_or(usize::MAX);
    let conversations = conversations
        .into_iter()
        .skip(offset_usize)
        .take(limit_usize)
        .collect::<Vec<_>>();
    let has_more = i64::from(offset.saturating_add(limit)) < total;

    Ok(AgentConversationListPageResponse {
        conversations: agent_conversation_responses_for_state(state.inner(), conversations).await?,
        total,
        limit,
        offset,
        has_more,
    })
}

async fn filter_agent_list_visible_conversations(
    state: &AppState,
    conversations: Vec<ChatConversation>,
) -> Result<Vec<ChatConversation>, String> {
    let mut visible = Vec::with_capacity(conversations.len());
    for conversation in conversations {
        if conversation.automation_run_id.is_some() {
            continue;
        }
        if conversation.context_type != ChatContextType::Project
            || conversation.parent_conversation_id.is_none()
            || state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&conversation.id)
                .await
                .map_err(|e| e.to_string())?
                .is_some()
        {
            visible.push(conversation);
        }
    }
    Ok(visible)
}

fn conversation_matches_agent_list_search(
    conversation: &ChatConversation,
    search: Option<&str>,
) -> bool {
    let Some(search) = search.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let title = conversation.title.as_deref().unwrap_or("Untitled agent");
    title.to_lowercase().contains(&search.to_lowercase())
}

/// Core archive logic, testable without Tauri `State` wrapper.
#[doc(hidden)]
pub async fn archive_agent_conversation_inner(
    conversation_id: &ChatConversationId,
    close_pull_request: bool,
    state: &AppState,
) -> Result<TerminalAgentWorkspaceOutcome, String> {
    archive_agent_conversation_for_state(conversation_id, state, close_pull_request).await
}

/// Archive a conversation.
/// An open PR is closed only when the caller opts in. Review PR workspaces
/// never close their reviewed pull request. Verified RalphX-owned local
/// worktree and branch artifacts are cleaned immediately after runtime shutdown.
#[tauri::command]
pub async fn archive_agent_conversation(
    conversation_id: String,
    close_pull_request: bool,
    state: State<'_, AppState>,
) -> Result<ArchiveAgentConversationResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let cleanup =
        archive_agent_conversation_inner(&conversation_id, close_pull_request, &state).await?;

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;
    Ok(ArchiveAgentConversationResponse {
        conversation: agent_conversation_response_for_state(state.inner(), conversation).await?,
        cleanup,
    })
}

/// Restore an archived conversation.
#[tauri::command]
pub async fn restore_agent_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<AgentConversationResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    state
        .chat_conversation_repo
        .restore(&conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;
    agent_conversation_response_for_state(state.inner(), conversation).await
}

/// Get workspace metadata for a project-backed agent conversation.
#[tauri::command]
pub async fn get_agent_conversation_workspace(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Option<AgentConversationWorkspaceResponse>, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    match workspace {
        Some(workspace) => {
            schedule_external_pr_reconciliation_for_workspace(
                state.inner(),
                &workspace,
                AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
                false,
            );
            Ok(Some(
                agent_workspace_response_for_state(state.inner(), workspace).await?,
            ))
        }
        None => Ok(None),
    }
}

fn normalize_agent_workspace_auto_merge_method(method: Option<String>) -> Result<String, String> {
    let method = method
        .unwrap_or_else(|| DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string())
        .trim()
        .to_ascii_lowercase();
    let method = if method.is_empty() {
        DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string()
    } else {
        method
    };
    match method.as_str() {
        "squash" | "merge" | "rebase" => Ok(method),
        _ => Err(format!(
            "Unsupported auto-merge method '{method}'. Use squash, merge, or rebase."
        )),
    }
}

#[derive(Debug, Clone)]
struct AgentWorkspacePrAutomationTarget {
    project: Option<Project>,
    working_dir: PathBuf,
    pr_number: i64,
    pr_url: Option<String>,
    pr_status: Option<String>,
    push_status: Option<String>,
}

#[derive(Clone, Copy)]
enum LinkedPlanPrAutomationCwd {
    GitHubSafeProjectCheckout,
    EnsuredPlanWorktree,
}

fn plan_branch_publication_status(plan_branch: &PlanBranch) -> Option<String> {
    if plan_branch.status == PlanBranchStatus::Merged {
        Some("merged".to_string())
    } else {
        plan_branch
            .pr_status
            .as_ref()
            .map(|status| status.to_db_string().to_ascii_lowercase())
    }
}

async fn apply_linked_plan_branch_edit_handoff(
    state: &AppState,
    workspace: &mut AgentConversationWorkspace,
) -> Result<bool, String> {
    let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
        return Ok(false);
    };
    let Some(plan_branch) = state
        .plan_branch_repo
        .get_by_id(plan_branch_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    if plan_branch.status != PlanBranchStatus::Active || plan_branch.pr_number.is_none() {
        return Ok(false);
    }
    let Some(project) = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Err(format!("Project not found: {}", workspace.project_id));
    };

    let base_ref = plan_branch_base_ref(&plan_branch, &project);
    let base_display_name = plan_branch_base_display_name(&base_ref);
    let worktree_path = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
        .await
        .map_err(|error| error.to_string())?;
    let worktree_path = worktree_path.to_string_lossy().to_string();
    let publication_pr_status = plan_branch_publication_status(&plan_branch);
    let publication_push_status = Some(plan_branch.pr_push_status.to_db_string().to_string());

    let changed = workspace.branch_name != plan_branch.branch_name
        || workspace.worktree_path != worktree_path
        || workspace.base_ref != base_ref
        || workspace.base_display_name != base_display_name
        || workspace.publication_pr_number != plan_branch.pr_number
        || workspace.publication_pr_url != plan_branch.pr_url
        || workspace.publication_pr_status != publication_pr_status
        || workspace.publication_push_status != publication_push_status;

    workspace.branch_name = plan_branch.branch_name;
    workspace.worktree_path = worktree_path;
    workspace.base_ref = base_ref;
    workspace.base_display_name = base_display_name;
    workspace.publication_pr_number = plan_branch.pr_number;
    workspace.publication_pr_url = plan_branch.pr_url;
    workspace.publication_pr_status = publication_pr_status;
    workspace.publication_push_status = publication_push_status;

    Ok(changed)
}

async fn resolve_agent_workspace_pr_automation_target(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> Result<Option<AgentWorkspacePrAutomationTarget>, String> {
    resolve_agent_workspace_pr_automation_target_with_linked_plan_cwd(
        state,
        workspace,
        LinkedPlanPrAutomationCwd::GitHubSafeProjectCheckout,
    )
    .await
}

async fn resolve_agent_workspace_pr_automation_target_with_ensured_linked_plan_worktree(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> Result<Option<AgentWorkspacePrAutomationTarget>, String> {
    resolve_agent_workspace_pr_automation_target_with_linked_plan_cwd(
        state,
        workspace,
        LinkedPlanPrAutomationCwd::EnsuredPlanWorktree,
    )
    .await
}

async fn resolve_agent_workspace_pr_automation_target_with_linked_plan_cwd(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    linked_plan_cwd: LinkedPlanPrAutomationCwd,
) -> Result<Option<AgentWorkspacePrAutomationTarget>, String> {
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?;

    if workspace.mode == AgentConversationWorkspaceMode::Ideation {
        let Some(project) = project else {
            return Ok(None);
        };
        let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
            return Ok(None);
        };
        let Some(plan_branch) = state
            .plan_branch_repo
            .get_by_id(plan_branch_id)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        let Some(pr_number) = plan_branch.pr_number else {
            return Ok(None);
        };
        let working_dir = match linked_plan_cwd {
            LinkedPlanPrAutomationCwd::GitHubSafeProjectCheckout => {
                let repo_path = PathBuf::from(&project.working_directory);
                crate::utils::path_safety::validate_absolute_non_root_path(
                    &repo_path,
                    "project checkout",
                )
                .map_err(|error| error.to_string())?
            }
            LinkedPlanPrAutomationCwd::EnsuredPlanWorktree => {
                ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
                    .await
                    .map_err(|error| error.to_string())?
            }
        };
        return Ok(Some(AgentWorkspacePrAutomationTarget {
            project: Some(project),
            working_dir,
            pr_number,
            pr_url: plan_branch.pr_url.clone(),
            pr_status: plan_branch_publication_status(&plan_branch),
            push_status: Some(plan_branch.pr_push_status.to_db_string().to_string()),
        }));
    }

    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(None);
    };
    let working_dir = PathBuf::from(&workspace.worktree_path);
    Ok(Some(AgentWorkspacePrAutomationTarget {
        project,
        working_dir,
        pr_number,
        pr_url: workspace.publication_pr_url.clone(),
        pr_status: workspace.publication_pr_status.clone(),
        push_status: workspace.publication_push_status.clone(),
    }))
}

async fn sync_agent_workspace_publication_from_pr_automation_target(
    state: &AppState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspacePrAutomationTarget,
) -> Result<(), String> {
    if workspace.publication_pr_number == Some(target.pr_number)
        && workspace.publication_pr_url == target.pr_url
        && workspace.publication_pr_status == target.pr_status
        && workspace.publication_push_status == target.push_status
    {
        return Ok(());
    }

    state
        .agent_conversation_workspace_repo
        .update_publication(
            conversation_id,
            Some(target.pr_number),
            target.pr_url.as_deref(),
            target.pr_status.as_deref(),
            target.push_status.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

async fn reconcile_agent_workspace_auto_merge_for_supervision_toggle(
    state: &AppState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    target: Option<&AgentWorkspacePrAutomationTarget>,
    auto_merge_desired: bool,
    auto_merge_method: &str,
) -> Result<(), String> {
    if !auto_merge_desired {
        crate::application::agent_workspace_review_auto_merge::
            cancel_workspace_review_auto_merge_guard(state, workspace)
                .await
                .map_err(|error| error.to_string())?;
    }
    let (Some(github), Some(target)) = (state.github_service.as_ref(), target) else {
        return Ok(());
    };

    let pr_number = target.pr_number;
    let working_dir = target.working_dir.as_path();
    if auto_merge_desired {
        let monitor = state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(conversation_id)
            .await
            .map_err(|error| error.to_string())?;
        if crate::application::agent_workspace_review_auto_merge::auto_merge_guard_blocks_enable(
            monitor.as_ref(),
        ) {
            let mut desired_workspace = workspace.clone();
            desired_workspace.pr_auto_merge_desired = true;
            desired_workspace.pr_auto_merge_method = auto_merge_method.to_string();
            sync_agent_workspace_auto_merge_preference_for_workspace(
                Arc::clone(github),
                working_dir,
                pr_number,
                &desired_workspace,
                Arc::clone(&state.agent_conversation_workspace_repo),
            )
            .await
            .map_err(|error| error.to_string())?;
            return Ok(());
        }
    }
    if auto_merge_desired {
        let enable_result = async {
            if target.pr_status.as_deref() == Some("draft") {
                github.mark_pr_ready(working_dir, pr_number).await?;
            }
            github
                .enable_pr_auto_merge(working_dir, pr_number, auto_merge_method)
                .await
        }
        .await;

        match enable_result {
            Ok(()) => {
                state
                    .agent_conversation_workspace_repo
                    .update_pr_auto_merge_state(
                        conversation_id,
                        Some(true),
                        Some("monitoring"),
                        Some("GitHub auto-merge is enabled for this PR."),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Err(error) => {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Agent workspace PR supervision deferred GitHub auto-merge enable"
                );
                state
                    .agent_conversation_workspace_repo
                    .update_pr_auto_merge_state(
                        conversation_id,
                        Some(false),
                        Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING),
                        Some(&auto_merge_enable_failure_summary(&error)),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
    } else {
        let mut desired_workspace = workspace.clone();
        desired_workspace.pr_auto_merge_desired = false;
        desired_workspace.pr_auto_merge_method = auto_merge_method.to_string();

        if let Err(error) = sync_agent_workspace_auto_merge_preference_for_workspace(
            Arc::clone(github),
            working_dir,
            pr_number,
            &desired_workspace,
            Arc::clone(&state.agent_conversation_workspace_repo),
        )
        .await
        {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                error = %error,
                "Agent workspace PR supervision deferred GitHub auto-merge disable"
            );
            state
                .agent_conversation_workspace_repo
                .update_pr_auto_merge_state(
                    conversation_id,
                    Some(true),
                    Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING),
                    Some(&auto_merge_disable_failure_summary(&error)),
                )
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

/// Update PR supervision preferences for a project-backed agent conversation.
#[tauri::command]
pub async fn set_agent_conversation_workspace_pr_supervision(
    conversation_id: String,
    input: AgentConversationWorkspacePrSupervisionInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationWorkspaceResponse, String> {
    set_agent_conversation_workspace_pr_supervision_for_state(conversation_id, input, state.inner())
        .await
}

pub async fn set_agent_conversation_workspace_pr_supervision_for_state(
    conversation_id: String,
    input: AgentConversationWorkspacePrSupervisionInput,
    state: &AppState,
) -> Result<AgentConversationWorkspaceResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let auto_merge_method = normalize_agent_workspace_auto_merge_method(input.auto_merge_method)?;
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Err("Agent conversation workspace not found".to_string());
    };
    if !workspace.allows_owned_pr_mutation() {
        return Err(
            if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
                "PR supervision is unavailable in Review PR mode".to_string()
            } else {
                "PR supervision is unavailable for this workspace".to_string()
            },
        );
    }

    let automation_target = resolve_agent_workspace_pr_automation_target(state, &workspace).await?;
    let terminal_publication_status = workspace.has_terminal_publication_pr_status()
        || automation_target.as_ref().is_some_and(|target| {
            is_terminal_agent_conversation_publication_status(target.pr_status.as_deref())
        });
    if terminal_publication_status {
        return Err("PR supervision cannot be changed for a closed or merged PR".to_string());
    }
    if !workspace.auto_publish_enabled && (input.auto_fix_enabled || input.auto_merge_desired) {
        return Err(
            "Auto Publish is paused for this workspace. Turn Auto Publish back on before enabling PR supervision."
                .to_string(),
        );
    }
    let newly_enables_pr_automation = (input.auto_fix_enabled && !workspace.pr_autofix_enabled)
        || (input.auto_merge_desired && !workspace.pr_auto_merge_desired);
    let ensured_automation_target = if newly_enables_pr_automation {
        resolve_agent_workspace_pr_automation_target_with_ensured_linked_plan_worktree(
            state, &workspace,
        )
        .await?
    } else {
        None
    };

    let _workspace_changed_guard = state
        .app_handle
        .as_ref()
        .map(|app| emit_workspace_changed_when_done(app, &conversation_id));

    if let Some(target) = automation_target.as_ref() {
        sync_agent_workspace_publication_from_pr_automation_target(
            state,
            &conversation_id,
            &workspace,
            target,
        )
        .await?;
    }

    state
        .agent_conversation_workspace_repo
        .update_pr_supervision_preferences(
            &conversation_id,
            input.auto_fix_enabled,
            input.auto_merge_desired,
            &auto_merge_method,
        )
        .await
        .map_err(|e| e.to_string())?;

    reconcile_agent_workspace_auto_merge_for_supervision_toggle(
        state,
        &conversation_id,
        &workspace,
        ensured_automation_target
            .as_ref()
            .or(automation_target.as_ref()),
        input.auto_merge_desired,
        &auto_merge_method,
    )
    .await?;

    if newly_enables_pr_automation {
        if let Some(target) = ensured_automation_target
            .as_ref()
            .or(automation_target.as_ref())
        {
            if let Some(project) = target.project.clone() {
                let chat_service: Arc<dyn ChatService> = Arc::new(state.build_chat_service());
                state
                    .pr_poller_registry
                    .start_agent_workspace_polling_with_repair_repo(
                        conversation_id.clone(),
                        target.pr_number,
                        project,
                        target.working_dir.clone(),
                        Arc::clone(&state.agent_conversation_workspace_repo),
                        Arc::clone(&state.agent_run_repo),
                        Arc::clone(&state.agent_workspace_repair_repo),
                        chat_service,
                    );
            }
        }
    }

    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_supervision",
            if input.auto_fix_enabled || input.auto_merge_desired {
                "enabled"
            } else {
                "disabled"
            },
            if input.auto_fix_enabled && input.auto_merge_desired {
                "RalphX will monitor PR failures/reviews and request GitHub auto-merge when possible."
            } else if input.auto_fix_enabled {
                "RalphX will monitor PR failures/reviews and request fixes when needed."
            } else if input.auto_merge_desired {
                "RalphX will request GitHub auto-merge when possible."
            } else {
                "RalphX PR supervision is disabled."
            },
            Some("pr_supervision_preferences".to_string()),
        ))
        .await
        .map_err(|e| e.to_string())?;

    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Agent conversation workspace not found".to_string())?;
    agent_workspace_response_for_state(state, updated).await
}

/// Enable or pause automatic publish behavior for a project-backed agent workspace.
#[tauri::command]
pub async fn set_agent_conversation_workspace_auto_publish(
    conversation_id: String,
    input: AgentConversationWorkspaceAutoPublishInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationWorkspaceResponse, String> {
    set_agent_conversation_workspace_auto_publish_for_state(conversation_id, input, state.inner())
        .await
}

/// A preference enable may resume only the exact durable `Ready + Publish` generation. The
/// coordinator's lease/CAS transition fences stale snapshots before it can start a repair-owned
/// Git or GitHub effect; its result is reflected by the normal response projection below.
async fn resume_ready_publish_repair_after_auto_publish_enabled(
    state: &AppState,
    conversation_id: &ChatConversationId,
) {
    let attempt = match state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await
    {
        Ok(Some(attempt))
            if attempt.phase == AgentWorkspaceRepairPhase::Ready
                && attempt.continuation == AgentWorkspaceRepairContinuation::Publish =>
        {
            attempt
        }
        Ok(_) => return,
        Err(error) => {
            tracing::warn!(
                conversation_id = %conversation_id,
                %error,
                "Auto Publish enable could not load the durable repair attempt"
            );
            return;
        }
    };

    let transition = match resume_ready_agent_workspace_repair_for_publish(
        state,
        attempt,
        "Auto Publish was enabled; resuming the durable workspace repair continuation.",
    )
    .await
    {
        Ok(transition) => transition,
        Err(error) => {
            tracing::warn!(
                conversation_id = %conversation_id,
                %error,
                "Auto Publish enable could not resume the durable repair continuation"
            );
            return;
        }
    };

    let AgentWorkspaceRepairTransitionOutcome::Applied(attempt) = transition else {
        return;
    };
    if !matches!(
        attempt.phase,
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
    ) {
        return;
    }

    if let Err(error) = continue_agent_workspace_repair_publish(state, attempt).await {
        tracing::warn!(
            conversation_id = %conversation_id,
            %error,
            "Auto Publish enable left the durable repair continuation pending reconciliation"
        );
    }
}

pub async fn set_agent_conversation_workspace_auto_publish_for_state(
    conversation_id: String,
    input: AgentConversationWorkspaceAutoPublishInput,
    state: &AppState,
) -> Result<AgentConversationWorkspaceResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Err("Agent conversation workspace not found".to_string());
    };
    if !workspace.allows_owned_pr_mutation() {
        return Err(
            if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
                "Auto Publish cannot be changed in Review PR mode".to_string()
            } else {
                "Auto Publish cannot be changed for this workspace".to_string()
            },
        );
    }

    let automation_target = resolve_agent_workspace_pr_automation_target(state, &workspace).await?;
    let terminal_publication_status = workspace.has_terminal_publication_pr_status()
        || automation_target.as_ref().is_some_and(|target| {
            is_terminal_agent_conversation_publication_status(target.pr_status.as_deref())
        });
    if terminal_publication_status {
        return Err("Auto Publish cannot be changed for a closed or merged PR".to_string());
    }

    let _workspace_changed_guard = state
        .app_handle
        .as_ref()
        .map(|app| emit_workspace_changed_when_done(app, &conversation_id));

    if let Some(target) = automation_target.as_ref() {
        sync_agent_workspace_publication_from_pr_automation_target(
            state,
            &conversation_id,
            &workspace,
            target,
        )
        .await?;
    }

    if automation_target.is_none() && workspace.publication_pr_number.is_none() {
        if input.auto_publish_enabled == workspace.auto_publish_initial_pr_enabled {
            return agent_workspace_response_without_repair_recovery_for_state(state, workspace)
                .await;
        }

        state
            .agent_conversation_workspace_repo
            .update_auto_publish_initial_pr_preference(&conversation_id, input.auto_publish_enabled)
            .await
            .map_err(|e| e.to_string())?;

        state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "auto_publish",
                if input.auto_publish_enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                if input.auto_publish_enabled {
                    "Auto Publish is enabled for the first pull request."
                } else {
                    "Auto Publish is disabled for the first pull request."
                },
                Some("auto_publish_preferences".to_string()),
            ))
            .await
            .map_err(|e| e.to_string())?;

        if input.auto_publish_enabled {
            resume_ready_publish_repair_after_auto_publish_enabled(state, &conversation_id).await;
        }

        let updated = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Agent conversation workspace not found".to_string())?;
        return agent_workspace_response_for_state(state, updated).await;
    }

    if input.auto_publish_enabled == workspace.auto_publish_enabled {
        return agent_workspace_response_without_repair_recovery_for_state(state, workspace).await;
    }

    let auto_merge_method = workspace.pr_auto_merge_method.clone();
    let (
        paused_pr_autofix_enabled,
        paused_pr_auto_merge_desired,
        pr_autofix_enabled,
        pr_auto_merge_desired,
        supervision_status,
        supervision_summary,
        event_status,
        event_summary,
    ) = if input.auto_publish_enabled {
        let restored_autofix = workspace
            .auto_publish_paused_pr_autofix_enabled
            .unwrap_or(workspace.pr_autofix_enabled);
        let restored_auto_merge = workspace
            .auto_publish_paused_pr_auto_merge_desired
            .unwrap_or(workspace.pr_auto_merge_desired);
        let summary = if restored_autofix || restored_auto_merge {
            Some("RalphX PR supervision is enabled.")
        } else {
            None
        };
        (
            None,
            None,
            restored_autofix,
            restored_auto_merge,
            Some(if restored_autofix || restored_auto_merge {
                "monitoring"
            } else {
                "disabled"
            }),
            summary,
            "enabled",
            if restored_autofix || restored_auto_merge {
                "Auto Publish is enabled; previous PR supervision preferences were restored."
            } else {
                "Auto Publish is enabled."
            },
        )
    } else {
        (
            Some(workspace.pr_autofix_enabled),
            Some(workspace.pr_auto_merge_desired),
            false,
            false,
            Some("paused"),
            Some("Auto Publish is paused. Manual Commit & Publish is still available."),
            "disabled",
            "Auto Publish is paused. Background publish, PR autofix, and auto-merge automation are disabled.",
        )
    };

    state
        .agent_conversation_workspace_repo
        .update_auto_publish_preferences(
            &conversation_id,
            input.auto_publish_enabled,
            paused_pr_autofix_enabled,
            paused_pr_auto_merge_desired,
            pr_autofix_enabled,
            pr_auto_merge_desired,
            supervision_status,
            supervision_summary,
        )
        .await
        .map_err(|e| e.to_string())?;

    if !input.auto_publish_enabled {
        crate::application::agent_workspace_review_auto_merge::
            cancel_workspace_review_auto_merge_guard(state, &workspace)
                .await
                .map_err(|error| error.to_string())?;
    }

    if input.auto_publish_enabled && pr_auto_merge_desired {
        reconcile_agent_workspace_auto_merge_for_supervision_toggle(
            state,
            &conversation_id,
            &workspace,
            automation_target.as_ref(),
            true,
            &auto_merge_method,
        )
        .await?;
    } else if !input.auto_publish_enabled {
        let refreshed_for_sync = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Agent conversation workspace not found".to_string())?;
        if let (Some(github), Some(pr_number)) = (
            state.github_service.as_ref(),
            automation_target
                .as_ref()
                .map(|target| target.pr_number)
                .or(refreshed_for_sync.publication_pr_number),
        ) {
            if let Err(error) = sync_agent_workspace_auto_merge_preference_for_workspace(
                Arc::clone(github),
                automation_target
                    .as_ref()
                    .map(|target| target.working_dir.as_path())
                    .unwrap_or_else(|| Path::new(&refreshed_for_sync.worktree_path)),
                pr_number,
                &refreshed_for_sync,
                Arc::clone(&state.agent_conversation_workspace_repo),
            )
            .await
            {
                state
                    .agent_conversation_workspace_repo
                    .update_pr_auto_merge_state(
                        &conversation_id,
                        refreshed_for_sync.pr_auto_merge_current,
                        Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING),
                        Some(&format!(
                            "GitHub auto-merge state could not be refreshed while pausing Auto Publish: {error}"
                        )),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "auto_publish",
            event_status,
            event_summary,
            Some("auto_publish_preferences".to_string()),
        ))
        .await
        .map_err(|e| e.to_string())?;

    if input.auto_publish_enabled {
        resume_ready_publish_repair_after_auto_publish_enabled(state, &conversation_id).await;
    }

    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Agent conversation workspace not found".to_string())?;
    agent_workspace_response_for_state(state, updated).await
}

/// Schedule a background publication reconciliation for a project-backed agent conversation.
#[tauri::command]
pub async fn reconcile_agent_conversation_workspace_publication(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    schedule_external_pr_reconciliation_for_conversation_id(
        state.inner(),
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::AgentRunCompleted,
        false,
    )
    .await?;
    schedule_pr_supervision_recovery_for_conversation_id(
        state.inner(),
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
        true,
    )
    .await
}

/// List workspace metadata for project-backed agent conversations.
#[tauri::command]
pub async fn list_agent_conversation_workspaces_by_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<AgentConversationWorkspaceResponse>, String> {
    let project_id = ProjectId::from_string(project_id);
    let workspaces = state
        .agent_conversation_workspace_repo
        .get_by_project_id(&project_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut responses = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        responses.push(agent_workspace_response_for_state(state.inner(), workspace).await?);
    }
    Ok(responses)
}

/// List durable publish events for a project-backed agent conversation workspace.
#[tauri::command]
pub async fn list_agent_conversation_workspace_publication_events(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<AgentConversationWorkspacePublicationEventResponse>, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .map_err(|e| e.to_string())
        .map(|events| {
            events
                .into_iter()
                .map(AgentConversationWorkspacePublicationEventResponse::from)
                .collect()
        })
}

/// Inspect whether the workspace's captured base commit is behind the current base ref.
#[tauri::command]
pub async fn get_agent_conversation_workspace_freshness(
    conversation_id: String,
    freshness_scope: Option<String>,
    state: State<'_, AppState>,
) -> Result<AgentConversationWorkspaceFreshnessResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    get_agent_conversation_workspace_freshness_for_app_state(
        &conversation_id,
        freshness_scope.as_deref(),
        state.inner(),
    )
    .await
}

#[doc(hidden)]
pub async fn get_agent_conversation_workspace_freshness_for_app_state(
    conversation_id: &ChatConversationId,
    freshness_scope: Option<&str>,
    state: &AppState,
) -> Result<AgentConversationWorkspaceFreshnessResponse, String> {
    let freshness_scope = AgentWorkspaceFreshnessScope::parse(freshness_scope)?;
    let started = Instant::now();
    let result =
        get_agent_conversation_workspace_freshness_cached(conversation_id, freshness_scope, state)
            .await;
    match &result {
        Ok((response, cache_status)) => tracing::info!(
            target: "ralphx_lib::commands::agent_workspace_freshness",
            conversation_id = %conversation_id,
            freshness_scope = freshness_scope.as_str(),
            elapsed_ms = started.elapsed().as_millis(),
            cache_status = cache_status.as_str(),
            base_status = response.base_status.as_str(),
            has_uncommitted_changes = response.has_uncommitted_changes,
            unpublished_commit_count = ?response.unpublished_commit_count,
            is_base_ahead = response.is_base_ahead,
            remote_refreshed = response.remote_refreshed,
            worktree_status_checked = response.worktree_status_checked,
            "Loaded agent workspace freshness"
        ),
        Err(error) => tracing::warn!(
            target: "ralphx_lib::commands::agent_workspace_freshness",
            conversation_id = %conversation_id,
            freshness_scope = freshness_scope.as_str(),
            elapsed_ms = started.elapsed().as_millis(),
            error,
            "Failed to load agent workspace freshness"
        ),
    }
    result.map(|(response, _)| response)
}

async fn get_agent_conversation_workspace_freshness_cached(
    conversation_id: &ChatConversationId,
    freshness_scope: AgentWorkspaceFreshnessScope,
    state: &AppState,
) -> Result<
    (
        AgentConversationWorkspaceFreshnessResponse,
        AgentWorkspaceFreshnessCacheStatus,
    ),
    String,
> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            )
        })?;
    ensure_agent_workspace_supports_freshness(&workspace)?;

    let phase_started_at = Instant::now();
    if let Some(response) = cached_agent_workspace_freshness(conversation_id, freshness_scope) {
        log_agent_workspace_freshness_phase(
            conversation_id,
            freshness_scope,
            "cache_lookup_initial",
            phase_started_at,
        );
        return Ok((response, AgentWorkspaceFreshnessCacheStatus::Hit));
    }
    log_agent_workspace_freshness_phase(
        conversation_id,
        freshness_scope,
        "cache_lookup_initial",
        phase_started_at,
    );

    let key = format!("{}:{}", conversation_id.as_str(), freshness_scope.as_str());
    let lock = agent_workspace_freshness_locks()
        .entry(key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let phase_started_at = Instant::now();
    let _guard = lock.lock().await;
    log_agent_workspace_freshness_phase(
        conversation_id,
        freshness_scope,
        "coalescing_lock_wait",
        phase_started_at,
    );

    let phase_started_at = Instant::now();
    if let Some(response) = cached_agent_workspace_freshness(conversation_id, freshness_scope) {
        log_agent_workspace_freshness_phase(
            conversation_id,
            freshness_scope,
            "cache_lookup_coalesced",
            phase_started_at,
        );
        return Ok((response, AgentWorkspaceFreshnessCacheStatus::Coalesced));
    }
    log_agent_workspace_freshness_phase(
        conversation_id,
        freshness_scope,
        "cache_lookup_coalesced",
        phase_started_at,
    );

    let phase_started_at = Instant::now();
    let response = get_agent_conversation_workspace_freshness_for_state(
        conversation_id,
        freshness_scope,
        state,
    )
    .await?;
    log_agent_workspace_freshness_phase(
        conversation_id,
        freshness_scope,
        "compute",
        phase_started_at,
    );
    let phase_started_at = Instant::now();
    store_agent_workspace_freshness(conversation_id, freshness_scope, &response);
    log_agent_workspace_freshness_phase(
        conversation_id,
        freshness_scope,
        "cache_store",
        phase_started_at,
    );
    Ok((response, AgentWorkspaceFreshnessCacheStatus::Miss))
}

async fn get_agent_conversation_workspace_local_freshness(
    state: &AppState,
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> Result<AgentConversationWorkspaceFreshnessResponse, String> {
    if workspace.mode == AgentConversationWorkspaceMode::Ideation {
        let phase_started_at = Instant::now();
        let target = resolve_agent_workspace_publish_target(state, project, workspace).await?;
        log_agent_workspace_freshness_phase(
            &workspace.conversation_id,
            AgentWorkspaceFreshnessScope::Local,
            "local_publish_target_resolution",
            phase_started_at,
        );
        return Ok(
            AgentConversationWorkspaceFreshnessResponse::from_local_summary(
                workspace.conversation_id.as_str(),
                target.base_ref,
                target.base_display_name,
                target.branch_name,
                workspace.base_commit.clone(),
            ),
        );
    }

    let phase_started_at = Instant::now();
    resolve_agent_conversation_workspace_path_for_send(project, workspace)
        .map_err(|e| e.to_string())?;
    log_agent_workspace_freshness_phase(
        &workspace.conversation_id,
        AgentWorkspaceFreshnessScope::Local,
        "local_path_resolution",
        phase_started_at,
    );

    Ok(
        AgentConversationWorkspaceFreshnessResponse::from_local_summary(
            workspace.conversation_id.as_str(),
            workspace.base_ref.clone(),
            workspace.base_display_name.clone(),
            workspace.branch_name.clone(),
            workspace.base_commit.clone(),
        ),
    )
}

fn ensure_agent_workspace_supports_freshness(
    workspace: &AgentConversationWorkspace,
) -> Result<(), String> {
    if workspace.mode == AgentConversationWorkspaceMode::Edit
        || (workspace.mode == AgentConversationWorkspaceMode::Ideation
            && workspace.linked_plan_branch_id.is_some())
    {
        return Ok(());
    }

    Err(
        "Only edit workspaces and ideation workspaces with linked plan branches can be inspected for freshness"
            .to_string(),
    )
}

async fn get_agent_conversation_workspace_freshness_for_state(
    conversation_id: &ChatConversationId,
    freshness_scope: AgentWorkspaceFreshnessScope,
    state: &AppState,
) -> Result<AgentConversationWorkspaceFreshnessResponse, String> {
    let phase_started_at = Instant::now();
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            )
        })?;
    log_agent_workspace_freshness_phase(
        conversation_id,
        freshness_scope,
        "workspace_read",
        phase_started_at,
    );
    ensure_agent_workspace_supports_freshness(&workspace)?;
    let phase_started_at = Instant::now();
    let mut workspace = recover_stale_publish_repair_for_workspace_in_state(state, workspace)
        .await
        .map_err(|e| e.to_string())?;
    log_agent_workspace_freshness_phase(
        conversation_id,
        freshness_scope,
        "stale_publish_repair",
        phase_started_at,
    );
    if is_terminal_agent_conversation_publication_status(workspace.publication_pr_status.as_deref())
    {
        return Ok(
            AgentConversationWorkspaceFreshnessResponse::from_terminal_publication(
                conversation_id.as_str(),
                freshness_scope,
                &workspace,
            ),
        );
    }

    let phase_started_at = Instant::now();
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;
    log_agent_workspace_freshness_phase(
        conversation_id,
        freshness_scope,
        "project_read",
        phase_started_at,
    );

    if freshness_scope == AgentWorkspaceFreshnessScope::Local {
        let phase_started_at = Instant::now();
        let response =
            get_agent_conversation_workspace_local_freshness(state, &project, &workspace).await?;
        log_agent_workspace_freshness_phase(
            conversation_id,
            freshness_scope,
            "local_summary",
            phase_started_at,
        );
        return Ok(response);
    }

    // For ideation workspaces linked to a plan branch, check freshness of the
    // plan branch against its base (the workspace's own branch has no commits).
    if workspace.mode == AgentConversationWorkspaceMode::Ideation {
        let mut target =
            resolve_agent_workspace_publish_target(state, &project, &workspace).await?;
        let base_resolution = resolve_workspace_base(&project, &workspace)
            .await
            .map_err(|e| e.to_string())?;
        if base_resolution.status == BaseStatus::Blocked {
            return Ok(AgentConversationWorkspaceFreshnessResponse::blocked(
                workspace.conversation_id.as_str(),
                AgentWorkspaceFreshnessScope::Full,
                &workspace,
                &base_resolution,
                false,
                Some(0),
                true,
                false,
            ));
        }
        apply_base_resolution_to_publish_target(&mut target, &base_resolution)?;
        let status = inspect_publish_branch_freshness_for_source_after_fetch(
            &target.worktree_path,
            &target.base_ref,
            &target.branch_name,
            workspace.base_commit.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;

        return Ok(
            AgentConversationWorkspaceFreshnessResponse::from_target_status(
                workspace.conversation_id.as_str(),
                AgentWorkspaceFreshnessScope::Full,
                target.base_ref,
                target.base_display_name,
                Some(&base_resolution),
                status,
                false,
                Some(0),
                true,
                false,
            ),
        );
    }
    let (worktree_path, base_resolution) = tokio::join!(
        resolve_valid_agent_conversation_workspace_path(&project, &workspace),
        resolve_workspace_base(&project, &workspace),
    );
    let worktree_path = worktree_path.map_err(|e| e.to_string())?;
    let base_resolution = base_resolution.map_err(|e| e.to_string())?;
    if base_resolution.status == BaseStatus::Blocked {
        let (has_uncommitted_changes, unpublished_commit_count) = tokio::join!(
            GitService::has_uncommitted_changes(&worktree_path),
            count_unpublished_publish_commits(&worktree_path, &workspace.branch_name),
        );
        let has_uncommitted_changes = has_uncommitted_changes.unwrap_or(false);
        let unpublished_commit_count = unpublished_commit_count.unwrap_or(None);
        return Ok(AgentConversationWorkspaceFreshnessResponse::blocked(
            workspace.conversation_id.as_str(),
            AgentWorkspaceFreshnessScope::Full,
            &workspace,
            &base_resolution,
            has_uncommitted_changes,
            unpublished_commit_count,
            true,
            true,
        ));
    }
    let effective_base_ref = base_resolution
        .effective_checkout_ref()
        .map_err(|e| e.to_string())?;
    let status = inspect_publish_branch_freshness_for_source_after_fetch(
        &worktree_path,
        effective_base_ref,
        &workspace.branch_name,
        workspace.base_commit.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    let captured_base_is_stale = matches!(
        workspace.base_commit.as_deref(),
        Some(captured_base_commit) if captured_base_commit != status.target_base_commit.as_str()
    );
    if workspace.publication_push_status.as_deref() == Some("needs_agent")
        && !status.is_base_ahead
        && captured_base_is_stale
        && base_resolution.status == BaseStatus::Valid
    {
        workspace.base_commit = Some(status.target_base_commit.clone());
        workspace = state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .map_err(|e| e.to_string())?;
        state
            .agent_conversation_workspace_repo
            .update_publication(
                &workspace.conversation_id,
                workspace.publication_pr_number,
                workspace.publication_pr_url.as_deref(),
                workspace.publication_pr_status.as_deref(),
                Some("refreshed"),
            )
            .await
            .map_err(|e| e.to_string())?;
        append_agent_workspace_publication_event(
            state,
            &workspace.conversation_id,
            "repair_resolved",
            "succeeded",
            "Workspace agent repair resolved the base branch update",
            Some("agent_fixable".to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;
        workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or(workspace);
    }

    let (has_uncommitted_changes, unpublished_commit_count) = tokio::join!(
        GitService::has_uncommitted_changes(&worktree_path),
        count_publishable_commits_with_base_fallback(
            &worktree_path,
            &workspace.branch_name,
            effective_base_ref,
        ),
    );
    let has_uncommitted_changes = has_uncommitted_changes.map_err(|e| e.to_string())?;
    let unpublished_commit_count = Some(unpublished_commit_count.map_err(|e| e.to_string())?);

    Ok(
        AgentConversationWorkspaceFreshnessResponse::from_target_status(
            workspace.conversation_id.as_str(),
            AgentWorkspaceFreshnessScope::Full,
            workspace.base_ref.clone(),
            workspace.base_display_name.clone(),
            Some(&base_resolution),
            status,
            has_uncommitted_changes,
            unpublished_commit_count,
            true,
            true,
        ),
    )
}

/// Update an edit-agent workspace branch from its captured base ref without publishing it.
#[tauri::command]
pub async fn update_agent_conversation_workspace_from_base(
    conversation_id: String,
    base_ref_kind: Option<String>,
    base_ref: Option<String>,
    base_display_name: Option<String>,
    base_source_pull_request: Option<AgentWorkspaceSourcePullRequestInput>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<UpdateAgentConversationWorkspaceFromBaseResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let _workspace_changed_event = emit_workspace_changed_when_done(&app, &conversation_id);
    let kind = parse_agent_workspace_base_kind(base_ref_kind.as_deref())?;
    let source_pull_request = normalize_agent_workspace_source_pull_request(
        base_source_pull_request,
        kind,
        base_ref.as_deref(),
    )?;
    let selection = AgentConversationWorkspaceBaseSelection {
        kind,
        branch_mode: None,
        base_ref,
        display_name: base_display_name,
        source_pull_request,
    };
    update_agent_conversation_workspace_from_base_for_app_state(
        state.inner(),
        execution_state.inner(),
        conversation_id,
        selection,
    )
    .await
}

#[doc(hidden)]
pub async fn update_agent_conversation_workspace_from_base_for_app_state(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    conversation_id: ChatConversationId,
    selection: AgentConversationWorkspaceBaseSelection,
) -> Result<UpdateAgentConversationWorkspaceFromBaseResponse, String> {
    update_agent_conversation_workspace_from_base_for_app_state_with_caller(
        state,
        execution_state,
        conversation_id,
        selection,
        None,
    )
    .await
}

#[doc(hidden)]
pub async fn update_agent_conversation_workspace_from_base_for_app_state_with_caller(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    conversation_id: ChatConversationId,
    selection: AgentConversationWorkspaceBaseSelection,
    created_by_run_id: Option<&str>,
) -> Result<UpdateAgentConversationWorkspaceFromBaseResponse, String> {
    let _freshness_invalidation = AgentWorkspaceFreshnessInvalidationGuard::new(&conversation_id);
    let _pr_description_invalidation =
        AgentWorkspacePrDescriptionInvalidationGuard::new(&conversation_id, true);
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            )
        })?;
    let preserve_pr_autofix_claim = if workspace.mode == AgentConversationWorkspaceMode::Edit
        && workspace.linked_plan_branch_id.is_none()
        && workspace.publication_push_status.as_deref() == Some("needs_agent")
        && workspace.pr_supervision_status.as_deref() == Some("fixing")
    {
        match workspace.publication_pr_number {
            Some(pr_number) if created_by_run_id.is_some() => matches!(
                load_pr_autofix_completion_authority(
                    state.agent_run_repo.as_ref(),
                    &conversation_id,
                    pr_number,
                    created_by_run_id,
                )
                .await
                .map_err(|error| error.to_string())?,
                PrAutofixCompletionAuthority::Current
            ),
            _ => false,
        }
    } else {
        false
    };

    let repair_service = state.build_chat_service_with_execution_state(Arc::clone(execution_state));

    if created_by_run_id.is_none()
        && retry_blocked_agent_workspace_repair_for_explicit_user_action(
            state,
            &workspace,
            &repair_service,
            AgentWorkspacePostRepairAction::UpdateOnly,
        )
        .await
    {
        let refreshed = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .unwrap_or(workspace);
        return Ok(UpdateAgentConversationWorkspaceFromBaseResponse {
            target_ref: refreshed.base_ref.clone(),
            base_commit: refreshed.base_commit.clone().unwrap_or_default(),
            workspace: agent_workspace_response_for_state(state, refreshed).await?,
            updated: false,
            repair_started: true,
            base_status: BaseStatus::Valid.as_str().to_string(),
            effective_base_display_name: None,
        });
    }

    let explicit_base = normalize_explicit_publish_base_selection(selection)?;

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;

    let mut publish_target =
        match resolve_agent_workspace_publish_target(state, &project, &workspace).await {
            Ok(target) => target,
            Err(error) => {
                if error.contains("Agent conversation workspace is missing") {
                    let _ = state
                        .agent_conversation_workspace_repo
                        .update_status(
                            &workspace.conversation_id,
                            crate::domain::entities::AgentConversationWorkspaceStatus::Missing,
                        )
                        .await;
                } else {
                    let repair_target =
                        AgentConversationWorkspaceRepairTarget::from_workspace(&workspace);
                    mark_agent_workspace_update_failure_with_target(
                        state,
                        &workspace,
                        &error,
                        None,
                        &repair_service,
                        &repair_target,
                    )
                    .await;
                }
                return Err(error);
            }
        };

    let base_resolution = if let Some(explicit_base) = explicit_base.as_ref() {
        publish_target.base_ref = explicit_base.base_ref.clone();
        publish_target.base_display_name = Some(explicit_base.display_name.clone());
        if explicit_base.source_pull_request.is_some() {
            if let Err(error) = GitService::fetch_origin(&publish_target.worktree_path).await {
                let message = format!("Failed to refresh selected pull request branch: {error}");
                mark_agent_workspace_update_failure_with_target(
                    state,
                    &workspace,
                    &message,
                    None,
                    &repair_service,
                    &publish_target.repair_target(),
                )
                .await;
                return Err(message);
            }
        }
        if let Err(message) = validate_explicit_publish_base_ref(
            &publish_target.worktree_path,
            &explicit_base.base_ref,
        )
        .await
        {
            mark_agent_workspace_update_failure_with_target(
                state,
                &workspace,
                &message,
                None,
                &repair_service,
                &publish_target.repair_target(),
            )
            .await;
            return Err(message);
        }
        let retargeted_base = BaseResolutionResult {
            status: BaseStatus::Retargeted,
            old_base_ref: workspace.base_ref.clone(),
            effective_base_ref: Some(explicit_base.base_ref.clone()),
            effective_checkout_ref: Some(explicit_base.base_ref.clone()),
            effective_base_commit: None,
            display_name: Some(explicit_base.display_name.clone()),
            block_reason: None,
        };
        if let Err(message) = retarget_existing_workspace_pr_base_if_needed(
            state,
            &publish_target,
            &workspace,
            &retargeted_base,
        )
        .await
        {
            mark_agent_workspace_update_failure_with_target(
                state,
                &workspace,
                &message,
                None,
                &repair_service,
                &publish_target.repair_target(),
            )
            .await;
            return Err(message);
        }
        None
    } else {
        let base_resolution = resolve_workspace_base(&project, &workspace)
            .await
            .map_err(|e| e.to_string())?;
        if base_resolution.status == BaseStatus::Blocked {
            let message = base_resolution
                .block_reason
                .clone()
                .unwrap_or_else(|| "Agent workspace base is blocked".to_string());
            mark_agent_workspace_update_failure_with_target(
                state,
                &workspace,
                &message,
                None,
                &repair_service,
                &publish_target.repair_target(),
            )
            .await;
            return Err(message);
        }
        apply_base_resolution_to_publish_target(&mut publish_target, &base_resolution)?;
        if let Err(message) = retarget_existing_workspace_pr_base_if_needed(
            state,
            &publish_target,
            &workspace,
            &base_resolution,
        )
        .await
        {
            mark_agent_workspace_update_failure_with_target(
                state,
                &workspace,
                &message,
                None,
                &repair_service,
                &publish_target.repair_target(),
            )
            .await;
            return Err(message);
        }
        Some(base_resolution)
    };

    if !preserve_pr_autofix_claim {
        mark_agent_workspace_publish_status(state, &workspace, "refreshing")
            .await
            .map_err(|e| e.to_string())?;
    }

    let freshness_conversation_id = workspace.conversation_id.as_str();
    let outcome = if publish_target.plan_branch.is_some() {
        ensure_plan_publish_branch_fresh(
            &publish_target.worktree_path,
            &project,
            &publish_target.branch_name,
            &publish_target.base_ref,
            &freshness_conversation_id,
            None,
        )
        .await
    } else {
        ensure_publish_branch_fresh(
            &publish_target.worktree_path,
            &project,
            &publish_target.branch_name,
            &publish_target.base_ref,
            &freshness_conversation_id,
            None,
        )
        .await
    };
    let (updated, target_ref, base_commit) = match outcome {
        PublishBranchFreshnessOutcome::AlreadyFresh {
            base_commit,
            target_ref,
        } => (false, target_ref, base_commit),
        PublishBranchFreshnessOutcome::Updated {
            base_commit,
            target_ref,
        } => (true, target_ref, base_commit),
        PublishBranchFreshnessOutcome::NeedsAgent { message, .. }
        | PublishBranchFreshnessOutcome::OperationalError { message } => {
            mark_agent_workspace_update_failure_with_target(
                state,
                &workspace,
                &message,
                None,
                &repair_service,
                &publish_target.repair_target(),
            )
            .await;
            return Err(message);
        }
    };

    let mut push_status = "refreshed";
    if let Some(plan_branch) = publish_target.plan_branch.as_ref() {
        if plan_branch.pr_number.is_some() {
            let Some(github) = state.github_service.as_ref() else {
                let message = "GitHub integration is not available".to_string();
                let _ = state
                    .plan_branch_repo
                    .update_pr_push_status(&plan_branch.id, PrPushStatus::Failed)
                    .await;
                mark_agent_workspace_update_failure_with_target(
                    state,
                    &workspace,
                    &message,
                    None,
                    &repair_service,
                    &publish_target.repair_target(),
                )
                .await;
                return Err(message);
            };
            if let Err(error) = push_publish_branch(
                github,
                &publish_target.worktree_path,
                &publish_target.branch_name,
            )
            .await
            {
                let message = error.to_string();
                let _ = state
                    .plan_branch_repo
                    .update_pr_push_status(&plan_branch.id, PrPushStatus::Failed)
                    .await;
                mark_agent_workspace_update_failure_with_target(
                    state,
                    &workspace,
                    &message,
                    None,
                    &repair_service,
                    &publish_target.repair_target(),
                )
                .await;
                return Err(message);
            }
            state
                .plan_branch_repo
                .update_pr_push_status(&plan_branch.id, PrPushStatus::Pushed)
                .await
                .map_err(|e| e.to_string())?;
            push_status = "pushed";
        }
    }

    if let Some(explicit_base) = explicit_base.as_ref() {
        workspace.base_ref_kind = explicit_base.kind;
        workspace.base_ref = explicit_base.base_ref.clone();
        workspace.base_display_name = Some(explicit_base.display_name.clone());
        workspace.source_pull_request = explicit_base.source_pull_request.clone();
        workspace.updated_at = chrono::Utc::now();
    } else if let Some(base_resolution) = base_resolution.as_ref() {
        persist_workspace_base_resolution_if_retargeted(state, &mut workspace, base_resolution)
            .await?;
    }
    workspace.base_commit = Some(base_commit.clone());
    workspace = state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .map_err(|e| e.to_string())?;
    let final_push_status = if preserve_pr_autofix_claim {
        "needs_agent"
    } else {
        push_status
    };
    state
        .agent_conversation_workspace_repo
        .update_publication(
            &workspace.conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            workspace.publication_pr_status.as_deref(),
            Some(final_push_status),
        )
        .await
        .map_err(|e| e.to_string())?;
    append_agent_workspace_publication_event(
        state,
        &workspace.conversation_id,
        if updated {
            "updated_from_base"
        } else {
            "base_current"
        },
        "succeeded",
        if updated {
            if publish_target.plan_branch.is_some() && push_status == "pushed" {
                "Plan branch updated from base and pushed"
            } else {
                "Workspace branch updated from base"
            }
        } else if publish_target.plan_branch.is_some() && push_status == "pushed" {
            "Plan branch is current with base and pushed"
        } else {
            "Workspace branch is current with base"
        },
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    let refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(workspace);

    let workspace_changed_emitter = state.app_handle.clone().map(|app_handle| {
        Box::new(move |conversation_id: &ChatConversationId| {
            let _ = app_handle.emit(
                "agent:workspace_changed",
                serde_json::json!({ "conversation_id": conversation_id.as_str() }),
            );
        }) as crate::commands::agent_workspace_auto_review::WorkspaceChangedEmitter
    });
    crate::commands::agent_workspace_auto_review::spawn_auto_review_after_workspace_change(
        state.clone(),
        Arc::clone(execution_state),
        refreshed.clone(),
        crate::commands::agent_workspace_auto_review::AutoReviewTrigger::BaseUpdate,
        workspace_changed_emitter,
    );

    Ok(UpdateAgentConversationWorkspaceFromBaseResponse {
        workspace: agent_workspace_response_for_state(state, refreshed).await?,
        updated,
        repair_started: false,
        target_ref,
        base_commit,
        base_status: base_resolution
            .as_ref()
            .map(|resolution| resolution.status)
            .unwrap_or(BaseStatus::Valid)
            .as_str()
            .to_string(),
        effective_base_display_name: explicit_base
            .as_ref()
            .map(|selection| selection.display_name.clone())
            .or_else(|| {
                base_resolution
                    .as_ref()
                    .and_then(|resolution| resolution.display_name.clone())
            }),
    })
}

/// Commit and publish a general edit agent conversation workspace.
#[tauri::command]
pub async fn publish_agent_conversation_workspace(
    conversation_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let _workspace_changed_event = emit_workspace_changed_when_done(&app, &conversation_id);
    publish_agent_conversation_workspace_for_app_state_with_repair_intent(
        state.inner(),
        execution_state.inner(),
        conversation_id,
        true,
        true,
    )
    .await
}

/// Commit an isolated Agent workspace branch without creating or updating a PR.
#[tauri::command]
pub async fn commit_agent_conversation_workspace_locally(
    input: CommitAgentConversationWorkspaceLocallyInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<CommitAgentConversationWorkspaceLocallyResponse, String> {
    let conversation_id = ChatConversationId::from_string(input.conversation_id);
    let result = commit_agent_workspace_locally(
        state.inner(),
        conversation_id.clone(),
        AgentWorkspaceLocalCommitRequest {
            expected_head_sha: input.expected_head_sha,
            review_artifact_id: input.review_artifact_id,
            review_artifact_version: input.review_artifact_version,
            reviewed_head_sha: input.reviewed_head_sha,
            reviewed_diff_fingerprint: input.reviewed_diff_fingerprint,
            attempt_token: input.attempt_token,
            #[cfg(test)]
            before_staging: None,
        },
    )
    .await?;
    let _ = app.emit(
        "agent:workspace_changed",
        serde_json::json!({ "conversation_id": conversation_id.as_str() }),
    );
    Ok(CommitAgentConversationWorkspaceLocallyResponse {
        workspace: agent_workspace_response_for_state(state.inner(), result.workspace).await?,
        outcome: result.outcome.as_str().to_string(),
        branch_name: result.branch_name,
        previous_head_sha: result.previous_head_sha,
        commit_sha: result.commit_sha,
        had_changes: result.had_changes,
        attempt_token: result.attempt_token,
    })
}

/// Precompute the PR description for a stable edit-agent workspace.
#[tauri::command]
pub async fn precompute_agent_conversation_workspace_pr_description(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<PrecomputeAgentConversationWorkspacePrDescriptionResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    precompute_agent_conversation_workspace_pr_description_for_app_state(
        state.inner(),
        conversation_id,
    )
    .await
}

#[doc(hidden)]
pub async fn precompute_agent_conversation_workspace_pr_description_for_app_state(
    state: &AppState,
    conversation_id: ChatConversationId,
) -> Result<PrecomputeAgentConversationWorkspacePrDescriptionResponse, String> {
    git_cmd::with_git_command_lane(GitCommandLane::Background, async move {
        precompute_agent_conversation_workspace_pr_description_inner(state, conversation_id).await
    })
    .await
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentWorkspacePrDescriptionReviewBaseResolution {
    Ready(String),
    Skip(&'static str),
}

async fn resolve_agent_workspace_pr_description_review_base(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    worktree_path: &Path,
) -> Result<AgentWorkspacePrDescriptionReviewBaseResolution, String> {
    let captured_review_base =
        review_base_for_publish(workspace.base_commit.as_deref(), &workspace.base_ref)?.to_string();
    let linked_review_base =
        if workspace.branch_mode == AgentConversationWorkspaceBranchMode::Linked {
            Some(
                resolve_agent_workspace_review_base(
                    worktree_path,
                    workspace,
                    "HEAD",
                    &captured_review_base,
                )
                .await
                .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };

    let base_resolution = match resolve_workspace_base(project, workspace).await {
        Ok(resolution) => Some(resolution),
        Err(fresh_error) => {
            tracing::debug!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                error = %fresh_error,
                "Fresh base resolution failed while preparing PR description; falling back to local snapshot"
            );
            match resolve_workspace_base_from_local_snapshot(project, workspace).await {
                Ok(resolution) => Some(resolution),
                Err(local_error) => {
                    tracing::debug!(
                        target: "ralphx_lib::commands::agent_workspace_publish",
                        conversation_id = %workspace.conversation_id,
                        project_id = %workspace.project_id,
                        branch = %workspace.branch_name,
                        error = %local_error,
                        "Local base resolution failed while preparing PR description; using captured review base"
                    );
                    None
                }
            }
        }
    };

    let Some(base_resolution) = base_resolution else {
        return Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Ready(
            linked_review_base.unwrap_or(captured_review_base),
        ));
    };
    if base_resolution.status == BaseStatus::Blocked {
        return Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Ready(
            linked_review_base.unwrap_or(captured_review_base),
        ));
    }

    let checkout_ref = match base_resolution.effective_checkout_ref() {
        Ok(checkout_ref) => checkout_ref.to_string(),
        Err(_) => {
            return Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Ready(
                linked_review_base.unwrap_or(captured_review_base),
            ));
        }
    };
    let freshness = match inspect_publish_branch_freshness_for_source_after_fetch(
        worktree_path,
        &checkout_ref,
        &workspace.branch_name,
        Some(&captured_review_base),
    )
    .await
    {
        Ok(freshness) => freshness,
        Err(error) => {
            tracing::debug!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                checkout_ref,
                error = %error,
                "Branch freshness check failed while preparing PR description; using captured review base"
            );
            return Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Ready(
                linked_review_base.unwrap_or(captured_review_base),
            ));
        }
    };

    if freshness.is_base_ahead {
        return Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Skip(
            "base_ahead",
        ));
    }

    let review_base = linked_review_base.unwrap_or_else(|| {
        freshness
            .captured_base_commit
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(captured_review_base)
    });
    Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Ready(
        review_base,
    ))
}

async fn resolve_agent_workspace_pr_metadata_target(
    github: Option<&dyn GithubServiceTrait>,
    worktree_path: &Path,
    workspace: &AgentConversationWorkspace,
) -> Result<ResolvedAgentWorkspacePrTarget, String> {
    let github = github.ok_or_else(|| {
        "GitHub integration is required to update metadata for an existing pull request".to_string()
    })?;
    let pr_number = match workspace.publication_pr_number {
        Some(pr_number) => pr_number,
        None => match github
            .find_pr_by_head_branch(worktree_path, &workspace.branch_name)
            .await
            .map_err(|error| error.to_string())?
        {
            Some((pr_number, _)) => pr_number,
            None => return Ok(ResolvedAgentWorkspacePrTarget::NewPr),
        },
    };
    let detail = github
        .fetch_pr_detail(worktree_path, pr_number)
        .await
        .map_err(|error| error.to_string())?;
    if detail.number != pr_number {
        return Err(format!(
            "pull request lookup returned #{}, expected #{pr_number}",
            detail.number
        ));
    }
    if !matches!(detail.state, crate::domain::services::PrStatus::Open) {
        return Err(format!("pull request #{pr_number} is not open"));
    }
    if detail.head_ref_name != workspace.branch_name {
        return Err(format!(
            "pull request #{pr_number} head branch does not match workspace branch"
        ));
    }
    Ok(ResolvedAgentWorkspacePrTarget::Existing(Box::new(
        ExistingPrMetadataSnapshot::from_detail(detail),
    )))
}

async fn normalize_drafted_agent_workspace_pr_metadata_decision(
    state: &AppState,
    conversation: &ChatConversation,
    workspace: &AgentConversationWorkspace,
    target: &ResolvedAgentWorkspacePrTarget,
    mut decision: AgentWorkspacePrMetadataDecision,
) -> AgentWorkspacePrMetadataDecision {
    let Some(token) =
        primary_clickup_token_for_conversation(state, &workspace.conversation_id).await
    else {
        return decision;
    };
    let AgentWorkspacePrMetadataDecision::Patch { title, .. } = &mut decision else {
        return decision;
    };
    if let Some(title) = title {
        *title = normalize_title_with_clickup_token(title, &token);
    } else if matches!(target, ResolvedAgentWorkspacePrTarget::NewPr) {
        let fallback_title = conversation.title.as_deref().unwrap_or("RalphX changes");
        *title = Some(normalize_title_with_clickup_token(fallback_title, &token));
    }
    decision
}

async fn confirm_agent_workspace_existing_pr_metadata_target(
    github: &dyn GithubServiceTrait,
    worktree_path: &Path,
    workspace: &AgentConversationWorkspace,
    expected_fingerprint: &str,
) -> Result<ExistingPrMetadataSnapshot, String> {
    let target =
        resolve_agent_workspace_pr_metadata_target(Some(github), worktree_path, workspace).await?;
    let ResolvedAgentWorkspacePrTarget::Existing(snapshot) = target else {
        return Err("existing pull request disappeared before metadata mutation".to_string());
    };
    if snapshot.authority_fingerprint() != expected_fingerprint {
        return Err("pull request changed again before metadata mutation".to_string());
    }
    Ok(*snapshot)
}

async fn recover_duplicate_agent_workspace_pr_publish(
    state: &AppState,
    github: &dyn GithubServiceTrait,
    publisher: &AgentWorkspacePrPublisher<'_>,
    conversation: &ChatConversation,
    project: &Project,
    workspace: &AgentConversationWorkspace,
    worktree_path: &Path,
    review_base: &str,
    conversation_id: ChatConversationId,
    branch_head_sha: &str,
    reviewable_commit_count: u32,
) -> crate::AppResult<AgentWorkspacePrPublishOutcome> {
    let duplicate_target =
        resolve_agent_workspace_pr_metadata_target(Some(github), worktree_path, workspace)
            .await
            .map_err(AppError::Validation)?;
    let ResolvedAgentWorkspacePrTarget::Existing(snapshot) = &duplicate_target else {
        return Err(AppError::Validation(
            "duplicate PR creation was not recoverable from the remote target".to_string(),
        ));
    };
    let cache_key = AgentWorkspacePrDescriptionCacheKey::for_target(
        conversation_id,
        review_base.to_string(),
        branch_head_sha.to_string(),
        reviewable_commit_count,
        &duplicate_target,
    )
    .ok_or_else(|| AppError::Validation("unable to bind duplicate PR target".to_string()))?;
    let decision = get_or_draft_agent_workspace_pr_metadata_decision(
        state,
        conversation,
        project,
        workspace,
        worktree_path,
        review_base,
        &duplicate_target,
        cache_key,
    )
    .await?
    .decision;
    let decision = normalize_drafted_agent_workspace_pr_metadata_decision(
        state,
        conversation,
        workspace,
        &duplicate_target,
        decision,
    )
    .await;
    if matches!(decision, AgentWorkspacePrMetadataDecision::Preserve) {
        return publisher
            .publish_existing_pr_metadata_decision(
                worktree_path,
                conversation,
                snapshot.number,
                snapshot.url.as_deref(),
                snapshot.body.as_deref(),
                &decision,
            )
            .await;
    }
    let confirmed_snapshot = confirm_agent_workspace_existing_pr_metadata_target(
        github,
        worktree_path,
        workspace,
        snapshot.authority_fingerprint(),
    )
    .await
    .map_err(AppError::Validation)?;
    publisher
        .publish_existing_pr_metadata_decision(
            worktree_path,
            conversation,
            confirmed_snapshot.number,
            confirmed_snapshot.url.as_deref(),
            confirmed_snapshot.body.as_deref(),
            &decision,
        )
        .await
}

async fn precompute_agent_conversation_workspace_pr_description_inner(
    state: &AppState,
    conversation_id: ChatConversationId,
) -> Result<PrecomputeAgentConversationWorkspacePrDescriptionResponse, String> {
    let started = Instant::now();
    let skip = |reason: &str| PrecomputeAgentConversationWorkspacePrDescriptionResponse {
        conversation_id: conversation_id.as_str(),
        status: "skipped".to_string(),
        cache_status: None,
        reason: Some(reason.to_string()),
    };

    let result = async {
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!(
                    "Agent conversation workspace not found for conversation {}",
                    conversation_id
                )
            })?;
        if workspace.mode != AgentConversationWorkspaceMode::Edit {
            return Ok(skip("not_edit_workspace"));
        }
        if workspace.is_execution_owned() {
            return Ok(skip("execution_owned_workspace"));
        }

        let conversation = state
            .chat_conversation_repo
            .get_by_id(&conversation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;
        let project = state
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;
        let worktree_path = resolve_valid_agent_conversation_workspace_path(&project, &workspace)
            .await
            .map_err(|e| e.to_string())?;

        if GitService::has_uncommitted_changes(&worktree_path)
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(skip("uncommitted_changes"));
        }

        let review_base = match resolve_agent_workspace_pr_description_review_base(
            &project,
            &workspace,
            &worktree_path,
        )
        .await
        {
            Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Ready(review_base)) => review_base,
            Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Skip(reason)) => {
                return Ok(skip(reason));
            }
            Err(_) => return Ok(skip("missing_review_base")),
        };

        let reviewable_commit_count =
            count_publish_reviewable_commits(&worktree_path, &workspace.branch_name, &review_base)
                .await
                .map_err(|e| e.to_string())?;
        if reviewable_commit_count == 0 {
            return Ok(skip("no_reviewable_commits"));
        }

        let branch_head_sha = GitService::get_head_sha(&worktree_path)
            .await
            .map_err(|e| e.to_string())?;
        let target = match resolve_agent_workspace_pr_metadata_target(
            state.github_service.as_deref(),
            &worktree_path,
            &workspace,
        )
        .await
        {
            Ok(target) => target,
            Err(_) => return Ok(skip("existing_pr_target_unavailable")),
        };
        let Some(cache_key) = AgentWorkspacePrDescriptionCacheKey::for_target(
            conversation_id.clone(),
            review_base.clone(),
            branch_head_sha,
            reviewable_commit_count,
            &target,
        ) else {
            return Ok(skip("uncacheable_key"));
        };

        let outcome = get_or_draft_agent_workspace_pr_metadata_decision(
            state,
            &conversation,
            &project,
            &workspace,
            &worktree_path,
            &review_base,
            &target,
            cache_key,
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(PrecomputeAgentConversationWorkspacePrDescriptionResponse {
            conversation_id: conversation_id.as_str(),
            status: "ready".to_string(),
            cache_status: Some(outcome.cache_status.as_str().to_string()),
            reason: None,
        })
    }
    .await;

    match &result {
        Ok(response) => tracing::info!(
            target: "ralphx_lib::commands::agent_workspace_publish",
            operation = "precompute_pr_description",
            conversation_id = %conversation_id,
            status = response.status.as_str(),
            cache_status = response.cache_status.as_deref().unwrap_or("none"),
            reason = response.reason.as_deref().unwrap_or("none"),
            elapsed_ms = started.elapsed().as_millis(),
            "Precomputed agent workspace PR description"
        ),
        Err(error) => tracing::warn!(
            target: "ralphx_lib::commands::agent_workspace_publish",
            operation = "precompute_pr_description",
            conversation_id = %conversation_id,
            error = %error,
            elapsed_ms = started.elapsed().as_millis(),
            "Failed to precompute agent workspace PR description"
        ),
    }

    result
}

/// Close the PR associated with an agent conversation workspace.
/// Sets publication_pr_status to "closed" so the existing conversation
/// continuity mechanism will create a fresh branch on the next user message.
#[tauri::command]
pub async fn close_agent_workspace_pr(
    conversation_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<AgentConversationWorkspaceResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let _freshness_invalidation = AgentWorkspaceFreshnessInvalidationGuard::new(&conversation_id);
    let _pr_description_invalidation =
        AgentWorkspacePrDescriptionInvalidationGuard::new(&conversation_id, true);
    let _workspace_changed_event = emit_workspace_changed_when_done(&app, &conversation_id);
    close_agent_workspace_pr_for_state(&conversation_id, &state).await?;

    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Workspace disappeared after update".to_string())?;

    agent_workspace_response_for_state(&state, updated).await
}

async fn linked_plan_branch_has_unfinished_regular_tasks(
    state: &AppState,
    plan_branch: &PlanBranch,
) -> Result<bool, String> {
    let tasks = if let Some(execution_plan_id) = plan_branch.execution_plan_id.as_ref() {
        state
            .task_repo
            .list_paginated(
                &plan_branch.project_id,
                None,
                0,
                10_000,
                false,
                None,
                Some(execution_plan_id.as_str()),
                None,
            )
            .await
            .map_err(|e| e.to_string())?
    } else {
        state
            .task_repo
            .get_by_ideation_session(&plan_branch.session_id)
            .await
            .map_err(|e| e.to_string())?
    };

    Ok(tasks
        .iter()
        .filter(|task| task.archived_at.is_none())
        .filter(|task| task.category == TaskCategory::Regular)
        .any(|task| !task.internal_status.is_terminal()))
}

async fn sync_workspace_publication_from_plan_branch_for_publish(
    state: &AppState,
    project: &Project,
    workspace: &AgentConversationWorkspace,
    publish_target: &AgentConversationWorkspacePublishTarget,
    plan_branch: &PlanBranch,
    push_status: PrPushStatus,
) -> Result<(), String> {
    let pr_number = plan_branch
        .pr_number
        .ok_or_else(|| "No PR associated with this linked plan branch".to_string())?;
    let target = AgentWorkspacePrAutomationTarget {
        project: Some(project.clone()),
        working_dir: publish_target.worktree_path.clone(),
        pr_number,
        pr_url: plan_branch.pr_url.clone(),
        pr_status: plan_branch_publication_status(plan_branch),
        push_status: Some(push_status.to_db_string().to_string()),
    };
    sync_agent_workspace_publication_from_pr_automation_target(
        state,
        &workspace.conversation_id,
        workspace,
        &target,
    )
    .await
}

async fn publish_linked_ideation_plan_branch_workspace_for_app_state(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    mut workspace: AgentConversationWorkspace,
    route_fixable_failures_to_agent: bool,
    repair_handoff: Option<&AgentWorkspaceRepairPrHandoff>,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    let branch_already_pushed = repair_handoff.is_some();
    let publish_started = Instant::now();
    let conversation_id = workspace.conversation_id.clone();

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != workspace.project_id.as_str()
    {
        return Err(format!(
            "Conversation {} does not match agent workspace project {}",
            conversation.id, workspace.project_id
        ));
    }

    let repair_service = state.build_chat_service_with_execution_state(Arc::clone(execution_state));

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;
    let publish_target = resolve_agent_workspace_publish_target(state, &project, &workspace)
        .await
        .map_err(|error| {
            format!("Linked ideation workspace cannot be published from its plan branch: {error}")
        })?;
    let plan_branch = publish_target.plan_branch.as_ref().ok_or_else(|| {
        "Linked ideation publish target did not include a plan branch".to_string()
    })?;
    let pr_number = plan_branch
        .pr_number
        .ok_or_else(|| "No PR associated with this linked plan branch".to_string())?;
    if plan_branch.status != PlanBranchStatus::Active {
        return Err("Cannot publish a plan branch that is no longer active".to_string());
    }
    if is_terminal_agent_conversation_publication_status(
        plan_branch_publication_status(plan_branch).as_deref(),
    ) {
        sync_workspace_publication_from_plan_branch_for_publish(
            state,
            &project,
            &workspace,
            &publish_target,
            plan_branch,
            plan_branch.pr_push_status,
        )
        .await?;
        return Err("Cannot publish a workspace whose PR is already closed or merged".to_string());
    }
    if linked_plan_branch_has_unfinished_regular_tasks(state, plan_branch).await? {
        return Err(
            "This plan branch still has active task work; finish the task pipeline before using Commit & Publish"
                .to_string(),
        );
    }

    sync_workspace_publication_from_plan_branch_for_publish(
        state,
        &project,
        &workspace,
        &publish_target,
        plan_branch,
        plan_branch.pr_push_status,
    )
    .await?;
    workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(workspace);

    let repair_target = publish_target.repair_target();
    let github = match state.github_service.as_ref() {
        Some(github) => github,
        None => {
            let error = "GitHub integration is not available".to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    };

    let current_branch = match GitService::get_current_branch(&publish_target.worktree_path).await {
        Ok(branch) => branch,
        Err(error) => {
            let error = error.to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    };
    if current_branch != publish_target.branch_name {
        let error = format!(
            "Commit & Publish for this task-managed PR must run from the isolated linked plan branch '{}' worktree but that worktree is on '{}'",
            publish_target.branch_name, current_branch
        );
        mark_agent_workspace_publish_failure_with_routing(
            state,
            &workspace,
            &error,
            None,
            &repair_service,
            false,
            &repair_target,
        )
        .await;
        return Err(error);
    }

    if let Some(handoff) = repair_handoff {
        if let Err(error) = verify_agent_workspace_repair_pr_handoff(
            &publish_target.worktree_path,
            &publish_target.branch_name,
            &publish_target.base_ref,
            handoff,
        )
        .await
        {
            let error = error.to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                false,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    }

    mark_agent_workspace_publish_status(state, &workspace, "checking")
        .await
        .map_err(|e| e.to_string())?;

    let has_uncommitted_changes =
        match GitService::has_uncommitted_changes(&publish_target.worktree_path).await {
            Ok(has_changes) => has_changes,
            Err(error) => {
                let error = error.to_string();
                mark_agent_workspace_publish_failure_with_routing(
                    state,
                    &workspace,
                    &error,
                    None,
                    &repair_service,
                    route_fixable_failures_to_agent,
                    &repair_target,
                )
                .await;
                return Err(error);
            }
        };

    let commit_sha = if has_uncommitted_changes {
        mark_agent_workspace_publish_status(state, &workspace, "committing")
            .await
            .map_err(|e| e.to_string())?;
        let message = build_agent_workspace_commit_message(&conversation);
        match GitService::commit_all_including_deletions(&publish_target.worktree_path, &message)
            .await
        {
            Ok(commit_sha) => commit_sha,
            Err(error) => {
                let error = error.to_string();
                mark_agent_workspace_publish_failure_with_routing(
                    state,
                    &workspace,
                    &error,
                    None,
                    &repair_service,
                    route_fixable_failures_to_agent,
                    &repair_target,
                )
                .await;
                return Err(error);
            }
        }
    } else {
        None
    };

    mark_agent_workspace_publish_status(state, &workspace, "refreshing")
        .await
        .map_err(|e| e.to_string())?;
    let freshness = match inspect_publish_branch_freshness_for_source(
        &publish_target.worktree_path,
        &publish_target.base_ref,
        &publish_target.branch_name,
        workspace.base_commit.as_deref(),
    )
    .await
    {
        Ok(freshness) => freshness,
        Err(error) => {
            let error = error.to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    };
    if freshness.is_base_ahead {
        let error = format!(
            "Plan branch '{}' is behind '{}'. Update from base before publishing this PR.",
            publish_target.branch_name, freshness.target_ref
        );
        mark_agent_workspace_publish_failure_with_routing(
            state,
            &workspace,
            &error,
            None,
            &repair_service,
            false,
            &repair_target,
        )
        .await;
        return Err(error);
    }
    if workspace.base_commit.as_deref() != Some(freshness.target_base_commit.as_str()) {
        workspace.base_commit = Some(freshness.target_base_commit.clone());
        workspace = state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .map_err(|e| e.to_string())?;
    }

    mark_agent_workspace_publish_status(state, &workspace, "checking")
        .await
        .map_err(|e| e.to_string())?;
    let reviewable_commit_count = match count_publish_reviewable_commits(
        &publish_target.worktree_path,
        &publish_target.branch_name,
        &freshness.target_base_commit,
    )
    .await
    {
        Ok(count) => count,
        Err(error) => {
            let error = error.to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    };
    if reviewable_commit_count == 0 {
        let _ = mark_agent_workspace_publish_status(state, &workspace, "no_changes").await;
        return Err("No committed changes to publish on this plan branch".to_string());
    }

    mark_agent_workspace_publish_status(state, &workspace, "pushing")
        .await
        .map_err(|e| e.to_string())?;
    if let Some(handoff) = repair_handoff {
        if let Err(error) = verify_agent_workspace_repair_pr_handoff(
            &publish_target.worktree_path,
            &publish_target.branch_name,
            &publish_target.base_ref,
            handoff,
        )
        .await
        {
            let error = error.to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                false,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    }
    let push_started = Instant::now();
    if !branch_already_pushed {
        if let Err(error) = push_publish_branch(
            github,
            &publish_target.worktree_path,
            &publish_target.branch_name,
        )
        .await
        {
            let error = error.to_string();
            tracing::warn!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %publish_target.branch_name,
                elapsed_ms = push_started.elapsed().as_millis(),
                error = %error,
                "Failed to push linked ideation plan publish branch"
            );
            let _ = state
                .plan_branch_repo
                .update_pr_push_status(&plan_branch.id, PrPushStatus::Failed)
                .await;
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    }
    tracing::info!(
        target: "ralphx_lib::commands::agent_workspace_publish",
        conversation_id = %workspace.conversation_id,
        project_id = %workspace.project_id,
        branch = %publish_target.branch_name,
        elapsed_ms = push_started.elapsed().as_millis(),
        "Pushed linked ideation plan publish branch"
    );

    state
        .plan_branch_repo
        .update_pr_push_status(&plan_branch.id, PrPushStatus::Pushed)
        .await
        .map_err(|e| e.to_string())?;
    state
        .agent_conversation_workspace_repo
        .update_publication(
            &workspace.conversation_id,
            Some(pr_number),
            plan_branch.pr_url.as_deref(),
            plan_branch_publication_status(plan_branch).as_deref(),
            Some("pushed"),
        )
        .await
        .map_err(|e| e.to_string())?;
    append_agent_workspace_publication_event(
        state,
        &workspace.conversation_id,
        "published",
        "succeeded",
        "Plan branch pull request is up to date",
        Some(format!("published:{pr_number}")),
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(workspace);

    if let Err(error) = crate::application::agent_workspace_review_auto_merge::
        restore_guarded_auto_merge_after_publish(state, &refreshed)
        .await
    {
        tracing::warn!(
            target: "ralphx_lib::commands::agent_workspace_publish",
            conversation_id = %refreshed.conversation_id,
            pr_number,
            error = %error,
            "Deferred workspace Review auto-merge restoration after publish"
        );
    }
    refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&refreshed.conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(refreshed);

    if refreshed.auto_publish_enabled && refreshed.pr_auto_merge_desired {
        match sync_agent_workspace_auto_merge_preference_for_workspace(
            Arc::clone(github),
            &publish_target.worktree_path,
            pr_number,
            &refreshed,
            Arc::clone(&state.agent_conversation_workspace_repo),
        )
        .await
        {
            Ok(_) => {
                refreshed = state
                    .agent_conversation_workspace_repo
                    .get_by_conversation_id(&refreshed.conversation_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .unwrap_or(refreshed);
            }
            Err(error) => {
                tracing::warn!(
                    target: "ralphx_lib::commands::agent_workspace_publish",
                    conversation_id = %refreshed.conversation_id,
                    project_id = %refreshed.project_id,
                    pr_number,
                    error = %error,
                    "Deferred linked ideation plan PR auto-merge synchronization after publish"
                );
                state
                    .agent_conversation_workspace_repo
                    .update_pr_auto_merge_state(
                        &refreshed.conversation_id,
                        Some(false),
                        Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING),
                        Some(&format!(
                            "GitHub auto-merge state could not be refreshed yet: {error}"
                        )),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                refreshed = state
                    .agent_conversation_workspace_repo
                    .get_by_conversation_id(&refreshed.conversation_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .unwrap_or(refreshed);
            }
        }
    }

    let review_chat_service: Arc<dyn ChatService> = Arc::new(repair_service);
    state
        .pr_poller_registry
        .start_agent_workspace_polling_with_repair_repo(
            refreshed.conversation_id.clone(),
            pr_number,
            project.clone(),
            publish_target.worktree_path.clone(),
            Arc::clone(&state.agent_conversation_workspace_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.agent_workspace_repair_repo),
            review_chat_service,
        );

    tracing::info!(
        target: "ralphx_lib::commands::agent_workspace_publish",
        conversation_id = %conversation_id,
        project_id = %project.id,
        branch = %publish_target.branch_name,
        reviewable_commit_count,
        pr_number,
        elapsed_ms = publish_started.elapsed().as_millis(),
        "Completed linked ideation plan branch publish"
    );

    Ok(PublishAgentConversationWorkspaceResponse {
        workspace: agent_workspace_response_for_state(state, refreshed).await?,
        commit_sha,
        pushed: true,
        created_pr: false,
        pr_number: Some(pr_number),
        pr_url: plan_branch.pr_url.clone(),
    })
}

#[doc(hidden)]
pub async fn publish_agent_conversation_workspace_for_app_state(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    conversation_id: ChatConversationId,
    route_fixable_failures_to_agent: bool,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    publish_agent_conversation_workspace_for_app_state_with_repair_intent(
        state,
        execution_state,
        conversation_id,
        route_fixable_failures_to_agent,
        false,
    )
    .await
}

/// The direct Commit & Publish entry must resume an active durable repair generation instead of
/// bypassing its exact generation/base/lease authority through the normal publisher.
#[doc(hidden)]
pub async fn publish_agent_conversation_workspace_for_app_state_with_repair_intent(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    conversation_id: ChatConversationId,
    route_fixable_failures_to_agent: bool,
    explicit_repair_publish: bool,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    if explicit_repair_publish {
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                format!("Agent conversation workspace not found for {conversation_id}")
            })?;
        let repair_service =
            state.build_chat_service_with_execution_state(Arc::clone(execution_state));
        if retry_blocked_agent_workspace_repair_for_explicit_user_action(
            state,
            &workspace,
            &repair_service,
            AgentWorkspacePostRepairAction::Publish,
        )
        .await
        {
            return durable_repair_publish_response(state, &conversation_id, false).await;
        }
    }
    if let Some(response) = resume_durable_agent_workspace_repair_publish(
        state,
        &conversation_id,
        explicit_repair_publish,
    )
    .await?
    {
        return Ok(response);
    }
    publish_agent_conversation_workspace_for_app_state_inner(
        state,
        execution_state,
        conversation_id,
        route_fixable_failures_to_agent,
        None,
    )
    .await
}

pub(crate) async fn resume_durable_agent_workspace_repair_publish(
    state: &AppState,
    conversation_id: &ChatConversationId,
    explicit_publish: bool,
) -> Result<Option<PublishAgentConversationWorkspaceResponse>, String> {
    let resume = resume_current_agent_workspace_repair_publish(
        state,
        conversation_id,
        "Resuming the durable workspace repair continuation for publish.",
        explicit_publish,
    )
    .await
    .map_err(|error| error.to_string())?;
    let attempt = match resume {
        AgentWorkspaceRepairPublishResumeOutcome::NoAttempt => return Ok(None),
        AgentWorkspaceRepairPublishResumeOutcome::Continue(attempt) => *attempt,
        AgentWorkspaceRepairPublishResumeOutcome::AwaitingReview => {
            return durable_repair_publish_response(state, conversation_id, false).await.map(Some)
        }
        AgentWorkspaceRepairPublishResumeOutcome::Ready => {
            return durable_repair_publish_response(state, conversation_id, false).await.map(Some)
        }
        AgentWorkspaceRepairPublishResumeOutcome::Blocked => {
            return Err("The durable workspace repair is blocked; retry that repair before publishing."
                .to_string())
        }
        AgentWorkspaceRepairPublishResumeOutcome::Busy => {
            return Err("The durable workspace repair is still active; wait for it to reach a publish boundary."
                .to_string())
        }
        AgentWorkspaceRepairPublishResumeOutcome::Stale => {
            return Err("The durable workspace repair changed before publish continuation could be resumed. Refresh and retry."
                .to_string())
        }
    };
    let before_pr_number = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Agent conversation workspace not found for {conversation_id}"))?
        .publication_pr_number;
    // Coverage builds need this large durable continuation future heap-allocated.
    match Box::pin(continue_agent_workspace_repair_publish(state, attempt))
        .await
        .map_err(|error| error.to_string())?
    {
        Some(AgentWorkspaceRepairPushOutcome::Busy) => Err(
            "The durable workspace repair is still publishing under its current owner. Refresh and retry after it settles."
                .to_string(),
        ),
        Some(AgentWorkspaceRepairPushOutcome::Stale) => Err(
            "The durable workspace repair became stale before publish continuation could be completed. Refresh and retry."
                .to_string(),
        ),
        Some(_) => durable_repair_publish_response_with_prior_pr(
            state,
            conversation_id,
            before_pr_number,
            true,
        )
        .await
        .map(Some),
        None => Err(
            "The durable workspace repair could not prove a publish continuation runtime. Refresh and retry the repair."
                .to_string(),
        ),
    }
}

async fn durable_repair_publish_response(
    state: &AppState,
    conversation_id: &ChatConversationId,
    pushed: bool,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    durable_repair_publish_response_with_prior_pr(state, conversation_id, None, pushed).await
}

async fn durable_repair_publish_response_with_prior_pr(
    state: &AppState,
    conversation_id: &ChatConversationId,
    prior_pr_number: Option<i64>,
    pushed: bool,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Agent conversation workspace not found for {conversation_id}"))?;
    let workspace = agent_workspace_response_for_state(state, workspace).await?;
    Ok(PublishAgentConversationWorkspaceResponse {
        created_pr: prior_pr_number.is_none() && workspace.publication_pr_number.is_some(),
        commit_sha: None,
        pushed,
        pr_number: workspace.publication_pr_number,
        pr_url: workspace.publication_pr_url.clone(),
        workspace,
    })
}

/// Reuses the normal workspace PR creation/update and monitoring pipeline after a durable
/// repair-owned branch push has already been observed. The normal publisher remains responsible
/// for PR target reconciliation, duplicate recovery, publication projection, auto-merge refresh,
/// and poller startup; this entry only suppresses its otherwise unconditional second push.
pub(crate) async fn publish_agent_conversation_workspace_after_repair_push(
    state: &AppState,
    conversation_id: ChatConversationId,
    repair_handoff: AgentWorkspaceRepairPrHandoff,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    let execution_state = Arc::new(ExecutionState::new());
    // The normal publisher is large enough to overflow Linux debug-test stacks when inlined here.
    Box::pin(publish_agent_conversation_workspace_for_app_state_inner(
        state,
        &execution_state,
        conversation_id,
        false,
        Some(repair_handoff),
    ))
    .await
}

async fn publish_agent_conversation_workspace_for_app_state_inner(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    conversation_id: ChatConversationId,
    route_fixable_failures_to_agent: bool,
    repair_handoff: Option<AgentWorkspaceRepairPrHandoff>,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    let _publish_guard = try_acquire_agent_workspace_publish_guard(&conversation_id)?;
    let _workspace_review_lifecycle_guard = lock_workspace_review_lifecycle(&conversation_id).await;
    publish_agent_conversation_workspace_for_app_state_unlocked(
        state,
        execution_state,
        conversation_id,
        route_fixable_failures_to_agent,
        repair_handoff,
    )
    .await
}

async fn publish_agent_conversation_workspace_for_app_state_unlocked(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    conversation_id: ChatConversationId,
    route_fixable_failures_to_agent: bool,
    repair_handoff: Option<AgentWorkspaceRepairPrHandoff>,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    let branch_already_pushed = repair_handoff.is_some();
    let _freshness_invalidation = AgentWorkspaceFreshnessInvalidationGuard::new(&conversation_id);
    let _pr_description_invalidation =
        AgentWorkspacePrDescriptionInvalidationGuard::new(&conversation_id, false);
    let publish_started = Instant::now();
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            )
        })?;

    if workspace.mode == AgentConversationWorkspaceMode::Ideation {
        if let Some(blocker) = load_workspace_review_publish_blocker(state, &workspace)
            .await
            .map_err(|e| e.to_string())?
        {
            return Err(blocker);
        }
        return publish_linked_ideation_plan_branch_workspace_for_app_state(
            state,
            execution_state,
            workspace,
            route_fixable_failures_to_agent,
            repair_handoff.as_ref(),
        )
        .await;
    }

    if workspace.mode != AgentConversationWorkspaceMode::Edit {
        return Err("Only Edit-mode agent conversations can be directly published".to_string());
    }
    if workspace.is_execution_owned() {
        return Err(
            "This agent conversation workspace is owned by an execution plan and cannot be directly published"
                .to_string(),
        );
    }
    if workspace.has_terminal_publication_pr_status() {
        return Err("Cannot publish a workspace whose PR is already closed or merged".to_string());
    }
    review_base_for_publish(workspace.base_commit.as_deref(), &workspace.base_ref)?;
    if let Some(blocker) = load_workspace_review_publish_blocker(state, &workspace)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err(blocker);
    }

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != workspace.project_id.as_str()
    {
        return Err(format!(
            "Conversation {} does not match agent workspace project {}",
            conversation.id, workspace.project_id
        ));
    }

    let repair_service = state.build_chat_service_with_execution_state(Arc::clone(execution_state));

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;
    let worktree_path =
        match resolve_valid_agent_conversation_workspace_path(&project, &workspace).await {
            Ok(path) => path,
            Err(error) => {
                if error
                    .to_string()
                    .contains("Agent conversation workspace is missing")
                {
                    let _ = state
                        .agent_conversation_workspace_repo
                        .update_status(
                            &workspace.conversation_id,
                            crate::domain::entities::AgentConversationWorkspaceStatus::Missing,
                        )
                        .await;
                }
                return Err(error.to_string());
            }
        };
    if workspace.publication_pr_number.is_none() {
        if !project.github_pr_enabled {
            return Err(
                "GitHub PR publishing is disabled for this project. Enable it before publishing a new pull request."
                    .to_string(),
            );
        }
        match inspect_repository_capability(&worktree_path).await {
            RepositoryCapability::Github { .. } => {}
            RepositoryCapability::LocalOnly => return Err(
                "This project has no GitHub origin, so RalphX cannot publish a new pull request."
                    .to_string(),
            ),
            RepositoryCapability::OtherRemote { .. } => return Err(
                "This project origin is not GitHub, so RalphX cannot publish a new pull request."
                    .to_string(),
            ),
            RepositoryCapability::InspectionFailed { message } => {
                return Err(format!(
                    "Could not inspect this project's Git origin before publishing: {message}"
                ))
            }
        }
    }
    let mut repair_target = AgentConversationWorkspaceRepairTarget::from_workspace(&workspace);

    if let Some(handoff) = repair_handoff.as_ref() {
        if let Err(error) = verify_agent_workspace_repair_pr_handoff(
            &worktree_path,
            &workspace.branch_name,
            &workspace.base_ref,
            handoff,
        )
        .await
        {
            let error = error.to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                false,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    }

    let github = match state.github_service.as_ref() {
        Some(github) => github,
        None => {
            let error = "GitHub integration is not available".to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    };

    let base_resolution = resolve_workspace_base(&project, &workspace)
        .await
        .map_err(|e| e.to_string())?;
    if base_resolution.status == BaseStatus::Blocked {
        let error = base_resolution
            .block_reason
            .clone()
            .unwrap_or_else(|| "Agent workspace base is blocked".to_string());
        mark_agent_workspace_publish_failure_with_routing(
            state,
            &workspace,
            &error,
            None,
            &repair_service,
            route_fixable_failures_to_agent,
            &repair_target,
        )
        .await;
        return Err(error);
    }
    let mut publish_target = AgentConversationWorkspacePublishTarget {
        worktree_path: worktree_path.clone(),
        branch_name: workspace.branch_name.clone(),
        base_ref: workspace.base_ref.clone(),
        base_display_name: workspace.base_display_name.clone(),
        plan_branch: None,
    };
    apply_base_resolution_to_publish_target(&mut publish_target, &base_resolution)?;
    if let Err(error) = retarget_existing_workspace_pr_base_if_needed(
        state,
        &publish_target,
        &workspace,
        &base_resolution,
    )
    .await
    {
        mark_agent_workspace_publish_failure_with_routing(
            state,
            &workspace,
            &error,
            None,
            &repair_service,
            route_fixable_failures_to_agent,
            &repair_target,
        )
        .await;
        return Err(error);
    }
    persist_workspace_base_resolution_if_retargeted(state, &mut workspace, &base_resolution)
        .await?;
    repair_target = AgentConversationWorkspaceRepairTarget::from_workspace(&workspace);

    mark_agent_workspace_publish_status(state, &workspace, "checking")
        .await
        .map_err(|e| e.to_string())?;

    let has_uncommitted_changes = match GitService::has_uncommitted_changes(&worktree_path).await {
        Ok(has_changes) => has_changes,
        Err(error) => {
            let error = error.to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    };

    let commit_sha = if has_uncommitted_changes {
        mark_agent_workspace_publish_status(state, &workspace, "committing")
            .await
            .map_err(|e| e.to_string())?;
        let message = build_agent_workspace_commit_message(&conversation);
        match GitService::commit_all_including_deletions(&worktree_path, &message).await {
            Ok(commit_sha) => commit_sha,
            Err(error) => {
                let error = error.to_string();
                mark_agent_workspace_publish_failure_with_routing(
                    state,
                    &workspace,
                    &error,
                    None,
                    &repair_service,
                    route_fixable_failures_to_agent,
                    &repair_target,
                )
                .await;
                return Err(error);
            }
        }
    } else {
        None
    };

    if let Err(error) =
        review_base_for_publish(workspace.base_commit.as_deref(), &workspace.base_ref)
    {
        mark_agent_workspace_publish_failure_with_routing(
            state,
            &workspace,
            &error,
            None,
            &repair_service,
            route_fixable_failures_to_agent,
            &repair_target,
        )
        .await;
        return Err(error);
    }

    mark_agent_workspace_publish_status(state, &workspace, "refreshing")
        .await
        .map_err(|e| e.to_string())?;

    let repo_path = std::path::Path::new(&project.working_directory);
    let freshness_conversation_id = workspace.conversation_id.as_str();
    let freshness_outcome = if let Some(handoff) = repair_handoff.as_ref() {
        match verify_agent_workspace_repair_pr_handoff(
            &worktree_path,
            &workspace.branch_name,
            &workspace.base_ref,
            handoff,
        )
        .await
        {
            Ok(freshness) => PublishBranchFreshnessOutcome::AlreadyFresh {
                base_commit: freshness.target_base_commit,
                target_ref: freshness.target_ref,
            },
            Err(error) => PublishBranchFreshnessOutcome::OperationalError {
                message: error.to_string(),
            },
        }
    } else {
        ensure_publish_branch_fresh(
            repo_path,
            &project,
            &workspace.branch_name,
            &workspace.base_ref,
            &freshness_conversation_id,
            None,
        )
        .await
    };
    let refreshed_base_commit = match freshness_outcome {
        PublishBranchFreshnessOutcome::AlreadyFresh { base_commit, .. }
        | PublishBranchFreshnessOutcome::Updated { base_commit, .. } => base_commit,
        PublishBranchFreshnessOutcome::NeedsAgent { message, .. } => {
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &message,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(message);
        }
        PublishBranchFreshnessOutcome::OperationalError { message } => {
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &message,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(message);
        }
    };

    if workspace.base_commit.as_deref() != Some(refreshed_base_commit.as_str()) {
        workspace.base_commit = Some(refreshed_base_commit);
        workspace = state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .map_err(|e| e.to_string())?;
    }

    let review_base =
        match review_base_for_publish(workspace.base_commit.as_deref(), &workspace.base_ref) {
            Ok(review_base) => review_base,
            Err(error) => {
                mark_agent_workspace_publish_failure_with_routing(
                    state,
                    &workspace,
                    &error,
                    None,
                    &repair_service,
                    route_fixable_failures_to_agent,
                    &repair_target,
                )
                .await;
                return Err(error);
            }
        };

    mark_agent_workspace_publish_status(state, &workspace, "checking")
        .await
        .map_err(|e| e.to_string())?;

    let reviewable_commit_count =
        match count_publish_reviewable_commits(&worktree_path, &workspace.branch_name, review_base)
            .await
        {
            Ok(count) => count,
            Err(error) => {
                let error = error.to_string();
                mark_agent_workspace_publish_failure_with_routing(
                    state,
                    &workspace,
                    &error,
                    None,
                    &repair_service,
                    route_fixable_failures_to_agent,
                    &repair_target,
                )
                .await;
                return Err(error);
            }
        };
    if reviewable_commit_count == 0 {
        let _ = mark_agent_workspace_publish_status(state, &workspace, "no_changes").await;
        return Err("No committed changes to publish on this agent branch".to_string());
    }

    let branch_head_sha = match commit_sha.as_deref() {
        Some(commit_sha) if !commit_sha.trim().is_empty() => commit_sha.to_string(),
        _ => match GitService::get_head_sha(&worktree_path).await {
            Ok(head_sha) => head_sha,
            Err(error) => {
                let error = error.to_string();
                mark_agent_workspace_publish_failure_with_routing(
                    state,
                    &workspace,
                    &error,
                    None,
                    &repair_service,
                    route_fixable_failures_to_agent,
                    &repair_target,
                )
                .await;
                return Err(error);
            }
        },
    };
    let mut pr_target = match resolve_agent_workspace_pr_metadata_target(
        Some(github.as_ref()),
        &worktree_path,
        &workspace,
    )
    .await
    {
        Ok(target) => target,
        Err(error) => {
            mark_agent_workspace_publish_description_failure(state, &workspace, &error).await;
            return Err(error);
        }
    };
    let pr_description_cache_key = AgentWorkspacePrDescriptionCacheKey::for_target(
        conversation_id.clone(),
        review_base.to_string(),
        branch_head_sha.clone(),
        reviewable_commit_count,
        &pr_target,
    );

    mark_agent_workspace_publish_status(state, &workspace, "describing")
        .await
        .map_err(|e| e.to_string())?;
    let describe_started = Instant::now();
    let mut pr_metadata_decision = match if let Some(cache_key) = pr_description_cache_key {
        get_or_draft_agent_workspace_pr_metadata_decision(
            state,
            &conversation,
            &project,
            &workspace,
            &worktree_path,
            review_base,
            &pr_target,
            cache_key,
        )
        .await
        .map(|outcome| {
            tracing::info!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                operation = "draft_pr_metadata_decision",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                cache_status = outcome.cache_status.as_str(),
                cache_age_ms = ?outcome.cache_age_ms,
                cache_wait_ms = outcome.cache_wait_ms,
                elapsed_ms = describe_started.elapsed().as_millis(),
                "Resolved agent workspace PR metadata decision"
            );
            outcome.decision
        })
    } else {
        draft_agent_workspace_pr_metadata_decision(
            state,
            &conversation,
            &project,
            &workspace,
            &worktree_path,
            review_base,
            &pr_target,
        )
        .await
        .inspect(|_| {
            tracing::info!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                operation = "draft_pr_metadata_decision",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                cache_status = "uncacheable",
                cache_age_ms = ?Option::<u128>::None,
                cache_wait_ms = 0_u128,
                elapsed_ms = describe_started.elapsed().as_millis(),
                "Resolved agent workspace PR metadata decision"
            );
        })
    } {
        Ok(decision) => decision,
        Err(error) => {
            let error = error.to_string();
            mark_agent_workspace_publish_description_failure(state, &workspace, &error).await;
            return Err(error);
        }
    };
    pr_metadata_decision = normalize_drafted_agent_workspace_pr_metadata_decision(
        state,
        &conversation,
        &workspace,
        &pr_target,
        pr_metadata_decision,
    )
    .await;

    // B1/B2/B5: for automation runs whose base is a local-only automation branch,
    // publish that base to origin BEFORE the PR references it as `--base`. Both
    // belts (automation scope + origin-absent safety) live in the helper. A push
    // failure fails the publish closed — never retarget to main, never proceed to
    // PR create.
    if let Err(error) =
        ensure_publish_base_pushed(github, &worktree_path, &conversation, &workspace).await
    {
        let error = error.to_string();
        tracing::warn!(
            target: "ralphx_lib::commands::agent_workspace_publish",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            base_ref = %workspace.base_ref,
            error = %error,
            "Failed to push automation base branch before publishing"
        );
        mark_agent_workspace_publish_failure_with_routing(
            state,
            &workspace,
            &error,
            None,
            &repair_service,
            route_fixable_failures_to_agent,
            &repair_target,
        )
        .await;
        return Err(error);
    }

    mark_agent_workspace_publish_status(state, &workspace, "pushing")
        .await
        .map_err(|e| e.to_string())?;

    if !branch_already_pushed {
        let push_started = Instant::now();
        if let Err(error) =
            push_publish_branch(github, &worktree_path, &workspace.branch_name).await
        {
            let error = error.to_string();
            tracing::warn!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                elapsed_ms = push_started.elapsed().as_millis(),
                error = %error,
                "Failed to push agent workspace publish branch"
            );
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(error);
        }
        tracing::info!(
            target: "ralphx_lib::commands::agent_workspace_publish",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            branch = %workspace.branch_name,
            elapsed_ms = push_started.elapsed().as_millis(),
            "Pushed agent workspace publish branch"
        );
    }

    mark_agent_workspace_publish_status(state, &workspace, "pushed")
        .await
        .map_err(|e| e.to_string())?;

    // The draft is bound to the fetched remote target. Re-read it after the
    // branch push, immediately before mutation, so a concurrent PR edit cannot
    // receive a decision drafted from stale authority.
    if let ResolvedAgentWorkspacePrTarget::Existing(snapshot) = &pr_target {
        let refreshed_target = match resolve_agent_workspace_pr_metadata_target(
            Some(github.as_ref()),
            &worktree_path,
            &workspace,
        )
        .await
        {
            Ok(target @ ResolvedAgentWorkspacePrTarget::Existing(_)) => target,
            Ok(ResolvedAgentWorkspacePrTarget::NewPr) => {
                let error =
                    "existing pull request disappeared before metadata mutation".to_string();
                mark_agent_workspace_publish_description_failure(state, &workspace, &error).await;
                return Err(error);
            }
            Err(error) => {
                mark_agent_workspace_publish_description_failure(state, &workspace, &error).await;
                return Err(error);
            }
        };
        let ResolvedAgentWorkspacePrTarget::Existing(refreshed_snapshot) = &refreshed_target else {
            unreachable!("existing target branch handled above");
        };
        if matches!(
            pr_metadata_decision,
            AgentWorkspacePrMetadataDecision::Patch { .. }
        ) && refreshed_snapshot.authority_fingerprint() != snapshot.authority_fingerprint()
        {
            let cache_key = AgentWorkspacePrDescriptionCacheKey::for_target(
                conversation_id.clone(),
                review_base.to_string(),
                branch_head_sha.clone(),
                reviewable_commit_count,
                &refreshed_target,
            )
            .ok_or_else(|| "unable to bind refreshed existing PR target".to_string())?;
            pr_metadata_decision = match get_or_draft_agent_workspace_pr_metadata_decision(
                state,
                &conversation,
                &project,
                &workspace,
                &worktree_path,
                review_base,
                &refreshed_target,
                cache_key,
            )
            .await
            {
                Ok(outcome) => {
                    normalize_drafted_agent_workspace_pr_metadata_decision(
                        state,
                        &conversation,
                        &workspace,
                        &refreshed_target,
                        outcome.decision,
                    )
                    .await
                }
                Err(error) => {
                    let error = error.to_string();
                    mark_agent_workspace_publish_description_failure(state, &workspace, &error)
                        .await;
                    return Err(error);
                }
            };
            if matches!(
                pr_metadata_decision,
                AgentWorkspacePrMetadataDecision::Patch { .. }
            ) {
                match confirm_agent_workspace_existing_pr_metadata_target(
                    github.as_ref(),
                    &worktree_path,
                    &workspace,
                    refreshed_snapshot.authority_fingerprint(),
                )
                .await
                {
                    Ok(confirmed_snapshot) => {
                        pr_target =
                            ResolvedAgentWorkspacePrTarget::Existing(Box::new(confirmed_snapshot));
                    }
                    Err(error) => {
                        mark_agent_workspace_publish_description_failure(state, &workspace, &error)
                            .await;
                        return Err(error);
                    }
                }
            } else {
                pr_target = refreshed_target;
            }
        } else {
            pr_target = refreshed_target;
        }
    }

    if let Some(handoff) = repair_handoff.as_ref() {
        if let Err(error) = verify_agent_workspace_repair_pr_handoff(
            &worktree_path,
            &workspace.branch_name,
            &workspace.base_ref,
            handoff,
        )
        .await
        {
            let error = error.to_string();
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                None,
                &repair_service,
                false,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    }

    let plan_markdown = resolve_linked_plan_markdown(state, &workspace).await;
    let mut publisher = AgentWorkspacePrPublisher::new(github);
    if let Some(markdown) = plan_markdown {
        publisher = publisher.with_plan_markdown(markdown);
    }
    let publish_pr_started = Instant::now();
    let pr_result = match (&pr_target, &pr_metadata_decision) {
        (
            ResolvedAgentWorkspacePrTarget::NewPr,
            AgentWorkspacePrMetadataDecision::Patch {
                title,
                body_markdown: Some(body_markdown),
            },
        ) => {
            let description =
                AgentWorkspacePrDescription::new(title.clone(), body_markdown.clone());
            match publisher
                .publish_draft_pr_without_duplicate_recovery(
                    &worktree_path,
                    &conversation,
                    &workspace,
                    &description,
                )
                .await
            {
                Err(AppError::DuplicatePr) => {
                    recover_duplicate_agent_workspace_pr_publish(
                        state,
                        github.as_ref(),
                        &publisher,
                        &conversation,
                        &project,
                        &workspace,
                        &worktree_path,
                        review_base,
                        conversation_id.clone(),
                        &branch_head_sha,
                        reviewable_commit_count,
                    )
                    .await
                }
                result => result,
            }
        }
        (ResolvedAgentWorkspacePrTarget::NewPr, _) => Err(AppError::Validation(
            "new pull requests require a complete metadata body patch".to_string(),
        )),
        (ResolvedAgentWorkspacePrTarget::Existing(snapshot), decision) => {
            publisher
                .publish_existing_pr_metadata_decision(
                    &worktree_path,
                    &conversation,
                    snapshot.number,
                    snapshot.url.as_deref(),
                    snapshot.body.as_deref(),
                    decision,
                )
                .await
        }
    };
    let outcome = match pr_result {
        Ok(result) => {
            tracing::info!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                pr_number = result.pr_number,
                created_pr = result.created_pr,
                elapsed_ms = publish_pr_started.elapsed().as_millis(),
                "Published agent workspace draft pull request"
            );
            result
        }
        Err(error) => {
            let error = error.to_string();
            tracing::warn!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                elapsed_ms = publish_pr_started.elapsed().as_millis(),
                error = %error,
                "Failed to publish agent workspace draft pull request"
            );
            mark_agent_workspace_publish_failure_with_routing(
                state,
                &workspace,
                &error,
                Some("failed"),
                &repair_service,
                route_fixable_failures_to_agent,
                &repair_target,
            )
            .await;
            return Err(error);
        }
    };

    state
        .agent_conversation_workspace_repo
        .update_publication(
            &workspace.conversation_id,
            Some(outcome.pr_number),
            Some(&outcome.pr_url),
            Some(outcome.pr_status),
            Some("pushed"),
        )
        .await
        .map_err(|e| e.to_string())?;
    append_agent_workspace_publication_event(
        state,
        &workspace.conversation_id,
        "published",
        "succeeded",
        "Draft pull request is ready",
        Some(format!("published:{}", outcome.pr_number)),
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(workspace);

    if let Err(error) = crate::application::agent_workspace_review_auto_merge::
        restore_guarded_auto_merge_after_publish(state, &refreshed)
        .await
    {
        tracing::warn!(
            target: "ralphx_lib::commands::agent_workspace_publish",
            conversation_id = %refreshed.conversation_id,
            pr_number = outcome.pr_number,
            error = %error,
            "Deferred workspace Review auto-merge restoration after publish"
        );
    }
    refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&refreshed.conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(refreshed);

    if refreshed.auto_publish_enabled && refreshed.pr_auto_merge_desired {
        match sync_agent_workspace_auto_merge_preference_for_workspace(
            Arc::clone(github),
            &worktree_path,
            outcome.pr_number,
            &refreshed,
            Arc::clone(&state.agent_conversation_workspace_repo),
        )
        .await
        {
            Ok(_) => {
                refreshed = state
                    .agent_conversation_workspace_repo
                    .get_by_conversation_id(&refreshed.conversation_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .unwrap_or(refreshed);
            }
            Err(error) => {
                tracing::warn!(
                    target: "ralphx_lib::commands::agent_workspace_publish",
                    conversation_id = %refreshed.conversation_id,
                    project_id = %refreshed.project_id,
                    pr_number = outcome.pr_number,
                    error = %error,
                    "Deferred agent workspace auto-merge synchronization after publish"
                );
                state
                    .agent_conversation_workspace_repo
                    .update_pr_auto_merge_state(
                        &refreshed.conversation_id,
                        Some(false),
                        Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING),
                        Some(&format!(
                            "GitHub auto-merge state could not be refreshed yet: {error}"
                        )),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                refreshed = state
                    .agent_conversation_workspace_repo
                    .get_by_conversation_id(&refreshed.conversation_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .unwrap_or(refreshed);
            }
        }
    }

    let review_chat_service: Arc<dyn ChatService> = Arc::new(repair_service);
    state
        .pr_poller_registry
        .start_agent_workspace_polling_with_repair_repo(
            refreshed.conversation_id.clone(),
            outcome.pr_number,
            project.clone(),
            worktree_path.clone(),
            Arc::clone(&state.agent_conversation_workspace_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.agent_workspace_repair_repo),
            review_chat_service,
        );

    tracing::info!(
        target: "ralphx_lib::commands::agent_workspace_publish",
        conversation_id = %conversation_id,
        project_id = %project.id,
        branch = %refreshed.branch_name,
        reviewable_commit_count,
        created_pr = outcome.created_pr,
        pr_number = outcome.pr_number,
        elapsed_ms = publish_started.elapsed().as_millis(),
        "Completed agent workspace publish"
    );

    Ok(PublishAgentConversationWorkspaceResponse {
        workspace: AgentConversationWorkspaceResponse::from(refreshed),
        commit_sha,
        pushed: true,
        created_pr: outcome.created_pr,
        pr_number: Some(outcome.pr_number),
        pr_url: Some(outcome.pr_url),
    })
}

async fn resolve_linked_plan_markdown(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> Option<String> {
    let session_id = workspace.linked_ideation_session_id.as_ref()?;
    let session = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await
        .ok()
        .flatten()?;
    let artifact_id = session.plan_artifact_id?;
    let artifact = state
        .artifact_repo
        .get_by_id(&artifact_id)
        .await
        .ok()
        .flatten()?;
    let raw = match artifact.content {
        ArtifactContent::Inline { text } => text,
        ArtifactContent::File { path } => tokio::fs::read_to_string(path).await.ok()?,
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

async fn mark_agent_workspace_publish_status(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    push_status: &str,
) -> crate::error::AppResult<()> {
    state
        .agent_conversation_workspace_repo
        .update_publication(
            &workspace.conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            workspace.publication_pr_status.as_deref(),
            Some(push_status),
        )
        .await?;
    append_agent_workspace_publication_event(
        state,
        &workspace.conversation_id,
        push_status,
        publication_event_status_for_push_status(push_status),
        publication_event_summary_for_push_status(push_status),
        None,
    )
    .await
}

async fn mark_agent_workspace_publish_description_failure(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    error: &str,
) {
    let _ = state
        .agent_conversation_workspace_repo
        .update_publication(
            &workspace.conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            workspace.publication_pr_status.as_deref(),
            Some("description_failed"),
        )
        .await;
    let _ = append_agent_workspace_publication_event(
        state,
        &workspace.conversation_id,
        "description_failed",
        "failed",
        error,
        Some("operational".to_string()),
    )
    .await;
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConversationWorkspaceRepairTarget {
    pub branch_name: String,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub worktree_path: Option<PathBuf>,
}

impl AgentConversationWorkspaceRepairTarget {
    fn from_workspace(workspace: &AgentConversationWorkspace) -> Self {
        Self {
            branch_name: workspace.branch_name.clone(),
            base_ref: workspace.base_ref.clone(),
            base_display_name: workspace.base_display_name.clone(),
            worktree_path: None,
        }
    }
}

async fn canonical_agent_workspace_repair_dispatch_target(
    target: &AgentConversationWorkspaceRepairTarget,
) -> crate::error::AppResult<GitTargetIdentity> {
    let worktree_path = target.worktree_path.as_ref().ok_or_else(|| {
        AppError::Validation(
            "workspace repair dispatch requires a resolved canonical target worktree".to_string(),
        )
    })?;
    GitService::canonical_target_identity(worktree_path, &target.branch_name).await
}

#[doc(hidden)]
pub fn build_agent_workspace_publish_repair_message(
    error: &str,
    workspace: &AgentConversationWorkspace,
) -> String {
    build_agent_workspace_publish_repair_message_for_target(
        error,
        workspace,
        &AgentConversationWorkspaceRepairTarget::from_workspace(workspace),
    )
}

#[doc(hidden)]
pub fn build_agent_workspace_publish_repair_message_for_target(
    error: &str,
    workspace: &AgentConversationWorkspace,
    target: &AgentConversationWorkspaceRepairTarget,
) -> String {
    build_agent_workspace_repair_message_for_target(
        error,
        workspace,
        target,
        AgentWorkspacePostRepairAction::Publish,
    )
}

fn build_agent_workspace_repair_message_for_target(
    error: &str,
    workspace: &AgentConversationWorkspace,
    target: &AgentConversationWorkspaceRepairTarget,
    post_repair_action: AgentWorkspacePostRepairAction,
) -> String {
    let base = target
        .base_display_name
        .as_deref()
        .unwrap_or(target.base_ref.as_str());
    [
        post_repair_action.failure_title().to_string(),
        String::new(),
        post_repair_action.repair_instruction().to_string(),
        "After the repair is committed, call complete_agent_workspace_repair with a summary; add a blocker if the repair cannot be completed safely, and use resolution to classify the outcome honestly."
            .to_string(),
        String::new(),
        format!("Error: {error}"),
        format!("Conversation ID: {}", workspace.conversation_id),
        format!("Workspace branch: {}", target.branch_name),
        format!("Base: {base}"),
        format!("Base ref: {}", target.base_ref),
    ]
    .join("\n")
}

#[derive(Debug, Default, Clone)]
pub struct AgentWorkspaceRepairRuntimeOverrides {
    pub harness: Option<AgentHarnessKind>,
    pub model: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
}

#[doc(hidden)]
pub async fn send_agent_workspace_publish_repair_message<S>(
    service: &S,
    workspace: &AgentConversationWorkspace,
    error: &str,
    runtime_overrides: AgentWorkspaceRepairRuntimeOverrides,
) -> Result<SendResult, ChatServiceError>
where
    S: ChatService + ?Sized,
{
    send_agent_workspace_publish_repair_message_for_target(
        service,
        workspace,
        error,
        runtime_overrides,
        &AgentConversationWorkspaceRepairTarget::from_workspace(workspace),
    )
    .await
}

#[doc(hidden)]
pub async fn send_agent_workspace_publish_repair_message_for_target<S>(
    service: &S,
    workspace: &AgentConversationWorkspace,
    error: &str,
    runtime_overrides: AgentWorkspaceRepairRuntimeOverrides,
    target: &AgentConversationWorkspaceRepairTarget,
) -> Result<SendResult, ChatServiceError>
where
    S: ChatService + ?Sized,
{
    send_agent_workspace_repair_message_for_target(
        service,
        workspace,
        error,
        runtime_overrides,
        target,
        AgentWorkspacePostRepairAction::Publish,
        None,
    )
    .await
}

async fn send_agent_workspace_repair_message_for_target<S>(
    service: &S,
    workspace: &AgentConversationWorkspace,
    error: &str,
    runtime_overrides: AgentWorkspaceRepairRuntimeOverrides,
    target: &AgentConversationWorkspaceRepairTarget,
    post_repair_action: AgentWorkspacePostRepairAction,
    preallocated_agent_run_id: Option<AgentRunId>,
) -> Result<SendResult, ChatServiceError>
where
    S: ChatService + ?Sized,
{
    service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &build_agent_workspace_repair_message_for_target(
                error,
                workspace,
                target,
                post_repair_action,
            ),
            SendMessageOptions {
                preallocated_agent_run_id,
                queue_policy: SendQueuePolicy::RequireImmediateStart,
                conversation_id_override: Some(workspace.conversation_id),
                agent_name_override: Some(AGENT_WORKSPACE_REPAIR.to_string()),
                harness_override: runtime_overrides.harness,
                model_override: runtime_overrides.model,
                logical_effort_override: runtime_overrides.logical_effort,
                working_directory_override: target.worktree_path.clone(),
                force_new_provider_session: true,
                preserve_conversation_provider_session_ref: true,
                ..Default::default()
            },
        )
        .await
}

#[doc(hidden)]
pub async fn mark_agent_workspace_publish_failure<S>(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    error: &str,
    pr_status_override: Option<&str>,
    repair_service: &S,
) where
    S: ChatService + ?Sized,
{
    let target = AgentConversationWorkspaceRepairTarget::from_workspace(workspace);
    mark_agent_workspace_publish_failure_with_routing(
        state,
        workspace,
        error,
        pr_status_override,
        repair_service,
        true,
        &target,
    )
    .await;
}

#[doc(hidden)]
pub async fn mark_agent_workspace_publish_failure_with_target<S>(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    error: &str,
    pr_status_override: Option<&str>,
    repair_service: &S,
    target: &AgentConversationWorkspaceRepairTarget,
) where
    S: ChatService + ?Sized,
{
    mark_agent_workspace_publish_failure_with_routing(
        state,
        workspace,
        error,
        pr_status_override,
        repair_service,
        true,
        target,
    )
    .await;
}

async fn mark_agent_workspace_update_failure_with_target<S>(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    error: &str,
    pr_status_override: Option<&str>,
    repair_service: &S,
    target: &AgentConversationWorkspaceRepairTarget,
) where
    S: ChatService + ?Sized,
{
    mark_agent_workspace_failure_with_routing_and_action(
        state,
        workspace,
        error,
        pr_status_override,
        repair_service,
        true,
        target,
        AgentWorkspacePostRepairAction::UpdateOnly,
    )
    .await;
}

async fn mark_agent_workspace_publish_failure_with_routing<S>(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    error: &str,
    pr_status_override: Option<&str>,
    repair_service: &S,
    route_fixable_failures_to_agent: bool,
    target: &AgentConversationWorkspaceRepairTarget,
) where
    S: ChatService + ?Sized,
{
    mark_agent_workspace_failure_with_routing_and_action(
        state,
        workspace,
        error,
        pr_status_override,
        repair_service,
        route_fixable_failures_to_agent,
        target,
        AgentWorkspacePostRepairAction::Publish,
    )
    .await;
}

async fn mark_agent_workspace_failure_with_routing_and_action<S>(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    error: &str,
    pr_status_override: Option<&str>,
    repair_service: &S,
    route_fixable_failures_to_agent: bool,
    target: &AgentConversationWorkspaceRepairTarget,
    post_repair_action: AgentWorkspacePostRepairAction,
) where
    S: ChatService + ?Sized,
{
    let failure_class = classify_publish_failure(error);
    mark_agent_workspace_failure_with_routing_and_action_classified(
        state,
        workspace,
        error,
        pr_status_override,
        repair_service,
        route_fixable_failures_to_agent,
        target,
        post_repair_action,
        failure_class,
        false,
    )
    .await;
}

/// Only direct user actions may supersede a blocked repair generation. Background failure
/// routing reaches the same dispatcher through the default `false` above. The dispatch target
/// must carry the resolved canonical worktree, or the superseding successor can never be sent.
async fn retry_blocked_agent_workspace_repair_for_explicit_user_action<S>(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    repair_service: &S,
    post_repair_action: AgentWorkspacePostRepairAction,
) -> bool
where
    S: ChatService + ?Sized,
{
    let Ok(Some(attempt)) = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
    else {
        return false;
    };
    if attempt.phase != AgentWorkspaceRepairPhase::Blocked {
        return false;
    }
    let Ok(Some(project)) = state.project_repo.get_by_id(&workspace.project_id).await else {
        tracing::warn!(
            conversation_id = %workspace.conversation_id,
            "Skipping blocked workspace repair retry: project could not be loaded"
        );
        return false;
    };
    let resolved =
        match crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path(
            &project,
            workspace,
            state.plan_branch_repo.as_ref(),
        )
        .await
        {
            Ok(resolved) => resolved,
            Err(error) => {
                tracing::warn!(
                    conversation_id = %workspace.conversation_id,
                    error = %error,
                    "Skipping blocked workspace repair retry: canonical workspace path did not resolve"
                );
                return false;
            }
        };
    let target = AgentConversationWorkspaceRepairTarget {
        branch_name: workspace.branch_name.clone(),
        base_ref: workspace.base_ref.clone(),
        base_display_name: workspace.base_display_name.clone(),
        worktree_path: Some(resolved.path),
    };

    let error = last_meaningful_agent_workspace_repair_reason(&attempt);
    mark_agent_workspace_failure_with_routing_and_action_classified(
        state,
        workspace,
        error,
        None,
        repair_service,
        true,
        &target,
        post_repair_action,
        PublishFailureClass::AgentFixable,
        true,
    )
    .await;
    true
}

/// Delivery retries append `auto_retry_*` streak markers to the durable reason history. Those
/// markers describe recovery bookkeeping rather than the publish failure the repair agent needs.
fn last_meaningful_agent_workspace_repair_reason(
    attempt: &AgentWorkspaceRepairAttempt,
) -> &str {
    attempt
        .pending_reasons
        .iter()
        .rev()
        .map(String::as_str)
        .find(|reason| {
            let trimmed = reason.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with(
                    crate::application::agent_workspace_publish_recovery::AUTO_RETRY_BLOCKED_REPAIR_REASON_PREFIX,
                )
                && !trimmed.starts_with(
                    crate::application::agent_workspace_publish_recovery::AUTO_RETRY_READY_REPAIR_REASON_PREFIX,
                )
        })
        .or_else(|| {
            attempt
                .summary
                .as_deref()
                .filter(|summary| !summary.trim().is_empty())
        })
        .unwrap_or("Retrying blocked workspace repair.")
}

async fn mark_agent_workspace_failure_with_routing_and_action_classified<S>(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    error: &str,
    pr_status_override: Option<&str>,
    repair_service: &S,
    route_fixable_failures_to_agent: bool,
    target: &AgentConversationWorkspaceRepairTarget,
    post_repair_action: AgentWorkspacePostRepairAction,
    failure_class: PublishFailureClass,
    retry_blocked: bool,
) where
    S: ChatService + ?Sized,
{
    if !route_fixable_failures_to_agent
        || !matches!(failure_class, PublishFailureClass::AgentFixable)
    {
        let push_status = "failed";
        let _ = state
            .agent_conversation_workspace_repo
            .update_publication(
                &workspace.conversation_id,
                workspace.publication_pr_number,
                workspace.publication_pr_url.as_deref(),
                pr_status_override.or(workspace.publication_pr_status.as_deref()),
                Some(push_status),
            )
            .await;
        let _ = append_agent_workspace_publication_event(
            state,
            &workspace.conversation_id,
            push_status,
            "failed",
            error,
            Some("operational".to_string()),
        )
        .await;
        return;
    }

    let start = start_or_join_agent_workspace_repair(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.agent_conversation_workspace_repo),
        AgentWorkspaceRepairStartRequest {
            conversation_id: workspace.conversation_id.clone(),
            source: match post_repair_action {
                AgentWorkspacePostRepairAction::Publish => AgentWorkspaceRepairSource::Publish,
                AgentWorkspacePostRepairAction::UpdateOnly => {
                    AgentWorkspaceRepairSource::BaseUpdate
                }
            },
            continuation: match post_repair_action {
                AgentWorkspacePostRepairAction::Publish => {
                    AgentWorkspaceRepairContinuation::Publish
                }
                AgentWorkspacePostRepairAction::UpdateOnly => {
                    AgentWorkspaceRepairContinuation::UpdateOnly
                }
            },
            target_base_ref: target.base_ref.clone(),
            target_base_commit: workspace.base_commit.clone(),
            verified_newer_base: false,
            reason: error.to_string(),
            summary: post_repair_action.repair_requested_summary().to_string(),
            auto_merge_current: workspace.pr_auto_merge_current,
            retry_blocked,
            carryover_pr_autofix_evidence: None,
        },
    )
    .await;
    let attempt = match start {
        Ok(
            AgentWorkspaceRepairStartOutcome::Started(attempt)
            | AgentWorkspaceRepairStartOutcome::SuccessorStarted(attempt),
        ) => attempt,
        Ok(
            AgentWorkspaceRepairStartOutcome::Joined(_)
            | AgentWorkspaceRepairStartOutcome::BlockedByCurrent(_),
        ) => return,
        Err(error) => {
            let summary =
                format!("Failed to persist the durable workspace repair request: {error}");
            tracing::warn!(
                conversation_id = %workspace.conversation_id,
                error = %error,
                "Failed to start or join the durable agent workspace repair attempt"
            );
            let _ = state
                .agent_conversation_workspace_repo
                .update_publication(
                    &workspace.conversation_id,
                    workspace.publication_pr_number,
                    workspace.publication_pr_url.as_deref(),
                    pr_status_override.or(workspace.publication_pr_status.as_deref()),
                    Some("failed"),
                )
                .await;
            let _ = state
                .agent_conversation_workspace_repo
                .update_pr_auto_merge_state(
                    &workspace.conversation_id,
                    workspace.pr_auto_merge_current,
                    Some("blocked"),
                    Some(&summary),
                )
                .await;
            return;
        }
    };

    let dispatch_target = match canonical_agent_workspace_repair_dispatch_target(target).await {
        Ok(target) => target,
        Err(error) => {
            tracing::warn!(
                conversation_id = %workspace.conversation_id,
                error = %error,
                "Failed to resolve the canonical Git target before workspace repair dispatch"
            );
            return;
        }
    };
    let runtime_overrides = AgentWorkspaceRepairRuntimeOverrides::default();
    if should_defer_agent_workspace_repair_message(state, workspace).await {
        let repair_run_id = AgentRunId::new();
        let dispatch = match reserve_agent_workspace_repair_dispatch(
            Arc::clone(&state.agent_workspace_repair_repo),
            Arc::clone(&state.branch_update_repo),
            dispatch_target.clone(),
            attempt,
            repair_run_id.clone(),
            post_repair_action.repair_requested_summary(),
            workspace.pr_auto_merge_current,
        )
        .await
        {
            Ok(AgentWorkspaceRepairDispatchOutcome::Reserved(attempt)) => attempt,
            Ok(
                AgentWorkspaceRepairDispatchOutcome::Stale(_)
                | AgentWorkspaceRepairDispatchOutcome::Missing,
            ) => {
                return;
            }
            Err(error) => {
                tracing::warn!(
                    conversation_id = %workspace.conversation_id,
                    error = %error,
                    "Failed to reserve deferred durable workspace repair dispatch"
                );
                return;
            }
        };
        spawn_deferred_agent_workspace_repair_message(
            state,
            workspace.clone(),
            error.to_string(),
            runtime_overrides,
            target.clone(),
            post_repair_action,
            Some(dispatch),
            Some(repair_run_id),
        )
        .await;
        return;
    }

    let repair_run_id = AgentRunId::new();
    let dispatch = reserve_agent_workspace_repair_dispatch(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        dispatch_target,
        attempt,
        repair_run_id.clone(),
        post_repair_action.repair_requested_summary(),
        workspace.pr_auto_merge_current,
    )
    .await;
    let dispatch = match dispatch {
        Ok(AgentWorkspaceRepairDispatchOutcome::Reserved(attempt)) => attempt,
        Ok(
            AgentWorkspaceRepairDispatchOutcome::Stale(_)
            | AgentWorkspaceRepairDispatchOutcome::Missing,
        ) => {
            return;
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = %workspace.conversation_id,
                error = %error,
                "Failed to reserve durable workspace repair dispatch"
            );
            return;
        }
    };

    match send_agent_workspace_repair_message_for_target(
        repair_service,
        workspace,
        error,
        runtime_overrides,
        target,
        post_repair_action,
        Some(repair_run_id.clone()),
    )
    .await
    {
        Ok(result) => {
            if let Some(authority_error) =
                repair_dispatch_authority_error(&result, &workspace.conversation_id, &repair_run_id)
            {
                let repair_summary =
                    post_repair_action.repair_send_failed_summary(&authority_error);
                settle_agent_workspace_repair_dispatch_failure(
                    state,
                    dispatch,
                    &repair_summary,
                    classify_agent_workspace_repair_delivery(
                        Ok(&result),
                        &workspace.conversation_id,
                        &repair_run_id,
                    ),
                )
                .await;
                return;
            }
            settle_agent_workspace_repair_dispatch_success(
                state,
                dispatch,
                post_repair_action.repair_sent_summary(),
            )
            .await;
        }
        Err(repair_error) => {
            tracing::warn!(
                conversation_id = %workspace.conversation_id,
                error = %repair_error,
                "Failed to send agent workspace publish repair message"
            );
            let repair_summary =
                post_repair_action.repair_send_failed_summary(&repair_error.to_string());
            settle_agent_workspace_repair_dispatch_failure(
                state,
                dispatch,
                &repair_summary,
                classify_agent_workspace_repair_delivery(
                    Err(&repair_error),
                    &workspace.conversation_id,
                    &repair_run_id,
                ),
            )
            .await;
        }
    }
}

async fn should_defer_agent_workspace_repair_message(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> bool {
    let execution_state = state
        .app_handle
        .as_ref()
        .and_then(|handle| handle.try_state::<Arc<ExecutionState>>())
        .map(|state| state.inner().clone());
    should_defer_agent_workspace_repair_message_for_registry(
        state.app_handle.is_some(),
        &state.running_agent_registry,
        execution_state.as_ref(),
        workspace,
    )
    .await
}

async fn should_defer_agent_workspace_repair_message_for_registry(
    app_handle_available: bool,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    execution_state: Option<&Arc<ExecutionState>>,
    workspace: &AgentConversationWorkspace,
) -> bool {
    if !app_handle_available {
        return false;
    }

    let key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        workspace.conversation_id.as_str(),
    );
    if !running_agent_registry.is_running(&key).await {
        return false;
    }

    let interactive_slot_key = agent_workspace_interactive_slot_key(&workspace.conversation_id);
    !execution_state
        .map(|state| state.is_interactive_idle(&interactive_slot_key))
        .unwrap_or(false)
}

async fn agent_workspace_repair_wait_released(
    state: &AppState,
    execution_state: Option<&Arc<ExecutionState>>,
    key: &RunningAgentKey,
    interactive_slot_key: &str,
) -> bool {
    if !state.running_agent_registry.is_running(key).await {
        return true;
    }

    execution_state
        .map(|state| state.is_interactive_idle(interactive_slot_key))
        .unwrap_or(false)
}

async fn spawn_deferred_agent_workspace_repair_message(
    state: &AppState,
    workspace: AgentConversationWorkspace,
    error: String,
    runtime_overrides: AgentWorkspaceRepairRuntimeOverrides,
    target: AgentConversationWorkspaceRepairTarget,
    post_repair_action: AgentWorkspacePostRepairAction,
    dispatch: Option<AgentWorkspaceRepairAttempt>,
    repair_run_id: Option<AgentRunId>,
) {
    let Some(app_handle) = state.app_handle.clone() else {
        return;
    };
    let (Some(dispatch), Some(repair_run_id)) = (dispatch, repair_run_id) else {
        return;
    };

    tauri::async_runtime::spawn(async move {
        let conversation_id = workspace.conversation_id;
        let key = RunningAgentKey::new(
            ChatContextType::Project.to_string(),
            conversation_id.as_str(),
        );
        let interactive_slot_key = agent_workspace_interactive_slot_key(&conversation_id);
        let wait_started = Instant::now();
        loop {
            let Some(state) = app_handle.try_state::<AppState>() else {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    "Deferred agent workspace repair could not access AppState"
                );
                return;
            };
            let execution_state = app_handle
                .try_state::<Arc<ExecutionState>>()
                .map(|state| state.inner().clone());
            if agent_workspace_repair_wait_released(
                state.inner(),
                execution_state.as_ref(),
                &key,
                &interactive_slot_key,
            )
            .await
            {
                break;
            }
            if wait_started.elapsed() >= Duration::from_secs(DEFERRED_REPAIR_WAIT_TIMEOUT_SECS) {
                let summary =
                    "Timed out waiting for active workspace agent turn before sending repair";
                settle_agent_workspace_repair_dispatch_failure(
                    state.inner(),
                    dispatch,
                    summary,
                    AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
                )
                .await;
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    elapsed_ms = wait_started.elapsed().as_millis(),
                    "Timed out waiting to send deferred agent workspace repair"
                );
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        let Some(state) = app_handle.try_state::<AppState>() else {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                "Deferred agent workspace repair could not access AppState after wait"
            );
            return;
        };
        let execution_state = app_handle
            .try_state::<Arc<ExecutionState>>()
            .map(|state| state.inner().clone());
        let repair_service = match execution_state {
            Some(execution_state) => state.build_chat_service_with_execution_state(execution_state),
            None => state.build_chat_service(),
        };

        match send_agent_workspace_repair_message_for_target(
            &repair_service,
            &workspace,
            &error,
            runtime_overrides,
            &target,
            post_repair_action,
            Some(repair_run_id.clone()),
        )
        .await
        {
            Ok(result) => {
                if let Some(authority_error) =
                    repair_dispatch_authority_error(&result, &conversation_id, &repair_run_id)
                {
                    let repair_summary =
                        post_repair_action.repair_send_failed_summary(&authority_error);
                    settle_agent_workspace_repair_dispatch_failure(
                        state.inner(),
                        dispatch,
                        &repair_summary,
                        classify_agent_workspace_repair_delivery(
                            Ok(&result),
                            &conversation_id,
                            &repair_run_id,
                        ),
                    )
                    .await;
                    return;
                }
                settle_agent_workspace_repair_dispatch_success(
                    state.inner(),
                    dispatch,
                    post_repair_action.deferred_repair_sent_summary(),
                )
                .await;
            }
            Err(repair_error) => {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    error = %repair_error,
                    "Failed to send deferred agent workspace publish repair message"
                );
                let repair_summary =
                    post_repair_action.repair_send_failed_summary(&repair_error.to_string());
                settle_agent_workspace_repair_dispatch_failure(
                    state.inner(),
                    dispatch,
                    &repair_summary,
                    classify_agent_workspace_repair_delivery(
                        Err(&repair_error),
                        &conversation_id,
                        &repair_run_id,
                    ),
                )
                .await;
            }
        }
    });
}

async fn append_agent_workspace_publication_event(
    state: &AppState,
    conversation_id: &ChatConversationId,
    step: &str,
    status: &str,
    summary: &str,
    classification: Option<String>,
) -> crate::error::AppResult<()> {
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            *conversation_id,
            step,
            status,
            summary,
            classification,
        ))
        .await
}

async fn settle_agent_workspace_repair_dispatch_failure(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    settlement: AgentWorkspaceRepairDispatchSettlement,
) {
    let conversation_id = attempt.conversation_id.clone();
    match settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        attempt,
        settlement,
        summary,
        None,
    )
    .await
    {
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                conversation_id = %conversation_id,
                error = %error,
                "Failed to persist durable agent workspace repair dispatch failure"
            );
        }
    }
}

async fn settle_agent_workspace_repair_dispatch_success(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
) {
    let conversation_id = attempt.conversation_id.clone();
    if let Err(error) = settle_agent_workspace_repair_dispatch_outcome(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        attempt,
        AgentWorkspaceRepairDispatchSettlement::Delivered,
        summary,
        None,
    )
    .await
    {
        tracing::warn!(
            conversation_id = %conversation_id,
            error = %error,
            "Failed to persist durable agent workspace repair dispatch success"
        );
    }
}

fn publication_event_status_for_push_status(push_status: &str) -> &'static str {
    match push_status {
        "pushed" => "succeeded",
        "no_changes" => "skipped",
        "failed" | "needs_agent" | "description_failed" => "failed",
        _ => "started",
    }
}

fn publication_event_summary_for_push_status(push_status: &str) -> &'static str {
    match push_status {
        "checking" => "Checking workspace changes",
        "committing" => "Committing workspace changes",
        "refreshing" => "Refreshing branch from base",
        "describing" => "Drafting pull request description",
        "pushing" => "Pushing agent branch",
        "pushed" => "Agent branch pushed",
        "no_changes" => "No committed changes to publish",
        "needs_agent" => "Publish needs workspace agent repair",
        "description_failed" => "Pull request description failed",
        "failed" => "Publish failed",
        _ => "Publish status changed",
    }
}

/// Get a conversation with all its messages
#[tauri::command]
pub async fn get_agent_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<Option<AgentConversationWithMessagesResponse>, String> {
    use crate::domain::entities::ChatConversationId;

    let conversation_id = ChatConversationId::from_string(&conversation_id);

    let service = create_chat_service(&state, app, &execution_state);
    if let Err(error) =
        wake_agent_workspace_for_bridge_events(&state, &service, &conversation_id).await
    {
        tracing::warn!(
            conversation_id = %conversation_id,
            error = %error,
            "Failed to wake agent workspace for bridge events"
        );
    }

    let conversation = service
        .get_conversation_with_messages(&conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    let Some(cwm) = conversation else {
        return Ok(None);
    };

    let mut messages = Vec::with_capacity(cwm.messages.len());
    for message in cwm.messages {
        let (tool_calls, content_blocks) = reconcile_delegated_result_payloads(
            &state,
            message.tool_calls.clone(),
            message.content_blocks.clone(),
        )
        .await;

        messages.push(AgentMessageResponse {
            id: message.id.as_str().to_string(),
            conversation_id: message
                .conversation_id
                .as_ref()
                .map(|id| id.as_str().to_string()),
            role: message.role.to_string(),
            content: message.content,
            metadata: message.metadata,
            tool_calls,
            content_blocks,
            attribution_source: message.attribution_source,
            provider_harness: message.provider_harness.map(|value| value.to_string()),
            provider_session_id: message.provider_session_id,
            upstream_provider: message.upstream_provider,
            provider_profile: message.provider_profile,
            logical_model: message.logical_model,
            effective_model_id: message.effective_model_id,
            logical_effort: message.logical_effort.map(|value| value.to_string()),
            effective_effort: message.effective_effort,
            input_tokens: message.input_tokens,
            output_tokens: message.output_tokens,
            cache_creation_tokens: message.cache_creation_tokens,
            cache_read_tokens: message.cache_read_tokens,
            estimated_usd: message.estimated_usd,
            usage_provenance: message.usage_provenance.map(|value| value.to_string()),
            created_at: message.created_at.to_rfc3339(),
        });
    }

    Ok(Some(AgentConversationWithMessagesResponse {
        conversation: agent_conversation_response_for_state(state.inner(), cwm.conversation)
            .await?,
        messages,
    }))
}

/// Get lightweight conversation metadata without loading any messages.
#[tauri::command]
pub async fn get_agent_conversation_summary(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Option<AgentConversationResponse>, String> {
    get_agent_conversation_summary_for_app_state(&state, conversation_id).await
}

pub async fn get_agent_conversation_summary_for_app_state(
    state: &AppState,
    conversation_id: String,
) -> Result<Option<AgentConversationResponse>, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    match conversation {
        Some(conversation) => Ok(Some(
            agent_conversation_response_for_state(state, conversation).await?,
        )),
        None => Ok(None),
    }
}

/// Get a tail-first page of conversation messages for fast conversation switching.
/// `offset` counts how many newest messages to skip before loading older history.
#[tauri::command]
pub async fn get_agent_conversation_messages_page(
    conversation_id: String,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<Option<AgentConversationMessagesPageResponse>, String> {
    let conversation_id = ChatConversationId::from_string(&conversation_id);
    let limit = limit.unwrap_or(40).clamp(1, 200);
    let offset = offset.unwrap_or(0);

    if let Err(error) = wake_agent_workspace_for_bridge_events_with_service_factory(
        &state,
        &conversation_id,
        || create_chat_service(&state, app, &execution_state),
    )
    .await
    {
        tracing::warn!(
            conversation_id = %conversation_id,
            error = %error,
            "Failed to wake agent workspace for bridge events"
        );
    }

    get_agent_conversation_messages_page_for_app_state(&state, conversation_id, limit, offset).await
}

pub async fn get_agent_conversation_messages_page_for_app_state(
    state: &AppState,
    conversation_id: ChatConversationId,
    limit: u32,
    offset: u32,
) -> Result<Option<AgentConversationMessagesPageResponse>, String> {
    let Some(conversation) = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };

    let raw_messages = state
        .chat_message_repo
        .get_recent_by_conversation_paginated(&conversation_id, limit, offset)
        .await
        .map_err(|e| e.to_string())?;

    let mut messages = Vec::with_capacity(raw_messages.len());
    for message in raw_messages {
        let (tool_calls, content_blocks) = reconcile_delegated_result_payloads(
            state,
            message.tool_calls.clone(),
            message.content_blocks.clone(),
        )
        .await;
        let (tool_calls, content_blocks) = preview_tool_payloads_for_message(
            &conversation_id.as_str(),
            message.id.as_str(),
            tool_calls,
            content_blocks,
        );

        messages.push(AgentMessageResponse {
            id: message.id.as_str().to_string(),
            conversation_id: message
                .conversation_id
                .as_ref()
                .map(|id| id.as_str().to_string()),
            role: message.role.to_string(),
            content: message.content,
            metadata: message.metadata,
            tool_calls,
            content_blocks,
            attribution_source: message.attribution_source,
            provider_harness: message.provider_harness.map(|value| value.to_string()),
            provider_session_id: message.provider_session_id,
            upstream_provider: message.upstream_provider,
            provider_profile: message.provider_profile,
            logical_model: message.logical_model,
            effective_model_id: message.effective_model_id,
            logical_effort: message.logical_effort.map(|value| value.to_string()),
            effective_effort: message.effective_effort,
            input_tokens: message.input_tokens,
            output_tokens: message.output_tokens,
            cache_creation_tokens: message.cache_creation_tokens,
            cache_read_tokens: message.cache_read_tokens,
            estimated_usd: message.estimated_usd,
            usage_provenance: message.usage_provenance.map(|value| value.to_string()),
            created_at: message.created_at.to_rfc3339(),
        });
    }

    let fetched_count = offset as i64 + messages.len() as i64;
    let total_message_count = conversation.message_count.max(0);
    let has_older = fetched_count < total_message_count;

    Ok(Some(AgentConversationMessagesPageResponse {
        conversation: agent_conversation_response_for_state(state, conversation).await?,
        messages,
        limit,
        offset,
        total_message_count,
        has_older,
    }))
}

/// Get a tail-first page of normalized visible conversation timeline items.
/// `before_sequence` loads the page older than the currently oldest loaded item.
#[tauri::command]
pub async fn get_agent_conversation_timeline_page(
    conversation_id: String,
    limit: Option<u32>,
    before_sequence: Option<i64>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<Option<AgentConversationTimelinePageResponse>, String> {
    let conversation_id = ChatConversationId::from_string(&conversation_id);
    let limit = limit.unwrap_or(40).clamp(1, 200);

    if let Err(error) = wake_agent_workspace_for_bridge_events_with_service_factory(
        &state,
        &conversation_id,
        || create_chat_service(&state, app, &execution_state),
    )
    .await
    {
        tracing::warn!(
            conversation_id = %conversation_id,
            error = %error,
            "Failed to wake agent workspace for timeline bridge events"
        );
    }

    get_agent_conversation_timeline_page_for_app_state(
        &state,
        conversation_id,
        limit,
        before_sequence,
    )
    .await
}

pub async fn get_agent_conversation_timeline_page_for_app_state(
    state: &AppState,
    conversation_id: ChatConversationId,
    limit: u32,
    before_sequence: Option<i64>,
) -> Result<Option<AgentConversationTimelinePageResponse>, String> {
    let Some(conversation) = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, limit, before_sequence)
        .await
        .map_err(|e| e.to_string())?;

    let mut items = page.items;
    let mut snapshot_cache = HashMap::new();
    for item in &mut items {
        reconcile_delegated_timeline_item_result(state, item, &mut snapshot_cache).await;
    }

    Ok(Some(AgentConversationTimelinePageResponse {
        conversation: agent_conversation_response_for_state(state, conversation).await?,
        items: items
            .into_iter()
            .map(AgentTimelineItemResponse::from)
            .collect(),
        limit: page.limit,
        before_sequence: page.before_sequence,
        total_item_count: page.total_item_count,
        has_older: page.has_older,
        oldest_loaded_sequence: page.oldest_loaded_sequence,
        newest_loaded_sequence: page.newest_loaded_sequence,
    }))
}

/// Get the full result payload for a previewed tool call in a persisted message.
#[tauri::command]
pub async fn get_agent_message_tool_call_detail(
    conversation_id: String,
    message_id: String,
    tool_call_id: Option<String>,
    content_block_index: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Option<AgentToolCallDetailResponse>, String> {
    let conversation_id = ChatConversationId::from_string(&conversation_id);
    let message_id = ChatMessageId::from_string(&message_id);

    let Some(message) = state
        .chat_message_repo
        .get_by_id(&message_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };

    if message.conversation_id.as_ref().map(|id| id.as_str()) != Some(conversation_id.as_str()) {
        return Ok(None);
    }

    let (tool_calls, content_blocks) = reconcile_delegated_result_payloads(
        &state,
        message.tool_calls.clone(),
        message.content_blocks.clone(),
    )
    .await;
    let detail = find_tool_call_detail(
        tool_calls.as_ref(),
        content_blocks.as_ref(),
        tool_call_id.as_deref(),
        content_block_index.map(|index| index as usize),
    );

    Ok(detail.map(|tool_call| AgentToolCallDetailResponse { tool_call }))
}

/// Get the full tool-call payload for a normalized timeline item.
#[tauri::command]
pub async fn get_agent_timeline_item_tool_call_detail(
    conversation_id: String,
    timeline_item_id: String,
    state: State<'_, AppState>,
) -> Result<Option<AgentToolCallDetailResponse>, String> {
    let conversation_id = ChatConversationId::from_string(&conversation_id);
    let timeline_item_id =
        crate::domain::entities::ChatTimelineItemId::from_string(timeline_item_id);

    get_agent_timeline_item_tool_call_detail_for_app_state(
        &state,
        conversation_id,
        timeline_item_id,
    )
    .await
}

pub async fn get_agent_timeline_item_tool_call_detail_for_app_state(
    state: &AppState,
    conversation_id: ChatConversationId,
    timeline_item_id: crate::domain::entities::ChatTimelineItemId,
) -> Result<Option<AgentToolCallDetailResponse>, String> {
    let Some(item) = state
        .chat_timeline_repo
        .get_by_id(&timeline_item_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };

    if item.conversation_id != conversation_id {
        return Ok(None);
    }

    let mut item = item;
    reconcile_delegated_timeline_item_result(state, &mut item, &mut HashMap::new()).await;
    let detail_message_id = item.message_id.as_ref().map(|id| id.as_str().to_string());
    let block = timeline_item_content_block(
        &item,
        &conversation_id.as_str(),
        detail_message_id.as_deref(),
        false,
    );
    Ok(Some(AgentToolCallDetailResponse { tool_call: block }))
}

/// Get the active agent run for a conversation
#[tauri::command]
pub async fn get_agent_run_status_unified(
    conversation_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<Option<AgentRunStatusResponse>, String> {
    use crate::domain::entities::ChatConversationId;
    use crate::domain::services::RunningAgentKey;
    use crate::infrastructure::agents::claude::model_labels::model_id_to_label;

    let conv_id = ChatConversationId::from_string(&conversation_id);

    let service = create_chat_service(&state, app, &execution_state);

    let Some(run) = service
        .get_active_run(&conv_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };

    // Look up conversation to get context_type/context_id for registry lookup
    let (model_id, model_label) =
        if let Ok(Some(conv)) = state.chat_conversation_repo.get_by_id(&conv_id).await {
            let runtime_context_id = if conv.context_type == ChatContextType::Project {
                conv.id.as_str().to_string()
            } else {
                conv.context_id.clone()
            };
            let key = RunningAgentKey::new(conv.context_type.to_string(), runtime_context_id);
            let agent_info = state.running_agent_registry.get(&key).await;
            let mid = agent_info.and_then(|info| info.model);
            let mlabel = mid.as_deref().map(|id| model_id_to_label(id));
            (mid, mlabel)
        } else {
            (None, None)
        };

    Ok(Some(AgentRunStatusResponse {
        id: run.id.as_str().to_string(),
        conversation_id: run.conversation_id.as_str().to_string(),
        status: run.status.to_string(),
        started_at: run.started_at.to_rfc3339(),
        completed_at: run.completed_at.map(|dt| dt.to_rfc3339()),
        error_message: run.error_message,
        model_id,
        model_label,
        persona_id: run.persona_id,
        persona_slug: run.persona_slug,
        persona_version: run.persona_version,
        persona_content_hash: run.persona_content_hash,
        persona_injected: run.persona_injected,
        persona_skipped_reason: run.persona_skipped_reason,
    }))
}

/// Check if the chat service is available
#[tauri::command]
pub async fn is_chat_service_available(
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let service = create_chat_service(&state, app, &execution_state);
    Ok(service.is_available().await)
}

/// Stop a running agent for a context
///
/// Sends SIGTERM to the running agent process and emits agent:stopped event.
/// Returns true if an agent was stopped, false if no agent was running.
///
/// Events emitted:
/// - agent:stopped - When agent is terminated
/// - agent:run_completed or agent:turn_completed (interactive) - So frontend knows agent is no longer running
#[tauri::command]
pub async fn stop_agent(
    context_type: String,
    context_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let context_type = parse_context_type(&context_type)?;

    let service = create_chat_service(&state, app, &execution_state);

    service
        .stop_agent(context_type, &context_id)
        .await
        .map_err(|e| e.to_string())
}

/// Check if an agent is running for a context
#[tauri::command]
pub async fn is_agent_running(
    context_type: String,
    context_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let context_type = parse_context_type(&context_type)?;

    let service = create_chat_service(&state, app, &execution_state);

    Ok(service.is_agent_running(context_type, &context_id).await)
}

/// Bulk-check whether agents are running for the requested context ids.
#[tauri::command]
pub async fn get_agent_running_states(
    context_type: String,
    context_ids: Vec<String>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<HashMap<String, AgentRunningState>, String> {
    let service =
        state.build_chat_service_with_execution_state(Arc::clone(execution_state.inner()));

    get_agent_running_states_for_service(&service, context_type, context_ids).await
}

#[doc(hidden)]
pub async fn get_agent_running_states_for_service(
    service: &dyn ChatService,
    context_type: String,
    context_ids: Vec<String>,
) -> Result<HashMap<String, AgentRunningState>, String> {
    let context_type = parse_context_type(&context_type)?;

    Ok(service
        .get_agent_running_states(context_type, &context_ids)
        .await)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationRuntimeSource {
    Workspace,
    WorkspaceReview,
    Ideation,
    Verification,
    TaskExecution,
    Review,
    Merge,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationRuntimeItem {
    pub source: AgentConversationRuntimeSource,
    pub context_type: String,
    pub context_id: String,
    pub label: String,
    pub title: String,
    pub agent_status: AgentRuntimeStatus,
    pub task_id: Option<String>,
    pub internal_status: Option<String>,
    pub running_process: Option<RunningProcess>,
    pub ideation_session: Option<RunningIdeationSession>,
    pub parent_session_id: Option<String>,
    pub child_session_id: Option<String>,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationRuntimeStatus {
    pub conversation_id: String,
    pub is_running: bool,
    pub agent_status: AgentRuntimeStatus,
    pub primary_source: Option<AgentConversationRuntimeSource>,
    pub summary_label: Option<String>,
    pub items: Vec<AgentConversationRuntimeItem>,
}

impl AgentConversationRuntimeStatus {
    fn idle(conversation_id: String) -> Self {
        Self {
            conversation_id,
            is_running: false,
            agent_status: AgentRuntimeStatus::Idle,
            primary_source: None,
            summary_label: None,
            items: Vec::new(),
        }
    }

    fn finalize(&mut self) {
        if self.items.is_empty() {
            self.is_running = false;
            self.agent_status = AgentRuntimeStatus::Idle;
            self.primary_source = None;
            self.summary_label = None;
            return;
        }

        self.is_running = true;
        self.agent_status = if self
            .items
            .iter()
            .any(|item| item.agent_status == AgentRuntimeStatus::Generating)
        {
            AgentRuntimeStatus::Generating
        } else {
            AgentRuntimeStatus::WaitingForInput
        };

        self.primary_source = self
            .items
            .iter()
            .max_by_key(|item| runtime_source_priority(item.source))
            .map(|item| item.source);
        self.summary_label = Some(summary_label_for_runtime_items(&self.items));
    }
}

fn runtime_source_priority(source: AgentConversationRuntimeSource) -> u8 {
    match source {
        AgentConversationRuntimeSource::Verification => 50,
        AgentConversationRuntimeSource::WorkspaceReview => 46,
        AgentConversationRuntimeSource::Merge => 45,
        AgentConversationRuntimeSource::Review => 44,
        AgentConversationRuntimeSource::TaskExecution => 43,
        AgentConversationRuntimeSource::Ideation => 30,
        AgentConversationRuntimeSource::Workspace => 20,
    }
}

fn summary_label_for_runtime_items(items: &[AgentConversationRuntimeItem]) -> String {
    if items
        .iter()
        .all(|item| item.agent_status == AgentRuntimeStatus::WaitingForInput)
    {
        return "Awaiting input".to_string();
    }

    if items
        .iter()
        .any(|item| item.source == AgentConversationRuntimeSource::Verification)
    {
        return "Verifying".to_string();
    }

    if items
        .iter()
        .any(|item| item.source == AgentConversationRuntimeSource::WorkspaceReview)
    {
        return "Reviewing".to_string();
    }

    let task_items = items
        .iter()
        .filter(|item| {
            matches!(
                item.source,
                AgentConversationRuntimeSource::TaskExecution
                    | AgentConversationRuntimeSource::Review
                    | AgentConversationRuntimeSource::Merge
            )
        })
        .count();
    if task_items > 0 {
        if items
            .iter()
            .any(|item| item.source == AgentConversationRuntimeSource::Merge)
        {
            return if task_items > 1 {
                "Merging tasks".to_string()
            } else {
                "Merging".to_string()
            };
        }
        if items
            .iter()
            .any(|item| item.source == AgentConversationRuntimeSource::Review)
        {
            return if task_items > 1 {
                "Reviewing tasks".to_string()
            } else {
                "Reviewing".to_string()
            };
        }
        return if task_items > 1 {
            "Executing tasks".to_string()
        } else {
            "Executing".to_string()
        };
    }

    if items
        .iter()
        .any(|item| item.source == AgentConversationRuntimeSource::Ideation)
    {
        return "Ideation running".to_string();
    }

    "Agent running".to_string()
}

fn idle_agent_running_state() -> AgentRunningState {
    AgentRunningState {
        is_running: false,
        agent_status: AgentRuntimeStatus::Idle,
    }
}

async fn direct_agent_running_state_for_context(
    state: &AppState,
    execution_state: &ExecutionState,
    context_type: ChatContextType,
    context_id: &str,
) -> Result<Option<AgentRunningState>, String> {
    let key = RunningAgentKey::new(context_type.to_string(), context_id.to_string());
    let Some(info) = state.running_agent_registry.get(&key).await else {
        return Ok(None);
    };

    let run_status = if info.agent_run_id.is_empty() {
        None
    } else {
        state
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(info.agent_run_id))
            .await
            .map_err(|error| error.to_string())?
            .map(|run| run.status)
    };

    Ok(Some(running_state_from_run_status_and_idle(
        run_status,
        execution_state.is_interactive_idle(&format!("{context_type}/{context_id}")),
    )))
}

fn ideation_generating_flag(execution_state: &ExecutionState, session_id: &str) -> bool {
    !execution_state.is_interactive_idle(&format!("ideation/{session_id}"))
}

async fn add_ideation_runtime_item(
    state: &AppState,
    execution_state: &ExecutionState,
    service: &dyn ChatService,
    runtime: &mut AgentConversationRuntimeStatus,
    session_id: &IdeationSessionId,
    source: AgentConversationRuntimeSource,
    parent_session_id: Option<&IdeationSessionId>,
) -> Result<(), String> {
    let session_id_str = session_id.as_str().to_string();
    let states = service
        .get_agent_running_states(
            ChatContextType::Ideation,
            std::slice::from_ref(&session_id_str),
        )
        .await;
    let running_state = states
        .get(&session_id_str)
        .copied()
        .unwrap_or_else(idle_agent_running_state);
    if !running_state.is_running {
        return Ok(());
    }

    let Some(session) = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };

    let now = chrono::Utc::now();
    let ideation_session = build_running_ideation_session(
        session_id_str.clone(),
        &session,
        ideation_generating_flag(execution_state, &session_id_str),
        now,
    );
    let label = match source {
        AgentConversationRuntimeSource::Verification => "Verifying",
        AgentConversationRuntimeSource::Ideation => "Ideation running",
        _ => "Agent running",
    };

    runtime.items.push(AgentConversationRuntimeItem {
        source,
        context_type: ChatContextType::Ideation.to_string(),
        context_id: session_id_str.clone(),
        label: label.to_string(),
        title: ideation_session.title.clone(),
        agent_status: running_state.agent_status,
        task_id: None,
        internal_status: None,
        running_process: None,
        ideation_session: Some(ideation_session),
        parent_session_id: parent_session_id.map(|id| id.as_str().to_string()),
        child_session_id: (source == AgentConversationRuntimeSource::Verification)
            .then_some(session_id_str),
        conversation_id: None,
    });

    Ok(())
}

async fn build_task_runtime_process(
    state: &AppState,
    task: &Task,
) -> Result<RunningProcess, String> {
    let task_id = task.id.clone();
    let steps = state
        .task_step_repo
        .get_by_task(&task_id)
        .await
        .map_err(|error| error.to_string())?;
    let step_progress = if steps.is_empty() {
        None
    } else {
        Some(StepProgressSummary::from_steps(&task_id, &steps))
    };
    let history = state
        .task_repo
        .get_status_history(&task_id)
        .await
        .map_err(|error| error.to_string())?;
    let elapsed_seconds =
        elapsed_seconds_for_status(&history, task.internal_status, chrono::Utc::now());
    let trigger_origin = get_trigger_origin(task);

    Ok(build_running_process(
        task,
        step_progress,
        elapsed_seconds,
        trigger_origin,
    ))
}

fn task_runtime_label(source: AgentConversationRuntimeSource, status: InternalStatus) -> String {
    match source {
        AgentConversationRuntimeSource::TaskExecution if status == InternalStatus::ReExecuting => {
            "Re-executing".to_string()
        }
        AgentConversationRuntimeSource::TaskExecution => "Executing".to_string(),
        AgentConversationRuntimeSource::Review => "Reviewing".to_string(),
        AgentConversationRuntimeSource::Merge => "Merging".to_string(),
        _ => "Agent running".to_string(),
    }
}

async fn add_task_runtime_items(
    state: &AppState,
    service: &dyn ChatService,
    runtime: &mut AgentConversationRuntimeStatus,
    workspace: &AgentConversationWorkspace,
) -> Result<(), String> {
    let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
        return Ok(());
    };
    let Some(plan_branch) = state
        .plan_branch_repo
        .get_by_id(plan_branch_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let Some(execution_plan_id) = plan_branch.execution_plan_id.as_ref() else {
        return Ok(());
    };

    let tasks = state
        .task_repo
        .list_paginated(
            &workspace.project_id,
            None,
            0,
            1000,
            false,
            None,
            Some(execution_plan_id.as_str()),
            None,
        )
        .await
        .map_err(|error| error.to_string())?;
    if tasks.is_empty() {
        return Ok(());
    }

    let task_id_strings = tasks
        .iter()
        .map(|task| task.id.as_str().to_string())
        .collect::<Vec<_>>();
    let execution_states = service
        .get_agent_running_states(ChatContextType::TaskExecution, &task_id_strings)
        .await;
    let review_states = service
        .get_agent_running_states(ChatContextType::Review, &task_id_strings)
        .await;
    let merge_states = service
        .get_agent_running_states(ChatContextType::Merge, &task_id_strings)
        .await;

    for task in tasks {
        let candidates = [
            (
                AgentConversationRuntimeSource::Merge,
                ChatContextType::Merge,
                &merge_states,
            ),
            (
                AgentConversationRuntimeSource::Review,
                ChatContextType::Review,
                &review_states,
            ),
            (
                AgentConversationRuntimeSource::TaskExecution,
                ChatContextType::TaskExecution,
                &execution_states,
            ),
        ];
        let task_id = task.id.as_str().to_string();
        for (source, context_type, states) in candidates {
            if !context_matches_running_status(context_type, task.internal_status) {
                continue;
            }
            let running_state = states
                .get(&task_id)
                .copied()
                .unwrap_or_else(idle_agent_running_state);
            if !running_state.is_running {
                continue;
            }

            let running_process = build_task_runtime_process(state, &task).await?;
            runtime.items.push(AgentConversationRuntimeItem {
                source,
                context_type: context_type.to_string(),
                context_id: task_id.clone(),
                label: task_runtime_label(source, task.internal_status),
                title: task.title.clone(),
                agent_status: running_state.agent_status,
                task_id: Some(task_id.clone()),
                internal_status: Some(task.internal_status.as_str().to_string()),
                running_process: Some(running_process),
                ideation_session: None,
                parent_session_id: None,
                child_session_id: None,
                conversation_id: None,
            });
            break;
        }
    }

    Ok(())
}

async fn add_workspace_runtime_item(
    state: &AppState,
    execution_state: &ExecutionState,
    runtime: &mut AgentConversationRuntimeStatus,
    conversation_id: &str,
) -> Result<(), String> {
    let Some(running_state) = direct_agent_running_state_for_context(
        state,
        execution_state,
        ChatContextType::Project,
        conversation_id,
    )
    .await?
    else {
        return Ok(());
    };
    if !running_state.is_running {
        return Ok(());
    }

    runtime.items.push(AgentConversationRuntimeItem {
        source: AgentConversationRuntimeSource::Workspace,
        context_type: ChatContextType::Project.to_string(),
        context_id: conversation_id.to_string(),
        label: "Agent running".to_string(),
        title: "Workspace chat".to_string(),
        agent_status: running_state.agent_status,
        task_id: None,
        internal_status: None,
        running_process: None,
        ideation_session: None,
        parent_session_id: None,
        child_session_id: None,
        conversation_id: Some(conversation_id.to_string()),
    });

    Ok(())
}

async fn add_workspace_review_runtime_item(
    state: &AppState,
    execution_state: &ExecutionState,
    runtime: &mut AgentConversationRuntimeStatus,
    workspace: &AgentConversationWorkspace,
) -> Result<(), String> {
    let Some(monitor) = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    if monitor.status != AgentWorkspaceReviewMonitorStatus::Reviewing {
        return Ok(());
    }

    let Some(review_conversation_id) = monitor.review_conversation_id.as_ref() else {
        return Ok(());
    };
    let review_conversation_id = review_conversation_id.as_str();
    let running_state = match direct_agent_running_state_for_context(
        state,
        execution_state,
        ChatContextType::Project,
        &review_conversation_id,
    )
    .await?
    {
        Some(state) if state.is_running => Some(state),
        _ => match monitor.last_run_id.as_deref() {
            Some(run_id) => state
                .agent_run_repo
                .get_by_id(&AgentRunId::from_string(run_id))
                .await
                .map_err(|error| error.to_string())?
                .and_then(|run| {
                    (run.status == AgentRunStatus::Running)
                        .then(|| running_state_from_run_status_and_idle(Some(run.status), false))
                }),
            None => None,
        },
    };

    let Some(running_state) = running_state else {
        return Ok(());
    };

    let title = state
        .chat_conversation_repo
        .get_by_id(&ChatConversationId::from_string(
            review_conversation_id.clone(),
        ))
        .await
        .map_err(|error| error.to_string())?
        .and_then(|conversation| conversation.title)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Review".to_string());

    runtime.items.push(AgentConversationRuntimeItem {
        source: AgentConversationRuntimeSource::WorkspaceReview,
        context_type: ChatContextType::Project.to_string(),
        context_id: review_conversation_id.clone(),
        label: "Reviewing".to_string(),
        title,
        agent_status: running_state.agent_status,
        task_id: None,
        internal_status: Some(monitor.status.to_string()),
        running_process: None,
        ideation_session: None,
        parent_session_id: None,
        child_session_id: None,
        conversation_id: Some(review_conversation_id),
    });

    Ok(())
}

async fn add_associated_runtime_items(
    state: &AppState,
    execution_state: &ExecutionState,
    service: &dyn ChatService,
    runtime: &mut AgentConversationRuntimeStatus,
    workspace: &AgentConversationWorkspace,
) -> Result<(), String> {
    if let Some(session_id) = workspace.linked_ideation_session_id.as_ref() {
        add_ideation_runtime_item(
            state,
            execution_state,
            service,
            runtime,
            session_id,
            AgentConversationRuntimeSource::Ideation,
            None,
        )
        .await?;

        let verification_children = state
            .ideation_session_repo
            .get_verification_children(session_id)
            .await
            .map_err(|error| error.to_string())?;
        for child in verification_children {
            add_ideation_runtime_item(
                state,
                execution_state,
                service,
                runtime,
                &child.id,
                AgentConversationRuntimeSource::Verification,
                Some(session_id),
            )
            .await?;
        }
    }

    add_workspace_review_runtime_item(state, execution_state, runtime, workspace).await?;
    add_task_runtime_items(state, service, runtime, workspace).await
}

#[tauri::command]
pub async fn get_agent_conversation_runtime_statuses(
    conversation_ids: Vec<String>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<HashMap<String, AgentConversationRuntimeStatus>, String> {
    get_agent_conversation_runtime_statuses_for_app_state(
        &state,
        Arc::clone(execution_state.inner()),
        conversation_ids,
    )
    .await
}

#[doc(hidden)]
pub async fn get_agent_conversation_runtime_statuses_for_app_state(
    state: &AppState,
    execution_state: Arc<ExecutionState>,
    conversation_ids: Vec<String>,
) -> Result<HashMap<String, AgentConversationRuntimeStatus>, String> {
    let mut requested = Vec::new();
    let mut seen = HashSet::new();
    for conversation_id in conversation_ids {
        let conversation_id = conversation_id.trim().to_string();
        if conversation_id.is_empty() || !seen.insert(conversation_id.clone()) {
            continue;
        }
        requested.push(conversation_id);
    }

    let service = state.build_chat_service_with_execution_state(Arc::clone(&execution_state));
    let mut response = HashMap::new();

    for conversation_id in requested {
        let mut runtime = AgentConversationRuntimeStatus::idle(conversation_id.clone());
        add_workspace_runtime_item(state, &execution_state, &mut runtime, &conversation_id).await?;

        let workspace_id = ChatConversationId::from_string(conversation_id.clone());
        if let Some(workspace) = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace_id)
            .await
            .map_err(|error| error.to_string())?
        {
            add_associated_runtime_items(
                state,
                &execution_state,
                &service,
                &mut runtime,
                &workspace,
            )
            .await?;
        }

        runtime.finalize();
        response.insert(conversation_id, runtime);
    }

    Ok(response)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationRuntimeIndexGroup {
    Main,
    IdeationVerification,
    Pipeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationRuntimeIndexKind {
    Workspace,
    WorkspaceReview,
    Ideation,
    Verification,
    Delegation,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationRuntimeLifecycle {
    Planned,
    Queued,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
    Blocked,
    Dropped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationRuntimeIndexMode {
    Chat,
    Agent,
    Plan,
    PrReview,
    Ideation,
    Automation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationRuntimeIndexRow {
    pub id: String,
    pub group: AgentConversationRuntimeIndexGroup,
    pub kind: AgentConversationRuntimeIndexKind,
    pub lifecycle: AgentConversationRuntimeLifecycle,
    pub status_label: String,
    pub title: String,
    pub mode: Option<AgentConversationRuntimeIndexMode>,
    pub order_index: usize,
    pub order_started_at: Option<String>,
    pub completed_at: Option<String>,
    pub conversation_id: Option<String>,
    pub context_type: Option<String>,
    pub context_id: Option<String>,
    pub task_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub child_session_id: Option<String>,
    pub provider_harness: Option<String>,
    pub provider_session_id: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationRuntimeIndexResponse {
    pub conversation_id: String,
    pub rows: Vec<AgentConversationRuntimeIndexRow>,
}

#[derive(Debug, Clone)]
struct RuntimeIndexDraftRow {
    row: AgentConversationRuntimeIndexRow,
    order_started_at: Option<chrono::DateTime<chrono::Utc>>,
    fallback_order: chrono::DateTime<chrono::Utc>,
}

impl RuntimeIndexDraftRow {
    fn new(
        row: AgentConversationRuntimeIndexRow,
        order_started_at: Option<chrono::DateTime<chrono::Utc>>,
        fallback_order: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            row,
            order_started_at,
            fallback_order,
        }
    }
}

fn runtime_index_mode(mode: AgentConversationWorkspaceMode) -> AgentConversationRuntimeIndexMode {
    match mode {
        AgentConversationWorkspaceMode::Chat => AgentConversationRuntimeIndexMode::Chat,
        AgentConversationWorkspaceMode::Edit => AgentConversationRuntimeIndexMode::Agent,
        AgentConversationWorkspaceMode::Plan => AgentConversationRuntimeIndexMode::Plan,
        AgentConversationWorkspaceMode::Tasks
        | AgentConversationWorkspaceMode::Autopilot
        | AgentConversationWorkspaceMode::Ideation => AgentConversationRuntimeIndexMode::Ideation,
        AgentConversationWorkspaceMode::ReviewPr => AgentConversationRuntimeIndexMode::PrReview,
        AgentConversationWorkspaceMode::Automation => AgentConversationRuntimeIndexMode::Automation,
        AgentConversationWorkspaceMode::PersonaBuilder => {
            AgentConversationRuntimeIndexMode::Automation
        }
    }
}

fn lifecycle_label(lifecycle: AgentConversationRuntimeLifecycle) -> &'static str {
    match lifecycle {
        AgentConversationRuntimeLifecycle::Planned => "Planned",
        AgentConversationRuntimeLifecycle::Queued => "Queued",
        AgentConversationRuntimeLifecycle::Running => "Running",
        AgentConversationRuntimeLifecycle::Waiting => "Waiting",
        AgentConversationRuntimeLifecycle::Completed => "Completed",
        AgentConversationRuntimeLifecycle::Failed => "Failed",
        AgentConversationRuntimeLifecycle::Cancelled => "Cancelled",
        AgentConversationRuntimeLifecycle::Blocked => "Blocked",
        AgentConversationRuntimeLifecycle::Dropped => "Dropped",
    }
}

fn lifecycle_from_agent_run(
    run: Option<&AgentRun>,
    running_state: Option<AgentRunningState>,
    fallback: AgentConversationRuntimeLifecycle,
) -> AgentConversationRuntimeLifecycle {
    match run.map(|run| run.status) {
        Some(AgentRunStatus::Running) => {
            if running_state
                .map(|state| state.agent_status == AgentRuntimeStatus::WaitingForInput)
                .unwrap_or(false)
            {
                AgentConversationRuntimeLifecycle::Waiting
            } else {
                AgentConversationRuntimeLifecycle::Running
            }
        }
        Some(AgentRunStatus::Completed) => AgentConversationRuntimeLifecycle::Completed,
        Some(AgentRunStatus::Failed) => AgentConversationRuntimeLifecycle::Failed,
        Some(AgentRunStatus::Cancelled) => AgentConversationRuntimeLifecycle::Cancelled,
        None => match running_state {
            Some(state) if state.is_running => {
                if state.agent_status == AgentRuntimeStatus::WaitingForInput {
                    AgentConversationRuntimeLifecycle::Waiting
                } else {
                    AgentConversationRuntimeLifecycle::Running
                }
            }
            _ => fallback,
        },
    }
}

fn provider_harness_for_row(
    run: Option<&AgentRun>,
    conversation: Option<&ChatConversation>,
) -> Option<String> {
    run.and_then(|run| run.harness)
        .or_else(|| conversation.and_then(|conversation| conversation.provider_harness))
        .map(|harness| harness.to_string())
}

fn provider_session_for_row(
    run: Option<&AgentRun>,
    conversation: Option<&ChatConversation>,
) -> Option<String> {
    run.and_then(|run| run.provider_session_id.clone())
        .or_else(|| conversation.and_then(|conversation| conversation.provider_session_id.clone()))
}

async fn latest_runtime_conversation_and_run(
    state: &AppState,
    context_type: ChatContextType,
    context_id: &str,
) -> Result<(Option<ChatConversation>, Option<AgentRun>), String> {
    let conversation = state
        .chat_conversation_repo
        .get_active_for_context(context_type, context_id)
        .await
        .map_err(|error| error.to_string())?;
    let run = match conversation.as_ref() {
        Some(conversation) => state
            .agent_run_repo
            .get_latest_for_conversation(&conversation.id)
            .await
            .map_err(|error| error.to_string())?,
        None => None,
    };
    Ok((conversation, run))
}

async fn runtime_index_row_for_main_workspace(
    state: &AppState,
    execution_state: &ExecutionState,
    conversation_id: &ChatConversationId,
    workspace: Option<&AgentConversationWorkspace>,
) -> Result<RuntimeIndexDraftRow, String> {
    let conversation = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let run = state
        .agent_run_repo
        .get_latest_for_conversation(conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let running_state = direct_agent_running_state_for_context(
        state,
        execution_state,
        ChatContextType::Project,
        &conversation_id.as_str(),
    )
    .await?;
    let lifecycle = lifecycle_from_agent_run(
        run.as_ref(),
        running_state,
        AgentConversationRuntimeLifecycle::Planned,
    );
    let title = conversation
        .as_ref()
        .and_then(|conversation| conversation.title.clone())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Workspace chat".to_string());
    let fallback_order = run
        .as_ref()
        .map(|run| run.started_at)
        .or_else(|| workspace.map(|workspace| workspace.created_at))
        .or_else(|| {
            conversation
                .as_ref()
                .map(|conversation| conversation.created_at)
        })
        .unwrap_or_else(chrono::Utc::now);
    let mode = workspace
        .map(|workspace| runtime_index_mode(workspace.mode))
        .or_else(|| {
            conversation
                .as_ref()
                .and_then(|conversation| conversation.agent_mode)
                .map(runtime_index_mode)
        });

    Ok(RuntimeIndexDraftRow::new(
        AgentConversationRuntimeIndexRow {
            id: format!("workspace:{}", conversation_id.as_str()),
            group: AgentConversationRuntimeIndexGroup::Main,
            kind: AgentConversationRuntimeIndexKind::Workspace,
            lifecycle,
            status_label: lifecycle_label(lifecycle).to_string(),
            title,
            mode,
            order_index: 0,
            order_started_at: run.as_ref().map(|run| run.started_at.to_rfc3339()),
            completed_at: run.as_ref().and_then(|run| {
                run.completed_at
                    .map(|completed_at| completed_at.to_rfc3339())
            }),
            conversation_id: Some(conversation_id.as_str()),
            context_type: Some(ChatContextType::Project.to_string()),
            context_id: Some(conversation_id.as_str()),
            task_id: None,
            agent_run_id: run.as_ref().map(|run| run.id.as_str()),
            parent_session_id: None,
            child_session_id: None,
            provider_harness: provider_harness_for_row(run.as_ref(), conversation.as_ref()),
            provider_session_id: provider_session_for_row(run.as_ref(), conversation.as_ref()),
            error_message: run.as_ref().and_then(|run| run.error_message.clone()),
        },
        run.as_ref().map(|run| run.started_at),
        fallback_order,
    ))
}

async fn runtime_index_row_for_ideation_session(
    state: &AppState,
    execution_state: &ExecutionState,
    session: &IdeationSession,
    kind: AgentConversationRuntimeIndexKind,
    parent_session_id: Option<&IdeationSessionId>,
) -> Result<RuntimeIndexDraftRow, String> {
    let session_id = session.id.as_str().to_string();
    let (conversation, run) =
        latest_runtime_conversation_and_run(state, ChatContextType::Ideation, &session_id).await?;
    let running_state = direct_agent_running_state_for_context(
        state,
        execution_state,
        ChatContextType::Ideation,
        &session_id,
    )
    .await?;
    let fallback_lifecycle = match session.status {
        crate::domain::entities::IdeationSessionStatus::Archived
        | crate::domain::entities::IdeationSessionStatus::Accepted => {
            AgentConversationRuntimeLifecycle::Completed
        }
        crate::domain::entities::IdeationSessionStatus::Active => {
            AgentConversationRuntimeLifecycle::Planned
        }
    };
    let lifecycle = lifecycle_from_agent_run(run.as_ref(), running_state, fallback_lifecycle);
    let title = session
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| {
            if kind == AgentConversationRuntimeIndexKind::Verification {
                "Verification run".to_string()
            } else {
                "Ideation run".to_string()
            }
        });
    let row_id = match kind {
        AgentConversationRuntimeIndexKind::Verification => format!(
            "verification:{}:{}",
            parent_session_id
                .map(|id| id.as_str().to_string())
                .unwrap_or_default(),
            session_id
        ),
        _ => format!("ideation:{session_id}"),
    };

    Ok(RuntimeIndexDraftRow::new(
        AgentConversationRuntimeIndexRow {
            id: row_id,
            group: AgentConversationRuntimeIndexGroup::IdeationVerification,
            kind,
            lifecycle,
            status_label: lifecycle_label(lifecycle).to_string(),
            title,
            mode: None,
            order_index: 0,
            order_started_at: run.as_ref().map(|run| run.started_at.to_rfc3339()),
            completed_at: run.as_ref().and_then(|run| {
                run.completed_at
                    .map(|completed_at| completed_at.to_rfc3339())
            }),
            conversation_id: conversation
                .as_ref()
                .map(|conversation| conversation.id.as_str()),
            context_type: Some(ChatContextType::Ideation.to_string()),
            context_id: Some(session_id.clone()),
            task_id: None,
            agent_run_id: run.as_ref().map(|run| run.id.as_str()),
            parent_session_id: parent_session_id.map(|id| id.as_str().to_string()),
            child_session_id: (kind == AgentConversationRuntimeIndexKind::Verification)
                .then_some(session_id),
            provider_harness: provider_harness_for_row(run.as_ref(), conversation.as_ref()),
            provider_session_id: provider_session_for_row(run.as_ref(), conversation.as_ref()),
            error_message: run.as_ref().and_then(|run| run.error_message.clone()),
        },
        run.as_ref().map(|run| run.started_at),
        run.as_ref()
            .map(|run| run.started_at)
            .unwrap_or(session.created_at),
    ))
}

fn workspace_review_fallback_lifecycle(
    status: AgentWorkspaceReviewMonitorStatus,
) -> AgentConversationRuntimeLifecycle {
    match status {
        AgentWorkspaceReviewMonitorStatus::Reviewing => AgentConversationRuntimeLifecycle::Running,
        AgentWorkspaceReviewMonitorStatus::Ready => AgentConversationRuntimeLifecycle::Queued,
        AgentWorkspaceReviewMonitorStatus::Blocked => AgentConversationRuntimeLifecycle::Blocked,
        AgentWorkspaceReviewMonitorStatus::Idle => AgentConversationRuntimeLifecycle::Planned,
    }
}

async fn maybe_runtime_index_row_for_workspace_review(
    state: &AppState,
    execution_state: &ExecutionState,
    workspace: &AgentConversationWorkspace,
) -> Result<Option<RuntimeIndexDraftRow>, String> {
    let Some(monitor) = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if monitor.status == AgentWorkspaceReviewMonitorStatus::Idle
        && monitor.review_conversation_id.is_none()
        && monitor.last_run_id.is_none()
    {
        return Ok(None);
    }

    let conversation = match monitor.review_conversation_id.as_ref() {
        Some(conversation_id) => state
            .chat_conversation_repo
            .get_by_id(conversation_id)
            .await
            .map_err(|error| error.to_string())?,
        None => None,
    };
    let run = match conversation.as_ref() {
        Some(conversation) => state
            .agent_run_repo
            .get_latest_for_conversation(&conversation.id)
            .await
            .map_err(|error| error.to_string())?,
        None => match monitor.last_run_id.as_deref() {
            Some(run_id) => state
                .agent_run_repo
                .get_by_id(&AgentRunId::from_string(run_id))
                .await
                .map_err(|error| error.to_string())?,
            None => None,
        },
    };
    let running_state = match monitor.review_conversation_id.as_ref() {
        Some(conversation_id) => {
            direct_agent_running_state_for_context(
                state,
                execution_state,
                ChatContextType::Project,
                &conversation_id.as_str(),
            )
            .await?
        }
        None => None,
    };
    let lifecycle = lifecycle_from_agent_run(
        run.as_ref(),
        running_state,
        workspace_review_fallback_lifecycle(monitor.status),
    );
    let title = conversation
        .as_ref()
        .and_then(|conversation| conversation.title.clone())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Review workspace changes".to_string());
    let context_id = monitor
        .review_conversation_id
        .as_ref()
        .map(|id| id.as_str())
        .unwrap_or_else(|| workspace.conversation_id.as_str());
    let fallback_order = run
        .as_ref()
        .map(|run| run.started_at)
        .unwrap_or(monitor.created_at);

    Ok(Some(RuntimeIndexDraftRow::new(
        AgentConversationRuntimeIndexRow {
            id: format!("workspace_review:{context_id}"),
            group: AgentConversationRuntimeIndexGroup::IdeationVerification,
            kind: AgentConversationRuntimeIndexKind::WorkspaceReview,
            lifecycle,
            status_label: lifecycle_label(lifecycle).to_string(),
            title,
            mode: None,
            order_index: 0,
            order_started_at: run.as_ref().map(|run| run.started_at.to_rfc3339()),
            completed_at: run.as_ref().and_then(|run| {
                run.completed_at
                    .map(|completed_at| completed_at.to_rfc3339())
            }),
            conversation_id: monitor
                .review_conversation_id
                .as_ref()
                .map(|id| id.as_str()),
            context_type: Some(ChatContextType::Project.to_string()),
            context_id: Some(context_id),
            task_id: None,
            agent_run_id: run.as_ref().map(|run| run.id.as_str()),
            parent_session_id: None,
            child_session_id: None,
            provider_harness: provider_harness_for_row(run.as_ref(), conversation.as_ref()),
            provider_session_id: provider_session_for_row(run.as_ref(), conversation.as_ref()),
            error_message: run
                .as_ref()
                .and_then(|run| run.error_message.clone())
                .or_else(|| monitor.last_error.clone()),
        },
        run.as_ref().map(|run| run.started_at),
        fallback_order,
    )))
}

fn delegated_lifecycle(status: &str) -> AgentConversationRuntimeLifecycle {
    match status {
        "running" => AgentConversationRuntimeLifecycle::Running,
        "queued" => AgentConversationRuntimeLifecycle::Queued,
        "completed" | "done" => AgentConversationRuntimeLifecycle::Completed,
        "failed" | "error" => AgentConversationRuntimeLifecycle::Failed,
        "cancelled" | "canceled" => AgentConversationRuntimeLifecycle::Cancelled,
        "blocked" => AgentConversationRuntimeLifecycle::Blocked,
        _ => AgentConversationRuntimeLifecycle::Planned,
    }
}

async fn add_delegated_runtime_index_rows(
    state: &AppState,
    rows: &mut Vec<RuntimeIndexDraftRow>,
    parent_context_type: ChatContextType,
    parent_context_id: &str,
) -> Result<(), String> {
    let delegated_sessions = state
        .delegated_session_repo
        .get_by_parent_context(&parent_context_type.to_string(), parent_context_id)
        .await
        .map_err(|error| error.to_string())?;
    for session in delegated_sessions {
        let session_id = session.id.as_str().to_string();
        let (conversation, run) =
            latest_runtime_conversation_and_run(state, ChatContextType::Delegation, &session_id)
                .await?;
        let lifecycle = lifecycle_from_agent_run(
            run.as_ref(),
            None,
            delegated_lifecycle(session.status.as_str()),
        );
        let title = session
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| session.agent_name.clone());
        rows.push(RuntimeIndexDraftRow::new(
            AgentConversationRuntimeIndexRow {
                id: format!("delegation:{session_id}"),
                group: AgentConversationRuntimeIndexGroup::IdeationVerification,
                kind: AgentConversationRuntimeIndexKind::Delegation,
                lifecycle,
                status_label: lifecycle_label(lifecycle).to_string(),
                title,
                mode: None,
                order_index: 0,
                order_started_at: run.as_ref().map(|run| run.started_at.to_rfc3339()),
                completed_at: run
                    .as_ref()
                    .and_then(|run| {
                        run.completed_at
                            .map(|completed_at| completed_at.to_rfc3339())
                    })
                    .or_else(|| {
                        session
                            .completed_at
                            .map(|completed_at| completed_at.to_rfc3339())
                    }),
                conversation_id: conversation
                    .as_ref()
                    .map(|conversation| conversation.id.as_str()),
                context_type: Some(ChatContextType::Delegation.to_string()),
                context_id: Some(session_id.clone()),
                task_id: None,
                agent_run_id: run.as_ref().map(|run| run.id.as_str()),
                parent_session_id: None,
                child_session_id: None,
                provider_harness: provider_harness_for_row(run.as_ref(), conversation.as_ref())
                    .or_else(|| Some(session.harness.to_string())),
                provider_session_id: provider_session_for_row(run.as_ref(), conversation.as_ref())
                    .or_else(|| session.provider_session_id.clone()),
                error_message: run
                    .as_ref()
                    .and_then(|run| run.error_message.clone())
                    .or_else(|| session.error.clone()),
            },
            run.as_ref().map(|run| run.started_at),
            run.as_ref()
                .map(|run| run.started_at)
                .unwrap_or(session.created_at),
        ));
    }
    Ok(())
}

fn task_runtime_context_type_for_index(status: InternalStatus) -> ChatContextType {
    match status {
        InternalStatus::Reviewing
        | InternalStatus::PendingReview
        | InternalStatus::ReviewPassed
        | InternalStatus::Escalated
        | InternalStatus::RevisionNeeded => ChatContextType::Review,
        InternalStatus::PendingMerge
        | InternalStatus::Merging
        | InternalStatus::WaitingOnPr
        | InternalStatus::MergeIncomplete
        | InternalStatus::MergeConflict
        | InternalStatus::Merged
        | InternalStatus::Approved => ChatContextType::Merge,
        _ => ChatContextType::TaskExecution,
    }
}

fn task_lifecycle(
    status: InternalStatus,
    run: Option<&AgentRun>,
) -> AgentConversationRuntimeLifecycle {
    if matches!(run.map(|run| run.status), Some(AgentRunStatus::Running)) {
        return AgentConversationRuntimeLifecycle::Running;
    }
    match status {
        InternalStatus::Backlog => AgentConversationRuntimeLifecycle::Planned,
        InternalStatus::Ready
        | InternalStatus::PendingReview
        | InternalStatus::QaPassed
        | InternalStatus::PendingMerge
        | InternalStatus::ReviewPassed
        | InternalStatus::Approved => AgentConversationRuntimeLifecycle::Queued,
        InternalStatus::Blocked
        | InternalStatus::MergeConflict
        | InternalStatus::BranchUpdateBlocked => AgentConversationRuntimeLifecycle::Blocked,
        InternalStatus::Executing
        | InternalStatus::QaRefining
        | InternalStatus::QaTesting
        | InternalStatus::Reviewing
        | InternalStatus::ReExecuting
        | InternalStatus::Merging
        | InternalStatus::WaitingOnPr
        | InternalStatus::UpdatingPlanBranch
        | InternalStatus::UpdatingTaskBranch => AgentConversationRuntimeLifecycle::Running,
        InternalStatus::RevisionNeeded => AgentConversationRuntimeLifecycle::Blocked,
        InternalStatus::Merged => AgentConversationRuntimeLifecycle::Completed,
        InternalStatus::Failed | InternalStatus::QaFailed | InternalStatus::MergeIncomplete => {
            AgentConversationRuntimeLifecycle::Failed
        }
        InternalStatus::Cancelled | InternalStatus::Stopped => {
            AgentConversationRuntimeLifecycle::Cancelled
        }
        InternalStatus::Paused => AgentConversationRuntimeLifecycle::Waiting,
        InternalStatus::Escalated => AgentConversationRuntimeLifecycle::Waiting,
    }
}

fn task_status_label(
    status: InternalStatus,
    lifecycle: AgentConversationRuntimeLifecycle,
) -> String {
    match status {
        InternalStatus::Reviewing
        | InternalStatus::PendingReview
        | InternalStatus::ReviewPassed => "Reviewing".to_string(),
        InternalStatus::ReExecuting | InternalStatus::RevisionNeeded => "Revising".to_string(),
        InternalStatus::Merging | InternalStatus::PendingMerge | InternalStatus::WaitingOnPr => {
            "Merging".to_string()
        }
        _ => lifecycle_label(lifecycle).to_string(),
    }
}

fn task_order_started_at(
    task: &Task,
    history: Option<&Vec<crate::domain::repositories::StatusTransition>>,
) -> chrono::DateTime<chrono::Utc> {
    history
        .and_then(|entries| {
            entries
                .iter()
                .filter(|entry| {
                    matches!(
                        entry.to,
                        InternalStatus::Executing
                            | InternalStatus::ReExecuting
                            | InternalStatus::Reviewing
                            | InternalStatus::Merging
                            | InternalStatus::WaitingOnPr
                    )
                })
                .map(|entry| entry.timestamp)
                .min()
        })
        .unwrap_or(task.created_at)
}

async fn add_task_runtime_index_rows(
    state: &AppState,
    rows: &mut Vec<RuntimeIndexDraftRow>,
    workspace: &AgentConversationWorkspace,
) -> Result<(), String> {
    let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
        return Ok(());
    };
    let Some(plan_branch) = state
        .plan_branch_repo
        .get_by_id(plan_branch_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let Some(execution_plan_id) = plan_branch.execution_plan_id.as_ref() else {
        return Ok(());
    };

    let tasks = state
        .task_repo
        .list_paginated(
            &workspace.project_id,
            None,
            0,
            1000,
            false,
            None,
            Some(execution_plan_id.as_str()),
            None,
        )
        .await
        .map_err(|error| error.to_string())?;
    let task_ids = tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>();
    let history_by_task = state
        .task_repo
        .get_status_history_batch(&task_ids)
        .await
        .map_err(|error| error.to_string())?;

    for task in tasks {
        let context_type = task_runtime_context_type_for_index(task.internal_status);
        let task_id = task.id.as_str().to_string();
        let (conversation, run) =
            latest_runtime_conversation_and_run(state, context_type, &task_id).await?;
        let lifecycle = task_lifecycle(task.internal_status, run.as_ref());
        let order_started_at = task_order_started_at(&task, history_by_task.get(&task.id));
        rows.push(RuntimeIndexDraftRow::new(
            AgentConversationRuntimeIndexRow {
                id: format!("task:{task_id}"),
                group: AgentConversationRuntimeIndexGroup::Pipeline,
                kind: AgentConversationRuntimeIndexKind::Task,
                lifecycle,
                status_label: task_status_label(task.internal_status, lifecycle),
                title: task.title.clone(),
                mode: None,
                order_index: 0,
                order_started_at: Some(order_started_at.to_rfc3339()),
                completed_at: task
                    .completed_at
                    .map(|completed_at| completed_at.to_rfc3339()),
                conversation_id: conversation
                    .as_ref()
                    .map(|conversation| conversation.id.as_str()),
                context_type: Some(context_type.to_string()),
                context_id: Some(task_id.clone()),
                task_id: Some(task_id),
                agent_run_id: run.as_ref().map(|run| run.id.as_str()),
                parent_session_id: None,
                child_session_id: None,
                provider_harness: provider_harness_for_row(run.as_ref(), conversation.as_ref()),
                provider_session_id: provider_session_for_row(run.as_ref(), conversation.as_ref()),
                error_message: run.as_ref().and_then(|run| run.error_message.clone()),
            },
            Some(order_started_at),
            order_started_at,
        ));
    }

    Ok(())
}

fn runtime_index_group_rank(group: AgentConversationRuntimeIndexGroup) -> u8 {
    match group {
        AgentConversationRuntimeIndexGroup::Main => 0,
        AgentConversationRuntimeIndexGroup::IdeationVerification => 1,
        AgentConversationRuntimeIndexGroup::Pipeline => 2,
    }
}

fn finalize_runtime_index_rows(
    mut rows: Vec<RuntimeIndexDraftRow>,
) -> Vec<AgentConversationRuntimeIndexRow> {
    rows.sort_by(|left, right| {
        runtime_index_group_rank(left.row.group)
            .cmp(&runtime_index_group_rank(right.row.group))
            .then_with(|| {
                left.order_started_at
                    .unwrap_or(left.fallback_order)
                    .cmp(&right.order_started_at.unwrap_or(right.fallback_order))
            })
            .then_with(|| left.row.id.cmp(&right.row.id))
    });
    for (index, row) in rows.iter_mut().enumerate() {
        row.row.order_index = index;
        if row.row.order_started_at.is_none() {
            row.row.order_started_at = Some(row.fallback_order.to_rfc3339());
        }
    }
    rows.into_iter().map(|row| row.row).collect()
}

#[tauri::command]
pub async fn get_agent_conversation_runtime_index(
    conversation_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<AgentConversationRuntimeIndexResponse, String> {
    get_agent_conversation_runtime_index_for_app_state(
        &state,
        execution_state.inner().as_ref(),
        conversation_id,
    )
    .await
}

#[doc(hidden)]
pub async fn get_agent_conversation_runtime_index_for_app_state(
    state: &AppState,
    execution_state: &ExecutionState,
    conversation_id: String,
) -> Result<AgentConversationRuntimeIndexResponse, String> {
    let conversation_id = conversation_id.trim().to_string();
    if conversation_id.is_empty() {
        return Err("conversationId is required".to_string());
    }
    let conversation_id_typed = ChatConversationId::from_string(conversation_id.clone());
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id_typed)
        .await
        .map_err(|error| error.to_string())?;
    let mut rows = vec![
        runtime_index_row_for_main_workspace(
            state,
            execution_state,
            &conversation_id_typed,
            workspace.as_ref(),
        )
        .await?,
    ];

    if let Some(workspace) = workspace.as_ref() {
        if let Some(session_id) = workspace.linked_ideation_session_id.as_ref() {
            if let Some(session) = state
                .ideation_session_repo
                .get_by_id(session_id)
                .await
                .map_err(|error| error.to_string())?
            {
                rows.push(
                    runtime_index_row_for_ideation_session(
                        state,
                        execution_state,
                        &session,
                        AgentConversationRuntimeIndexKind::Ideation,
                        None,
                    )
                    .await?,
                );

                let verification_children = state
                    .ideation_session_repo
                    .get_children(session_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .filter(|child| {
                        child.session_purpose
                            == crate::domain::entities::SessionPurpose::Verification
                    })
                    .collect::<Vec<_>>();
                for child in verification_children {
                    rows.push(
                        runtime_index_row_for_ideation_session(
                            state,
                            execution_state,
                            &child,
                            AgentConversationRuntimeIndexKind::Verification,
                            Some(session_id),
                        )
                        .await?,
                    );
                }

                add_delegated_runtime_index_rows(
                    state,
                    &mut rows,
                    ChatContextType::Ideation,
                    session_id.as_str(),
                )
                .await?;
            }
        }

        if let Some(review_row) =
            maybe_runtime_index_row_for_workspace_review(state, execution_state, workspace).await?
        {
            rows.push(review_row);
        }

        add_delegated_runtime_index_rows(
            state,
            &mut rows,
            ChatContextType::Project,
            &conversation_id_typed.as_str(),
        )
        .await?;
        add_task_runtime_index_rows(state, &mut rows, workspace).await?;
    }

    Ok(AgentConversationRuntimeIndexResponse {
        conversation_id,
        rows: finalize_runtime_index_rows(rows),
    })
}

/// Input for create_agent_conversation command
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentConversationInput {
    pub context_type: String,
    /// Required for every context except `standalone` (self-keyed; must be
    /// absent/empty for standalone creation requests — see
    /// `STANDALONE_CONTEXT_ID_MUST_BE_ABSENT_ERROR`).
    #[serde(default)]
    pub context_id: Option<String>,
    pub title: Option<String>,
    /// Optional initial mode for pre-send seeded conversations.
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(alias = "capabilityIntent")]
    pub team_intent: Option<TeamIntent>,
}

const STANDALONE_CONTEXT_ID_MUST_BE_ABSENT_ERROR: &str =
    "Standalone conversation creation does not accept a context_id (the backend self-keys it)";
const STANDALONE_CONTEXT_ID_REQUIRED_FOR_CONTEXT_ERROR: &str =
    "context_id is required for this context_type";
const STANDALONE_CONVERSATIONS_DISABLED_ERROR: &str =
    "Standalone conversations are disabled (flag: standalone_conversations)";
const STANDALONE_TEAM_INTENT_REJECTED_ERROR: &str =
    "Team mode is not supported for standalone conversations";

/// Input for update_agent_conversation_title command
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentConversationTitleInput {
    pub conversation_id: String,
    pub title: String,
}

/// Input for update_agent_conversation_coordination_mode command
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentConversationCoordinationModeInput {
    pub conversation_id: String,
    pub coordination_mode: String,
    pub model_override: Option<String>,
}

/// Create a new conversation for a context
#[tauri::command]
pub async fn create_agent_conversation(
    input: CreateAgentConversationInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationResponse, String> {
    use crate::domain::entities::{
        ChatConversation, DelegatedSessionId, IdeationSessionId, ProjectId, TaskId,
    };

    let context_type = parse_context_type(&input.context_type)?;
    let mode = parse_agent_workspace_mode_for_creation(input.mode.as_deref())?;
    if mode == Some(AgentConversationWorkspaceMode::PersonaBuilder)
        && !ChatConversation::is_persona_builder_identity(context_type, mode)
    {
        return Err(
            "PersonaBuilder conversations must use Project or Standalone context".to_string(),
        );
    }
    let coordination_mode = coordination_mode_from_team_intent(input.team_intent.as_ref())?;
    if (context_type == ChatContextType::Standalone
        || mode == Some(AgentConversationWorkspaceMode::PersonaBuilder))
        && coordination_mode != CoordinationMode::Solo
    {
        return Err(
            if mode == Some(AgentConversationWorkspaceMode::PersonaBuilder) {
                "Team mode is not supported for persona builder conversations".to_string()
            } else {
                STANDALONE_TEAM_INTENT_REJECTED_ERROR.to_string()
            },
        );
    }
    crate::application::agent_capability_validation::validate_agent_capability(
        coordination_mode,
        DEFAULT_AGENT_HARNESS,
        &state.agent_capability_gate,
        None,
    )
    .map_err(|error| error.to_string())?;

    let context_id = input
        .context_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut conversation = match context_type {
        ChatContextType::Standalone => {
            if context_id.is_some() {
                return Err(STANDALONE_CONTEXT_ID_MUST_BE_ABSENT_ERROR.to_string());
            }
            if !crate::infrastructure::agents::standalone_conversations_enabled() {
                return Err(STANDALONE_CONVERSATIONS_DISABLED_ERROR.to_string());
            }
            ChatConversation::new_standalone()
        }
        ChatContextType::Ideation => {
            let context_id = context_id
                .ok_or_else(|| STANDALONE_CONTEXT_ID_REQUIRED_FOR_CONTEXT_ERROR.to_string())?;
            ChatConversation::new_ideation(IdeationSessionId::from_string(context_id))
        }
        ChatContextType::Delegation => {
            let context_id = context_id
                .ok_or_else(|| STANDALONE_CONTEXT_ID_REQUIRED_FOR_CONTEXT_ERROR.to_string())?;
            ChatConversation::new_delegation(DelegatedSessionId::from_string(context_id))
        }
        ChatContextType::Task => {
            let context_id = context_id
                .ok_or_else(|| STANDALONE_CONTEXT_ID_REQUIRED_FOR_CONTEXT_ERROR.to_string())?;
            ChatConversation::new_task(TaskId::from_string(context_id.to_string()))
        }
        ChatContextType::Project => {
            let context_id = context_id
                .ok_or_else(|| STANDALONE_CONTEXT_ID_REQUIRED_FOR_CONTEXT_ERROR.to_string())?;
            ChatConversation::new_project(ProjectId::from_string(context_id.to_string()))
        }
        ChatContextType::TaskExecution => {
            let context_id = context_id
                .ok_or_else(|| STANDALONE_CONTEXT_ID_REQUIRED_FOR_CONTEXT_ERROR.to_string())?;
            ChatConversation::new_task_execution(TaskId::from_string(context_id.to_string()))
        }
        ChatContextType::Review => {
            let context_id = context_id
                .ok_or_else(|| STANDALONE_CONTEXT_ID_REQUIRED_FOR_CONTEXT_ERROR.to_string())?;
            ChatConversation::new_review(TaskId::from_string(context_id.to_string()))
        }
        ChatContextType::Merge => {
            let context_id = context_id
                .ok_or_else(|| STANDALONE_CONTEXT_ID_REQUIRED_FOR_CONTEXT_ERROR.to_string())?;
            ChatConversation::new_merge(TaskId::from_string(context_id.to_string()))
        }
        ChatContextType::BranchUpdate => {
            let context_id = context_id
                .ok_or_else(|| STANDALONE_CONTEXT_ID_REQUIRED_FOR_CONTEXT_ERROR.to_string())?;
            ChatConversation::new_branch_update(TaskId::from_string(context_id.to_string()))
        }
    };
    conversation.set_coordination_mode(coordination_mode);
    if let Some(mode) = mode {
        conversation.set_agent_mode(Some(mode));
    }

    if let Some(title) = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        conversation.set_title(title.to_string());
    }

    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .map_err(|e| e.to_string())?;
    if conversation.context_type == ChatContextType::Standalone || conversation.is_persona_builder()
    {
        if let Err(error) = crate::application::standalone_workspace::create_workspace(
            state.app_paths.app_data_dir(),
            &conversation.id.as_str(),
        ) {
            if let Err(cleanup_error) = state.chat_conversation_repo.delete(&conversation.id).await
            {
                tracing::warn!(
                    conversation_id = %conversation.id,
                    %cleanup_error,
                    "Failed to delete seeded conversation after private workspace creation failed"
                );
            } else if let Err(cleanup_error) =
                crate::application::standalone_workspace::remove_workspace_if_present(
                    state.app_paths.app_data_dir(),
                    &conversation.id.as_str(),
                )
            {
                tracing::warn!(
                    conversation_id = %conversation.id,
                    %cleanup_error,
                    "Failed to remove partial private workspace after conversation creation failed"
                );
            }
            return Err(error.to_string());
        }
    }
    agent_conversation_response_for_state(state.inner(), conversation).await
}

/// Update an existing Agent conversation's team coordination mode.
#[tauri::command]
pub async fn update_agent_conversation_coordination_mode(
    input: UpdateAgentConversationCoordinationModeInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationResponse, String> {
    let conversation_id = ChatConversationId::from_string(input.conversation_id);
    let coordination_mode = parse_agent_coordination_mode(&input.coordination_mode)?;

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;
    if conversation.context_type != ChatContextType::Project {
        return Err("Only project agent conversations can change capabilities".to_string());
    }
    if conversation.agent_mode == Some(AgentConversationWorkspaceMode::PersonaBuilder)
        && coordination_mode != CoordinationMode::Solo
    {
        return Err("Team mode is not supported for persona builder conversations".to_string());
    }

    let harness = conversation
        .provider_harness
        .unwrap_or(DEFAULT_AGENT_HARNESS);
    let codex_ultra_supported = (coordination_mode == CoordinationMode::CodexNativeUltra)
        .then(|| {
            crate::application::agent_capability_validation::codex_ultra_support_for_model(
                harness,
                input.model_override.as_deref(),
            )
        })
        .flatten();
    crate::application::agent_capability_validation::validate_agent_capability(
        coordination_mode,
        harness,
        &state.agent_capability_gate,
        codex_ultra_supported,
    )
    .map_err(|error| error.to_string())?;

    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    if state.running_agent_registry.is_running(&running_key).await {
        return Err("Cannot change capabilities while the agent is running".to_string());
    }

    state
        .chat_conversation_repo
        .update_coordination_mode(&conversation_id, coordination_mode)
        .await
        .map_err(|e| e.to_string())?;

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;
    agent_conversation_response_for_state(state.inner(), conversation).await
}

/// Update an existing conversation title.
#[tauri::command]
pub async fn update_agent_conversation_title(
    input: UpdateAgentConversationTitleInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationResponse, String> {
    let mut title = input.title.trim().to_string();
    if title.is_empty() {
        return Err("Conversation title cannot be empty".to_string());
    }

    let conversation_id = ChatConversationId::from_string(input.conversation_id);
    if let Some(jira_key) = primary_jira_key_for_conversation(state.inner(), &conversation_id).await
    {
        title = normalize_title_with_jira_key(&title, &jira_key);
    }
    state
        .chat_conversation_repo
        .update_title(&conversation_id, &title)
        .await
        .map_err(|e| e.to_string())?;
    sync_linked_planning_session_title_from_conversation(state.inner(), &conversation_id, &title)
        .await
        .map_err(|e| e.to_string())?;

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;
    agent_conversation_response_for_state(state.inner(), conversation).await
}

async fn primary_jira_key_for_conversation(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Option<String> {
    state
        .chat_message_repo
        .get_recent_by_conversation_paginated(conversation_id, 50, 0)
        .await
        .ok()?
        .into_iter()
        .find_map(|message| primary_jira_key_from_composer_metadata(message.metadata.as_deref()))
}

async fn primary_clickup_token_for_conversation(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Option<String> {
    let conversation_id = conversation_id.as_str();
    state
        .external_issue_link_service
        .list_ticket_links_for_conversation(&conversation_id)
        .await
        .ok()?
        .into_iter()
        .find(|link| {
            link.provider.eq_ignore_ascii_case("clickup")
                && link.external_kind.eq_ignore_ascii_case("clickup")
        })
        .map(|link| {
            link.external_key
                .filter(|key| !key.trim().is_empty())
                .unwrap_or_else(|| {
                    let id = link.external_id.trim();
                    if id.to_ascii_uppercase().starts_with("CU-") {
                        id.to_string()
                    } else {
                        format!("CU-{id}")
                    }
                })
        })
}

fn normalize_title_with_clickup_token(title: &str, token: &str) -> String {
    let token = token.trim();
    let title = title.trim();
    if token.is_empty() {
        return title.to_string();
    }
    let identity = crate::application::clickup_git_association::ClickUpTaskIdentity::new(
        token,
        Some(token.to_string()),
        None,
    );
    let evidence = crate::application::clickup_git_association::ClickUpGitEvidence {
        title: title.to_string(),
        ..Default::default()
    };
    if crate::application::clickup_git_association::matching_clickup_evidence(&identity, &evidence)
        .is_some()
    {
        title.to_string()
    } else if title.is_empty() {
        token.to_string()
    } else {
        format!("{token}: {title}")
    }
}

#[cfg(test)]
mod tests;
