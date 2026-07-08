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

use crate::application::agent_conversation_fork::{
    fork_agent_conversation as fork_agent_conversation_in_state, AgentConversationForkResult,
};
use crate::application::agent_conversation_archive::{
    archive_agent_conversation_for_state, close_agent_workspace_pr_for_state,
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
    resolve_agent_conversation_workspace_path_for_send,
    resolve_valid_agent_conversation_workspace_path, AgentConversationWorkspaceBaseSelection,
    AgentConversationWorkspacePrAutomationDefaults, AgentConversationWorkspaceSetupMode,
};
use crate::application::agent_conversation_workspace_base::{
    apply_workspace_base_resolution, resolve_workspace_base,
    resolve_workspace_base_from_local_snapshot, BaseResolutionResult, BaseStatus,
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
    external_pr_reconciliation_skip_reason, schedule_agent_workspace_external_pr_reconciliation,
    AgentWorkspaceExternalPrReconciliationDeps, AgentWorkspaceExternalPrReconciliationTrigger,
};
use crate::application::agent_workspace_pr_description::{
    draft_agent_workspace_pr_description, get_or_draft_agent_workspace_pr_description,
    invalidate_agent_workspace_pr_description_cache, AgentWorkspacePrDescriptionCacheKey,
};
use crate::application::agent_workspace_pr_supervision_recovery::{
    pr_supervision_recovery_schedule_skip_reason, schedule_agent_workspace_pr_supervision_recovery,
    AgentWorkspacePrSupervisionRecoveryDeps, AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::agent_workspace_publish_recovery::recover_stale_publish_repair_for_workspace_in_state;
use crate::application::agent_workspace_review::load_workspace_review_publish_blocker;
use crate::application::chat_service::tool_result_preview::{
    preview_tool_arguments_object, preview_tool_result_object, tool_detail_ref,
};
use crate::application::chat_service::{
    message_metadata_hidden_from_ui, running_state_from_run_status_and_idle,
    AgentConversationCreatedPayload, AgentRunningState, AgentRuntimeStatus, SendMessageOptions,
};
use crate::application::git_service::{
    git_cmd::{self, GitCommandLane},
    GitService,
};
use crate::application::ideation_workspace::prepare_ideation_analysis_state_from_agent_workspace;
use crate::application::publish_resilience::{
    classify_publish_failure, count_publish_reviewable_commits,
    count_publishable_commits_with_base_fallback, count_unpublished_publish_commits,
    ensure_plan_publish_branch_fresh, ensure_publish_base_pushed, ensure_publish_branch_fresh,
    inspect_publish_branch_freshness_for_source,
    inspect_publish_branch_freshness_for_source_after_fetch, push_publish_branch,
    remote_tracking_ref_for_publish, review_base_for_publish, PublishBranchFreshnessOutcome,
    PublishBranchFreshnessStatus, PublishFailureClass,
};
use crate::application::services::pr_merge_poller::sync_agent_workspace_auto_merge_preference_for_workspace;
use crate::application::session_namer_agent::{spawn_session_namer_agent, SessionNamerTarget};
use crate::application::{AppChatService, AppState, ChatService, ChatServiceError, SendResult};
use crate::commands::agent_model_commands::load_agent_model_registry;
use crate::commands::ExecutionState;
use crate::domain::agents::{
    default_effort_for_provider, default_efforts_for_provider, AgentHarnessKind, LogicalEffort,
    DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::plan_branch::PrPushStatus;
use crate::domain::entities::task_step::StepProgressSummary;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentConversationWorkspacePublicationEvent, AgentRun,
    AgentRunId, AgentRunStatus,
    AgentWorkspaceReviewMonitorStatus, AgentWorkspaceSourcePullRequest, ArtifactContent,
    ChatAttachmentId, ChatContextType, ChatConversation, ChatConversationId, ChatMessage,
    ChatMessageId, ChatTimelineItem, DelegatedSessionId, ExecutionPlanStatus,
    IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow, IdeationSessionId,
    InternalStatus, PlanBranch, PlanBranchStatus, Project, ProjectId, Task, TaskCategory, TaskId,
    CoordinationMode, TeamIntent, TeamMessageTarget, DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::execution::{
    build_running_ideation_session, build_running_process, context_matches_running_status,
    elapsed_seconds_for_status, RunningIdeationSession, RunningProcess,
};
use crate::domain::services::{
    normalize_title_with_jira_key, primary_jira_key_from_composer_metadata,
    AgentWorkspacePrPublisher, ComposerArtifactReference, ComposerIntegrationReference,
    ComposerProjectReference, QueuedMessage, RunningAgentKey, RunningAgentRegistry,
};
use crate::domain::state_machine::transition_handler::get_trigger_origin;
use crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_REPAIR;
use crate::infrastructure::agents::claude::git_runtime_config;

const AGENT_WORKSPACE_REPAIR_REQUESTED_STEP: &str = "repair_requested";
const AGENT_WORKSPACE_REPAIR_DEFERRED_STEP: &str = "repair_deferred";
const AGENT_WORKSPACE_REPAIR_SENT_STEP: &str = "repair_sent";
const AGENT_WORKSPACE_REPAIR_ACTION_PREFIX: &str = "agent_fixable:";
const AGENT_WORKSPACE_REPAIR_ACTION_PUBLISH: &str = "publish";
const AGENT_WORKSPACE_REPAIR_ACTION_UPDATE_ONLY: &str = "update_only";
pub const AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE: &str =
    "Agent workspace publish is already in progress";
#[doc(hidden)]
pub const AUTOMATION_RUN_MODE_LOCKED_ERROR_CODE: &str = "[ralphx:automation_run_mode_locked]";

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
    /// Internal handoff messages should reach the runtime without rendering as user chat.
    #[serde(default)]
    pub suppress_user_message: bool,
    /// Structured composer project references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_project_references: Vec<ComposerProjectReference>,
    /// Structured external integration references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_integration_references: Vec<ComposerIntegrationReference>,
    /// Structured artifact references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_artifact_references: Vec<ComposerArtifactReference>,
    /// Optional native team-mode overlay request for this send.
    pub team_intent: Option<TeamIntent>,
    /// Optional native team mailbox target.
    pub team_message_target: Option<TeamMessageTarget>,
    /// Attachment IDs selected by the composer for this message.
    #[serde(default)]
    pub attachment_ids: Vec<String>,
    /// Optional target for team message routing.
    /// When set to a teammate name, the message is routed to that teammate's stdin
    /// instead of the lead's. "lead" or None routes to the lead (default behavior).
    pub target: Option<String>,
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
        parse_chat_attachment_ids, visible_queued_message_responses, QueuedMessageResponse,
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

        let response = QueuedMessageResponse::from(queued);

        assert_eq!(response.attachment_ids, vec![attachment_id.to_string()]);
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

async fn linked_ideation_session_is_planning(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> Result<bool, String> {
    let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
        return Ok(false);
    };

    let Some(session) = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };

    Ok(session.session_flow == IdeationSessionFlow::Planning)
}

async fn ensure_plan_workspace_planning_session_link(
    state: &AppState,
    project: &Project,
    workspace: &mut AgentConversationWorkspace,
) -> Result<bool, String> {
    if workspace.mode != AgentConversationWorkspaceMode::Plan {
        return Ok(false);
    }

    if linked_ideation_session_is_planning(state, workspace).await? {
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

pub(crate) async fn agent_workspace_response_for_state(
    state: &AppState,
    workspace: AgentConversationWorkspace,
) -> Result<AgentConversationWorkspaceResponse, String> {
    let workspace = recover_stale_publish_repair_for_workspace_in_state(state, workspace)
        .await
        .map_err(|e| e.to_string())?;
    schedule_pr_supervision_recovery_for_workspace(
        state,
        &workspace,
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
        false,
    );

    let mode_lock = resolve_agent_conversation_workspace_mode_lock(state, &workspace).await?;
    let linked_plan_branch_id = workspace.linked_plan_branch_id.clone();
    let mut response = AgentConversationWorkspaceResponse::from(workspace);
    response.mode_switch_locked = mode_lock.locked;
    response.mode_switch_lock_reason = mode_lock.reason;

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
    let chat_service: Arc<dyn ChatService> = Arc::new(state.build_chat_service());
    schedule_agent_workspace_external_pr_reconciliation(
        AgentWorkspaceExternalPrReconciliationDeps {
            workspace_repo: Arc::clone(&state.agent_conversation_workspace_repo),
            project_repo: Arc::clone(&state.project_repo),
            github,
            pr_poller_registry: Some(Arc::clone(&state.pr_poller_registry)),
            chat_service: Some(chat_service),
            agent_run_repo: Arc::clone(&state.agent_run_repo),
            app_handle: state.app_handle.clone(),
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
    if pr_supervision_recovery_schedule_skip_reason(workspace).is_some() {
        return;
    }
    let Some(github) = state.github_service.as_ref().map(Arc::clone) else {
        return;
    };
    let chat_service: Arc<dyn ChatService> = Arc::new(state.build_chat_service());
    schedule_agent_workspace_pr_supervision_recovery(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&state.agent_conversation_workspace_repo),
            project_repo: Arc::clone(&state.project_repo),
            plan_branch_repo: Arc::clone(&state.plan_branch_repo),
            github,
            pr_poller_registry: Some(Arc::clone(&state.pr_poller_registry)),
            transition_service: None,
            chat_service: Some(chat_service),
            agent_run_repo: Arc::clone(&state.agent_run_repo),
            app_handle: state.app_handle.clone(),
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

/// Response from switch_agent_conversation_mode command.
#[derive(Debug, Serialize)]
pub struct SwitchAgentConversationModeResponse {
    pub conversation: AgentConversationResponse,
    pub workspace: Option<AgentConversationWorkspaceResponse>,
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
    fn as_str(self) -> &'static str {
        match self {
            Self::Publish => AGENT_WORKSPACE_REPAIR_ACTION_PUBLISH,
            Self::UpdateOnly => AGENT_WORKSPACE_REPAIR_ACTION_UPDATE_ONLY,
        }
    }

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
    /// Optional target for team message routing (teammate name or "lead").
    pub target: Option<String>,
}

/// Response for queued message
#[derive(Debug, Serialize)]
pub struct QueuedMessageResponse {
    pub id: String,
    pub content: String,
    pub created_at: String,
    pub is_editing: bool,
    pub attachment_ids: Vec<String>,
}

impl From<QueuedMessage> for QueuedMessageResponse {
    fn from(msg: QueuedMessage) -> Self {
        Self {
            id: msg.id,
            content: msg.content,
            created_at: msg.created_at,
            is_editing: msg.is_editing,
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
            coordination_mode: CoordinationMode::Solo.to_string(),
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

impl AgentConversationResponse {
    fn apply_runtime_attribution(&mut self, attribution: ConversationRuntimeAttribution) {
        self.logical_model = attribution.logical_model;
        self.effective_model_id = attribution.effective_model_id;
        self.logical_effort = attribution.logical_effort.map(|value| value.to_string());
        self.effective_effort = attribution.effective_effort;
        self.service_tier = attribution.service_tier;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ConversationRuntimeAttribution {
    logical_model: Option<String>,
    effective_model_id: Option<String>,
    logical_effort: Option<LogicalEffort>,
    effective_effort: Option<String>,
    service_tier: Option<String>,
}

impl ConversationRuntimeAttribution {
    fn is_empty(&self) -> bool {
        self.logical_model.is_none()
            && self.effective_model_id.is_none()
            && self.logical_effort.is_none()
            && self.effective_effort.is_none()
            && self.service_tier.is_none()
    }
}

fn runtime_attribution_from_run(run: &AgentRun) -> Option<ConversationRuntimeAttribution> {
    let attribution = ConversationRuntimeAttribution {
        logical_model: run.logical_model.clone(),
        effective_model_id: run.effective_model_id.clone(),
        logical_effort: run.logical_effort,
        effective_effort: run.effective_effort.clone(),
        service_tier: run.service_tier.clone(),
    };
    (!attribution.is_empty()).then_some(attribution)
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
    };
    (!attribution.is_empty()).then_some(attribution)
}

async fn latest_conversation_runtime_attribution(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<Option<ConversationRuntimeAttribution>, String> {
    let runs = state
        .agent_run_repo
        .get_by_conversation(conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(attribution) = runs.iter().find_map(runtime_attribution_from_run) {
        return Ok(Some(attribution));
    }

    let messages = state
        .chat_message_repo
        .get_recent_by_conversation_paginated(conversation_id, 200, 0)
        .await
        .map_err(|error| error.to_string())?;
    Ok(messages.iter().find_map(runtime_attribution_from_message))
}

pub(crate) async fn agent_conversation_response_for_state(
    state: &AppState,
    conversation: ChatConversation,
) -> Result<AgentConversationResponse, String> {
    let conversation_id = conversation.id;
    let mut response = AgentConversationResponse::from(conversation);
    if let Some(attribution) =
        latest_conversation_runtime_attribution(state, &conversation_id).await?
    {
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
    name == "delegate_start" || name.ends_with("::delegate_start")
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
    let total = run.input_tokens.unwrap_or(0)
        + run.output_tokens.unwrap_or(0)
        + run.cache_creation_tokens.unwrap_or(0)
        + run.cache_read_tokens.unwrap_or(0);
    if total == 0
        && run.input_tokens.is_none()
        && run.output_tokens.is_none()
        && run.cache_creation_tokens.is_none()
        && run.cache_read_tokens.is_none()
    {
        None
    } else {
        Some(total)
    }
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

    let conversation_id = delegated_conversation_id.map(str::to_string);
    let latest_run = if let Some(run_id) = delegated_agent_run_id {
        state
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(run_id))
            .await
            .ok()
            .flatten()
    } else if let Some(conversation_id) = delegated_conversation_id {
        state
            .agent_run_repo
            .get_latest_for_conversation(&ChatConversationId::from_string(conversation_id))
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let recent_messages = if let Some(conversation_id) = delegated_conversation_id {
        state
            .chat_message_repo
            .get_by_conversation(&ChatConversationId::from_string(conversation_id))
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
            .unwrap_or_default()
    } else {
        Vec::new()
    };

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
        conversation_id,
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

fn merge_delegated_snapshot_into_result(
    result: &mut JsonValue,
    snapshot: &DelegatedToolRuntimeSnapshot,
) {
    let JsonValue::Object(result_object) = result else {
        return;
    };

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
            let Some(parsed_result) = parse_wrapped_mcp_result_object(result) else {
                continue;
            };

            let delegated_session_id = get_string_field(&parsed_result, "delegated_session_id")
                .or_else(|| get_string_field(&parsed_result, "delegatedSessionId"));
            let Some(delegated_session_id) = delegated_session_id else {
                continue;
            };
            let delegated_conversation_id =
                get_string_field(&parsed_result, "delegated_conversation_id")
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
                    continue;
                };
                snapshot_cache.insert(delegated_session_id.to_string(), snapshot.clone());
                snapshot
            };

            merge_delegated_snapshot_into_result(result, &snapshot);
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
    team_service: Option<std::sync::Arc<crate::application::TeamService>>,
) -> AppChatService<R> {
    let mut service =
        state.build_chat_service_for_runtime(Some(Arc::clone(execution_state)), Some(app_handle));
    if let Some(svc) = team_service {
        service = service.with_team_service(svc);
    }
    service
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
    mode.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("edit")
        .parse::<AgentConversationWorkspaceMode>()
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
    _current_mode: AgentConversationWorkspaceMode,
    target_mode: AgentConversationWorkspaceMode,
    workspace_mode_lock: &AgentConversationWorkspaceModeLock,
) -> Result<(), String> {
    if workspace_mode_lock.locked && target_mode != AgentConversationWorkspaceMode::Ideation {
        return Err(workspace_mode_lock.reason.clone().unwrap_or_else(|| {
            "This workspace is owned by active ideation or execution state and cannot leave Ideation Mode"
                .to_string()
        }));
    }

    Ok(())
}

#[cfg(test)]
mod agent_mode_workspace_tests {
    use super::*;

    #[test]
    fn only_write_capable_agent_conversation_modes_require_workspace() {
        assert!(!agent_mode_requires_workspace(
            AgentConversationWorkspaceMode::Chat
        ));
        assert!(agent_mode_requires_workspace(
            AgentConversationWorkspaceMode::Edit
        ));
        assert!(agent_mode_requires_workspace(
            AgentConversationWorkspaceMode::Plan
        ));
        assert!(agent_mode_requires_workspace(
            AgentConversationWorkspaceMode::Ideation
        ));
    }

    #[test]
    fn source_pr_backed_chat_mode_creates_workspace() {
        let source_pull_request = AgentWorkspaceSourcePullRequest {
            number: 123,
            url: None,
            title: None,
            head_ref_name: "feature/source-pr".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: None,
        };

        assert!(agent_mode_should_create_workspace(
            AgentConversationWorkspaceMode::Chat,
            Some(&source_pull_request),
        ));
        assert!(!agent_mode_should_create_workspace(
            AgentConversationWorkspaceMode::Chat,
            None,
        ));
        assert!(agent_mode_should_create_workspace(
            AgentConversationWorkspaceMode::Edit,
            None,
        ));
    }

    #[test]
    fn plan_agent_conversation_mode_round_trips_through_api_string() {
        let mode = "plan"
            .parse::<AgentConversationWorkspaceMode>()
            .expect("plan mode should parse");

        assert_eq!(mode, AgentConversationWorkspaceMode::Plan);
        assert_eq!(mode.to_string(), "plan");
    }

    #[test]
    fn review_pr_agent_conversation_mode_round_trips_through_api_string() {
        let mode = "review_pr"
            .parse::<AgentConversationWorkspaceMode>()
            .expect("review_pr mode should parse");

        assert_eq!(mode, AgentConversationWorkspaceMode::ReviewPr);
        assert_eq!(mode.to_string(), "review_pr");
    }

    #[test]
    fn active_agent_conversations_support_expected_valid_mode_transition_matrix() {
        let modes = [
            AgentConversationWorkspaceMode::Chat,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceMode::Plan,
            AgentConversationWorkspaceMode::Ideation,
            AgentConversationWorkspaceMode::ReviewPr,
        ];

        for current_mode in modes {
            for target_mode in modes {
                assert!(
                    validate_agent_conversation_mode_transition(
                        current_mode,
                        target_mode,
                        &AgentConversationWorkspaceModeLock::unlocked()
                    )
                    .is_ok(),
                    "{current_mode} -> {target_mode} should be allowed"
                )
            }
        }
    }

    #[test]
    fn active_state_owned_conversations_cannot_leave_ideation_mode() {
        for target_mode in [
            AgentConversationWorkspaceMode::Chat,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceMode::Plan,
            AgentConversationWorkspaceMode::ReviewPr,
        ] {
            let error = validate_agent_conversation_mode_transition(
                AgentConversationWorkspaceMode::Ideation,
                target_mode,
                &AgentConversationWorkspaceModeLock::locked("Plan execution is still active"),
            )
            .expect_err("state-owned conversations should not leave ideation mode");

            assert!(error.contains("Plan execution is still active"));
        }
    }

    #[test]
    fn state_owned_workspaces_can_target_ideation_mode() {
        for target_mode in [
            AgentConversationWorkspaceMode::Chat,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceMode::Plan,
            AgentConversationWorkspaceMode::ReviewPr,
        ] {
            let error = validate_agent_conversation_mode_transition(
                AgentConversationWorkspaceMode::Chat,
                target_mode,
                &AgentConversationWorkspaceModeLock::locked("Ideation session is still active"),
            )
            .expect_err("state-owned workspaces should not leave ideation ownership");

            assert!(error.contains("Ideation session is still active"));
        }

        assert!(validate_agent_conversation_mode_transition(
            AgentConversationWorkspaceMode::Chat,
            AgentConversationWorkspaceMode::Ideation,
            &AgentConversationWorkspaceModeLock::locked("Ideation session is still active"),
        )
        .is_ok());
    }
}

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
    team_service: State<'_, std::sync::Arc<crate::application::TeamService>>,
    app: tauri::AppHandle<R>,
) -> Result<StartAgentConversationResponse, String> {
    start_agent_conversation_for_state(
        input,
        state.inner(),
        execution_state.inner(),
        team_service.inner().clone(),
        app,
    )
    .await
}

#[doc(hidden)]
pub(crate) async fn start_agent_conversation_for_state<R: Runtime + 'static>(
    input: StartAgentConversationInput,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    team_service: std::sync::Arc<crate::application::TeamService>,
    app: tauri::AppHandle<R>,
) -> Result<StartAgentConversationResponse, String> {
    let result = AgentConversationStartService::new(AgentConversationStartDeps {
        state,
        execution_state,
        team_service: Some(team_service),
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
    let service = create_chat_service(&state, app, &execution_state, None);
    switch_agent_conversation_mode_for_state_stopping_running_agent(input, state.inner(), &service)
        .await
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
    if initiator == ModeSwitchInitiator::User && conversation.automation_run_id.is_some() {
        return Err(format!(
            "{AUTOMATION_RUN_MODE_LOCKED_ERROR_CODE} Automation run conversations cannot be switched manually"
        ));
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

    let existing_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .map_err(|error| error.to_string())?;
    let current_mode = conversation
        .agent_mode
        .or_else(|| existing_workspace.as_ref().map(|workspace| workspace.mode))
        .unwrap_or(AgentConversationWorkspaceMode::Chat);
    let workspace_mode_lock = match existing_workspace.as_ref() {
        Some(workspace) => resolve_agent_conversation_workspace_mode_lock(state, workspace).await?,
        None => AgentConversationWorkspaceModeLock::unlocked(),
    };

    validate_agent_conversation_mode_transition(current_mode, target_mode, &workspace_mode_lock)?;

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

    let workspace = match existing_workspace {
        Some(mut workspace) => {
            let preserve_planning_session_link = if target_mode
                != AgentConversationWorkspaceMode::Ideation
                && workspace.linked_plan_branch_id.is_none()
            {
                linked_ideation_session_is_planning(state, &workspace).await?
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
            let should_detach_inactive_owner = target_mode
                != AgentConversationWorkspaceMode::Ideation
                && !workspace_mode_lock.locked
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

    state
        .chat_conversation_repo
        .update_agent_mode(&conversation.id, Some(target_mode))
        .await
        .map_err(|error| error.to_string())?;
    conversation.set_agent_mode(Some(target_mode));

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
    team_service: State<'_, std::sync::Arc<crate::application::TeamService>>,
    app: tauri::AppHandle,
) -> Result<SendAgentMessageResponse, String> {
    tracing::info!(
        context_type = %input.context_type,
        context_id = %input.context_id,
        content_len = input.content.len(),
        target = ?input.target,
        "[SEND_MSG] send_agent_message command invoked"
    );
    let context_type = parse_context_type(&input.context_type)?;
    let harness_override = input
        .provider_harness
        .as_deref()
        .map(str::parse::<AgentHarnessKind>)
        .transpose()?;
    let requested_harness = harness_override.unwrap_or(DEFAULT_AGENT_HARNESS);
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

    let mut service = create_chat_service(
        &state,
        app.clone(),
        &execution_state,
        Some(team_service.inner().clone()),
    );

    // For ideation contexts, check if the session has team_mode enabled
    if context_type == ChatContextType::Ideation {
        let session_id = IdeationSessionId::from_string(&input.context_id);
        if let Ok(Some(session)) = state.ideation_session_repo.get_by_id(&session_id).await {
            let is_team = session.team_mode.as_deref().is_some_and(|m| m != "solo");
            if is_team {
                service = service.with_team_mode(true);
            }
        }
    }

    // For execution contexts, check if the task's metadata has agent_variant = "team"
    if context_type == ChatContextType::TaskExecution {
        let task_id = TaskId::from_string(input.context_id.clone());
        if let Ok(Some(task)) = state.task_repo.get_by_id(&task_id).await {
            let is_team = task
                .metadata
                .as_ref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|meta| {
                    meta.get("agent_variant")
                        .and_then(|v| v.as_str())
                        .map(|s| s == "team")
                })
                .unwrap_or(false);
            if is_team {
                service = service.with_team_mode(true);
            }
        }
    }

    crate::application::validate_chat_runtime_for_context_with_override(
        &state,
        context_type,
        &input.context_id,
        "send_agent_message",
        harness_override,
    )
    .await?;

    // Route to teammate stdin when target is a specific teammate (not "lead")
    let target = input.target.as_deref();
    if let Some(teammate_name) = target.filter(|t| *t != "lead") {
        // Find the active team for this context
        if let Some(team_name) = team_service
            .find_team_by_context_id(&input.context_id)
            .await
        {
            let formatted =
                crate::infrastructure::agents::claude::format_stream_json_input(&input.content);
            team_service
                .send_stdin_message(&team_name, teammate_name, &formatted)
                .await
                .map_err(|e| format!("Failed to send to teammate {}: {}", teammate_name, e))?;

            tracing::info!(
                teammate = %teammate_name,
                team = %team_name,
                "Routed user message to teammate stdin"
            );

            // Return a synthetic response — the teammate's stream processor handles
            // conversation persistence and event emission.
            return Ok(SendAgentMessageResponse {
                conversation_id: String::new(),
                agent_run_id: uuid::Uuid::new_v4().to_string(),
                is_new_conversation: false,
                was_queued: false,
                queued_as_pending: false,
                queued_message_id: None,
            });
        }
        // Team not found for context — fall through to normal lead path
        tracing::warn!(
            target = %teammate_name,
            context_id = %input.context_id,
            "No active team found for context, falling back to lead"
        );
    }

    let model_override = input
        .model_override
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    let (model_override, logical_effort_override) = normalize_agent_runtime_selection(
        &state,
        harness_override,
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
            state.inner(),
            &app,
            parent_conversation_id.as_ref(),
            &input.content,
            harness_override,
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
            && ensure_plan_workspace_planning_session_link_for_send(state.inner(), conversation_id)
                .await?
        {
            let _ = app.emit(
                "agent:workspace_changed",
                serde_json::json!({ "conversation_id": conversation_id.as_str() }),
            );
        }
    }
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
                harness_override,
                model_override,
                logical_effort_override,
                service_tier_override,
                conversation_id_override,
                composer_project_references: input.composer_project_references,
                composer_integration_references: input.composer_integration_references,
                composer_artifact_references: input.composer_artifact_references,
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

    let service = create_chat_service(&state, app, &execution_state, None);

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

    let service = create_chat_service(&state, app, &execution_state, None);

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

    let service = create_chat_service(&state, app, &execution_state, None);

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
    team_service: std::sync::Arc<crate::application::TeamService>,
    app: tauri::AppHandle<R>,
) -> Result<SendAgentMessageResponse, String> {
    let context_type = parse_context_type(&context_type)?;
    let mut service = create_chat_service(state, app, execution_state, Some(team_service));

    if context_type == ChatContextType::Ideation {
        let session_id = IdeationSessionId::from_string(&context_id);
        if let Ok(Some(session)) = state.ideation_session_repo.get_by_id(&session_id).await {
            let is_team = session.team_mode.as_deref().is_some_and(|m| m != "solo");
            if is_team {
                service = service.with_team_mode(true);
            }
        }
    }

    if context_type == ChatContextType::TaskExecution {
        let task_id = TaskId::from_string(context_id.clone());
        if let Ok(Some(task)) = state.task_repo.get_by_id(&task_id).await {
            let is_team = task
                .metadata
                .as_ref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|meta| {
                    meta.get("agent_variant")
                        .and_then(|v| v.as_str())
                        .map(|s| s == "team")
                })
                .unwrap_or(false);
            if is_team {
                service = service.with_team_mode(true);
            }
        }
    }

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
    team_service: State<'_, std::sync::Arc<crate::application::TeamService>>,
    app: tauri::AppHandle,
) -> Result<SendAgentMessageResponse, String> {
    send_queued_agent_message_now_for_state(
        context_type,
        context_id,
        message_id,
        &state,
        &execution_state,
        team_service.inner().clone(),
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
        let service = create_chat_service(&state, app, &execution_state, None);
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
    state: &AppState,
) -> Result<(), String> {
    archive_agent_conversation_for_state(conversation_id, state).await
}

/// Archive a conversation.
/// If the workspace has an open PR, close it immediately on the remote
/// and mark publication_pr_status as "closed". The local worktree/branch
/// will be cleaned up on next app restart via the terminal cleanup pipeline.
#[tauri::command]
pub async fn archive_agent_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<AgentConversationResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    archive_agent_conversation_inner(&conversation_id, &state).await?;

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;
    agent_conversation_response_for_state(state.inner(), conversation).await
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
    let (Some(github), Some(target)) = (state.github_service.as_ref(), target) else {
        return Ok(());
    };

    let pr_number = target.pr_number;
    let working_dir = target.working_dir.as_path();
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
                        Some("waiting"),
                        Some(&format!(
                            "GitHub auto-merge could not be enabled yet: {error}"
                        )),
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
                    Some("waiting"),
                    Some(&format!(
                        "GitHub auto-merge could not be disabled yet: {error}"
                    )),
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
                state.pr_poller_registry.start_agent_workspace_polling(
                    conversation_id.clone(),
                    target.pr_number,
                    project,
                    target.working_dir.clone(),
                    Arc::clone(&state.agent_conversation_workspace_repo),
                    Arc::clone(&state.agent_run_repo),
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
            return agent_workspace_response_for_state(state, workspace).await;
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

        let updated = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Agent conversation workspace not found".to_string())?;
        return agent_workspace_response_for_state(state, updated).await;
    }

    if input.auto_publish_enabled == workspace.auto_publish_enabled {
        return agent_workspace_response_for_state(state, workspace).await;
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
                        Some("waiting"),
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
        true,
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

    if workspace.mode != AgentConversationWorkspaceMode::Edit {
        return Err(
            "Only edit workspaces and ideation workspaces with linked plan branches can be inspected for freshness"
                .to_string(),
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
    if workspace.mode != AgentConversationWorkspaceMode::Edit {
        return Err(
            "Only edit workspaces and ideation workspaces with linked plan branches can be inspected for freshness"
                .to_string(),
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
    team_service: State<'_, std::sync::Arc<crate::application::TeamService>>,
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
        Some(team_service.inner().clone()),
        conversation_id,
        selection,
    )
    .await
}

#[doc(hidden)]
pub async fn update_agent_conversation_workspace_from_base_for_app_state(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    team_service: Option<Arc<crate::application::TeamService>>,
    conversation_id: ChatConversationId,
    selection: AgentConversationWorkspaceBaseSelection,
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

    let mut repair_service =
        state.build_chat_service_with_execution_state(Arc::clone(execution_state));
    if let Some(team_service) = team_service {
        repair_service = repair_service.with_team_service(team_service);
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

    mark_agent_workspace_publish_status(state, &workspace, "refreshing")
        .await
        .map_err(|e| e.to_string())?;

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
    state
        .agent_conversation_workspace_repo
        .update_publication(
            &workspace.conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            workspace.publication_pr_status.as_deref(),
            Some(push_status),
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
    team_service: State<'_, std::sync::Arc<crate::application::TeamService>>,
    app: tauri::AppHandle,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let _workspace_changed_event = emit_workspace_changed_when_done(&app, &conversation_id);
    publish_agent_conversation_workspace_for_app_state(
        state.inner(),
        execution_state.inner(),
        Some(team_service.inner().clone()),
        conversation_id,
        true,
    )
    .await
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
            captured_review_base,
        ));
    };
    if base_resolution.status == BaseStatus::Blocked {
        return Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Ready(
            captured_review_base,
        ));
    }

    let checkout_ref = match base_resolution.effective_checkout_ref() {
        Ok(checkout_ref) => checkout_ref.to_string(),
        Err(_) => {
            return Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Ready(
                captured_review_base,
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
                captured_review_base,
            ));
        }
    };

    if freshness.is_base_ahead {
        return Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Skip(
            "base_ahead",
        ));
    }

    let review_base = freshness
        .captured_base_commit
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(captured_review_base);
    Ok(AgentWorkspacePrDescriptionReviewBaseResolution::Ready(
        review_base,
    ))
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
        let Some(cache_key) = AgentWorkspacePrDescriptionCacheKey::new(
            conversation_id.clone(),
            review_base.clone(),
            branch_head_sha,
            reviewable_commit_count,
        ) else {
            return Ok(skip("uncacheable_key"));
        };

        let outcome = get_or_draft_agent_workspace_pr_description(
            state,
            &conversation,
            &project,
            &workspace,
            &worktree_path,
            &review_base,
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
    team_service: Option<Arc<crate::application::TeamService>>,
    mut workspace: AgentConversationWorkspace,
    route_fixable_failures_to_agent: bool,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
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

    let mut repair_service =
        state.build_chat_service_with_execution_state(Arc::clone(execution_state));
    if let Some(team_service) = team_service {
        repair_service = repair_service.with_team_service(team_service);
    }

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
    let push_started = Instant::now();
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
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(workspace);

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
                        Some("waiting"),
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
    state.pr_poller_registry.start_agent_workspace_polling(
        refreshed.conversation_id.clone(),
        pr_number,
        project.clone(),
        publish_target.worktree_path.clone(),
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
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
    team_service: Option<Arc<crate::application::TeamService>>,
    conversation_id: ChatConversationId,
    route_fixable_failures_to_agent: bool,
) -> Result<PublishAgentConversationWorkspaceResponse, String> {
    let _publish_guard = try_acquire_agent_workspace_publish_guard(&conversation_id)?;
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
            team_service,
            workspace,
            route_fixable_failures_to_agent,
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

    let mut repair_service =
        state.build_chat_service_with_execution_state(Arc::clone(execution_state));
    if let Some(team_service) = team_service {
        repair_service = repair_service.with_team_service(team_service);
    }

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
    let mut repair_target = AgentConversationWorkspaceRepairTarget::from_workspace(&workspace);

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
    let freshness_outcome = ensure_publish_branch_fresh(
        repo_path,
        &project,
        &workspace.branch_name,
        &workspace.base_ref,
        &freshness_conversation_id,
        None,
    )
    .await;
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
    let pr_description_cache_key = AgentWorkspacePrDescriptionCacheKey::new(
        conversation_id.clone(),
        review_base.to_string(),
        branch_head_sha,
        reviewable_commit_count,
    );

    mark_agent_workspace_publish_status(state, &workspace, "describing")
        .await
        .map_err(|e| e.to_string())?;
    let describe_started = Instant::now();
    let pr_description = match if let Some(cache_key) = pr_description_cache_key {
        get_or_draft_agent_workspace_pr_description(
            state,
            &conversation,
            &project,
            &workspace,
            &worktree_path,
            review_base,
            cache_key,
        )
        .await
        .map(|outcome| {
            tracing::info!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                operation = "draft_pr_description",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                cache_status = outcome.cache_status.as_str(),
                cache_age_ms = ?outcome.cache_age_ms,
                cache_wait_ms = outcome.cache_wait_ms,
                elapsed_ms = describe_started.elapsed().as_millis(),
                "Resolved agent workspace PR description"
            );
            outcome.description
        })
    } else {
        draft_agent_workspace_pr_description(
            state,
            &conversation,
            &project,
            &workspace,
            &worktree_path,
            review_base,
        )
        .await
        .inspect(|_| {
            tracing::info!(
                target: "ralphx_lib::commands::agent_workspace_publish",
                operation = "draft_pr_description",
                conversation_id = %workspace.conversation_id,
                project_id = %workspace.project_id,
                branch = %workspace.branch_name,
                cache_status = "uncacheable",
                cache_age_ms = ?Option::<u128>::None,
                cache_wait_ms = 0_u128,
                elapsed_ms = describe_started.elapsed().as_millis(),
                "Resolved agent workspace PR description"
            );
        })
    } {
        Ok(description) => description,
        Err(error) => {
            let error = error.to_string();
            mark_agent_workspace_publish_description_failure(state, &workspace, &error).await;
            return Err(error);
        }
    };

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

    let push_started = Instant::now();
    if let Err(error) = push_publish_branch(github, &worktree_path, &workspace.branch_name).await {
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

    mark_agent_workspace_publish_status(state, &workspace, "pushed")
        .await
        .map_err(|e| e.to_string())?;

    let plan_markdown = resolve_linked_plan_markdown(state, &workspace).await;
    let mut publisher = AgentWorkspacePrPublisher::new(github);
    if let Some(markdown) = plan_markdown {
        publisher = publisher.with_plan_markdown(markdown);
    }
    let publish_pr_started = Instant::now();
    let pr_result = publisher
        .publish_draft_pr(&worktree_path, &conversation, &workspace, &pr_description)
        .await;
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
        None,
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(workspace);

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
                        Some("waiting"),
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
    state.pr_poller_registry.start_agent_workspace_polling(
        refreshed.conversation_id.clone(),
        outcome.pr_number,
        project.clone(),
        worktree_path.clone(),
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
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
        "After the repair is committed, call complete_agent_workspace_repair with the conversation ID, repair commit SHA, resolved base ref, resolved base commit, and summary."
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

async fn resolve_agent_workspace_repair_runtime_overrides(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AgentWorkspaceRepairRuntimeOverrides {
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&workspace.conversation_id)
        .await
        .ok()
        .flatten();
    let latest_run = state
        .agent_run_repo
        .get_latest_for_conversation(&workspace.conversation_id)
        .await
        .ok()
        .flatten();

    AgentWorkspaceRepairRuntimeOverrides {
        harness: conversation
            .as_ref()
            .and_then(ChatConversation::provider_session_ref)
            .map(|session_ref| session_ref.harness)
            .or_else(|| latest_run.as_ref().and_then(|run| run.harness)),
        model: latest_run.as_ref().and_then(|run| {
            run.logical_model
                .clone()
                .or_else(|| run.effective_model_id.clone())
        }),
        logical_effort: latest_run.as_ref().and_then(|run| run.logical_effort),
    }
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
    )
    .await;
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
) where
    S: ChatService + ?Sized,
{
    let push_status = match failure_class {
        PublishFailureClass::AgentFixable => "needs_agent",
        PublishFailureClass::Operational => "failed",
    };
    let classification = match failure_class {
        PublishFailureClass::AgentFixable => "agent_fixable",
        PublishFailureClass::Operational => "operational",
    };
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
        Some(classification.to_string()),
    )
    .await;

    if !route_fixable_failures_to_agent
        || !matches!(failure_class, PublishFailureClass::AgentFixable)
    {
        return;
    }

    let _ = append_agent_workspace_publication_event(
        state,
        &workspace.conversation_id,
        AGENT_WORKSPACE_REPAIR_REQUESTED_STEP,
        "started",
        post_repair_action.repair_requested_summary(),
        Some(post_repair_action.classification()),
    )
    .await;

    let runtime_overrides =
        resolve_agent_workspace_repair_runtime_overrides(state, workspace).await;
    if should_defer_agent_workspace_repair_message(state, workspace).await {
        spawn_deferred_agent_workspace_repair_message(
            state,
            workspace.clone(),
            error.to_string(),
            runtime_overrides,
            target.clone(),
            post_repair_action,
        )
        .await;
        return;
    }

    match send_agent_workspace_repair_message_for_target(
        repair_service,
        workspace,
        error,
        runtime_overrides,
        target,
        post_repair_action,
    )
    .await
    {
        Ok(_) => {
            let _ = append_agent_workspace_publication_event(
                state,
                &workspace.conversation_id,
                AGENT_WORKSPACE_REPAIR_SENT_STEP,
                "succeeded",
                post_repair_action.repair_sent_summary(),
                Some("agent_fixable".to_string()),
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
            let _ = append_agent_workspace_publication_event(
                state,
                &workspace.conversation_id,
                AGENT_WORKSPACE_REPAIR_SENT_STEP,
                "failed",
                &repair_summary,
                Some("operational".to_string()),
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
) {
    let Some(app_handle) = state.app_handle.clone() else {
        return;
    };

    let _ = append_agent_workspace_publication_event(
        state,
        &workspace.conversation_id,
        AGENT_WORKSPACE_REPAIR_DEFERRED_STEP,
        "started",
        "Waiting for the active workspace agent turn to finish before sending repair",
        Some("agent_fixable".to_string()),
    )
    .await;

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
            if wait_started.elapsed() >= Duration::from_secs(300) {
                let _ = append_agent_workspace_publication_event(
                    state.inner(),
                    &conversation_id,
                    AGENT_WORKSPACE_REPAIR_SENT_STEP,
                    "failed",
                    "Timed out waiting for active workspace agent turn before sending repair",
                    Some("operational".to_string()),
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
        let mut repair_service = match execution_state {
            Some(execution_state) => state.build_chat_service_with_execution_state(execution_state),
            None => state.build_chat_service(),
        };
        if let Some(team_service) = app_handle
            .try_state::<Arc<crate::application::TeamService>>()
            .map(|state| state.inner().clone())
        {
            repair_service = repair_service.with_team_service(team_service);
        }

        match send_agent_workspace_repair_message_for_target(
            &repair_service,
            &workspace,
            &error,
            runtime_overrides,
            &target,
            post_repair_action,
        )
        .await
        {
            Ok(_) => {
                let _ = append_agent_workspace_publication_event(
                    state.inner(),
                    &conversation_id,
                    AGENT_WORKSPACE_REPAIR_SENT_STEP,
                    "succeeded",
                    post_repair_action.deferred_repair_sent_summary(),
                    Some("agent_fixable".to_string()),
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
                let _ = append_agent_workspace_publication_event(
                    state.inner(),
                    &conversation_id,
                    AGENT_WORKSPACE_REPAIR_SENT_STEP,
                    "failed",
                    &repair_summary,
                    Some("operational".to_string()),
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

    let service = create_chat_service(&state, app, &execution_state, None);
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
        || create_chat_service(&state, app, &execution_state, None),
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
        || create_chat_service(&state, app, &execution_state, None),
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

    Ok(Some(AgentConversationTimelinePageResponse {
        conversation: agent_conversation_response_for_state(state, conversation).await?,
        items: page
            .items
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

    let service = create_chat_service(&state, app, &execution_state, None);

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
    }))
}

/// Check if the chat service is available
#[tauri::command]
pub async fn is_chat_service_available(
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let service = create_chat_service(&state, app, &execution_state, None);
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

    let service = create_chat_service(&state, app, &execution_state, None);

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

    let service = create_chat_service(&state, app, &execution_state, None);

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
        AgentConversationWorkspaceMode::Ideation => AgentConversationRuntimeIndexMode::Ideation,
        AgentConversationWorkspaceMode::ReviewPr => AgentConversationRuntimeIndexMode::PrReview,
        AgentConversationWorkspaceMode::Automation => AgentConversationRuntimeIndexMode::Automation,
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
        InternalStatus::Blocked | InternalStatus::MergeConflict => {
            AgentConversationRuntimeLifecycle::Blocked
        }
        InternalStatus::Executing
        | InternalStatus::QaRefining
        | InternalStatus::QaTesting
        | InternalStatus::Reviewing
        | InternalStatus::ReExecuting
        | InternalStatus::Merging
        | InternalStatus::WaitingOnPr => AgentConversationRuntimeLifecycle::Running,
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
    pub context_id: String,
    pub title: Option<String>,
}

/// Input for update_agent_conversation_title command
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentConversationTitleInput {
    pub conversation_id: String,
    pub title: String,
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

    let mut conversation = match context_type {
        ChatContextType::Ideation => {
            ChatConversation::new_ideation(IdeationSessionId::from_string(&input.context_id))
        }
        ChatContextType::Delegation => {
            ChatConversation::new_delegation(DelegatedSessionId::from_string(&input.context_id))
        }
        ChatContextType::Task => {
            ChatConversation::new_task(TaskId::from_string(input.context_id.clone()))
        }
        ChatContextType::Project => {
            ChatConversation::new_project(ProjectId::from_string(input.context_id.clone()))
        }
        ChatContextType::TaskExecution => {
            ChatConversation::new_task_execution(TaskId::from_string(input.context_id.clone()))
        }
        ChatContextType::Review => {
            ChatConversation::new_review(TaskId::from_string(input.context_id.clone()))
        }
        ChatContextType::Merge => {
            ChatConversation::new_merge(TaskId::from_string(input.context_id.clone()))
        }
    };

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

#[cfg(test)]
mod tests {
    use super::{
        agent_conversation_response_for_state, agent_conversation_responses_for_state,
        agent_workspace_freshness_cache, agent_workspace_freshness_cache_key,
        agent_workspace_interactive_slot_key, agent_workspace_post_repair_action_from_events,
        agent_workspace_repair_wait_released, agent_workspace_response_for_state,
        apply_base_resolution_to_publish_target, archive_agent_conversation,
        build_agent_workspace_publish_repair_message_for_target,
        build_agent_workspace_repair_message_for_target, cached_agent_workspace_freshness,
        create_agent_conversation, emit_agent_conversation_fork_events,
        ensure_plan_workspace_planning_session_link_for_send, existing_pr_retarget_block_reason,
        filter_agent_list_visible_conversations,
        fork_agent_conversation, fork_agent_conversation_response_for_state,
        fork_terminal_agent_conversation_for_send,
        get_agent_conversation_runtime_index_for_app_state,
        get_agent_conversation_runtime_statuses_for_app_state,
        get_agent_conversation_summary_for_app_state,
        get_agent_conversation_timeline_page_for_app_state,
        get_agent_conversation_workspace_freshness,
        get_agent_timeline_item_tool_call_detail_for_app_state, hidden_user_message_metadata,
        invalidate_agent_workspace_freshness_cache, list_agent_conversations_page,
        mark_agent_workspace_failure_with_routing_and_action, merge_delegated_snapshot_into_result,
        normalize_agent_runtime_selection, normalize_agent_workspace_source_pull_request,
        normalize_explicit_publish_base_selection, normalized_effort_for_supported,
        parse_wrapped_mcp_result_object, persist_workspace_base_resolution_if_retargeted,
        precompute_agent_conversation_workspace_pr_description_for_app_state,
        preview_tool_payloads_for_message, project_plan_branch_publication_into_workspace_response,
        publication_event_status_for_push_status, publication_event_summary_for_push_status,
        publish_agent_conversation_workspace_for_app_state, restore_agent_conversation,
        retarget_existing_workspace_pr_base_if_needed,
        schedule_external_pr_reconciliation_for_conversation_id,
        schedule_external_pr_reconciliation_for_workspace,
        schedule_pr_supervision_recovery_for_conversation_id,
        send_agent_workspace_publish_repair_message_for_target,
        send_queued_agent_message_now_for_state,
        set_agent_conversation_workspace_auto_publish_for_state,
        set_agent_conversation_workspace_pr_supervision_for_state,
        should_defer_agent_workspace_repair_message_for_registry,
        spawn_deferred_agent_workspace_repair_message, store_agent_workspace_freshness,
        switch_agent_conversation_mode_for_state,
        switch_agent_conversation_mode_for_state_allowing_running,
        try_acquire_agent_workspace_publish_guard,
        update_agent_conversation_workspace_from_base_for_app_state,
        validate_explicit_publish_base_ref, AgentConversationResponse,
        AgentConversationRuntimeIndexGroup, AgentConversationRuntimeIndexKind,
        AgentConversationRuntimeLifecycle, AgentConversationRuntimeSource,
        AgentConversationWorkspaceAutoPublishInput, AgentConversationWorkspaceFreshnessResponse,
        AgentConversationWorkspacePrSupervisionInput, AgentConversationWorkspacePublishTarget,
        AgentConversationWorkspaceRepairTarget, AgentConversationWorkspaceResponse,
        AgentTimelineItemResponse, AgentWorkspaceExternalPrReconciliationTrigger,
        AgentWorkspaceFreshnessCacheEntry, AgentWorkspaceFreshnessCacheStatus,
        AgentWorkspaceFreshnessInvalidationGuard, AgentWorkspaceFreshnessScope,
        AgentWorkspacePostRepairAction, AgentWorkspacePrDescriptionInvalidationGuard,
        AgentWorkspaceRepairRuntimeOverrides, AgentWorkspaceSourcePullRequestInput,
        CreateAgentConversationInput, DelegatedToolRuntimeSnapshot, ForkAgentConversationInput,
        ForkAgentConversationResponse, ModeSwitchInitiator, SwitchAgentConversationModeInput,
        AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE,
    };
    use crate::application::agent_conversation_workspace::{
        ensure_linked_plan_branch_agent_worktree, prepare_agent_conversation_workspace,
        resolve_linked_plan_branch_agent_worktree_path, AgentConversationWorkspaceBaseSelection,
    };
    use crate::application::agent_conversation_workspace_base::{
        BaseResolutionResult, BaseStatus, BLOCK_REASON_MISSING_BASE_COMMIT,
    };
    use crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrSupervisionRecoveryTrigger;
    use crate::application::git_service::GitService;
    use crate::application::publish_resilience::PublishBranchFreshnessStatus;
    use crate::application::{
        chat_service::{AgentRuntimeStatus, MockChatService},
        AppState, TeamService, TeamStateTracker,
    };
    use crate::commands::ExecutionState;
    use crate::domain::agents::{
        AgentConfig, AgentHandle, AgentHarnessKind, AgentModelDefinition, AgentOutput,
        AgentResponse, AgentResult, AgenticClient, ClientCapabilities, LogicalEffort,
        ProviderSessionRef, ResponseChunk,
    };
    use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus};
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
        AgentConversationWorkspaceMode, AgentConversationWorkspacePublicationEvent, AgentRun,
        AgentWorkspacePrDescription, AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor,
        AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
        AgentWorkspaceSourcePullRequest, ArtifactId, AutomationId, AutomationRunId,
        ChatContextType, ChatConversation, ChatConversationId, ChatMessage, ChatMessageId,
        ChatTimelineItem, ChatTimelineItemId, ChatTimelineItemKind, ChatTimelineItemStatus,
        ExecutionPlan, ExecutionPlanId, ExecutionPlanStatus, IdeationAnalysisBaseRefKind,
        IdeationSession, IdeationSessionFlow, IdeationSessionId, InternalStatus, MessageRole,
        PlanBranch, PlanBranchId, PlanBranchStatus, Project, ProjectId, SessionPurpose, Task,
        TaskId,
        DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
    };
    use crate::domain::execution::ExecutionSettings;
    use crate::domain::repositories::AgentConversationWorkspaceRepository;
    use crate::domain::review::ReviewSettings;
    use crate::domain::services::github_service::{PrAutoMergeRequest, PrHealth};
    use crate::domain::services::{
        GithubServiceTrait, MemoryRunningAgentRegistry, PrBranchMatch, PrMergeStateStatus,
        PrMergeableState, PrStatus as GithubPrStatus, PrSyncState, RunningAgentKey,
        RunningAgentRegistry,
    };
    use crate::error::AppError;
    use crate::infrastructure::{MockAgenticClient, MockCallType};
    use crate::tests::mock_github_service::MockGithubService;
    use async_trait::async_trait;
    use futures::{stream, Stream};
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::process::Command;
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::Manager;

    #[test]
    fn hidden_user_message_metadata_suppresses_visible_chat_message() {
        let metadata: serde_json::Value =
            serde_json::from_str(&hidden_user_message_metadata()).expect("metadata json");

        assert_eq!(metadata["source"], "hidden_user_message");
        assert_eq!(metadata["resume_in_place"], true);
        assert_eq!(metadata["persist_hidden_marker"], true);
        assert_eq!(metadata["hidden_from_ui"], true);
        assert_eq!(metadata["recovery_context"], true);
    }

    fn workspace_for_runtime_test(
        conversation_id: &ChatConversationId,
        project_id: &ProjectId,
    ) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            conversation_id.clone(),
            project_id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            None,
            "ralphx/test".to_string(),
            "/tmp/ralphx-test-worktree".to_string(),
        )
    }

    async fn register_runtime_context(
        state: &AppState,
        context_type: ChatContextType,
        context_id: &str,
    ) {
        state
            .running_agent_registry
            .register(
                RunningAgentKey::new(context_type.to_string(), context_id.to_string()),
                0,
                format!("{context_type}-{context_id}-conversation"),
                String::new(),
                None,
                None,
            )
            .await;
    }

    #[tokio::test]
    async fn agent_conversation_runtime_status_includes_linked_ideation_and_verification() {
        let state = AppState::new_sqlite_test();
        let execution_state = Arc::new(ExecutionState::new());
        let project_id = ProjectId::from_string("project-runtime-status".to_string());
        let conversation_id = ChatConversationId::new();

        let parent = IdeationSession::new_with_title(project_id.clone(), "Plan draft");
        let parent_id = parent.id.clone();
        state.ideation_session_repo.create(parent).await.unwrap();

        let mut child = IdeationSession::new_with_title(project_id.clone(), "Verification run");
        child.parent_session_id = Some(parent_id.clone());
        child.session_purpose = SessionPurpose::Verification;
        let child_id = child.id.clone();
        state.ideation_session_repo.create(child).await.unwrap();

        let mut workspace = workspace_for_runtime_test(&conversation_id, &project_id);
        workspace.linked_ideation_session_id = Some(parent_id.clone());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        register_runtime_context(&state, ChatContextType::Ideation, parent_id.as_str()).await;
        register_runtime_context(&state, ChatContextType::Ideation, child_id.as_str()).await;

        let statuses = get_agent_conversation_runtime_statuses_for_app_state(
            &state,
            execution_state,
            vec![conversation_id.as_str().to_string()],
        )
        .await
        .unwrap();
        let conversation_key = conversation_id.as_str();
        let runtime = statuses.get(&conversation_key).unwrap();

        assert!(runtime.is_running);
        assert_eq!(runtime.summary_label.as_deref(), Some("Verifying"));
        assert_eq!(
            runtime.primary_source,
            Some(AgentConversationRuntimeSource::Verification)
        );
        assert!(runtime.items.iter().any(|item| item.source
            == AgentConversationRuntimeSource::Ideation
            && item.context_id == parent_id.as_str()));
        let verification = runtime
            .items
            .iter()
            .find(|item| item.source == AgentConversationRuntimeSource::Verification)
            .expect("verification child item");
        assert_eq!(verification.context_id, child_id.as_str());
        assert_eq!(
            verification.parent_session_id.as_deref(),
            Some(parent_id.as_str())
        );
        assert_eq!(
            verification.child_session_id.as_deref(),
            Some(child_id.as_str())
        );
    }

    #[tokio::test]
    async fn agent_conversation_runtime_status_includes_workspace_review_child_chat() {
        let state = AppState::new_sqlite_test();
        let execution_state = Arc::new(ExecutionState::new());
        let project_id = ProjectId::from_string("project-workspace-review-runtime".to_string());
        let conversation_id = ChatConversationId::new();
        let review_conversation_id = ChatConversationId::new();

        let workspace = workspace_for_runtime_test(&conversation_id, &project_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        let mut review_conversation = ChatConversation::new_project(project_id.clone());
        review_conversation.id = review_conversation_id.clone();
        review_conversation.parent_conversation_id = Some(conversation_id.as_str());
        review_conversation.title = Some("Review workspace changes".to_string());
        state
            .chat_conversation_repo
            .create(review_conversation)
            .await
            .unwrap();

        let review_run = AgentRun::new(review_conversation_id.clone());
        let review_run_id = review_run.id;
        state.agent_run_repo.create(review_run).await.unwrap();
        state
            .running_agent_registry
            .register(
                RunningAgentKey::new(
                    ChatContextType::Project.to_string(),
                    review_conversation_id.as_str(),
                ),
                0,
                review_conversation_id.as_str(),
                review_run_id.as_str().to_string(),
                None,
                None,
            )
            .await;

        let mut monitor =
            AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id.clone());
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.review_conversation_id = Some(review_conversation_id.clone());
        monitor.last_run_id = Some(review_run_id.as_str().to_string());
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .unwrap();

        let statuses = get_agent_conversation_runtime_statuses_for_app_state(
            &state,
            execution_state,
            vec![conversation_id.as_str().to_string()],
        )
        .await
        .unwrap();
        let conversation_key = conversation_id.as_str();
        let review_conversation_key = review_conversation_id.as_str();
        let runtime = statuses.get(&conversation_key).unwrap();

        assert!(runtime.is_running);
        assert_eq!(runtime.summary_label.as_deref(), Some("Reviewing"));
        assert_eq!(
            runtime.primary_source,
            Some(AgentConversationRuntimeSource::WorkspaceReview)
        );
        assert_eq!(runtime.items.len(), 1);
        let item = &runtime.items[0];
        assert_eq!(item.source, AgentConversationRuntimeSource::WorkspaceReview);
        assert_eq!(item.context_type, "project");
        assert_eq!(item.context_id, review_conversation_key);
        assert_eq!(
            item.conversation_id.as_deref(),
            Some(review_conversation_key.as_str())
        );
        assert_eq!(item.title, "Review workspace changes");
        assert!(item.task_id.is_none());
    }

    #[tokio::test]
    async fn agent_conversation_runtime_status_ignores_terminal_workspace_review_child_run() {
        let state = AppState::new_sqlite_test();
        let execution_state = Arc::new(ExecutionState::new());
        let project_id =
            ProjectId::from_string("project-workspace-review-terminal-runtime".to_string());
        let conversation_id = ChatConversationId::new();
        let review_conversation_id = ChatConversationId::new();

        let workspace = workspace_for_runtime_test(&conversation_id, &project_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        let mut review_conversation = ChatConversation::new_project(project_id.clone());
        review_conversation.id = review_conversation_id.clone();
        review_conversation.parent_conversation_id = Some(conversation_id.as_str());
        review_conversation.title = Some("Review workspace changes".to_string());
        state
            .chat_conversation_repo
            .create(review_conversation)
            .await
            .unwrap();

        let mut review_run = AgentRun::new(review_conversation_id.clone());
        let review_run_id = review_run.id;
        review_run.fail("Workspace reviewer stopped by user");
        state.agent_run_repo.create(review_run).await.unwrap();

        let mut monitor =
            AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id.clone());
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.review_conversation_id = Some(review_conversation_id);
        monitor.last_run_id = Some(review_run_id.as_str().to_string());
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .unwrap();

        let statuses = get_agent_conversation_runtime_statuses_for_app_state(
            &state,
            execution_state,
            vec![conversation_id.as_str().to_string()],
        )
        .await
        .unwrap();
        let runtime = statuses.get(&conversation_id.as_str()).unwrap();

        assert!(!runtime.is_running);
        assert!(runtime.items.is_empty());
    }

    #[tokio::test]
    async fn agent_conversation_runtime_index_keeps_terminal_workspace_review_row() {
        let state = AppState::new_sqlite_test();
        let execution_state = ExecutionState::new();
        let project_id =
            ProjectId::from_string("project-workspace-review-index-terminal".to_string());
        let conversation_id = ChatConversationId::new();
        let review_conversation_id = ChatConversationId::new();

        let workspace = workspace_for_runtime_test(&conversation_id, &project_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        let mut review_conversation = ChatConversation::new_project(project_id.clone());
        review_conversation.id = review_conversation_id.clone();
        review_conversation.parent_conversation_id = Some(conversation_id.as_str());
        review_conversation.title = Some("Review workspace changes".to_string());
        state
            .chat_conversation_repo
            .create(review_conversation)
            .await
            .unwrap();

        let mut review_run = AgentRun::new(review_conversation_id.clone());
        let review_run_id = review_run.id;
        review_run.fail("Workspace reviewer stopped by user");
        state.agent_run_repo.create(review_run).await.unwrap();

        let mut monitor =
            AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id.clone());
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.review_conversation_id = Some(review_conversation_id.clone());
        monitor.last_run_id = Some(review_run_id.as_str().to_string());
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .unwrap();

        let index = get_agent_conversation_runtime_index_for_app_state(
            &state,
            &execution_state,
            conversation_id.as_str(),
        )
        .await
        .unwrap();

        assert_eq!(
            index.rows[0].group,
            AgentConversationRuntimeIndexGroup::Main
        );
        assert_eq!(
            index.rows[0].kind,
            AgentConversationRuntimeIndexKind::Workspace
        );
        let review = index
            .rows
            .iter()
            .find(|row| row.kind == AgentConversationRuntimeIndexKind::WorkspaceReview)
            .expect("durable workspace review row");
        assert_eq!(review.lifecycle, AgentConversationRuntimeLifecycle::Failed);
        assert_eq!(
            review.conversation_id.as_deref(),
            Some(review_conversation_id.as_str().as_str())
        );
        assert_eq!(
            review.error_message.as_deref(),
            Some("Workspace reviewer stopped by user")
        );
    }

    #[tokio::test]
    async fn agent_conversation_runtime_index_includes_terminal_children_and_planned_tasks() {
        let state = AppState::new_sqlite_test();
        let execution_state = ExecutionState::new();
        let project_id = ProjectId::from_string("project-runtime-index-children".to_string());
        let conversation_id = ChatConversationId::new();
        let plan_branch_id = PlanBranchId::from_string("plan-branch-runtime-index");
        let execution_plan_id = ExecutionPlanId::from_string("execution-plan-runtime-index");

        let parent = IdeationSession::new_with_title(project_id.clone(), "Plan draft");
        let parent_id = parent.id.clone();
        state.ideation_session_repo.create(parent).await.unwrap();

        let mut parent_conversation = ChatConversation::new_ideation(parent_id.clone());
        parent_conversation.provider_harness = Some(AgentHarnessKind::Codex);
        parent_conversation.provider_session_id = Some("codex-session-parent".to_string());
        let parent_conversation = state
            .chat_conversation_repo
            .create(parent_conversation)
            .await
            .unwrap();
        let mut parent_run = AgentRun::new(parent_conversation.id.clone());
        parent_run.harness = Some(AgentHarnessKind::Codex);
        parent_run.provider_session_id = Some("codex-run-parent".to_string());
        parent_run.complete();
        state.agent_run_repo.create(parent_run).await.unwrap();

        let mut child = IdeationSession::new_with_title(project_id.clone(), "Verification run");
        child.parent_session_id = Some(parent_id.clone());
        child.session_purpose = SessionPurpose::Verification;
        child.status = crate::domain::entities::IdeationSessionStatus::Accepted;
        let child_id = child.id.clone();
        state.ideation_session_repo.create(child).await.unwrap();

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-runtime-index"),
            parent_id.clone(),
            project_id.clone(),
            "ralphx/test-plan".to_string(),
            "main".to_string(),
        );
        plan_branch.id = plan_branch_id.clone();
        plan_branch.execution_plan_id = Some(execution_plan_id.clone());
        state.plan_branch_repo.create(plan_branch).await.unwrap();

        let mut workspace = workspace_for_runtime_test(&conversation_id, &project_id);
        workspace.linked_ideation_session_id = Some(parent_id.clone());
        workspace.linked_plan_branch_id = Some(plan_branch_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        let mut planned_task = Task::new(project_id.clone(), "Planned pipeline task".to_string());
        planned_task.internal_status = InternalStatus::Ready;
        planned_task.execution_plan_id = Some(execution_plan_id);
        let planned_task = state.task_repo.create(planned_task).await.unwrap();

        let index = get_agent_conversation_runtime_index_for_app_state(
            &state,
            &execution_state,
            conversation_id.as_str(),
        )
        .await
        .unwrap();

        let ideation = index
            .rows
            .iter()
            .find(|row| row.kind == AgentConversationRuntimeIndexKind::Ideation)
            .expect("ideation row");
        assert_eq!(
            ideation.lifecycle,
            AgentConversationRuntimeLifecycle::Completed
        );
        assert_eq!(ideation.provider_harness.as_deref(), Some("codex"));
        assert_eq!(
            ideation.provider_session_id.as_deref(),
            Some("codex-run-parent")
        );

        let verification = index
            .rows
            .iter()
            .find(|row| row.kind == AgentConversationRuntimeIndexKind::Verification)
            .expect("verification row");
        assert_eq!(
            verification.parent_session_id.as_deref(),
            Some(parent_id.as_str())
        );
        assert_eq!(
            verification.child_session_id.as_deref(),
            Some(child_id.as_str())
        );
        assert_eq!(
            verification.lifecycle,
            AgentConversationRuntimeLifecycle::Completed
        );

        let task = index
            .rows
            .iter()
            .find(|row| row.kind == AgentConversationRuntimeIndexKind::Task)
            .expect("planned task row");
        assert_eq!(task.task_id.as_deref(), Some(planned_task.id.as_str()));
        assert_eq!(task.lifecycle, AgentConversationRuntimeLifecycle::Queued);
        assert_eq!(task.status_label, "Queued");
        assert_eq!(task.group, AgentConversationRuntimeIndexGroup::Pipeline);
    }

    #[tokio::test]
    async fn agent_conversation_runtime_status_filters_task_runs_to_linked_plan_branch() {
        let state = AppState::new_sqlite_test();
        let execution_state = Arc::new(ExecutionState::new());
        let project_id = ProjectId::from_string("project-task-runtime-status".to_string());
        let conversation_id = ChatConversationId::new();
        let plan_branch_id = PlanBranchId::from_string("plan-branch-runtime-status");
        let execution_plan_id = ExecutionPlanId::from_string("execution-plan-runtime-status");
        let other_execution_plan_id = ExecutionPlanId::from_string("execution-plan-other");

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-runtime-status"),
            IdeationSessionId::from_string("session-runtime-status"),
            project_id.clone(),
            "ralphx/test-plan".to_string(),
            "main".to_string(),
        );
        plan_branch.id = plan_branch_id.clone();
        plan_branch.execution_plan_id = Some(execution_plan_id.clone());
        state.plan_branch_repo.create(plan_branch).await.unwrap();

        let mut workspace = workspace_for_runtime_test(&conversation_id, &project_id);
        workspace.linked_plan_branch_id = Some(plan_branch_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        let mut owned_task = Task::new(project_id.clone(), "Owned execution task".to_string());
        owned_task.internal_status = InternalStatus::Executing;
        owned_task.execution_plan_id = Some(execution_plan_id);
        let owned_task = state.task_repo.create(owned_task).await.unwrap();

        let mut unrelated_task = Task::new(project_id.clone(), "Other execution task".to_string());
        unrelated_task.internal_status = InternalStatus::Executing;
        unrelated_task.execution_plan_id = Some(other_execution_plan_id);
        let unrelated_task = state.task_repo.create(unrelated_task).await.unwrap();

        register_runtime_context(
            &state,
            ChatContextType::TaskExecution,
            owned_task.id.as_str(),
        )
        .await;
        register_runtime_context(
            &state,
            ChatContextType::TaskExecution,
            unrelated_task.id.as_str(),
        )
        .await;

        let statuses = get_agent_conversation_runtime_statuses_for_app_state(
            &state,
            execution_state,
            vec![conversation_id.as_str().to_string()],
        )
        .await
        .unwrap();
        let conversation_key = conversation_id.as_str();
        let runtime = statuses.get(&conversation_key).unwrap();

        assert!(runtime.is_running);
        assert_eq!(runtime.summary_label.as_deref(), Some("Executing"));
        assert_eq!(
            runtime.primary_source,
            Some(AgentConversationRuntimeSource::TaskExecution)
        );
        assert_eq!(runtime.items.len(), 1);
        let item = &runtime.items[0];
        assert_eq!(item.source, AgentConversationRuntimeSource::TaskExecution);
        assert_eq!(item.task_id.as_deref(), Some(owned_task.id.as_str()));
        assert_ne!(item.task_id.as_deref(), Some(unrelated_task.id.as_str()));
        assert_eq!(item.context_type, "task_execution");
    }

    #[tokio::test]
    async fn agent_conversation_runtime_status_reports_idle_workspace_ipr_as_waiting() {
        let state = AppState::new_sqlite_test();
        let execution_state = Arc::new(ExecutionState::new());
        let conversation_id = ChatConversationId::new();
        let run = AgentRun::new(conversation_id);
        let run_id = run.id;
        state.agent_run_repo.create(run).await.unwrap();
        state
            .running_agent_registry
            .register(
                RunningAgentKey::new(
                    ChatContextType::Project.to_string(),
                    conversation_id.as_str(),
                ),
                std::process::id(),
                conversation_id.as_str(),
                run_id.as_str().to_string(),
                None,
                None,
            )
            .await;
        execution_state
            .mark_interactive_idle(&agent_workspace_interactive_slot_key(&conversation_id));

        let statuses = get_agent_conversation_runtime_statuses_for_app_state(
            &state,
            execution_state,
            vec![conversation_id.as_str().to_string()],
        )
        .await
        .unwrap();
        let runtime = statuses.get(&conversation_id.as_str()).unwrap();

        assert!(runtime.is_running);
        assert_eq!(runtime.agent_status, AgentRuntimeStatus::WaitingForInput);
        assert_eq!(runtime.summary_label.as_deref(), Some("Awaiting input"));
        assert_eq!(runtime.items.len(), 1);
        let item = &runtime.items[0];
        assert_eq!(item.source, AgentConversationRuntimeSource::Workspace);
        assert_eq!(item.agent_status, AgentRuntimeStatus::WaitingForInput);
    }

    fn build_send_now_command_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .manage(state)
            .manage(Arc::new(ExecutionState::new()))
            .manage(Arc::new(TeamService::new_without_events(Arc::new(
                TeamStateTracker::new(),
            ))))
            .build(mock_context(noop_assets()))
            .expect("mock app should build")
    }

    #[tokio::test]
    async fn send_queued_agent_message_now_command_enables_ideation_team_mode() {
        let state = AppState::new_test();
        let session = IdeationSession::builder()
            .project_id(ProjectId::new())
            .team_mode("team")
            .build();
        let session_id = session.id.as_str().to_string();
        state
            .ideation_session_repo
            .create(session)
            .await
            .expect("session should persist");
        let app = build_send_now_command_app(state);
        let app_state = app.state::<AppState>();
        let execution_state = app.state::<Arc<ExecutionState>>();
        let team_service = app.state::<Arc<TeamService>>().inner().clone();

        let error = send_queued_agent_message_now_for_state(
            "ideation".to_string(),
            session_id,
            "missing-message".to_string(),
            app_state.inner(),
            execution_state.inner(),
            team_service,
            app.handle().clone(),
        )
        .await
        .expect_err("missing queued message should fail after command setup");

        assert!(error.contains("Queued message not found"));
    }

    #[tokio::test]
    async fn send_queued_agent_message_now_command_enables_task_team_mode() {
        let state = AppState::new_test();
        let mut task = Task::new(ProjectId::new(), "Team execution".to_string());
        task.metadata = Some(r#"{"agent_variant":"team"}"#.to_string());
        let task_id = task.id.as_str().to_string();
        state
            .task_repo
            .create(task)
            .await
            .expect("task should persist");
        let app = build_send_now_command_app(state);
        let app_state = app.state::<AppState>();
        let execution_state = app.state::<Arc<ExecutionState>>();
        let team_service = app.state::<Arc<TeamService>>().inner().clone();

        let error = send_queued_agent_message_now_for_state(
            "task_execution".to_string(),
            task_id,
            "missing-message".to_string(),
            app_state.inner(),
            execution_state.inner(),
            team_service,
            app.handle().clone(),
        )
        .await
        .expect_err("missing queued message should fail after command setup");

        assert!(error.contains("Queued message not found"));
    }

    #[test]
    fn normalized_effort_for_supported_keeps_supported_request_or_default() {
        let supported = [
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
        ];

        assert_eq!(
            normalized_effort_for_supported(
                Some(LogicalEffort::High),
                &supported,
                LogicalEffort::Medium,
            ),
            LogicalEffort::High
        );
        assert_eq!(
            normalized_effort_for_supported(
                Some(LogicalEffort::Max),
                &supported,
                LogicalEffort::Medium,
            ),
            LogicalEffort::Medium
        );
        assert_eq!(
            normalized_effort_for_supported(None, &supported, LogicalEffort::Low),
            LogicalEffort::Low
        );
    }

    #[test]
    fn normalize_agent_workspace_source_pull_request_trims_and_maps_valid_metadata() {
        let normalized = normalize_agent_workspace_source_pull_request(
            Some(AgentWorkspaceSourcePullRequestInput {
                number: 123,
                url: Some(" https://github.com/owner/repo/pull/123 ".to_string()),
                title: Some(" Add PR source context ".to_string()),
                head_ref_name: " feature/source-pr ".to_string(),
                base_ref_name: Some(" main ".to_string()),
                head_ref_oid: Some(" abc123 ".to_string()),
            }),
            Some(IdeationAnalysisBaseRefKind::LocalBranch),
            Some("feature/source-pr"),
        )
        .expect("valid source PR metadata should normalize")
        .expect("source PR metadata should be present");

        assert_eq!(normalized.number, 123);
        assert_eq!(
            normalized.url.as_deref(),
            Some("https://github.com/owner/repo/pull/123")
        );
        assert_eq!(normalized.title.as_deref(), Some("Add PR source context"));
        assert_eq!(normalized.head_ref_name, "feature/source-pr");
        assert_eq!(normalized.base_ref_name.as_deref(), Some("main"));
        assert_eq!(normalized.head_ref_oid.as_deref(), Some("abc123"));
    }

    #[test]
    fn normalize_agent_workspace_source_pull_request_validates_pr_base_contract() {
        let input = AgentWorkspaceSourcePullRequestInput {
            number: 123,
            url: None,
            title: None,
            head_ref_name: "feature/source-pr".to_string(),
            base_ref_name: None,
            head_ref_oid: None,
        };

        assert_eq!(
            normalize_agent_workspace_source_pull_request(
                Some(input.clone()),
                Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                Some("main"),
            )
            .expect_err("source PR metadata must use local branch base"),
            "Source pull request metadata requires a local_branch base ref"
        );
        assert_eq!(
            normalize_agent_workspace_source_pull_request(
                Some(input.clone()),
                Some(IdeationAnalysisBaseRefKind::LocalBranch),
                Some("different-branch"),
            )
            .expect_err("source PR head must match selected base"),
            "Source pull request head branch must match the selected base ref"
        );
        assert_eq!(
            normalize_agent_workspace_source_pull_request(
                Some(AgentWorkspaceSourcePullRequestInput { number: 0, ..input }),
                Some(IdeationAnalysisBaseRefKind::LocalBranch),
                Some("feature/source-pr"),
            )
            .expect_err("source PR number must be positive"),
            "Source pull request number must be positive"
        );
    }

    #[tokio::test]
    async fn normalize_agent_runtime_without_provider_preserves_overrides() {
        let state = AppState::new_test();

        let normalized = normalize_agent_runtime_selection(
            &state,
            None,
            Some("manual-model".to_string()),
            Some(LogicalEffort::Max),
        )
        .await
        .expect("normalization should preserve providerless overrides");

        assert_eq!(
            normalized,
            (Some("manual-model".to_string()), Some(LogicalEffort::Max))
        );
    }

    #[tokio::test]
    async fn normalize_agent_runtime_uses_known_model_compatibility() {
        let state = AppState::new_test();

        let normalized = normalize_agent_runtime_selection(
            &state,
            Some(AgentHarnessKind::Claude),
            Some("haiku".to_string()),
            Some(LogicalEffort::Max),
        )
        .await
        .expect("known model should normalize");

        assert_eq!(
            normalized,
            (Some("haiku".to_string()), Some(LogicalEffort::Medium))
        );
    }

    #[tokio::test]
    async fn normalize_agent_runtime_uses_provider_defaults_for_unknown_model() {
        let state = AppState::new_test();

        let normalized = normalize_agent_runtime_selection(
            &state,
            Some(AgentHarnessKind::Codex),
            Some("gpt-5.6".to_string()),
            Some(LogicalEffort::Max),
        )
        .await
        .expect("unknown model should use provider defaults");

        assert_eq!(
            normalized,
            (Some("gpt-5.6".to_string()), Some(LogicalEffort::XHigh))
        );
    }

    #[tokio::test]
    async fn normalize_agent_runtime_uses_registry_default_when_model_absent() {
        let state = AppState::new_test();

        let normalized = normalize_agent_runtime_selection(
            &state,
            Some(AgentHarnessKind::Codex),
            None,
            Some(LogicalEffort::Low),
        )
        .await
        .expect("missing model should use registry defaults");

        assert_eq!(normalized, (None, Some(LogicalEffort::Low)));
    }

    #[tokio::test]
    async fn normalize_agent_runtime_falls_back_when_provider_models_disabled() {
        let state = AppState::new_test();
        for model_id in [
            "sonnet",
            "claude-sonnet-4-6",
            "claude-sonnet-5",
            "opus",
            "haiku",
            "fable",
        ] {
            state
                .agent_model_registry_repo
                .upsert_custom_model(&AgentModelDefinition::custom(
                    AgentHarnessKind::Claude,
                    model_id,
                    model_id,
                    model_id,
                    None,
                    vec![LogicalEffort::Low],
                    LogicalEffort::Low,
                    false,
                ))
                .await
                .expect("disabled override should save");
        }

        let normalized = normalize_agent_runtime_selection(
            &state,
            Some(AgentHarnessKind::Claude),
            None,
            Some(LogicalEffort::Max),
        )
        .await
        .expect("missing enabled default should use provider fallback");

        assert_eq!(normalized, (None, Some(LogicalEffort::Medium)));
    }

    #[test]
    fn linked_plan_branch_publication_is_projected_into_workspace_response() {
        let mut response = AgentConversationWorkspaceResponse {
            conversation_id: "conversation-1".to_string(),
            project_id: "project-1".to_string(),
            mode: AgentConversationWorkspaceMode::Ideation.to_string(),
            branch_mode: "isolated".to_string(),
            base_ref_kind: "project_default".to_string(),
            base_ref: "main".to_string(),
            base_display_name: Some("Project default (main)".to_string()),
            base_commit: None,
            branch_name: "agent-d619a9fd".to_string(),
            worktree_path: "/tmp/workspace".to_string(),
            linked_ideation_session_id: Some("session-1".to_string()),
            linked_plan_branch_id: Some("plan-branch-1".to_string()),
            source_pull_request: None,
            publication_pr_number: None,
            publication_pr_url: None,
            publication_pr_status: None,
            publication_push_status: None,
            auto_publish_enabled: true,
            auto_publish_initial_pr_enabled: false,
            auto_publish_paused_pr_autofix_enabled: None,
            auto_publish_paused_pr_auto_merge_desired: None,
            pr_autofix_enabled: false,
            pr_auto_merge_desired: false,
            pr_auto_merge_method: DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string(),
            pr_auto_merge_current: None,
            pr_supervision_status: None,
            pr_supervision_summary: None,
            pr_supervision_updated_at: None,
            status: "active".to_string(),
            created_at: "2026-04-28T12:00:00+00:00".to_string(),
            updated_at: "2026-04-28T12:00:00+00:00".to_string(),
            mode_switch_locked: false,
            mode_switch_lock_reason: None,
        };
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-1"),
            IdeationSessionId::from_string("session-1"),
            ProjectId::from_string("project-1".to_string()),
            "agent-d619a9fd".to_string(),
            "feature/agent-screen".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Active;
        plan_branch.pr_number = Some(90);
        plan_branch.pr_url = Some("https://github.com/mock/project/pull/90".to_string());
        plan_branch.pr_status = Some(PrStatus::Open);
        plan_branch.pr_push_status = PrPushStatus::Pushed;

        project_plan_branch_publication_into_workspace_response(&mut response, &plan_branch);

        assert_eq!(response.publication_pr_number, Some(90));
        assert_eq!(
            response.publication_pr_url.as_deref(),
            Some("https://github.com/mock/project/pull/90")
        );
        assert_eq!(response.publication_pr_status.as_deref(), Some("open"));
        assert_eq!(response.publication_push_status.as_deref(), Some("pushed"));

        response.publication_pr_status = None;
        plan_branch.status = PlanBranchStatus::Merged;
        project_plan_branch_publication_into_workspace_response(&mut response, &plan_branch);

        assert_eq!(response.publication_pr_status.as_deref(), Some("merged"));
    }

    #[test]
    fn linked_plan_branch_publication_overrides_stale_workspace_publication_response() {
        let mut response = AgentConversationWorkspaceResponse {
            conversation_id: "conversation-1".to_string(),
            project_id: "project-1".to_string(),
            mode: AgentConversationWorkspaceMode::Ideation.to_string(),
            branch_mode: "isolated".to_string(),
            base_ref_kind: "project_default".to_string(),
            base_ref: "main".to_string(),
            base_display_name: Some("Project default (main)".to_string()),
            base_commit: None,
            branch_name: "agent-shell-branch".to_string(),
            worktree_path: "/tmp/workspace".to_string(),
            linked_ideation_session_id: Some("session-1".to_string()),
            linked_plan_branch_id: Some("plan-branch-1".to_string()),
            source_pull_request: None,
            publication_pr_number: Some(12),
            publication_pr_url: Some("https://github.com/mock/project/pull/12".to_string()),
            publication_pr_status: Some("open".to_string()),
            publication_push_status: Some("needs_agent".to_string()),
            auto_publish_enabled: true,
            auto_publish_initial_pr_enabled: false,
            auto_publish_paused_pr_autofix_enabled: None,
            auto_publish_paused_pr_auto_merge_desired: None,
            pr_autofix_enabled: false,
            pr_auto_merge_desired: false,
            pr_auto_merge_method: DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string(),
            pr_auto_merge_current: None,
            pr_supervision_status: None,
            pr_supervision_summary: None,
            pr_supervision_updated_at: None,
            status: "missing".to_string(),
            created_at: "2026-04-28T12:00:00+00:00".to_string(),
            updated_at: "2026-04-28T12:00:00+00:00".to_string(),
            mode_switch_locked: true,
            mode_switch_lock_reason: Some("Plan execution is still active".to_string()),
        };
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-1"),
            IdeationSessionId::from_string("session-1"),
            ProjectId::from_string("project-1".to_string()),
            "plan-branch".to_string(),
            "feature/agent-screen".to_string(),
        );
        plan_branch.pr_number = Some(90);
        plan_branch.pr_url = Some("https://github.com/mock/project/pull/90".to_string());
        plan_branch.pr_status = Some(PrStatus::Closed);
        plan_branch.pr_push_status = PrPushStatus::Pushed;

        project_plan_branch_publication_into_workspace_response(&mut response, &plan_branch);

        assert_eq!(response.publication_pr_number, Some(90));
        assert_eq!(
            response.publication_pr_url.as_deref(),
            Some("https://github.com/mock/project/pull/90")
        );
        assert_eq!(response.publication_pr_status.as_deref(), Some("closed"));
        assert_eq!(response.publication_push_status.as_deref(), Some("pushed"));
    }

    #[test]
    fn publish_repair_message_uses_effective_target_branch_and_base() {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("conversation-1"),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            "agent-shell-branch".to_string(),
            "/tmp/agent-shell".to_string(),
        );
        workspace.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-1"));
        let target = AgentConversationWorkspaceRepairTarget {
            branch_name: "plan-branch".to_string(),
            base_ref: "feature/agent-screen".to_string(),
            base_display_name: Some("Current branch (feature/agent-screen)".to_string()),
            worktree_path: Some(PathBuf::from("/tmp/project-repo")),
        };

        let message = build_agent_workspace_publish_repair_message_for_target(
            "merge conflict",
            &workspace,
            &target,
        );

        assert!(message.contains("Workspace branch: plan-branch"));
        assert!(message.contains("Base: Current branch (feature/agent-screen)"));
        assert!(message.contains("Base ref: feature/agent-screen"));
        assert!(!message.contains("agent-shell-branch"));
        assert!(!message.contains("Project default (main)"));
    }

    #[test]
    fn update_only_repair_action_metadata_is_preserved() {
        let workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("conversation-1"),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            "agent-branch".to_string(),
            "/tmp/agent-worktree".to_string(),
        );
        let target = AgentConversationWorkspaceRepairTarget {
            branch_name: "agent-branch".to_string(),
            base_ref: "main".to_string(),
            base_display_name: Some("Project default (main)".to_string()),
            worktree_path: Some(PathBuf::from("/tmp/agent-worktree")),
        };

        assert_eq!(
            AgentWorkspacePostRepairAction::Publish.classification(),
            "agent_fixable:publish"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::UpdateOnly.classification(),
            "agent_fixable:update_only"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::Publish.repair_requested_summary(),
            "Workspace agent repair requested before publishing can continue"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::UpdateOnly.repair_requested_summary(),
            "Workspace agent repair requested before the base update can complete"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::Publish.repair_sent_summary(),
            "Sent publish failure to workspace agent"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::UpdateOnly.repair_sent_summary(),
            "Sent base update failure to workspace agent"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::Publish.deferred_repair_sent_summary(),
            "Sent publish failure to workspace agent after active turn completed"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::UpdateOnly.deferred_repair_sent_summary(),
            "Sent base update failure to workspace agent after active turn completed"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::Publish.repair_send_failed_summary("unavailable"),
            "Failed to send publish failure to workspace agent: unavailable"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::UpdateOnly.repair_send_failed_summary("unavailable"),
            "Failed to send base update failure to workspace agent: unavailable"
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::from_classification(Some("agent_fixable:publish")),
            Some(AgentWorkspacePostRepairAction::Publish)
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::from_classification(Some("agent_fixable:update_only")),
            Some(AgentWorkspacePostRepairAction::UpdateOnly)
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::from_classification(Some("agent_fixable:unknown")),
            None
        );
        assert_eq!(
            AgentWorkspacePostRepairAction::from_classification(None),
            None
        );

        let message = build_agent_workspace_repair_message_for_target(
            "merge conflict",
            &workspace,
            &target,
            AgentWorkspacePostRepairAction::UpdateOnly,
        );
        assert!(message.contains("Update from base failed for this agent workspace."));
        assert!(message.contains("Please fix the workspace so the base update can be completed."));

        let events = vec![
            AgentConversationWorkspacePublicationEvent::new(
                workspace.conversation_id,
                "repair_requested",
                "started",
                "publish repair",
                Some("agent_fixable:publish".to_string()),
            ),
            AgentConversationWorkspacePublicationEvent::new(
                workspace.conversation_id,
                "repair_requested",
                "started",
                "update repair",
                Some("agent_fixable:update_only".to_string()),
            ),
        ];
        assert_eq!(
            agent_workspace_post_repair_action_from_events(&events),
            AgentWorkspacePostRepairAction::UpdateOnly
        );
    }

    #[tokio::test]
    async fn publish_repair_message_routes_spawn_to_effective_target_worktree() {
        let service = MockChatService::new();
        let workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("conversation-1"),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            "agent-shell-branch".to_string(),
            "/tmp/agent-shell".to_string(),
        );
        let target = AgentConversationWorkspaceRepairTarget {
            branch_name: "plan-branch".to_string(),
            base_ref: "feature/agent-screen".to_string(),
            base_display_name: Some("Current branch (feature/agent-screen)".to_string()),
            worktree_path: Some(PathBuf::from("/tmp/project-repo")),
        };

        send_agent_workspace_publish_repair_message_for_target(
            &service,
            &workspace,
            "merge conflict",
            AgentWorkspaceRepairRuntimeOverrides::default(),
            &target,
        )
        .await
        .expect("repair message should send");

        let options = service.get_sent_options().await;
        assert_eq!(options.len(), 1);
        assert_eq!(
            options[0].working_directory_override.as_deref(),
            Some(Path::new("/tmp/project-repo"))
        );
    }

    #[tokio::test]
    async fn repair_message_defers_only_when_app_handle_available_and_workspace_agent_running() {
        let workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("conversation-1"),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            "agent-branch".to_string(),
            "/tmp/agent-worktree".to_string(),
        );
        let registry = Arc::new(MemoryRunningAgentRegistry::new());
        registry
            .set_running(RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                workspace.conversation_id.as_str(),
            ))
            .await;
        let registry_trait: Arc<dyn RunningAgentRegistry> = registry.clone();

        assert!(
            should_defer_agent_workspace_repair_message_for_registry(
                true,
                &registry_trait,
                None,
                &workspace
            )
            .await
        );
        assert!(
            !should_defer_agent_workspace_repair_message_for_registry(
                false,
                &registry_trait,
                None,
                &workspace
            )
            .await
        );
        let execution_state = Arc::new(ExecutionState::new());
        execution_state.mark_interactive_idle(&agent_workspace_interactive_slot_key(
            &workspace.conversation_id,
        ));
        assert!(
            !should_defer_agent_workspace_repair_message_for_registry(
                true,
                &registry_trait,
                Some(&execution_state),
                &workspace
            )
            .await
        );

        let idle_registry: Arc<dyn RunningAgentRegistry> =
            Arc::new(MemoryRunningAgentRegistry::new());
        assert!(
            !should_defer_agent_workspace_repair_message_for_registry(
                true,
                &idle_registry,
                None,
                &workspace
            )
            .await
        );
    }

    #[tokio::test]
    async fn repair_wait_releases_when_ipr_is_idle_or_process_exited() {
        let state = AppState::new_test();
        let workspace = command_test_workspace();
        let key = RunningAgentKey::new(
            ChatContextType::Project.to_string(),
            workspace.conversation_id.as_str(),
        );
        let interactive_slot_key = agent_workspace_interactive_slot_key(&workspace.conversation_id);
        let execution_state = Arc::new(ExecutionState::new());

        assert!(
            agent_workspace_repair_wait_released(
                &state,
                Some(&execution_state),
                &key,
                &interactive_slot_key,
            )
            .await,
            "Codex-style process exit should release the deferred repair"
        );

        state
            .running_agent_registry
            .register(
                key.clone(),
                123,
                workspace.conversation_id.as_str(),
                "run-repair-wait".to_string(),
                None,
                None,
            )
            .await;

        assert!(
            !agent_workspace_repair_wait_released(
                &state,
                Some(&execution_state),
                &key,
                &interactive_slot_key,
            )
            .await,
            "active generation should keep the repair deferred"
        );

        execution_state.mark_interactive_idle(&interactive_slot_key);
        assert!(
            agent_workspace_repair_wait_released(
                &state,
                Some(&execution_state),
                &key,
                &interactive_slot_key,
            )
            .await,
            "Claude-style reusable idle process should release the deferred repair"
        );
    }

    #[tokio::test]
    async fn fixable_publish_failure_routes_repair_and_records_events() {
        let state = AppState::new_test();
        let workspace = command_test_workspace();
        let target = AgentConversationWorkspaceRepairTarget::from_workspace(&workspace);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");
        let service = MockChatService::new();

        mark_agent_workspace_failure_with_routing_and_action(
            &state,
            &workspace,
            "merge conflict while updating from base",
            None,
            &service,
            true,
            &target,
            AgentWorkspacePostRepairAction::Publish,
        )
        .await;

        let messages = service.get_sent_messages().await;
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Commit & Publish failed for this agent workspace."));
        assert!(messages[0].contains("Workspace branch: ralphx/test/agent-command"));

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.iter().any(|event| {
            event.step == "needs_agent"
                && event.status == "failed"
                && event.classification.as_deref() == Some("agent_fixable")
        }));
        assert!(events.iter().any(|event| {
            event.step == "repair_requested"
                && event.status == "started"
                && event.classification.as_deref() == Some("agent_fixable:publish")
                && event.summary.contains("publishing can continue")
        }));
        assert!(events.iter().any(|event| {
            event.step == "repair_sent"
                && event.status == "succeeded"
                && event.summary == "Sent publish failure to workspace agent"
        }));
    }

    #[tokio::test]
    async fn fixable_update_failure_records_repair_send_failure() {
        let state = AppState::new_test();
        let workspace = command_test_workspace();
        let target = AgentConversationWorkspaceRepairTarget::from_workspace(&workspace);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");
        let service = MockChatService::new();
        service.set_available(false).await;

        mark_agent_workspace_failure_with_routing_and_action(
            &state,
            &workspace,
            "merge conflict while updating from base",
            None,
            &service,
            true,
            &target,
            AgentWorkspacePostRepairAction::UpdateOnly,
        )
        .await;

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.iter().any(|event| {
            event.step == "repair_requested"
                && event.classification.as_deref() == Some("agent_fixable:update_only")
                && event.summary.contains("base update can complete")
        }));
        assert!(events.iter().any(|event| {
            event.step == "repair_sent"
                && event.status == "failed"
                && event.summary.contains("Failed to send base update failure")
                && event.classification.as_deref() == Some("operational")
        }));
    }

    #[tokio::test]
    async fn pr_supervision_enable_marks_draft_ready_and_enables_auto_merge() {
        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(251);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/251".to_string());
        workspace.publication_pr_status = Some("draft".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let response = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: true,
                auto_merge_desired: true,
                auto_merge_method: Some(" ReBase ".to_string()),
            },
            &state,
        )
        .await
        .expect("PR supervision should enable");

        assert!(response.pr_autofix_enabled);
        assert!(response.pr_auto_merge_desired);
        assert_eq!(response.pr_auto_merge_method, "rebase");
        assert_eq!(response.pr_auto_merge_current, Some(true));
        assert_eq!(
            response.pr_supervision_status.as_deref(),
            Some("monitoring")
        );
        assert!(response
            .pr_supervision_summary
            .as_deref()
            .unwrap_or_default()
            .contains("auto-merge is enabled"));

        {
            let github_state = github.state();
            assert_eq!(github_state.mark_pr_ready_calls, 1);
            assert_eq!(github_state.last_mark_pr_ready_number, Some(251));
            assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
            assert_eq!(
                github_state.last_enable_pr_auto_merge_args.as_ref(),
                Some(&(251, "rebase".to_string()))
            );
        }

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.iter().any(|event| {
            event.step == "pr_supervision"
                && event.status == "enabled"
                && event.classification.as_deref() == Some("pr_supervision_preferences")
        }));
    }

    #[tokio::test]
    async fn pr_supervision_enable_uses_linked_plan_branch_pr_for_ideation_workspace() {
        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_publish_repo(&repo_path);
        let plan_branch_name = "ralphx/test/plan-pr-supervision";
        git(&repo_path, &["checkout", "-b", plan_branch_name]);
        git(&repo_path, &["checkout", "main"]);

        let mut project = Project::new(
            "Plan PR supervision".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project should persist");

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-plan-pr-supervision"),
            IdeationSessionId::from_string("session-plan-pr-supervision"),
            project.id.clone(),
            plan_branch_name.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Active;
        plan_branch.pr_number = Some(377);
        plan_branch.pr_url = Some("https://github.com/owner/repo/pull/377".to_string());
        plan_branch.pr_status = Some(PrStatus::Draft);
        plan_branch.pr_push_status = PrPushStatus::Pushed;
        let plan_branch_id = plan_branch.id.clone();
        let expected_plan_worktree =
            resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
                .expect("plan worktree path should resolve");
        state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch should persist");

        let mut workspace = command_test_workspace();
        workspace.project_id = project.id.clone();
        workspace.mode = AgentConversationWorkspaceMode::Ideation;
        workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string(
            "session-plan-pr-supervision",
        ));
        workspace.linked_plan_branch_id = Some(plan_branch_id);
        workspace.publication_pr_number = None;
        workspace.publication_pr_url = None;
        workspace.publication_pr_status = None;
        workspace.publication_push_status = None;
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let response = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: true,
                auto_merge_desired: true,
                auto_merge_method: Some("squash".to_string()),
            },
            &state,
        )
        .await
        .expect("linked plan branch PR supervision should enable");

        assert_eq!(response.publication_pr_number, Some(377));
        assert_eq!(
            response.publication_pr_url.as_deref(),
            Some("https://github.com/owner/repo/pull/377")
        );
        assert_eq!(response.publication_pr_status.as_deref(), Some("draft"));
        assert_eq!(response.publication_push_status.as_deref(), Some("pushed"));
        assert!(response.pr_autofix_enabled);
        assert!(response.pr_auto_merge_desired);
        assert_eq!(response.pr_auto_merge_current, Some(true));

        let persisted = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(persisted.publication_pr_number, Some(377));
        assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
        assert_eq!(
            GitService::get_current_branch(&expected_plan_worktree)
                .await
                .expect("plan worktree branch should be readable"),
            plan_branch_name
        );

        let github_state = github.state();
        assert_eq!(github_state.mark_pr_ready_calls, 1);
        assert_eq!(github_state.last_mark_pr_ready_number, Some(377));
        assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
        assert_eq!(
            github_state.last_enable_pr_auto_merge_args.as_ref(),
            Some(&(377, "squash".to_string()))
        );
    }

    #[tokio::test]
    async fn pr_supervision_disable_uses_linked_plan_pr_without_ensuring_locked_plan_worktree() {
        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(true)));
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let repo_path_string = repo_path.to_string_lossy().to_string();
        let worktree_parent = temp.path().join("worktrees");
        setup_publish_repo(&repo_path);
        let plan_branch_name = "ralphx/test/plan-pr-disable";
        git(&repo_path, &["checkout", "-b", plan_branch_name]);
        git(&repo_path, &["checkout", "main"]);
        let other_worktree_path = temp.path().join("active-merge-worktree");
        let other_worktree_arg = other_worktree_path.to_string_lossy().to_string();
        git(
            &repo_path,
            &[
                "worktree",
                "add",
                other_worktree_arg.as_str(),
                plan_branch_name,
            ],
        );

        let mut project = Project::new("Plan PR disable".to_string(), repo_path_string.clone());
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project should persist");

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-plan-pr-disable"),
            IdeationSessionId::from_string("session-plan-pr-disable"),
            project.id.clone(),
            plan_branch_name.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Active;
        plan_branch.pr_number = Some(630);
        plan_branch.pr_url = Some("https://github.com/owner/repo/pull/630".to_string());
        plan_branch.pr_status = Some(PrStatus::Open);
        plan_branch.pr_push_status = PrPushStatus::Pushed;
        let plan_branch_id = plan_branch.id.clone();
        let expected_plan_worktree =
            resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
                .expect("plan worktree path should resolve");
        state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch should persist");

        let mut workspace = command_test_workspace();
        workspace.project_id = project.id.clone();
        workspace.mode = AgentConversationWorkspaceMode::Ideation;
        workspace.linked_ideation_session_id =
            Some(IdeationSessionId::from_string("session-plan-pr-disable"));
        workspace.linked_plan_branch_id = Some(plan_branch_id);
        workspace.publication_pr_number = None;
        workspace.publication_pr_url = None;
        workspace.publication_pr_status = None;
        workspace.publication_push_status = None;
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        workspace.pr_auto_merge_current = Some(true);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let response = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: false,
                auto_merge_desired: false,
                auto_merge_method: None,
            },
            &state,
        )
        .await
        .expect("linked plan branch PR supervision should disable without ensuring worktree");

        assert!(!response.pr_autofix_enabled);
        assert!(!response.pr_auto_merge_desired);
        assert_eq!(response.pr_auto_merge_current, Some(false));
        assert_eq!(response.publication_pr_number, Some(630));
        assert_eq!(
            response.publication_pr_url.as_deref(),
            Some("https://github.com/owner/repo/pull/630")
        );
        assert_eq!(response.publication_pr_status.as_deref(), Some("open"));
        assert_eq!(response.publication_push_status.as_deref(), Some("pushed"));

        let persisted = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert!(!persisted.pr_auto_merge_desired);
        assert_eq!(persisted.publication_pr_number, Some(630));
        assert!(!expected_plan_worktree.exists());
        assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");

        let github_state = github.state();
        assert_eq!(github_state.fetch_pr_health_calls, 1);
        assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
        assert_eq!(github_state.last_disable_pr_auto_merge_number, Some(630));
        assert_eq!(
            github_state
                .last_disable_pr_auto_merge_working_dir
                .as_deref(),
            Some(repo_path_string.as_str())
        );
    }

    #[tokio::test]
    async fn pr_supervision_enable_rejects_locked_linked_plan_worktree_before_persisting() {
        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_publish_repo(&repo_path);
        let plan_branch_name = "ralphx/test/plan-pr-enable-locked";
        git(&repo_path, &["checkout", "-b", plan_branch_name]);
        git(&repo_path, &["checkout", "main"]);
        let other_worktree_path = temp.path().join("active-merge-worktree");
        let other_worktree_arg = other_worktree_path.to_string_lossy().to_string();
        git(
            &repo_path,
            &[
                "worktree",
                "add",
                other_worktree_arg.as_str(),
                plan_branch_name,
            ],
        );

        let mut project = Project::new(
            "Plan PR enable locked".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project should persist");

        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-plan-pr-enable-locked"),
            IdeationSessionId::from_string("session-plan-pr-enable-locked"),
            project.id.clone(),
            plan_branch_name.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Active;
        plan_branch.pr_number = Some(631);
        plan_branch.pr_url = Some("https://github.com/owner/repo/pull/631".to_string());
        plan_branch.pr_status = Some(PrStatus::Open);
        plan_branch.pr_push_status = PrPushStatus::Pushed;
        let plan_branch_id = plan_branch.id.clone();
        let expected_plan_worktree =
            resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
                .expect("plan worktree path should resolve");
        state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch should persist");

        let mut workspace = command_test_workspace();
        workspace.project_id = project.id.clone();
        workspace.mode = AgentConversationWorkspaceMode::Ideation;
        workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string(
            "session-plan-pr-enable-locked",
        ));
        workspace.linked_plan_branch_id = Some(plan_branch_id);
        workspace.publication_pr_number = None;
        workspace.publication_pr_url = None;
        workspace.publication_pr_status = None;
        workspace.publication_push_status = None;
        workspace.pr_autofix_enabled = false;
        workspace.pr_auto_merge_desired = false;
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let error = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: true,
                auto_merge_desired: true,
                auto_merge_method: Some("squash".to_string()),
            },
            &state,
        )
        .await
        .expect_err("locked linked plan branch should reject enable");

        assert!(error.contains("already checked out at"));
        assert!(error.contains("refusing to move or delete another worktree"));

        let persisted = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert!(!persisted.pr_autofix_enabled);
        assert!(!persisted.pr_auto_merge_desired);
        assert_eq!(persisted.publication_pr_number, None);
        assert!(!expected_plan_worktree.exists());

        let github_state = github.state();
        assert_eq!(github_state.enable_pr_auto_merge_calls, 0);
        assert_eq!(github_state.mark_pr_ready_calls, 0);
    }

    #[tokio::test]
    async fn pr_supervision_enable_records_waiting_when_auto_merge_enable_fails() {
        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.state().enable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
            "GitHub auto-merge is not ready".to_string(),
        )));
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(254);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/254".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let response = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: true,
                auto_merge_desired: true,
                auto_merge_method: Some("squash".to_string()),
            },
            &state,
        )
        .await
        .expect("PR supervision should persist even when GitHub auto-merge waits");

        assert!(response.pr_autofix_enabled);
        assert!(response.pr_auto_merge_desired);
        assert_eq!(response.pr_auto_merge_current, Some(false));
        assert_eq!(response.pr_supervision_status.as_deref(), Some("waiting"));
        assert!(response
            .pr_supervision_summary
            .as_deref()
            .unwrap_or_default()
            .contains("could not be enabled yet"));

        {
            let github_state = github.state();
            assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
        }

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.iter().any(|event| {
            event.step == "pr_supervision"
                && event.status == "enabled"
                && event
                    .summary
                    .contains("request GitHub auto-merge when possible")
        }));
    }

    #[tokio::test]
    async fn pr_supervision_disable_turns_off_existing_auto_merge() {
        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(true)));
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(252);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        workspace.pr_auto_merge_current = Some(true);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let response = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: false,
                auto_merge_desired: false,
                auto_merge_method: None,
            },
            &state,
        )
        .await
        .expect("PR supervision should disable");

        assert!(!response.pr_autofix_enabled);
        assert!(!response.pr_auto_merge_desired);
        assert_eq!(
            response.pr_auto_merge_method,
            DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD
        );
        assert_eq!(response.pr_auto_merge_current, Some(false));
        assert_eq!(
            response.pr_supervision_status.as_deref(),
            Some("disabled")
        );
        assert!(response
            .pr_supervision_summary
            .as_deref()
            .unwrap_or_default()
            .contains("auto-merge is disabled"));

        {
            let github_state = github.state();
            assert_eq!(github_state.fetch_pr_health_calls, 1);
            assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
            assert_eq!(github_state.last_disable_pr_auto_merge_number, Some(252));
        }

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.iter().any(|event| {
            event.step == "pr_supervision"
                && event.status == "disabled"
                && event.summary == "RalphX PR supervision is disabled."
        }));
    }

    #[tokio::test]
    async fn pr_supervision_disable_treats_absent_remote_auto_merge_as_idempotent() {
        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(false)));
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(253);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        workspace.pr_auto_merge_current = Some(true);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let response = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: false,
                auto_merge_desired: false,
                auto_merge_method: None,
            },
            &state,
        )
        .await
        .expect("PR supervision should disable idempotently when GitHub auto-merge is absent");

        assert!(!response.pr_autofix_enabled);
        assert!(!response.pr_auto_merge_desired);
        assert_eq!(response.pr_auto_merge_current, Some(false));
        assert_eq!(response.pr_supervision_status.as_deref(), Some("disabled"));

        let persisted = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert!(!persisted.pr_auto_merge_desired);
        assert_eq!(persisted.pr_auto_merge_current, Some(false));

        {
            let github_state = github.state();
            assert_eq!(github_state.fetch_pr_health_calls, 1);
            assert_eq!(github_state.disable_pr_auto_merge_calls, 0);
        }

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.iter().any(|event| {
            event.step == "pr_supervision"
                && event.status == "disabled"
                && event.summary == "RalphX PR supervision is disabled."
        }));
    }

    #[tokio::test]
    async fn pr_supervision_disable_records_waiting_when_auto_merge_disable_fails() {
        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(true)));
        github.state().disable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
            "GitHub auto-merge cannot be disabled yet".to_string(),
        )));
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(255);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        workspace.pr_auto_merge_current = Some(true);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let response = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: false,
                auto_merge_desired: false,
                auto_merge_method: None,
            },
            &state,
        )
        .await
        .expect("PR supervision preference should persist even when GitHub disable waits");

        assert!(!response.pr_autofix_enabled);
        assert!(!response.pr_auto_merge_desired);
        assert_eq!(response.pr_auto_merge_current, Some(true));
        assert_eq!(response.pr_supervision_status.as_deref(), Some("waiting"));
        assert!(response
            .pr_supervision_summary
            .as_deref()
            .unwrap_or_default()
            .contains("could not be disabled yet"));

        {
            let github_state = github.state();
            assert_eq!(github_state.fetch_pr_health_calls, 1);
            assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
        }

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.iter().any(|event| {
            event.step == "pr_supervision"
                && event.status == "disabled"
                && event.summary == "RalphX PR supervision is disabled."
        }));
    }

    #[tokio::test]
    async fn auto_publish_pause_disables_and_restores_pr_supervision_preferences() {
        let state = AppState::new_test();
        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(256);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let paused = set_agent_conversation_workspace_auto_publish_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspaceAutoPublishInput {
                auto_publish_enabled: false,
            },
            &state,
        )
        .await
        .expect("Auto Publish should pause");

        assert!(!paused.auto_publish_enabled);
        assert_eq!(paused.auto_publish_paused_pr_autofix_enabled, Some(true));
        assert_eq!(paused.auto_publish_paused_pr_auto_merge_desired, Some(true));
        assert!(!paused.pr_autofix_enabled);
        assert!(!paused.pr_auto_merge_desired);
        assert_eq!(paused.pr_supervision_status.as_deref(), Some("paused"));

        let resumed = set_agent_conversation_workspace_auto_publish_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspaceAutoPublishInput {
                auto_publish_enabled: true,
            },
            &state,
        )
        .await
        .expect("Auto Publish should resume");

        assert!(resumed.auto_publish_enabled);
        assert_eq!(resumed.auto_publish_paused_pr_autofix_enabled, None);
        assert_eq!(resumed.auto_publish_paused_pr_auto_merge_desired, None);
        assert!(resumed.pr_autofix_enabled);
        assert!(resumed.pr_auto_merge_desired);
        assert_eq!(resumed.pr_supervision_status.as_deref(), Some("monitoring"));

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.iter().any(|event| {
            event.step == "auto_publish"
                && event.status == "disabled"
                && event.classification.as_deref() == Some("auto_publish_preferences")
        }));
        assert!(events
            .iter()
            .any(|event| event.step == "auto_publish" && event.status == "enabled"));
    }

    #[tokio::test]
    async fn auto_publish_enable_before_pr_sets_initial_pr_opt_in() {
        let state = AppState::new_test();
        let workspace = command_test_workspace();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let updated = set_agent_conversation_workspace_auto_publish_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspaceAutoPublishInput {
                auto_publish_enabled: true,
            },
            &state,
        )
        .await
        .expect("Auto Publish should enable before PR publication");

        assert!(updated.auto_publish_enabled);
        assert!(updated.auto_publish_initial_pr_enabled);

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.iter().any(|event| {
            event.step == "auto_publish"
                && event.status == "enabled"
                && event.summary == "Auto Publish is enabled for the first pull request."
        }));
    }

    #[tokio::test]
    async fn pr_supervision_rejects_enable_when_auto_publish_is_paused() {
        let state = AppState::new_test();
        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(257);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.auto_publish_enabled = false;
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let error = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: true,
                auto_merge_desired: false,
                auto_merge_method: Some("squash".to_string()),
            },
            &state,
        )
        .await
        .expect_err("PR supervision enable should be rejected while paused");

        assert!(error.contains("Auto Publish is paused"));
    }

    #[tokio::test]
    async fn pr_supervision_rejects_terminal_pr_and_invalid_merge_method() {
        let state = AppState::new_test();
        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(253);
        workspace.publication_pr_status = Some("merged".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");

        let terminal_error = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: true,
                auto_merge_desired: false,
                auto_merge_method: Some("squash".to_string()),
            },
            &state,
        )
        .await
        .expect_err("terminal PR supervision should be rejected");
        assert!(terminal_error.contains("closed or merged PR"));

        let method_error = set_agent_conversation_workspace_pr_supervision_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspacePrSupervisionInput {
                auto_fix_enabled: true,
                auto_merge_desired: true,
                auto_merge_method: Some("octopus".to_string()),
            },
            &state,
        )
        .await
        .expect_err("invalid auto-merge method should be rejected before workspace load");
        assert!(method_error.contains("Unsupported auto-merge method"));
    }

    #[tokio::test]
    async fn deferred_repair_spawn_without_app_handle_noops() {
        let state = AppState::new_test();
        let workspace = command_test_workspace();
        let target = AgentConversationWorkspaceRepairTarget::from_workspace(&workspace);

        spawn_deferred_agent_workspace_repair_message(
            &state,
            workspace.clone(),
            "merge conflict while updating from base".to_string(),
            AgentWorkspaceRepairRuntimeOverrides::default(),
            target,
            AgentWorkspacePostRepairAction::Publish,
        )
        .await;

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("events should list");
        assert!(events.is_empty());
    }

    fn retargeted_base_resolution() -> BaseResolutionResult {
        BaseResolutionResult {
            status: BaseStatus::Retargeted,
            old_base_ref: "feature/deleted-base".to_string(),
            effective_base_ref: Some("main".to_string()),
            effective_checkout_ref: Some("origin/main".to_string()),
            effective_base_commit: Some("main-sha".to_string()),
            display_name: Some("Project default (main)".to_string()),
            block_reason: None,
        }
    }

    fn blocked_base_resolution(reason: &str) -> BaseResolutionResult {
        BaseResolutionResult {
            status: BaseStatus::Blocked,
            old_base_ref: "feature/deleted-base".to_string(),
            effective_base_ref: None,
            effective_checkout_ref: None,
            effective_base_commit: None,
            display_name: None,
            block_reason: Some(reason.to_string()),
        }
    }

    #[test]
    fn normalize_explicit_publish_base_selection_trims_defaults_and_rejects_prs() {
        assert!(normalize_explicit_publish_base_selection(
            AgentConversationWorkspaceBaseSelection {
                kind: None,
                branch_mode: None,
                base_ref: Some("  ".to_string()),
                display_name: Some("ignored".to_string()),
                source_pull_request: None,
            }
        )
        .expect("blank base ref should be allowed as no explicit selection")
        .is_none());

        let local =
            normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
                kind: None,
                branch_mode: None,
                base_ref: Some("  release/0.8  ".to_string()),
                display_name: None,
                source_pull_request: None,
            })
            .expect("local branch should normalize")
            .expect("local branch should produce a selection");
        assert_eq!(local.kind, IdeationAnalysisBaseRefKind::LocalBranch);
        assert_eq!(local.base_ref, "release/0.8");
        assert_eq!(local.display_name, "release/0.8");

        let project =
            normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                branch_mode: None,
                base_ref: Some("main".to_string()),
                display_name: Some("  ".to_string()),
                source_pull_request: None,
            })
            .expect("project default should normalize")
            .expect("project default should produce a selection");
        assert_eq!(project.display_name, "Project default (main)");

        let current =
            normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::CurrentBranch),
                branch_mode: None,
                base_ref: Some("feature/base".to_string()),
                display_name: None,
                source_pull_request: None,
            })
            .expect("current branch should normalize")
            .expect("current branch should produce a selection");
        assert_eq!(current.display_name, "Current branch (feature/base)");

        let source_pull_request = AgentWorkspaceSourcePullRequest {
            number: 42,
            url: Some("https://github.com/mock/repo/pull/42".to_string()),
            title: Some("Add PR base".to_string()),
            head_ref_name: "feature/pr-base".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some("pr-head-sha".to_string()),
        };
        let pr_base =
            normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
                branch_mode: None,
                base_ref: Some("feature/pr-base".to_string()),
                display_name: Some("PR #42: Add PR base".to_string()),
                source_pull_request: Some(source_pull_request.clone()),
            })
            .expect("PR-backed local branch should normalize")
            .expect("PR-backed local branch should produce a selection");
        assert_eq!(pr_base.kind, IdeationAnalysisBaseRefKind::LocalBranch);
        assert_eq!(pr_base.base_ref, "feature/pr-base");
        assert_eq!(pr_base.display_name, "PR #42: Add PR base");
        assert_eq!(pr_base.source_pull_request, Some(source_pull_request));

        let error =
            normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::PullRequest),
                branch_mode: None,
                base_ref: Some("123".to_string()),
                display_name: None,
                source_pull_request: None,
            })
            .expect_err("pull-request bases should be rejected");
        assert!(error.contains("Pull-request base refs are not supported"));
    }

    #[tokio::test]
    async fn validate_explicit_publish_base_ref_accepts_remote_tracking_ref() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        setup_publish_repo(&repo_path);
        let head = git(&repo_path, &["rev-parse", "HEAD"]);
        git(
            &repo_path,
            &["update-ref", "refs/remotes/origin/release/0.8", &head],
        );

        validate_explicit_publish_base_ref(&repo_path, "release/0.8")
            .await
            .expect("remote-tracking branch should validate");
        let error = validate_explicit_publish_base_ref(&repo_path, "release/missing")
            .await
            .expect_err("missing branch should fail validation");
        assert!(error.contains("Selected base branch 'release/missing' does not exist"));
    }

    fn command_test_workspace() -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            ChatConversationId::from_string("conversation-command-base"),
            ProjectId::from_string("project-command-base".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::CurrentBranch,
            "feature/deleted-base".to_string(),
            Some("Current branch (feature/deleted-base)".to_string()),
            Some("old-base-sha".to_string()),
            "ralphx/test/agent-command".to_string(),
            "/tmp/agent-command-workspace".to_string(),
        )
    }

    fn command_test_pr_health(auto_merge_active: bool) -> PrHealth {
        PrHealth {
            sync_state: PrSyncState {
                status: GithubPrStatus::Open,
                merge_state_status: None,
                mergeable: None,
                is_draft: false,
                head_ref_name: "feature".to_string(),
                base_ref_name: "main".to_string(),
                head_ref_oid: None,
                base_ref_oid: None,
            },
            review_decision: None,
            checks: Vec::new(),
            issue_comments: Vec::new(),
            auto_merge_request: if auto_merge_active {
                Some(PrAutoMergeRequest {
                    enabled_by: Some("github-user".to_string()),
                    merge_method: Some("squash".to_string()),
                })
            } else {
                None
            },
        }
    }

    fn command_publish_target() -> AgentConversationWorkspacePublishTarget {
        AgentConversationWorkspacePublishTarget {
            worktree_path: PathBuf::from("/tmp/project-repo"),
            branch_name: "ralphx/test/agent-command".to_string(),
            base_ref: "feature/deleted-base".to_string(),
            base_display_name: Some("Current branch (feature/deleted-base)".to_string()),
            plan_branch: None,
        }
    }

    fn external_pr_test_project(name: &str) -> Project {
        let mut project = Project::new(name.to_string(), format!("/tmp/{name}"));
        project.base_branch = Some("main".to_string());
        project
    }

    fn external_pr_test_workspace(project: &Project, suffix: &str) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            ChatConversationId::new(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-sha".to_string()),
            format!("ralphx/test/agent-{suffix}"),
            format!("/tmp/external-pr-command-{suffix}"),
        )
    }

    async fn wait_for_latest_pr_lookup_calls(github: &MockGithubService, expected: u32) {
        for _ in 0..100 {
            if github.state().find_latest_pr_by_head_branch_calls >= expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "expected at least {expected} latest PR lookups, got {}",
            github.state().find_latest_pr_by_head_branch_calls
        );
    }

    async fn wait_for_pr_sync_state_calls(github: &MockGithubService, expected: u32) {
        for _ in 0..100 {
            if github.state().check_pr_sync_state_calls >= expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "expected at least {expected} PR sync-state lookups, got {}",
            github.state().check_pr_sync_state_calls
        );
    }

    #[tokio::test]
    async fn workspace_load_external_pr_reconciliation_schedules_for_reconcilable_workspace() {
        let mut state = AppState::new_test();
        let project = external_pr_test_project("external-pr-command-load");
        let workspace = external_pr_test_workspace(&project, "load");
        let github = Arc::new(MockGithubService::new());
        state.github_service = Some(github.clone());
        state.project_repo.create(project).await.unwrap();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .unwrap();

        schedule_external_pr_reconciliation_for_workspace(
            &state,
            &workspace,
            AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
            false,
        );

        wait_for_latest_pr_lookup_calls(&github, 1).await;
        assert_eq!(
            github
                .state()
                .last_find_latest_pr_by_head_branch_name
                .as_deref(),
            Some(workspace.branch_name.as_str())
        );
    }

    #[tokio::test]
    async fn workspace_load_external_pr_reconciliation_skips_unreconcilable_workspace() {
        let mut state = AppState::new_test();
        let project = external_pr_test_project("external-pr-command-skip");
        let mut workspace = external_pr_test_workspace(&project, "skip");
        workspace.publication_pr_number = Some(77);
        workspace.publication_pr_status = Some("open".to_string());
        let github = Arc::new(MockGithubService::new());
        state.github_service = Some(github.clone());

        schedule_external_pr_reconciliation_for_workspace(
            &state,
            &workspace,
            AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
            false,
        );
        tokio::time::sleep(Duration::from_millis(25)).await;

        assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 0);
    }

    #[tokio::test]
    async fn run_completed_external_pr_reconciliation_links_terminal_pr() {
        let mut state = AppState::new_test();
        let project = external_pr_test_project("external-pr-command-run-completed");
        let workspace = external_pr_test_workspace(&project, "run-completed");
        let conversation_id = workspace.conversation_id.clone();
        let github = Arc::new(MockGithubService::new());
        github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
            number: 123,
            url: "https://github.com/owner/repo/pull/123".to_string(),
            status: GithubPrStatus::Closed,
            is_draft: false,
            head_ref_name: workspace.branch_name.clone(),
            updated_at: Some("2026-05-14T10:00:00Z".to_string()),
            author_login: None,
        })));
        state.github_service = Some(github.clone());
        state.project_repo.create(project).await.unwrap();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        schedule_external_pr_reconciliation_for_conversation_id(
            &state,
            conversation_id.clone(),
            AgentWorkspaceExternalPrReconciliationTrigger::AgentRunCompleted,
            true,
        )
        .await
        .unwrap();

        wait_for_latest_pr_lookup_calls(&github, 1).await;
        let updated = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace should still exist");
        assert_eq!(updated.publication_pr_number, Some(123));
        assert_eq!(updated.publication_pr_status.as_deref(), Some("closed"));

        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].step, "external_pr_closed");
    }

    #[tokio::test]
    async fn run_completed_pr_supervision_recovery_rearms_blocked_workspace() {
        let (_temp, state, conversation_id, github) = setup_publish_command_state(
            "pr-supervision-command-recovery",
            true,
            Some(257),
            Arc::new(MockGithubService::new()),
        )
        .await;
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.publication_push_status = Some("failed".to_string());
        workspace.pr_supervision_status = Some("blocked".to_string());
        workspace.pr_autofix_enabled = true;
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace update should persist");
        let head_sha = git(Path::new(&workspace.worktree_path), &["rev-parse", "HEAD"]);
        github.will_return_sync_state(PrSyncState {
            status: GithubPrStatus::Open,
            merge_state_status: Some(PrMergeStateStatus::Clean),
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: workspace.branch_name.clone(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some(head_sha),
            base_ref_oid: None,
        });

        schedule_pr_supervision_recovery_for_conversation_id(
            &state,
            conversation_id.clone(),
            AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
            true,
        )
        .await
        .expect("recovery scheduling should succeed");

        wait_for_pr_sync_state_calls(&github, 1).await;
        let updated = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    }

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should spawn");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    struct SubmittingPrDescriptionClient {
        repo: Arc<dyn AgentConversationWorkspaceRepository>,
        conversation_id: ChatConversationId,
        spawned: tokio::sync::Mutex<usize>,
        spawned_configs: tokio::sync::Mutex<Vec<AgentConfig>>,
    }

    impl SubmittingPrDescriptionClient {
        fn new(
            repo: Arc<dyn AgentConversationWorkspaceRepository>,
            conversation_id: ChatConversationId,
        ) -> Self {
            Self {
                repo,
                conversation_id,
                spawned: tokio::sync::Mutex::new(0),
                spawned_configs: tokio::sync::Mutex::new(Vec::new()),
            }
        }

        async fn spawned_count(&self) -> usize {
            *self.spawned.lock().await
        }

        async fn spawned_configs(&self) -> Vec<AgentConfig> {
            self.spawned_configs.lock().await.clone()
        }
    }

    #[async_trait]
    impl AgenticClient for SubmittingPrDescriptionClient {
        async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
            *self.spawned.lock().await += 1;
            self.spawned_configs.lock().await.push(config.clone());
            Ok(AgentHandle::mock(config.role))
        }

        async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
            Ok(())
        }

        async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
            self.repo
                .save_pr_description(
                    &self.conversation_id,
                    AgentWorkspacePrDescription::new(
                        Some("Cached publication title".to_string()),
                        "## Summary\n\nReady to publish.".to_string(),
                    ),
                )
                .await
                .expect("test PR description should save");
            Ok(AgentOutput::success("submitted"))
        }

        async fn send_prompt(
            &self,
            _handle: &AgentHandle,
            _prompt: &str,
        ) -> AgentResult<AgentResponse> {
            Ok(AgentResponse::new(""))
        }

        fn stream_response(
            &self,
            _handle: &AgentHandle,
            _prompt: &str,
        ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
            Box::pin(stream::empty())
        }

        fn capabilities(&self) -> &ClientCapabilities {
            static CAPS: std::sync::OnceLock<ClientCapabilities> = std::sync::OnceLock::new();
            CAPS.get_or_init(ClientCapabilities::mock)
        }

        async fn is_available(&self) -> AgentResult<bool> {
            Ok(true)
        }
    }

    fn setup_publish_repo(repo_path: &Path) -> String {
        std::fs::create_dir_all(repo_path).expect("repo root should be created");
        git(repo_path, &["init", "-b", "main"]);
        git(repo_path, &["config", "user.email", "test@example.com"]);
        git(repo_path, &["config", "user.name", "Test User"]);
        std::fs::write(repo_path.join("README.md"), "base\n")
            .expect("fixture file should be written");
        git(repo_path, &["add", "README.md"]);
        git(repo_path, &["commit", "-m", "base"]);
        git(repo_path, &["rev-parse", "HEAD"])
    }

    async fn setup_publish_command_state(
        suffix: &str,
        capture_base_commit: bool,
        publication_pr_number: Option<i64>,
        github: Arc<MockGithubService>,
    ) -> (
        tempfile::TempDir,
        AppState,
        ChatConversationId,
        Arc<MockGithubService>,
    ) {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        let main_sha = setup_publish_repo(&repo_path);

        let mut project = Project::new(
            format!("Publish Base {suffix}"),
            repo_path.to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        let conversation_id = ChatConversationId::from_string(uuid::Uuid::new_v4().to_string());
        let mut workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                branch_mode: None,
                base_ref: Some("main".to_string()),
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("workspace should be prepared");
        workspace.base_ref = "feature/deleted-base".to_string();
        workspace.base_display_name = Some("Current branch (feature/deleted-base)".to_string());
        workspace.base_commit = capture_base_commit.then_some(main_sha);
        workspace.publication_pr_number = publication_pr_number;
        workspace.publication_pr_url = publication_pr_number
            .map(|number| format!("https://github.com/mock/repo/pull/{number}"));
        workspace.publication_pr_status = publication_pr_number.map(|_| "open".to_string());

        let mut state = AppState::new_test();
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project should be persisted");
        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should be persisted");
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be persisted");

        (temp, state, conversation_id, github)
    }

    async fn published_workspace_and_project(
        state: &AppState,
        conversation_id: &ChatConversationId,
    ) -> (AgentConversationWorkspace, Project) {
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let project = state
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
            .expect("project lookup should succeed")
            .expect("project should exist");
        (workspace, project)
    }

    async fn use_main_as_publish_base(
        state: &AppState,
        conversation_id: &ChatConversationId,
    ) -> AgentConversationWorkspace {
        let (mut workspace, _project) =
            published_workspace_and_project(state, conversation_id).await;
        workspace.base_ref = "main".to_string();
        workspace.base_display_name = Some("Project default (main)".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace base should update");
        workspace
    }

    async fn seed_current_passing_workspace_review(
        state: &AppState,
        conversation_id: &ChatConversationId,
    ) {
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let context =
            crate::application::agent_workspace_review::load_agent_workspace_review_context(
                state, &workspace,
            )
            .await
            .expect("review context should load");
        let target = context.target.expect("review target should exist");
        let mut monitor = context.monitor;
        crate::application::agent_workspace_review::apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some("seeded-passing-review".to_string()),
            ArtifactId::from_string(format!("review-artifact-{}", conversation_id.as_str())),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("passing review monitor should persist");
    }

    fn commit_file(repo: &Path, relative_path: &str, contents: &str, message: &str) -> String {
        let path = repo.join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent directory should be created");
        }
        std::fs::write(&path, contents).expect("fixture file should be written");
        git(repo, &["add", relative_path]);
        git(repo, &["commit", "-m", message]);
        git(repo, &["rev-parse", "HEAD"])
    }

    async fn setup_linked_plan_publish_command_state(
        suffix: &str,
        active_regular_task: bool,
        github: Arc<MockGithubService>,
    ) -> (
        tempfile::TempDir,
        AppState,
        ChatConversationId,
        PlanBranchId,
        Arc<MockGithubService>,
    ) {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        let main_sha = setup_publish_repo(&repo_path);
        let origin_path = repo_path.to_string_lossy().to_string();
        git(
            &repo_path,
            &["remote", "add", "origin", origin_path.as_str()],
        );
        let plan_branch_name = format!("feature/plan-publish-{suffix}");
        git(&repo_path, &["checkout", "-b", &plan_branch_name]);
        std::fs::write(repo_path.join("plan.txt"), "plan branch change\n")
            .expect("plan fixture should be written");
        git(&repo_path, &["add", "plan.txt"]);
        git(&repo_path, &["commit", "-m", "plan branch change"]);
        git(&repo_path, &["checkout", "main"]);

        let mut project = Project::new(
            format!("Plan Publish {suffix}"),
            repo_path.to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        let conversation_id = ChatConversationId::from_string(uuid::Uuid::new_v4().to_string());
        let session_id = IdeationSessionId::from_string(format!("session-plan-publish-{suffix}"));
        let execution_plan = ExecutionPlan::new(session_id.clone());
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string(format!("artifact-plan-publish-{suffix}")),
            session_id.clone(),
            project.id.clone(),
            plan_branch_name.clone(),
            "main".to_string(),
        );
        plan_branch.execution_plan_id = Some(execution_plan.id.clone());
        plan_branch.pr_number = Some(77);
        plan_branch.pr_url = Some("https://github.com/mock/repo/pull/77".to_string());
        plan_branch.pr_status = Some(PrStatus::Open);
        plan_branch.pr_push_status = PrPushStatus::Pending;
        let plan_branch_id = plan_branch.id.clone();
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(main_sha),
            "agent-shell-plan-publish".to_string(),
            temp.path()
                .join("agent-shell-plan-publish")
                .to_string_lossy()
                .to_string(),
        );
        workspace.linked_ideation_session_id = Some(session_id.clone());
        workspace.linked_plan_branch_id = Some(plan_branch_id.clone());

        let mut task = Task::new(project.id.clone(), "Plan task".to_string());
        task.ideation_session_id = Some(session_id);
        task.execution_plan_id = Some(execution_plan.id.clone());
        task.internal_status = if active_regular_task {
            InternalStatus::Executing
        } else {
            InternalStatus::Merged
        };

        let mut state = AppState::new_test();
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project should be persisted");
        state
            .execution_plan_repo
            .create(execution_plan)
            .await
            .expect("execution plan should be persisted");
        state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch should be persisted");
        state
            .task_repo
            .create(task)
            .await
            .expect("task should be persisted");
        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should be persisted");
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be persisted");

        (temp, state, conversation_id, plan_branch_id, github)
    }

    #[tokio::test]
    async fn precompute_pr_description_skips_workspace_without_reviewable_commits() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "precompute-no-commits",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;

        let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
            &state,
            conversation_id,
        )
        .await
        .expect("precompute should skip without error");

        assert_eq!(response.status, "skipped");
        assert_eq!(response.reason.as_deref(), Some("no_reviewable_commits"));
        assert!(response.cache_status.is_none());
    }

    #[tokio::test]
    async fn precompute_pr_description_skips_non_edit_workspaces() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "precompute-non-edit",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.mode = AgentConversationWorkspaceMode::Chat;
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace mode should update");

        let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
            &state,
            conversation_id,
        )
        .await
        .expect("precompute should skip without error");

        assert_eq!(response.status, "skipped");
        assert_eq!(response.reason.as_deref(), Some("not_edit_workspace"));
        assert!(response.cache_status.is_none());
    }

    #[tokio::test]
    async fn precompute_pr_description_skips_missing_review_base() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "precompute-missing-base",
            false,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;

        let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
            &state,
            conversation_id,
        )
        .await
        .expect("precompute should skip without error");

        assert_eq!(response.status, "skipped");
        assert_eq!(response.reason.as_deref(), Some("missing_review_base"));
        assert!(response.cache_status.is_none());
    }

    #[tokio::test]
    async fn precompute_pr_description_skips_dirty_workspace() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "precompute-dirty",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        std::fs::write(
            PathBuf::from(workspace.worktree_path).join("dirty.txt"),
            "uncommitted\n",
        )
        .expect("dirty file should be written");

        let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
            &state,
            conversation_id,
        )
        .await
        .expect("precompute should skip without error");

        assert_eq!(response.status, "skipped");
        assert_eq!(response.reason.as_deref(), Some("uncommitted_changes"));
        assert!(response.cache_status.is_none());
    }

    #[tokio::test]
    async fn precompute_pr_description_skips_when_base_is_ahead() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "precompute-base-ahead",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let workspace = use_main_as_publish_base(&state, &conversation_id).await;
        let project = state
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
            .expect("project lookup should succeed")
            .expect("project should exist");
        let worktree_path = PathBuf::from(&workspace.worktree_path);
        commit_file(
            &worktree_path,
            "feature-only.txt",
            "feature\n",
            "Add feature-only change",
        );

        let repo_path = PathBuf::from(&project.working_directory);
        git(&repo_path, &["checkout", "main"]);
        commit_file(&repo_path, "base-only.txt", "base\n", "Advance base branch");

        let client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
        ));
        let state = state.with_agent_client(client.clone());

        let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
            &state,
            conversation_id,
        )
        .await
        .expect("precompute should skip behind-base workspace without error");

        assert_eq!(response.status, "skipped");
        assert_eq!(response.reason.as_deref(), Some("base_ahead"));
        assert!(response.cache_status.is_none());
        assert_eq!(client.spawned_count().await, 0);
    }

    #[tokio::test]
    async fn precompute_pr_description_uses_current_base_when_branch_contains_target_base() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "precompute-stale-base-contained",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let workspace = use_main_as_publish_base(&state, &conversation_id).await;
        let project = state
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
            .expect("project lookup should succeed")
            .expect("project should exist");
        let worktree_path = PathBuf::from(&workspace.worktree_path);
        commit_file(
            &worktree_path,
            "feature-only.txt",
            "feature\n",
            "Add feature-only change",
        );

        let repo_path = PathBuf::from(&project.working_directory);
        git(&repo_path, &["checkout", "main"]);
        let current_base =
            commit_file(&repo_path, "base-only.txt", "base\n", "Advance base branch");
        git(&worktree_path, &["merge", "--no-edit", "main"]);

        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_ne!(
            stored.base_commit.as_deref(),
            Some(current_base.as_str()),
            "fixture should keep the stored base commit stale"
        );

        let client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
        ));
        let state = state.with_agent_client(client.clone());

        let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
            &state,
            conversation_id,
        )
        .await
        .expect("precompute should draft from the effective current base");

        assert_eq!(response.status, "ready");
        assert_eq!(response.cache_status.as_deref(), Some("miss"));
        assert_eq!(client.spawned_count().await, 1);
        let configs = client.spawned_configs().await;
        let prompt = &configs
            .first()
            .expect("describer should have been spawned")
            .prompt;
        assert!(
            prompt.contains(&format!("<review_base>{current_base}</review_base>")),
            "prompt should use the current target base as the review base"
        );
        assert!(
            prompt.contains("feature-only.txt"),
            "feature file should remain in the PR diff context"
        );
        assert!(
            !prompt.contains("base-only.txt"),
            "base-only file must not appear in the PR diff context"
        );
    }

    #[tokio::test]
    async fn precompute_pr_description_caches_ready_workspace_description() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "precompute-ready",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let worktree_path = PathBuf::from(&workspace.worktree_path);
        std::fs::write(worktree_path.join("publish-ready.txt"), "ready\n")
            .expect("publish fixture should be written");
        git(&worktree_path, &["add", "publish-ready.txt"]);
        git(
            &worktree_path,
            &["commit", "-m", "Add publish ready fixture"],
        );

        let client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
        ));
        let state = state.with_agent_client(client.clone());

        let first = precompute_agent_conversation_workspace_pr_description_for_app_state(
            &state,
            conversation_id.clone(),
        )
        .await
        .expect("precompute should prepare a description");
        assert_eq!(first.status, "ready");
        assert_eq!(first.cache_status.as_deref(), Some("miss"));
        assert_eq!(first.reason, None);

        let second = precompute_agent_conversation_workspace_pr_description_for_app_state(
            &state,
            conversation_id,
        )
        .await
        .expect("precompute should reuse cached description");
        assert_eq!(second.status, "ready");
        assert_eq!(second.cache_status.as_deref(), Some("hit"));
        assert_eq!(client.spawned_count().await, 1);
    }

    #[test]
    fn base_resolution_updates_publish_target_or_blocks_with_reason() {
        let resolution = retargeted_base_resolution();
        let mut target = command_publish_target();

        apply_base_resolution_to_publish_target(&mut target, &resolution)
            .expect("retargeted base should update publish target");

        assert_eq!(target.base_ref, "main");
        assert_eq!(
            target.base_display_name.as_deref(),
            Some("Project default (main)")
        );

        let blocked = blocked_base_resolution("cannot verify base");
        let error = apply_base_resolution_to_publish_target(&mut target, &blocked)
            .expect_err("blocked base should stop publish target update");
        assert_eq!(error, "cannot verify base");
    }

    #[tokio::test]
    async fn persisting_retargeted_base_resolution_updates_workspace_metadata() {
        let state = AppState::new_test();
        let mut workspace = command_test_workspace();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should be persisted");

        persist_workspace_base_resolution_if_retargeted(
            &state,
            &mut workspace,
            &retargeted_base_resolution(),
        )
        .await
        .expect("retargeted workspace metadata should persist");

        assert_eq!(
            workspace.base_ref_kind,
            IdeationAnalysisBaseRefKind::ProjectDefault
        );
        assert_eq!(workspace.base_ref, "main");
        assert_eq!(workspace.base_commit.as_deref(), Some("main-sha"));
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.base_ref, "main");
        assert_eq!(
            stored.base_display_name.as_deref(),
            Some("Project default (main)")
        );
    }

    #[tokio::test]
    async fn retargeting_existing_workspace_pr_updates_github_base() {
        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);
        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(123);
        let target = command_publish_target();

        retarget_existing_workspace_pr_base_if_needed(
            &state,
            &target,
            &workspace,
            &retargeted_base_resolution(),
        )
        .await
        .expect("existing PR should be retargeted");

        let mock_state = github.state();
        assert_eq!(mock_state.update_pr_base_calls, 1);
        assert_eq!(
            mock_state.last_update_pr_base_args,
            Some((123, "main".to_string()))
        );
    }

    #[tokio::test]
    async fn retargeting_existing_workspace_pr_blocks_when_github_is_missing_or_fails() {
        let mut workspace = command_test_workspace();
        workspace.publication_pr_number = Some(123);
        let target = command_publish_target();
        let resolution = retargeted_base_resolution();

        let missing_error = retarget_existing_workspace_pr_base_if_needed(
            &AppState::new_test(),
            &target,
            &workspace,
            &resolution,
        )
        .await
        .expect_err("missing GitHub service should block existing PR retarget");
        assert_eq!(
            missing_error,
            existing_pr_retarget_block_reason(123, &resolution)
        );

        let mut state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        {
            github.state().update_pr_base_result =
                Some(Err(AppError::Infrastructure("denied".to_string())));
        }
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_trait);

        let failure_error =
            retarget_existing_workspace_pr_base_if_needed(&state, &target, &workspace, &resolution)
                .await
                .expect_err("GitHub retarget failure should block existing PR");
        assert_eq!(
            failure_error,
            existing_pr_retarget_block_reason(123, &resolution)
        );
        assert_eq!(github.state().update_pr_base_calls, 1);
    }

    #[tokio::test]
    async fn retargeting_workspace_without_existing_pr_is_a_noop() {
        let state = AppState::new_test();
        let workspace = command_test_workspace();
        let target = command_publish_target();

        retarget_existing_workspace_pr_base_if_needed(
            &state,
            &target,
            &workspace,
            &retargeted_base_resolution(),
        )
        .await
        .expect("workspace without PR should not require GitHub");
    }

    #[test]
    fn freshness_response_includes_effective_and_blocked_base_state() {
        let status = PublishBranchFreshnessStatus {
            target_ref: "origin/main".to_string(),
            captured_base_commit: Some("old-base-sha".to_string()),
            target_base_commit: "main-sha".to_string(),
            is_base_ahead: true,
        };
        let retargeted = retargeted_base_resolution();
        let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
            "conversation-command-base".to_string(),
            AgentWorkspaceFreshnessScope::Full,
            "feature/deleted-base".to_string(),
            Some("Current branch (feature/deleted-base)".to_string()),
            Some(&retargeted),
            status.clone(),
            true,
            Some(2),
            true,
            true,
        );

        assert_eq!(response.base_status, "retargeted");
        assert_eq!(response.effective_base_ref.as_deref(), Some("main"));
        assert_eq!(
            response.effective_base_display_name.as_deref(),
            Some("Project default (main)")
        );
        assert_eq!(response.base_block_reason, None);
        assert!(response.has_uncommitted_changes);
        assert_eq!(response.unpublished_commit_count, Some(2));

        let fallback = AgentConversationWorkspaceFreshnessResponse::from_target_status(
            "conversation-command-base".to_string(),
            AgentWorkspaceFreshnessScope::Full,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            status,
            false,
            Some(0),
            true,
            true,
        );
        assert_eq!(fallback.base_status, "valid");
        assert_eq!(fallback.effective_base_ref.as_deref(), Some("main"));
        assert_eq!(
            fallback.effective_base_display_name.as_deref(),
            Some("Project default (main)")
        );

        let workspace = command_test_workspace();
        let blocked = blocked_base_resolution(BLOCK_REASON_MISSING_BASE_COMMIT);
        let blocked_response = AgentConversationWorkspaceFreshnessResponse::blocked(
            "conversation-command-base".to_string(),
            AgentWorkspaceFreshnessScope::Full,
            &workspace,
            &blocked,
            true,
            Some(1),
            true,
            true,
        );
        assert_eq!(blocked_response.base_status, "blocked");
        assert_eq!(
            blocked_response.base_block_reason.as_deref(),
            Some(BLOCK_REASON_MISSING_BASE_COMMIT)
        );
        assert_eq!(blocked_response.effective_base_ref, None);
        assert_eq!(blocked_response.target_ref, "");
    }

    #[test]
    fn workspace_freshness_cache_status_labels_are_stable() {
        assert_eq!(AgentWorkspaceFreshnessCacheStatus::Hit.as_str(), "hit");
        assert_eq!(
            AgentWorkspaceFreshnessCacheStatus::Coalesced.as_str(),
            "coalesced"
        );
        assert_eq!(AgentWorkspaceFreshnessCacheStatus::Miss.as_str(), "miss");
    }

    #[test]
    fn workspace_freshness_cache_hits_and_invalidates_recent_response() {
        let conversation_id =
            ChatConversationId::from_string("77777777-7777-4777-8777-777777777777".to_string());
        invalidate_agent_workspace_freshness_cache(&conversation_id);
        assert!(cached_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full
        )
        .is_none());

        let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
            conversation_id.as_str().to_string(),
            AgentWorkspaceFreshnessScope::Full,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            PublishBranchFreshnessStatus {
                target_ref: "origin/main".to_string(),
                captured_base_commit: Some("old-base-sha".to_string()),
                target_base_commit: "main-sha".to_string(),
                is_base_ahead: true,
            },
            false,
            Some(1),
            true,
            true,
        );
        store_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full,
            &response,
        );

        let cached =
            cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
                .expect("recent freshness response should be cached");
        assert_eq!(cached.conversation_id, response.conversation_id);
        assert_eq!(cached.target_base_commit, "main-sha");
        assert!(cached.is_base_ahead);

        invalidate_agent_workspace_freshness_cache(&conversation_id);
        assert!(cached_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full
        )
        .is_none());
    }

    #[test]
    fn workspace_freshness_cache_keeps_local_and_full_scopes_separate() {
        let conversation_id =
            ChatConversationId::from_string("78777777-7777-4777-8777-777777777777".to_string());
        invalidate_agent_workspace_freshness_cache(&conversation_id);
        let local = AgentConversationWorkspaceFreshnessResponse::from_local_summary(
            conversation_id.as_str(),
            "main".to_string(),
            Some("Project default (main)".to_string()),
            "ralphx/test/workspace".to_string(),
            Some("base-sha".to_string()),
        );
        let full = AgentConversationWorkspaceFreshnessResponse::from_target_status(
            conversation_id.as_str(),
            AgentWorkspaceFreshnessScope::Full,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            PublishBranchFreshnessStatus {
                target_ref: "origin/main".to_string(),
                captured_base_commit: Some("base-sha".to_string()),
                target_base_commit: "new-main-sha".to_string(),
                is_base_ahead: true,
            },
            false,
            Some(3),
            true,
            true,
        );

        store_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Local,
            &local,
        );
        store_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full,
            &full,
        );

        let cached_local =
            cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Local)
                .expect("local response should be cached");
        let cached_full =
            cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
                .expect("full response should be cached");

        assert_eq!(cached_local.freshness_scope, "local");
        assert_eq!(cached_local.target_base_commit, "base-sha");
        assert_eq!(cached_full.freshness_scope, "full");
        assert_eq!(cached_full.target_base_commit, "new-main-sha");
    }

    #[test]
    fn workspace_freshness_cache_expires_stale_entries() {
        let conversation_id =
            ChatConversationId::from_string("87777777-7777-4777-8777-777777777777");
        invalidate_agent_workspace_freshness_cache(&conversation_id);
        let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
            conversation_id.as_str(),
            AgentWorkspaceFreshnessScope::Full,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            PublishBranchFreshnessStatus {
                target_ref: "origin/main".to_string(),
                captured_base_commit: Some("old-base-sha".to_string()),
                target_base_commit: "main-sha".to_string(),
                is_base_ahead: false,
            },
            false,
            Some(0),
            true,
            true,
        );
        let key = agent_workspace_freshness_cache_key(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full,
        )
        .expect("conversation id should be cacheable");
        agent_workspace_freshness_cache().insert(
            key.clone(),
            AgentWorkspaceFreshnessCacheEntry {
                inserted_at: Instant::now()
                    .checked_sub(Duration::from_secs(60))
                    .expect("stale instant should be representable"),
                response,
            },
        );

        assert!(cached_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full
        )
        .is_none());
        assert!(!agent_workspace_freshness_cache().contains_key(&key));
    }

    #[test]
    fn workspace_freshness_invalidation_guard_clears_cache_on_create_and_drop() {
        let conversation_id =
            ChatConversationId::from_string("97777777-7777-4777-8777-777777777777");
        let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
            conversation_id.as_str(),
            AgentWorkspaceFreshnessScope::Full,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            PublishBranchFreshnessStatus {
                target_ref: "origin/main".to_string(),
                captured_base_commit: Some("old-base-sha".to_string()),
                target_base_commit: "main-sha".to_string(),
                is_base_ahead: false,
            },
            false,
            Some(0),
            true,
            true,
        );

        store_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full,
            &response,
        );
        assert!(cached_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full
        )
        .is_some());
        {
            let _guard = AgentWorkspaceFreshnessInvalidationGuard::new(&conversation_id);
            assert!(cached_agent_workspace_freshness(
                &conversation_id,
                AgentWorkspaceFreshnessScope::Full
            )
            .is_none());
            store_agent_workspace_freshness(
                &conversation_id,
                AgentWorkspaceFreshnessScope::Full,
                &response,
            );
            assert!(cached_agent_workspace_freshness(
                &conversation_id,
                AgentWorkspaceFreshnessScope::Full
            )
            .is_some());
        }
        assert!(cached_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full
        )
        .is_none());
    }

    #[test]
    fn pr_description_invalidation_guard_can_defer_initial_invalidation() {
        let conversation_id =
            ChatConversationId::from_string("a7777777-7777-4777-8777-777777777777");
        let _guard = AgentWorkspacePrDescriptionInvalidationGuard::new(&conversation_id, false);
    }

    #[test]
    fn workspace_freshness_cache_skips_nil_conversation_ids() {
        let conversation_id = ChatConversationId::from_string("not-a-uuid");
        assert!(conversation_id.as_uuid().is_nil());

        let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
            conversation_id.as_str(),
            AgentWorkspaceFreshnessScope::Full,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            None,
            PublishBranchFreshnessStatus {
                target_ref: "origin/main".to_string(),
                captured_base_commit: None,
                target_base_commit: "main-sha".to_string(),
                is_base_ahead: false,
            },
            false,
            Some(0),
            true,
            true,
        );
        store_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full,
            &response,
        );

        assert!(cached_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full
        )
        .is_none());
    }

    #[tokio::test]
    async fn workspace_freshness_command_blocks_stale_base_without_commit() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "freshness-blocked",
            false,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("full".to_string()),
            app.state(),
        )
        .await
        .expect("freshness should return blocked state");

        assert_eq!(response.base_status, "blocked");
        assert_eq!(response.base_ref, "feature/deleted-base");
        assert_eq!(response.effective_base_ref, None);
        assert_eq!(
            response.base_block_reason.as_deref(),
            Some(BLOCK_REASON_MISSING_BASE_COMMIT)
        );
        assert_eq!(response.target_ref, "");
    }

    #[tokio::test]
    async fn workspace_freshness_command_reports_retargeted_base() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "freshness-retargeted",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("full".to_string()),
            app.state(),
        )
        .await
        .expect("freshness should resolve retargeted base");

        assert_eq!(response.base_status, "retargeted");
        assert_eq!(response.base_ref, "feature/deleted-base");
        assert_eq!(response.effective_base_ref.as_deref(), Some("main"));
        assert_eq!(
            response.effective_base_display_name.as_deref(),
            Some("Project default (main)")
        );
        assert_eq!(response.target_ref, "main");
        assert!(!response.is_base_ahead);
    }

    #[tokio::test]
    async fn workspace_freshness_command_caches_local_summary_after_first_lookup() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "freshness-local-cache",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let first = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("local".to_string()),
            app.state(),
        )
        .await
        .expect("local freshness should load");
        assert_eq!(first.freshness_scope, "local");
        assert_eq!(first.base_ref, "feature/deleted-base");
        assert!(first.target_ref.starts_with("ralphx/"));
        assert!(!first.remote_refreshed);
        assert!(!first.worktree_status_checked);

        let second = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("local".to_string()),
            app.state(),
        )
        .await
        .expect("cached local freshness should load");

        assert_eq!(second.conversation_id, first.conversation_id);
        assert_eq!(second.freshness_scope, "local");
        assert_eq!(second.target_base_commit, first.target_base_commit);
        assert_eq!(second.target_ref, first.target_ref);
    }

    #[tokio::test]
    async fn workspace_freshness_command_treats_merged_missing_workspace_as_terminal() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "freshness-merged-missing",
            true,
            Some(243),
            Arc::new(MockGithubService::new()),
        )
        .await;
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let worktree_path = PathBuf::from(&workspace.worktree_path);
        workspace.publication_pr_status = Some("merged".to_string());
        workspace.publication_push_status = Some("pushed".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should update");
        std::fs::remove_dir_all(&worktree_path).expect("worktree should be removed");

        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("local".to_string()),
            app.state(),
        )
        .await
        .expect("terminal workspace freshness should not require the removed worktree");

        assert_eq!(response.freshness_scope, "local");
        assert_eq!(response.base_status, "valid");
        assert_eq!(response.unpublished_commit_count, Some(0));
        assert!(!response.remote_refreshed);
        assert!(!response.worktree_status_checked);
    }

    #[tokio::test]
    async fn update_workspace_from_explicit_base_recovers_blocked_base() {
        let (temp, state, conversation_id, _github) = setup_publish_command_state(
            "explicit-base-recovery",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let repo_path = temp.path().join("repo");
        git(&repo_path, &["checkout", "-b", "release/0.8"]);
        std::fs::write(repo_path.join("release.txt"), "release\n")
            .expect("release fixture should be written");
        git(&repo_path, &["add", "release.txt"]);
        git(&repo_path, &["commit", "-m", "release base"]);
        let release_sha = git(&repo_path, &["rev-parse", "HEAD"]);
        git(&repo_path, &["checkout", "main"]);
        git(&repo_path, &["checkout", "--orphan", "rewritten-main"]);
        git(&repo_path, &["rm", "-rf", "."]);
        std::fs::write(repo_path.join("README.md"), "rewritten\n")
            .expect("rewritten fixture should be written");
        git(&repo_path, &["add", "README.md"]);
        git(&repo_path, &["commit", "-m", "rewrite main"]);
        git(&repo_path, &["branch", "-M", "main"]);

        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));
        let app = mock_builder()
            .manage(state)
            .manage(execution_state)
            .manage(team_service)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");
        let blocked = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("full".to_string()),
            app.state(),
        )
        .await
        .expect("freshness should load");
        assert_eq!(blocked.base_status, "blocked");

        let response = update_agent_conversation_workspace_from_base_for_app_state(
            app.state::<AppState>().inner(),
            app.state::<Arc<ExecutionState>>().inner(),
            Some(app.state::<Arc<TeamService>>().inner().clone()),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
                branch_mode: None,
                base_ref: Some("release/0.8".to_string()),
                display_name: Some("release/0.8".to_string()),
                source_pull_request: None,
            },
        )
        .await
        .expect("explicit base update should recover workspace");

        assert!(response.updated);
        assert_eq!(response.base_status, "valid");
        assert_eq!(response.target_ref, "release/0.8");
        assert_eq!(response.base_commit, release_sha);
        assert_eq!(response.workspace.base_ref_kind, "local_branch");
        assert_eq!(response.workspace.base_ref, "release/0.8");
        assert_eq!(
            response.workspace.base_display_name.as_deref(),
            Some("release/0.8")
        );
        assert_eq!(
            response.workspace.base_commit.as_deref(),
            Some(release_sha.as_str())
        );
    }

    #[tokio::test]
    async fn update_workspace_from_base_running_conversation_does_not_stick_refreshing() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "update-running-conversation",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));
        state
            .running_agent_registry
            .register(
                RunningAgentKey::new(
                    ChatContextType::Project.to_string(),
                    conversation_id.as_str(),
                ),
                123,
                conversation_id.as_str(),
                "run-update-base".to_string(),
                None,
                None,
            )
            .await;

        let result = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: None,
                branch_mode: None,
                base_ref: None,
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("running conversation should allow workspace base update");

        assert_eq!(result.workspace.conversation_id, conversation_id.as_str());
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_ne!(
            stored.publication_push_status.as_deref(),
            Some("refreshing")
        );
    }

    #[tokio::test]
    async fn update_workspace_from_base_succeeds_when_agent_is_running() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "update-running-conversation-allowed",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));
        state
            .running_agent_registry
            .register(
                RunningAgentKey::new(
                    ChatContextType::Project.to_string(),
                    conversation_id.as_str(),
                ),
                123,
                conversation_id.as_str(),
                "run-update-base".to_string(),
                None,
                None,
            )
            .await;

        let result = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: None,
                branch_mode: None,
                base_ref: None,
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("running conversation should allow workspace base update");

        assert_eq!(result.workspace.conversation_id, conversation_id.as_str());
    }

    #[tokio::test]
    async fn update_workspace_from_base_allows_interactive_idle_conversation() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "update-interactive-idle-conversation",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));
        state
            .running_agent_registry
            .register(
                RunningAgentKey::new(
                    ChatContextType::Project.to_string(),
                    conversation_id.as_str(),
                ),
                123,
                conversation_id.as_str(),
                "run-update-base-idle".to_string(),
                None,
                None,
            )
            .await;
        execution_state.mark_interactive_idle(&format!(
            "{}/{}",
            ChatContextType::Project,
            conversation_id.as_str()
        ));

        let result = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: None,
                branch_mode: None,
                base_ref: None,
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("interactive-idle conversation should allow workspace base update");

        assert_eq!(result.workspace.conversation_id, conversation_id.as_str());
    }

    #[tokio::test]
    async fn update_workspace_from_base_pr_selection_persists_source_pull_request() {
        let (temp, state, conversation_id, _github) = setup_publish_command_state(
            "update-pr-base",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let repo_path = temp.path().join("repo");
        let head = git(&repo_path, &["rev-parse", "HEAD"]);
        git(
            &repo_path,
            &["update-ref", "refs/heads/feature/pr-base", &head],
        );
        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));
        let source_pull_request = AgentWorkspaceSourcePullRequest {
            number: 42,
            url: Some("https://github.com/mock/repo/pull/42".to_string()),
            title: Some("Add PR base".to_string()),
            head_ref_name: "feature/pr-base".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some("pr-head-sha".to_string()),
        };

        let result = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
                branch_mode: None,
                base_ref: Some("feature/pr-base".to_string()),
                display_name: Some("PR #42: Add PR base".to_string()),
                source_pull_request: Some(source_pull_request.clone()),
            },
        )
        .await
        .expect("PR-backed base update should succeed");

        assert_eq!(result.workspace.base_ref_kind, "local_branch");
        assert_eq!(result.workspace.base_ref, "feature/pr-base");
        assert_eq!(
            result.workspace.base_display_name.as_deref(),
            Some("PR #42: Add PR base")
        );
        let response_source = result
            .workspace
            .source_pull_request
            .as_ref()
            .expect("response should include source PR metadata");
        assert_eq!(response_source.number, 42);
        assert_eq!(response_source.head_ref_name, "feature/pr-base");

        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.source_pull_request, Some(source_pull_request));
    }

    #[tokio::test]
    async fn update_workspace_from_base_pr_selection_fetches_remote_head_before_validation() {
        let (temp, state, conversation_id, _github) = setup_publish_command_state(
            "update-pr-base-remote-only",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let repo_path = temp.path().join("repo");
        let origin_path = temp.path().join("origin.git");
        git(
            &repo_path,
            &["init", "--bare", origin_path.to_str().expect("origin path")],
        );
        git(
            &repo_path,
            &[
                "remote",
                "add",
                "origin",
                origin_path.to_str().expect("origin path"),
            ],
        );
        git(&repo_path, &["push", "origin", "main"]);
        git(&repo_path, &["checkout", "-b", "feature/pr-remote-only"]);
        std::fs::write(repo_path.join("pr.txt"), "remote pr head\n")
            .expect("fixture file should be written");
        git(&repo_path, &["add", "pr.txt"]);
        git(&repo_path, &["commit", "-m", "remote pr head"]);
        let pr_head = git(&repo_path, &["rev-parse", "HEAD"]);
        git(&repo_path, &["push", "origin", "feature/pr-remote-only"]);
        git(&repo_path, &["checkout", "main"]);
        git(&repo_path, &["branch", "-D", "feature/pr-remote-only"]);
        git(
            &repo_path,
            &[
                "update-ref",
                "-d",
                "refs/remotes/origin/feature/pr-remote-only",
            ],
        );
        assert!(
            !GitService::ref_exists(&repo_path, "feature/pr-remote-only")
                .await
                .expect("local branch check should succeed")
        );
        assert!(
            !GitService::ref_exists(&repo_path, "origin/feature/pr-remote-only")
                .await
                .expect("remote tracking check should succeed")
        );
        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));

        let result = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
                branch_mode: None,
                base_ref: Some("feature/pr-remote-only".to_string()),
                display_name: Some("PR #43: Remote-only PR base".to_string()),
                source_pull_request: Some(AgentWorkspaceSourcePullRequest {
                    number: 43,
                    url: Some("https://github.com/mock/repo/pull/43".to_string()),
                    title: Some("Remote-only PR base".to_string()),
                    head_ref_name: "feature/pr-remote-only".to_string(),
                    base_ref_name: Some("main".to_string()),
                    head_ref_oid: Some(pr_head),
                }),
            },
        )
        .await
        .expect("PR-backed remote-only base update should fetch and succeed");

        assert_eq!(result.workspace.base_ref, "feature/pr-remote-only");
        assert!(
            GitService::ref_exists(&repo_path, "origin/feature/pr-remote-only")
                .await
                .expect("remote tracking check should succeed after update")
        );
    }

    #[tokio::test]
    async fn update_workspace_from_saved_base_retargets_to_project_default() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "saved-base-retarget",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));
        let app = mock_builder()
            .manage(state)
            .manage(execution_state)
            .manage(team_service)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let freshness = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("full".to_string()),
            app.state(),
        )
        .await
        .expect("freshness should resolve retargeted base");
        assert_eq!(freshness.base_status, "retargeted");

        let response = update_agent_conversation_workspace_from_base_for_app_state(
            app.state::<AppState>().inner(),
            app.state::<Arc<ExecutionState>>().inner(),
            Some(app.state::<Arc<TeamService>>().inner().clone()),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: None,
                branch_mode: None,
                base_ref: None,
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("saved-base update should retarget workspace");

        assert!(!response.updated);
        assert_eq!(response.base_status, "retargeted");
        assert_eq!(response.target_ref, "main");
        assert_eq!(response.workspace.base_ref_kind, "project_default");
        assert_eq!(response.workspace.base_ref, "main");
        assert_eq!(
            response.effective_base_display_name.as_deref(),
            Some("Project default (main)")
        );
        assert_eq!(
            response.workspace.base_display_name.as_deref(),
            Some("Project default (main)")
        );
    }

    #[tokio::test]
    async fn update_ideation_workspace_from_base_refuses_primary_checkout_plan_branch() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        let base_sha = setup_publish_repo(&repo_path);
        let plan_branch_name = "feature/plan-primary-checkout";

        git(&repo_path, &["checkout", "-b", plan_branch_name]);
        git(&repo_path, &["checkout", "main"]);
        std::fs::write(repo_path.join("fix.txt"), "base fix\n")
            .expect("fixture file should be written");
        git(&repo_path, &["add", "fix.txt"]);
        git(&repo_path, &["commit", "-m", "base fix"]);
        let main_sha = git(&repo_path, &["rev-parse", "HEAD"]);
        git(&repo_path, &["checkout", plan_branch_name]);

        let mut project = Project::new(
            "Primary Checkout Plan Update".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        let conversation_id = ChatConversationId::from_string("conversation-plan-primary-checkout");
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-primary-checkout"),
            IdeationSessionId::from_string("session-primary-checkout"),
            project.id.clone(),
            plan_branch_name.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Active;
        let plan_branch_id = plan_branch.id.clone();
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha.clone()),
            "agent-shell-primary-checkout".to_string(),
            temp.path()
                .join("agent-shell-primary-checkout")
                .to_string_lossy()
                .to_string(),
        );
        workspace.linked_ideation_session_id = Some(plan_branch.session_id.clone());
        workspace.linked_plan_branch_id = Some(plan_branch_id.clone());

        let state = AppState::new_test();
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project should be persisted");
        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should be persisted");
        state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch should be persisted");
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be persisted");

        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));
        let error = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: None,
                branch_mode: None,
                base_ref: None,
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect_err("primary checkout plan branch should not be updated in place");

        assert!(
            error.to_ascii_lowercase().contains("primary checkout"),
            "unexpected primary checkout refusal: {error}"
        );
        assert_eq!(
            git(&repo_path, &["branch", "--show-current"]),
            plan_branch_name
        );
        assert!(!repo_path.join("fix.txt").exists());
        assert_eq!(git(&repo_path, &["rev-parse", "main"]), main_sha);
        assert_eq!(git(&repo_path, &["rev-parse", plan_branch_name]), base_sha);
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn update_ideation_workspace_from_base_updates_linked_plan_worktree() {
        let (temp, state, conversation_id, plan_branch_id, github) =
            setup_linked_plan_publish_command_state(
                "base-update",
                false,
                Arc::new(MockGithubService::new()),
            )
            .await;
        let repo_path = temp.path().join("repo");
        std::fs::write(repo_path.join("base-fix.txt"), "base fix\n")
            .expect("base fixture should be written");
        git(&repo_path, &["add", "base-fix.txt"]);
        git(&repo_path, &["commit", "-m", "base fix"]);
        let main_sha = git(&repo_path, &["rev-parse", "HEAD"]);
        assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");

        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));
        let response = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: None,
                branch_mode: None,
                base_ref: None,
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("linked plan branch worktree should update from base");

        assert!(response.updated);
        assert_eq!(response.base_commit, main_sha);
        assert_eq!(response.target_ref, "origin/main");
        assert_eq!(
            response.workspace.publication_push_status.as_deref(),
            Some("pushed")
        );
        assert_eq!(github.state().push_branch_calls, 1);
        let project = state
            .project_repo
            .get_all()
            .await
            .expect("project lookup should succeed")
            .pop()
            .expect("project should exist");
        let plan_branch = state
            .plan_branch_repo
            .get_by_id(&plan_branch_id)
            .await
            .expect("plan branch lookup should succeed")
            .expect("plan branch should exist");
        assert_eq!(
            github.state().last_push_branch_name.as_deref(),
            Some(plan_branch.branch_name.as_str())
        );
        assert_eq!(plan_branch.pr_push_status, PrPushStatus::Pushed);
        let plan_worktree = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
            .await
            .expect("linked plan worktree should resolve");
        git(
            &repo_path,
            &["merge-base", "--is-ancestor", &main_sha, &plan_branch.branch_name],
        );
        assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
        assert_eq!(git(&repo_path, &["status", "--short"]), "");
        assert_eq!(git(&plan_worktree, &["status", "--short"]), "");
    }

    #[tokio::test]
    async fn update_workspace_from_saved_base_blocks_when_base_commit_is_missing() {
        let (_temp, state, conversation_id, github) = setup_publish_command_state(
            "update-missing-base",
            false,
            Some(987),
            Arc::new(MockGithubService::new()),
        )
        .await;
        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));

        let error = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: None,
                branch_mode: None,
                base_ref: None,
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect_err("missing saved base commit should block update");

        assert_eq!(error, BLOCK_REASON_MISSING_BASE_COMMIT);
        assert_eq!(github.state().update_pr_base_calls, 0);
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.base_ref, "feature/deleted-base");
        assert_eq!(stored.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn update_workspace_from_explicit_base_blocks_when_pr_retarget_fails() {
        let github = Arc::new(MockGithubService::new());
        {
            github.state().update_pr_base_result =
                Some(Err(AppError::Infrastructure("denied".to_string())));
        }
        let (temp, state, conversation_id, github) =
            setup_publish_command_state("update-explicit-retarget-fails", true, Some(988), github)
                .await;
        let repo_path = temp.path().join("repo");
        git(&repo_path, &["checkout", "-b", "release/0.8"]);
        git(&repo_path, &["checkout", "main"]);
        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));

        let error = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
                branch_mode: None,
                base_ref: Some("release/0.8".to_string()),
                display_name: Some("release/0.8".to_string()),
                source_pull_request: None,
            },
        )
        .await
        .expect_err("failed explicit-base PR retarget should block update");

        assert!(error.contains("Existing PR #988 targets the deleted branch"));
        assert_eq!(
            github.state().last_update_pr_base_args,
            Some((988, "release/0.8".to_string()))
        );
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.base_ref, "feature/deleted-base");
        assert_eq!(stored.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn update_workspace_from_explicit_base_blocks_when_selection_is_missing() {
        let (_temp, state, conversation_id, github) = setup_publish_command_state(
            "update-explicit-missing-branch",
            true,
            Some(989),
            Arc::new(MockGithubService::new()),
        )
        .await;
        let execution_state = Arc::new(ExecutionState::new());
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        )));

        let error = update_agent_conversation_workspace_from_base_for_app_state(
            &state,
            &execution_state,
            Some(team_service),
            conversation_id.clone(),
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
                branch_mode: None,
                base_ref: Some("release/missing".to_string()),
                display_name: Some("release/missing".to_string()),
                source_pull_request: None,
            },
        )
        .await
        .expect_err("missing explicit branch should block before PR retarget");

        assert!(error.contains("Selected base branch 'release/missing' does not exist"));
        assert_eq!(github.state().update_pr_base_calls, 0);
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.base_ref, "feature/deleted-base");
        assert_eq!(stored.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn publish_linked_ideation_plan_branch_commits_and_pushes_existing_pr() {
        let (temp, state, conversation_id, plan_branch_id, github) =
            setup_linked_plan_publish_command_state(
                "success",
                false,
                Arc::new(MockGithubService::new()),
            )
            .await;
        let repo_path = temp.path().join("repo");
        let project = state
            .project_repo
            .get_all()
            .await
            .expect("project lookup should succeed")
            .pop()
            .expect("project should exist");
        let plan_branch = state
            .plan_branch_repo
            .get_by_id(&plan_branch_id)
            .await
            .expect("plan branch lookup should succeed")
            .expect("plan branch should exist");
        let plan_worktree = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
            .await
            .expect("linked plan worktree should resolve");
        std::fs::write(plan_worktree.join("manual-fix.txt"), "manual follow-up\n")
            .expect("manual plan fix should be written");
        assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
        assert_eq!(git(&repo_path, &["status", "--short"]), "");
        let execution_state = Arc::new(ExecutionState::new());

        let response = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect("linked ideation plan publish should succeed");
        state
            .pr_poller_registry
            .stop_agent_workspace_polling(&conversation_id);

        assert_eq!(response.pr_number, Some(77));
        assert!(!response.created_pr);
        assert!(response.commit_sha.is_some());
        assert_eq!(
            response.workspace.publication_push_status.as_deref(),
            Some("pushed")
        );
        assert_eq!(github.state().push_branch_calls, 1);
        assert_eq!(
            github.state().last_push_branch_name.as_deref(),
            Some("feature/plan-publish-success")
        );
        assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
        assert_eq!(git(&repo_path, &["status", "--short"]), "");
        assert_eq!(git(&plan_worktree, &["status", "--short"]), "");
        let stored_plan = state
            .plan_branch_repo
            .get_by_id(&plan_branch_id)
            .await
            .expect("plan branch lookup should succeed")
            .expect("plan branch should exist");
        assert_eq!(stored_plan.pr_push_status, PrPushStatus::Pushed);
        let stored_workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored_workspace.publication_pr_number, Some(77));
        assert_eq!(
            stored_workspace.publication_push_status.as_deref(),
            Some("pushed")
        );
    }

    #[tokio::test]
    async fn publish_linked_ideation_plan_branch_rejects_active_regular_tasks() {
        let (temp, state, conversation_id, _plan_branch_id, github) =
            setup_linked_plan_publish_command_state(
                "active-task",
                true,
                Arc::new(MockGithubService::new()),
            )
            .await;
        let repo_path = temp.path().join("repo");
        let project = state
            .project_repo
            .get_all()
            .await
            .expect("project lookup should succeed")
            .pop()
            .expect("project should exist");
        let plan_branch = state
            .plan_branch_repo
            .get_by_id(&_plan_branch_id)
            .await
            .expect("plan branch lookup should succeed")
            .expect("plan branch should exist");
        let plan_worktree = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
            .await
            .expect("linked plan worktree should resolve");
        std::fs::write(plan_worktree.join("manual-fix.txt"), "manual follow-up\n")
            .expect("manual plan fix should be written");
        let execution_state = Arc::new(ExecutionState::new());

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect_err("active regular task should retain publish ownership");

        assert!(error.contains("active task work"));
        assert_eq!(github.state().push_branch_calls, 0);
        assert_eq!(git(&repo_path, &["status", "--short"]), "");
        assert_ne!(git(&plan_worktree, &["status", "--short"]), "");
    }

    #[tokio::test]
    async fn publish_workspace_rejects_concurrent_publish_attempt() {
        let (_temp, state, conversation_id, _github) = setup_publish_command_state(
            "concurrent-publish",
            true,
            None,
            Arc::new(MockGithubService::new()),
        )
        .await;
        let execution_state = Arc::new(ExecutionState::new());
        let _guard = try_acquire_agent_workspace_publish_guard(&conversation_id)
            .expect("test should acquire publish guard");

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect_err("concurrent publish should be rejected");

        assert_eq!(error, AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE);
    }

    #[tokio::test]
    async fn publish_workspace_rejects_terminal_pr_without_mutating_status() {
        let (_temp, state, conversation_id, github) = setup_publish_command_state(
            "terminal-pr",
            true,
            Some(333),
            Arc::new(MockGithubService::new()),
        )
        .await;
        let execution_state = Arc::new(ExecutionState::new());
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.publication_pr_status = Some("merged".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace update should persist");

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect_err("terminal PR should block publish");

        assert!(error.contains("closed or merged"));
        assert_eq!(github.state().update_pr_base_calls, 0);
        assert_eq!(github.state().push_branch_calls, 0);
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.publication_pr_status.as_deref(), Some("merged"));
        assert_eq!(
            stored.publication_push_status.as_deref(),
            Some("needs_agent")
        );
    }

    #[tokio::test]
    async fn publish_workspace_blocks_before_pr_mutation_when_base_commit_is_missing() {
        let (_temp, state, conversation_id, github) = setup_publish_command_state(
            "missing-base",
            false,
            Some(321),
            Arc::new(MockGithubService::new()),
        )
        .await;
        let execution_state = Arc::new(ExecutionState::new());

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect_err("missing base commit should block publish");

        assert!(error.contains("missing its captured base commit"));
        assert_eq!(github.state().update_pr_base_calls, 0);
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.base_ref, "feature/deleted-base");
    }

    #[tokio::test]
    async fn publish_workspace_blocks_on_review_gate_before_push_when_base_is_valid() {
        let (_temp, state, conversation_id, github) = setup_publish_command_state(
            "review-required",
            true,
            Some(322),
            Arc::new(MockGithubService::new()),
        )
        .await;
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        std::fs::write(
            Path::new(&workspace.worktree_path).join("implementation.txt"),
            "change requiring review\n",
        )
        .expect("workspace change should be written");
        let execution_state = Arc::new(ExecutionState::new());

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id,
            false,
        )
        .await
        .expect_err("review gate should block publish");

        assert_eq!(error, "Workspace Review is required before publishing");
        assert_eq!(github.state().push_branch_calls, 0);
        assert_eq!(github.state().update_pr_base_calls, 0);
    }

    #[tokio::test]
    async fn publish_workspace_allows_required_review_gate_when_policy_is_disabled() {
        let (_temp, state, conversation_id, github) = setup_publish_command_state(
            "review-disabled",
            true,
            Some(323),
            Arc::new(MockGithubService::new()),
        )
        .await;
        state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                require_workspace_review: false,
                ..ReviewSettings::default()
            })
            .await
            .expect("review settings should update");
        let project = state
            .project_repo
            .get_all()
            .await
            .expect("projects load")
            .into_iter()
            .next()
            .expect("project exists");
        git(
            Path::new(&project.working_directory),
            &["remote", "add", "origin", &project.working_directory],
        );
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        std::fs::write(
            Path::new(&workspace.worktree_path).join("implementation.txt"),
            "change that would otherwise require review\n",
        )
        .expect("workspace change should be written");
        let client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
        ));
        let state = state.with_agent_client(client);
        let execution_state = Arc::new(ExecutionState::new());

        let response = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect("publish should succeed when workspace review policy is disabled");
        state
            .pr_poller_registry
            .stop_agent_workspace_polling(&conversation_id);

        assert_eq!(
            response.workspace.publication_push_status.as_deref(),
            Some("pushed")
        );
        assert_eq!(github.state().push_branch_calls, 1);
        assert_eq!(github.state().update_pr_base_calls, 1);
    }

    #[tokio::test]
    async fn publish_workspace_blocks_when_existing_pr_base_retarget_fails() {
        let github = Arc::new(MockGithubService::new());
        {
            github.state().update_pr_base_result =
                Some(Err(AppError::Infrastructure("denied".to_string())));
        }
        let (_temp, state, conversation_id, github) =
            setup_publish_command_state("pr-retarget-fails", true, Some(654), github).await;
        let execution_state = Arc::new(ExecutionState::new());

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect_err("failed PR base retarget should block publish");

        assert!(error.contains("Existing PR #654 targets the deleted branch"));
        assert_eq!(
            github.state().last_update_pr_base_args,
            Some((654, "main".to_string()))
        );
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.base_ref, "feature/deleted-base");
    }

    #[test]
    fn publication_event_status_helpers_include_description_states() {
        assert_eq!(
            publication_event_status_for_push_status("describing"),
            "started"
        );
        assert_eq!(
            publication_event_summary_for_push_status("describing"),
            "Drafting pull request description"
        );
        assert_eq!(
            publication_event_status_for_push_status("description_failed"),
            "failed"
        );
        assert_eq!(
            publication_event_summary_for_push_status("description_failed"),
            "Pull request description failed"
        );
    }

    #[tokio::test]
    async fn publish_workspace_syncs_requested_auto_merge_before_returning() {
        let github = Arc::new(MockGithubService::new());
        let (_temp, state, conversation_id, github) =
            setup_publish_command_state("auto-merge-publish", true, None, github).await;
        let project = state
            .project_repo
            .get_all()
            .await
            .expect("projects load")
            .into_iter()
            .next()
            .expect("project exists");
        git(
            Path::new(&project.working_directory),
            &["remote", "add", "origin", &project.working_directory],
        );
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.pr_auto_merge_desired = true;
        workspace.pr_auto_merge_method = "rebase".to_string();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace update should persist");
        std::fs::write(
            Path::new(&workspace.worktree_path).join("auto-merge.txt"),
            "ready for review\n",
        )
        .expect("workspace change should be written");
        seed_current_passing_workspace_review(&state, &conversation_id).await;
        github.state().fetch_pr_health_result = Some(Ok(PrHealth {
            sync_state: PrSyncState {
                status: GithubPrStatus::Open,
                merge_state_status: None,
                mergeable: None,
                is_draft: true,
                head_ref_name: workspace.branch_name.clone(),
                base_ref_name: "main".to_string(),
                head_ref_oid: None,
                base_ref_oid: None,
            },
            review_decision: None,
            checks: Vec::new(),
            issue_comments: Vec::new(),
            auto_merge_request: None,
        }));
        let client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
        ));
        let state = state.with_agent_client(client);
        let execution_state = Arc::new(ExecutionState::new());

        let response = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect("publish should succeed");
        state
            .pr_poller_registry
            .stop_agent_workspace_polling(&conversation_id);

        assert_eq!(
            response.workspace.publication_push_status.as_deref(),
            Some("pushed")
        );
        assert_eq!(response.workspace.pr_auto_merge_current, Some(true));
        assert_eq!(
            response.workspace.pr_supervision_status.as_deref(),
            Some("monitoring")
        );
        let github_state = github.state();
        assert!(github_state.fetch_pr_health_calls >= 1);
        assert!(github_state.mark_pr_ready_calls >= 1);
        assert!(github_state.enable_pr_auto_merge_calls >= 1);
        assert_eq!(
            github_state.last_enable_pr_auto_merge_args.as_ref(),
            Some(&(1, "rebase".to_string()))
        );
    }

    #[tokio::test]
    async fn publish_workspace_records_waiting_when_auto_merge_sync_fails() {
        let github = Arc::new(MockGithubService::new());
        let (_temp, state, conversation_id, github) =
            setup_publish_command_state("auto-merge-publish-waiting", true, None, github).await;
        let project = state
            .project_repo
            .get_all()
            .await
            .expect("projects load")
            .into_iter()
            .next()
            .expect("project exists");
        git(
            Path::new(&project.working_directory),
            &["remote", "add", "origin", &project.working_directory],
        );
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.pr_auto_merge_desired = true;
        workspace.pr_auto_merge_method = "squash".to_string();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace update should persist");
        std::fs::write(
            Path::new(&workspace.worktree_path).join("auto-merge-waiting.txt"),
            "ready for review\n",
        )
        .expect("workspace change should be written");
        seed_current_passing_workspace_review(&state, &conversation_id).await;
        github.state().fetch_pr_health_result = Some(Err(AppError::Infrastructure(
            "GitHub health unavailable".to_string(),
        )));
        let client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
        ));
        let state = state.with_agent_client(client);
        let execution_state = Arc::new(ExecutionState::new());

        let response = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect("publish should still succeed when auto-merge sync waits");
        state
            .pr_poller_registry
            .stop_agent_workspace_polling(&conversation_id);

        assert_eq!(
            response.workspace.publication_push_status.as_deref(),
            Some("pushed")
        );
        assert_eq!(response.workspace.pr_auto_merge_current, Some(false));
        assert_eq!(
            response.workspace.pr_supervision_status.as_deref(),
            Some("waiting")
        );
        assert!(response
            .workspace
            .pr_supervision_summary
            .as_deref()
            .unwrap_or_default()
            .contains("could not be refreshed yet"));
        let github_state = github.state();
        assert!(github_state.fetch_pr_health_calls >= 1);
        assert_eq!(github_state.enable_pr_auto_merge_calls, 0);
    }

    #[tokio::test]
    async fn publish_workspace_stops_before_push_when_pr_description_fails() {
        let github = Arc::new(MockGithubService::new());
        let (_temp, state, conversation_id, github) =
            setup_publish_command_state("description-fails", true, None, github).await;
        let project = state
            .project_repo
            .get_all()
            .await
            .expect("projects load")
            .into_iter()
            .next()
            .expect("project exists");
        git(
            Path::new(&project.working_directory),
            &["remote", "add", "origin", &project.working_directory],
        );
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        std::fs::write(
            Path::new(&workspace.worktree_path).join("implementation.txt"),
            "change that should be described\n",
        )
        .expect("workspace change should be written");
        seed_current_passing_workspace_review(&state, &conversation_id).await;
        let execution_state = Arc::new(ExecutionState::new());

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
            false,
        )
        .await
        .expect_err("missing generated PR description should block publish");

        assert!(error.contains("completed without submitting a PR description"));
        assert_eq!(github.state().push_branch_calls, 0);
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(
            stored.publication_push_status.as_deref(),
            Some("description_failed")
        );
        let events = state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("publication events should load");
        assert!(events.iter().any(|event| {
            event.step == "describing"
                && event.status == "started"
                && event.summary == "Drafting pull request description"
        }));
        assert!(events.iter().any(|event| {
            event.step == "description_failed"
                && event.status == "failed"
                && event.classification.as_deref() == Some("operational")
        }));
    }

    #[test]
    fn agent_conversation_response_derives_provider_metadata_from_legacy_claude_session() {
        let mut conversation =
            ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
        conversation.claude_session_id = Some("claude-session-123".to_string());

        let response = AgentConversationResponse::from(conversation);

        assert_eq!(
            response.claude_session_id,
            Some("claude-session-123".to_string())
        );
        assert_eq!(
            response.provider_session_id,
            Some("claude-session-123".to_string())
        );
        assert_eq!(response.provider_harness, Some("claude".to_string()));
        assert_eq!(response.coordination_mode, "solo");
    }

    #[test]
    fn agent_conversation_response_keeps_codex_metadata_without_legacy_alias() {
        let mut conversation =
            ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
        conversation.set_provider_session_ref(ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: "codex-thread-123".to_string(),
        });

        let response = AgentConversationResponse::from(conversation);

        assert_eq!(response.claude_session_id, None);
        assert_eq!(
            response.provider_session_id,
            Some("codex-thread-123".to_string())
        );
        assert_eq!(response.provider_harness, Some("codex".to_string()));
    }

    #[test]
    fn agent_conversation_response_restores_legacy_alias_for_canonical_claude_provider_metadata() {
        let mut conversation =
            ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
        conversation.provider_harness = Some(AgentHarnessKind::Claude);
        conversation.provider_session_id = Some("claude-session-456".to_string());
        conversation.claude_session_id = None;

        let response = AgentConversationResponse::from(conversation);

        assert_eq!(
            response.claude_session_id,
            Some("claude-session-456".to_string())
        );
        assert_eq!(
            response.provider_session_id,
            Some("claude-session-456".to_string())
        );
        assert_eq!(response.provider_harness, Some("claude".to_string()));
    }

    #[test]
    fn agent_conversation_response_includes_automation_ownership() {
        let mut conversation =
            ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
        conversation.automation_id = Some(AutomationId::from_string("automation-1"));
        conversation.automation_run_id = Some(AutomationRunId::from_string("run-1"));

        let response = AgentConversationResponse::from(conversation);

        assert_eq!(response.automation_id.as_deref(), Some("automation-1"));
        assert_eq!(response.automation_run_id.as_deref(), Some("run-1"));
    }

    #[tokio::test]
    async fn agent_conversation_response_hydrates_runtime_from_copied_message_attribution() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        let conversation = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project_id.clone()))
            .await
            .expect("conversation should be created");
        let mut message = ChatMessage::user_in_project(project_id, "assistant response");
        message.role = MessageRole::Orchestrator;
        message.conversation_id = Some(conversation.id);
        message.logical_model = Some("gpt-5.5".to_string());
        message.effective_model_id = Some("gpt-5.5".to_string());
        message.logical_effort = Some(LogicalEffort::High);
        message.effective_effort = Some("high".to_string());
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("message should be created");

        let response = agent_conversation_response_for_state(&state, conversation)
            .await
            .expect("response should hydrate runtime attribution");

        assert_eq!(response.logical_model.as_deref(), Some("gpt-5.5"));
        assert_eq!(response.effective_model_id.as_deref(), Some("gpt-5.5"));
        assert_eq!(response.logical_effort.as_deref(), Some("high"));
        assert_eq!(response.effective_effort.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn agent_conversation_response_prefers_latest_run_runtime_over_message_attribution() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        let conversation = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project_id.clone()))
            .await
            .expect("conversation should be created");
        let mut message = ChatMessage::user_in_project(project_id, "assistant response");
        message.role = MessageRole::Orchestrator;
        message.conversation_id = Some(conversation.id);
        message.effective_model_id = Some("sonnet".to_string());
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("message should be created");

        let mut run = AgentRun::new(conversation.id);
        run.logical_model = Some("opus".to_string());
        run.effective_model_id = Some("opus".to_string());
        run.logical_effort = Some(LogicalEffort::Medium);
        run.effective_effort = Some("medium".to_string());
        state
            .agent_run_repo
            .create(run)
            .await
            .expect("run should be created");

        let response = agent_conversation_response_for_state(&state, conversation)
            .await
            .expect("response should hydrate runtime attribution");

        assert_eq!(response.logical_model.as_deref(), Some("opus"));
        assert_eq!(response.effective_model_id.as_deref(), Some("opus"));
        assert_eq!(response.logical_effort.as_deref(), Some("medium"));
        assert_eq!(response.effective_effort.as_deref(), Some("medium"));
    }

    #[tokio::test]
    async fn agent_conversation_responses_for_state_hydrates_each_conversation() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-response-list".to_string());
        let first = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project_id.clone()))
            .await
            .expect("first conversation should be created");
        let second = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project_id.clone()))
            .await
            .expect("second conversation should be created");
        let first_id = first.id.as_str();
        let second_id = second.id.as_str();

        let mut first_run = AgentRun::new(first.id);
        first_run.logical_model = Some("gpt-5.5".to_string());
        first_run.effective_model_id = Some("gpt-5.5".to_string());
        first_run.logical_effort = Some(LogicalEffort::High);
        first_run.effective_effort = Some("high".to_string());
        state
            .agent_run_repo
            .create(first_run)
            .await
            .expect("first run should be created");

        let mut second_message = ChatMessage::user_in_project(project_id, "copied attribution");
        second_message.role = MessageRole::Orchestrator;
        second_message.conversation_id = Some(second.id);
        second_message.logical_model = Some("claude-sonnet".to_string());
        second_message.effective_model_id = Some("claude-sonnet-4".to_string());
        second_message.logical_effort = Some(LogicalEffort::Medium);
        second_message.effective_effort = Some("medium".to_string());
        state
            .chat_message_repo
            .create(second_message)
            .await
            .expect("second message should be created");

        let responses = agent_conversation_responses_for_state(&state, vec![first, second])
            .await
            .expect("responses should hydrate runtime attribution");

        let first_response = responses
            .iter()
            .find(|response| response.id == first_id)
            .expect("first response should be present");
        assert_eq!(first_response.logical_model.as_deref(), Some("gpt-5.5"));
        assert_eq!(
            first_response.effective_model_id.as_deref(),
            Some("gpt-5.5")
        );
        assert_eq!(first_response.logical_effort.as_deref(), Some("high"));
        assert_eq!(first_response.effective_effort.as_deref(), Some("high"));

        let second_response = responses
            .iter()
            .find(|response| response.id == second_id)
            .expect("second response should be present");
        assert_eq!(
            second_response.logical_model.as_deref(),
            Some("claude-sonnet")
        );
        assert_eq!(
            second_response.effective_model_id.as_deref(),
            Some("claude-sonnet-4")
        );
        assert_eq!(second_response.logical_effort.as_deref(), Some("medium"));
        assert_eq!(second_response.effective_effort.as_deref(), Some("medium"));
    }

    #[tokio::test]
    async fn fork_response_for_state_includes_workspace_counts_parent_and_runtime() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-fork-response".to_string());
        let mut parent = ChatConversation::new_project(project_id.clone());
        parent.set_title("[Fork] Source conversation");
        let parent_id = parent.id.as_str();
        let mut child = ChatConversation::new_project(project_id.clone());
        child.parent_conversation_id = Some(parent_id.clone());
        child.set_provider_session_ref(ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: "child-thread".to_string(),
        });
        let child_id = child.id.as_str();

        let mut run = AgentRun::new(child.id);
        run.logical_model = Some("gpt-5.5".to_string());
        run.effective_model_id = Some("gpt-5.5".to_string());
        run.logical_effort = Some(LogicalEffort::High);
        run.effective_effort = Some("high".to_string());
        state
            .agent_run_repo
            .create(run)
            .await
            .expect("child run should be created");

        let workspace = AgentConversationWorkspace::new(
            child.id,
            project_id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-sha".to_string()),
            "ralphx/test/fork-response".to_string(),
            "/tmp/fork-response".to_string(),
        );
        let result = crate::application::agent_conversation_fork::AgentConversationForkResult {
            parent_conversation: parent,
            conversation: child,
            workspace: Some(workspace),
            provider_session: Some(
                crate::application::provider_session_fork::ProviderSessionForkResult {
                    session_ref: ProviderSessionRef {
                        harness: AgentHarnessKind::Codex,
                        provider_session_id: "child-thread".to_string(),
                    },
                    source_path: PathBuf::from("/tmp/source.jsonl"),
                    destination_path: PathBuf::from("/tmp/dest.jsonl"),
                },
            ),
            copied_message_count: 2,
            copied_timeline_item_count: 3,
        };

        let response = fork_agent_conversation_response_for_state(&state, result)
            .await
            .expect("fork response should be built");

        assert_eq!(response.parent_conversation.id, parent_id);
        assert_eq!(response.conversation.id, child_id);
        assert_eq!(
            response.conversation.parent_conversation_id.as_deref(),
            Some(parent_id.as_str())
        );
        assert_eq!(
            response.conversation.provider_harness.as_deref(),
            Some("codex")
        );
        assert_eq!(
            response.conversation.provider_session_id.as_deref(),
            Some("child-thread")
        );
        assert_eq!(
            response.conversation.logical_model.as_deref(),
            Some("gpt-5.5")
        );
        assert_eq!(
            response.conversation.logical_effort.as_deref(),
            Some("high")
        );
        assert!(response.provider_session_forked);
        assert_eq!(response.copied_message_count, 2);
        assert_eq!(response.copied_timeline_item_count, 3);
        assert_eq!(
            response
                .workspace
                .as_ref()
                .map(|workspace| workspace.mode.as_str()),
            Some("edit")
        );
    }

    #[test]
    fn emit_agent_conversation_fork_events_accepts_response_payload() {
        let project_id = ProjectId::from_string("project-fork-events".to_string());
        let parent =
            AgentConversationResponse::from(ChatConversation::new_project(project_id.clone()));
        let mut child_conversation = ChatConversation::new_project(project_id);
        child_conversation.parent_conversation_id = Some(parent.id.clone());
        let child = AgentConversationResponse::from(child_conversation);
        let response = ForkAgentConversationResponse {
            parent_conversation: parent,
            conversation: child,
            workspace: None,
            provider_session_forked: false,
            copied_message_count: 0,
            copied_timeline_item_count: 0,
        };
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        emit_agent_conversation_fork_events(app.handle(), &response);
    }

    #[tokio::test]
    async fn fork_terminal_agent_conversation_for_send_skips_without_terminal_workspace() {
        let state = AppState::new_test();
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app should build");
        assert!(fork_terminal_agent_conversation_for_send(
            &state,
            app.handle(),
            None,
            "",
            None,
            None
        )
        .await
        .expect("missing conversation id should be ignored")
        .is_none());

        let project_id = ProjectId::from_string("project-terminal-fork-skip".to_string());
        let mut project = Project::new(
            "Terminal Fork Skip".to_string(),
            "/tmp/terminal-fork-skip".to_string(),
        );
        project.id = project_id.clone();
        state
            .project_repo
            .create(project)
            .await
            .expect("project should be created");
        let conversation = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project_id.clone()))
            .await
            .expect("conversation should be created");
        assert!(fork_terminal_agent_conversation_for_send(
            &state,
            app.handle(),
            Some(&conversation.id),
            "",
            None,
            None
        )
        .await
        .expect("missing workspace should be ignored")
        .is_none());

        let workspace = AgentConversationWorkspace::new(
            conversation.id,
            project_id,
            AgentConversationWorkspaceMode::Chat,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-sha".to_string()),
            "ralphx/test/non-terminal".to_string(),
            "/tmp/non-terminal".to_string(),
        );
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be created");

        assert!(fork_terminal_agent_conversation_for_send(
            &state,
            app.handle(),
            Some(&conversation.id),
            "",
            None,
            None
        )
        .await
        .expect("non-terminal workspace should be ignored")
        .is_none());
    }

    #[tokio::test]
    async fn fork_terminal_agent_conversation_for_send_forks_terminal_workspace() {
        let state = AppState::new_test();
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app should build");
        let project_id = ProjectId::from_string("project-terminal-fork".to_string());
        let mut project = Project::new(
            "Terminal Fork".to_string(),
            "/tmp/terminal-fork".to_string(),
        );
        project.id = project_id.clone();
        state
            .project_repo
            .create(project)
            .await
            .expect("project should be created");
        let parent = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project_id.clone()))
            .await
            .expect("parent conversation should be created");
        let parent_id = parent.id.as_str();
        let mut workspace = AgentConversationWorkspace::new(
            parent.id,
            project_id,
            AgentConversationWorkspaceMode::Chat,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-sha".to_string()),
            "ralphx/test/terminal".to_string(),
            "/tmp/terminal".to_string(),
        );
        workspace.publication_pr_status = Some("merged".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be created");

        let child_id = fork_terminal_agent_conversation_for_send(
            &state,
            app.handle(),
            Some(&parent.id),
            "",
            None,
            None,
        )
        .await
        .expect("terminal workspace should fork")
        .expect("forked conversation id should be returned");
        let child = state
            .chat_conversation_repo
            .get_by_id(&child_id)
            .await
            .expect("child lookup should succeed")
            .expect("child conversation should exist");

        assert_eq!(
            child.parent_conversation_id.as_deref(),
            Some(parent_id.as_str())
        );
        assert_eq!(child.agent_mode, Some(AgentConversationWorkspaceMode::Chat));
        assert!(state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&child_id)
            .await
            .expect("workspace lookup should succeed")
            .is_none());
    }

    #[tokio::test]
    async fn fork_terminal_agent_conversation_for_send_spawns_session_namer_with_new_message() {
        let concrete_client = Arc::new(MockAgenticClient::new());
        let agent_client: Arc<dyn AgenticClient> = concrete_client.clone();
        let state = AppState::new_test().with_agent_client(agent_client);
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app should build");
        let project_id = ProjectId::from_string("project-terminal-fork-namer".to_string());
        let mut project = Project::new(
            "Terminal Fork Namer".to_string(),
            "/tmp/terminal-fork-namer".to_string(),
        );
        project.id = project_id.clone();
        state
            .project_repo
            .create(project)
            .await
            .expect("project should be created");
        let mut parent = ChatConversation::new_project(project_id.clone());
        parent.set_title("Stabilize publication recovery");
        let parent = state
            .chat_conversation_repo
            .create(parent)
            .await
            .expect("parent conversation should be created");
        let mut prior_message = ChatMessage::user_in_project(
            project_id.clone(),
            "The merged workspace still reopens with stale publication state.",
        );
        prior_message.conversation_id = Some(parent.id.clone());
        state
            .chat_message_repo
            .create(prior_message)
            .await
            .expect("prior message should be created");

        let mut workspace = AgentConversationWorkspace::new(
            parent.id.clone(),
            project_id,
            AgentConversationWorkspaceMode::Chat,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-sha".to_string()),
            "ralphx/test/terminal-namer".to_string(),
            "/tmp/terminal-namer".to_string(),
        );
        workspace.publication_pr_status = Some("merged".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be created");

        let child_id = fork_terminal_agent_conversation_for_send(
            &state,
            app.handle(),
            Some(&parent.id),
            "Please continue the closed run and fix the title fallback.",
            None,
            None,
        )
        .await
        .expect("terminal workspace should fork")
        .expect("forked conversation id should be returned");

        for _ in 0..20 {
            if !concrete_client.get_spawn_calls().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let spawn_calls = concrete_client.get_spawn_calls().await;
        let prompt = spawn_calls
            .iter()
            .find_map(|call| match &call.call_type {
                MockCallType::Spawn { prompt, .. } => Some(prompt.as_str()),
                _ => None,
            })
            .expect("session namer should be spawned");

        assert!(prompt.contains(&format!(
            "<conversation_id>{}</conversation_id>",
            child_id.as_str()
        )));
        assert!(prompt.contains(
            "<user_message>Please continue the closed run and fix the title fallback.</user_message>"
        ));
        assert!(prompt.contains(
            "<content>The merged workspace still reopens with stale publication state.</content>"
        ));
    }

    #[tokio::test]
    async fn fork_agent_conversation_command_returns_hydrated_child_response() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-fork-command".to_string());
        let mut project = Project::new(
            "Fork Command Project".to_string(),
            "/tmp/fork-command-project".to_string(),
        );
        project.id = project_id.clone();
        state
            .project_repo
            .create(project)
            .await
            .expect("project should be created");
        let mut parent = ChatConversation::new_project(project_id.clone());
        parent.set_title("Source conversation");
        parent.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
        let parent = state
            .chat_conversation_repo
            .create(parent)
            .await
            .expect("parent conversation should be created");
        let mut message = ChatMessage::user_in_project(project_id, "copied runtime");
        message.conversation_id = Some(parent.id);
        message.logical_model = Some("gpt-5.4".to_string());
        message.effective_model_id = Some("gpt-5.4".to_string());
        message.logical_effort = Some(LogicalEffort::Medium);
        message.effective_effort = Some("medium".to_string());
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("message should be created");
        let parent_id = parent.id.as_str();
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = fork_agent_conversation(
            ForkAgentConversationInput {
                conversation_id: parent.id.as_str(),
            },
            app.state(),
            app.handle().clone(),
        )
        .await
        .expect("fork command should succeed");

        assert_eq!(response.parent_conversation.id, parent_id);
        assert_eq!(
            response.conversation.parent_conversation_id.as_deref(),
            Some(parent_id.as_str())
        );
        assert_eq!(
            response.conversation.title.as_deref(),
            Some("[Fork] Source conversation")
        );
        assert_eq!(response.conversation.agent_mode.as_deref(), Some("chat"));
        assert_eq!(response.copied_message_count, 1);
        assert!(response.workspace.is_none());
        assert_eq!(
            response.conversation.logical_model.as_deref(),
            Some("gpt-5.4")
        );
        assert_eq!(
            response.conversation.logical_effort.as_deref(),
            Some("medium")
        );
    }

    #[tokio::test]
    async fn list_page_create_archive_restore_and_summary_hydrate_runtime_attribution() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-command-runtime".to_string());
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.set_title("Runtime conversation");
        let conversation = state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should be created");
        let conversation_id = conversation.id.as_str();
        let mut run = AgentRun::new(conversation.id);
        run.logical_model = Some("gpt-5.5".to_string());
        run.effective_model_id = Some("gpt-5.5".to_string());
        run.logical_effort = Some(LogicalEffort::High);
        run.effective_effort = Some("high".to_string());
        state
            .agent_run_repo
            .create(run)
            .await
            .expect("run should be created");
        let mut child = ChatConversation::new_project(project_id.clone());
        child.parent_conversation_id = Some(conversation.id.as_str().to_string());
        child.set_title("Review workspace changes");
        let child = state
            .chat_conversation_repo
            .create(child)
            .await
            .expect("child conversation should be created");
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let page = list_agent_conversations_page(
            ChatContextType::Project.to_string(),
            project_id.as_str().to_string(),
            Some(true),
            Some(false),
            Some(0),
            Some(10),
            None,
            app.state(),
        )
        .await
        .expect("conversation page should load");
        assert_eq!(page.total, 1);
        let page_conversation = page
            .conversations
            .iter()
            .find(|conversation| conversation.id == conversation_id)
            .expect("seeded conversation should be listed");
        assert_eq!(page_conversation.logical_model.as_deref(), Some("gpt-5.5"));
        assert_eq!(page_conversation.logical_effort.as_deref(), Some("high"));
        assert!(page
            .conversations
            .iter()
            .all(|conversation| conversation.id != child.id.as_str()));

        let summary = get_agent_conversation_summary_for_app_state(
            app.state::<AppState>().inner(),
            conversation_id.clone(),
        )
        .await
        .expect("summary should load")
        .expect("summary should exist");
        assert_eq!(summary.effective_model_id.as_deref(), Some("gpt-5.5"));
        assert_eq!(summary.effective_effort.as_deref(), Some("high"));

        let created = create_agent_conversation(
            CreateAgentConversationInput {
                context_type: ChatContextType::Project.to_string(),
                context_id: project_id.as_str().to_string(),
                title: Some("Created from command".to_string()),
            },
            app.state(),
        )
        .await
        .expect("conversation should be created");
        assert_eq!(created.title.as_deref(), Some("Created from command"));

        let archived = archive_agent_conversation(conversation_id.clone(), app.state())
            .await
            .expect("conversation should be archived");
        assert!(archived.archived_at.is_some());
        assert_eq!(archived.logical_model.as_deref(), Some("gpt-5.5"));

        let restored = restore_agent_conversation(conversation_id, app.state())
            .await
            .expect("conversation should be restored");
        assert!(restored.archived_at.is_none());
        assert_eq!(restored.logical_effort.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn list_page_includes_child_conversations_with_owned_workspaces() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-command-child-workspace".to_string());
        let mut parent = ChatConversation::new_project(project_id.clone());
        parent.set_title("Merged parent workspace");
        let parent = state
            .chat_conversation_repo
            .create(parent)
            .await
            .expect("parent conversation should be created");

        let mut embedded_child = ChatConversation::new_project(project_id.clone());
        embedded_child.parent_conversation_id = Some(parent.id.as_str().to_string());
        embedded_child.set_title("Embedded review child");
        let embedded_child = state
            .chat_conversation_repo
            .create(embedded_child)
            .await
            .expect("embedded child should be created");

        let mut workspace_child = ChatConversation::new_project(project_id.clone());
        workspace_child.parent_conversation_id = Some(parent.id.as_str().to_string());
        workspace_child.set_title("Continued child workspace");
        let workspace_child = state
            .chat_conversation_repo
            .create(workspace_child)
            .await
            .expect("workspace child should be created");
        let workspace = workspace_for_runtime_test(&workspace_child.id, &project_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should be created");

        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let page = list_agent_conversations_page(
            ChatContextType::Project.to_string(),
            project_id.as_str().to_string(),
            Some(false),
            Some(false),
            Some(0),
            Some(10),
            None,
            app.state(),
        )
        .await
        .expect("conversation page should load");

        let conversation_ids = page
            .conversations
            .iter()
            .map(|conversation| conversation.id.clone())
            .collect::<Vec<_>>();
        assert!(
            conversation_ids.contains(&workspace_child.id.as_str()),
            "child conversations with their own workspace should be listed"
        );
        assert!(
            !conversation_ids.contains(&embedded_child.id.as_str()),
            "embedded child conversations without workspaces should stay hidden"
        );
    }

    #[tokio::test]
    async fn agent_list_filter_keeps_task_runtime_child_conversations() {
        let state = AppState::new_test();
        let task_id = TaskId::from_string("task-runtime-visible".to_string());
        let parent = state
            .chat_conversation_repo
            .create(ChatConversation::new_task_execution(task_id.clone()))
            .await
            .expect("parent task runtime conversation should be created");

        let mut child = ChatConversation::new_task_execution(task_id);
        child.parent_conversation_id = Some(parent.id.as_str().to_string());
        let child = state
            .chat_conversation_repo
            .create(child)
            .await
            .expect("child task runtime conversation should be created");

        let filtered =
            filter_agent_list_visible_conversations(&state, vec![child.clone(), parent.clone()])
                .await
                .expect("shared list filter should run");
        let filtered_ids = filtered
            .iter()
            .map(|conversation| conversation.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            filtered_ids,
            vec![child.id.as_str(), parent.id.as_str()],
            "task runtime attempts should stay visible even when parented"
        );
    }

    #[tokio::test]
    async fn agent_list_endpoints_show_automation_setup_and_hide_runs_but_direct_fetch_works() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-command-automation-hidden".to_string());
        let mut visible = ChatConversation::new_project(project_id.clone());
        visible.set_title("Manual agent conversation");
        let visible = state
            .chat_conversation_repo
            .create(visible)
            .await
            .expect("visible conversation should be created");

        let automation_id = AutomationId::from_string("automation-1");
        let mut setup = ChatConversation::new_project(project_id.clone());
        setup.set_title("Automation setup conversation");
        setup.automation_id = Some(automation_id.clone());
        let setup = state
            .chat_conversation_repo
            .create(setup)
            .await
            .expect("setup conversation should be created");

        let mut run = ChatConversation::new_project(project_id.clone());
        run.set_title("Automation run conversation");
        run.automation_id = Some(automation_id);
        run.automation_run_id = Some(AutomationRunId::from_string("run-1"));
        let run = state
            .chat_conversation_repo
            .create(run)
            .await
            .expect("run conversation should be created");

        let filtered = filter_agent_list_visible_conversations(
            &state,
            vec![visible.clone(), setup.clone(), run.clone()],
        )
        .await
        .expect("shared list filter should run");
        let filtered_ids = filtered
            .iter()
            .map(|conversation| conversation.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(filtered_ids, vec![visible.id.as_str(), setup.id.as_str()]);

        let setup_conversation = state
            .chat_conversation_repo
            .get_by_id(&setup.id)
            .await
            .expect("direct setup conversation fetch should load")
            .expect("direct setup conversation should exist");
        let setup_response = agent_conversation_response_for_state(&state, setup_conversation)
            .await
            .expect("setup response should hydrate");
        assert_eq!(setup_response.id, setup.id.as_str());
        assert_eq!(
            setup_response.automation_id.as_deref(),
            Some("automation-1")
        );

        let run_conversation = state
            .chat_conversation_repo
            .get_by_id(&run.id)
            .await
            .expect("direct run conversation fetch should load")
            .expect("direct run conversation should exist");
        let run_response = agent_conversation_response_for_state(&state, run_conversation)
            .await
            .expect("run response should hydrate");
        assert_eq!(run_response.id, run.id.as_str());
        assert_eq!(run_response.automation_run_id.as_deref(), Some("run-1"));

        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let page = list_agent_conversations_page(
            ChatContextType::Project.to_string(),
            project_id.as_str().to_string(),
            Some(false),
            Some(false),
            Some(0),
            Some(10),
            None,
            app.state(),
        )
        .await
        .expect("conversation page should load");
        let page_ids = page
            .conversations
            .iter()
            .map(|conversation| conversation.id.clone())
            .collect::<Vec<_>>();
        let visible_id = visible.id.as_str();
        let setup_id = setup.id.as_str();
        let run_id = run.id.as_str();
        assert_eq!(page_ids.len(), 2);
        assert!(
            page_ids.contains(&visible_id),
            "manual conversations should be listed"
        );
        assert!(
            page_ids.contains(&setup_id),
            "automation setup conversations should be listed"
        );
        assert!(
            !page_ids.contains(&run_id),
            "automation run conversations should stay hidden from list endpoints"
        );
    }

    fn mode_lock_test_workspace(
        conversation_id: ChatConversationId,
        project_id: ProjectId,
    ) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            conversation_id,
            project_id,
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::CurrentBranch,
            "feature/mode-lock".to_string(),
            Some("Current branch (feature/mode-lock)".to_string()),
            Some("base-sha".to_string()),
            "ralphx/project/mode-lock".to_string(),
            "/tmp/ralphx-mode-lock".to_string(),
        )
    }

    #[tokio::test]
    async fn workspace_response_projects_active_ideation_mode_lock() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-active-mode-lock".to_string());
        let conversation_id =
            ChatConversationId::from_string("77777777-7777-4777-8777-777777777777");
        let session = state
            .ideation_session_repo
            .create(IdeationSession::new(project_id.clone()))
            .await
            .expect("ideation session persisted");
        let mut workspace = mode_lock_test_workspace(conversation_id, project_id);
        workspace.linked_ideation_session_id = Some(session.id);

        let response = agent_workspace_response_for_state(&state, workspace)
            .await
            .expect("workspace response resolves mode lock");

        assert!(response.mode_switch_locked);
        assert_eq!(
            response.mode_switch_lock_reason.as_deref(),
            Some("Ideation session is still active")
        );
    }

    #[tokio::test]
    async fn workspace_response_keeps_active_planning_session_unlocked() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-planning-mode-unlocked".to_string());
        let conversation_id =
            ChatConversationId::from_string("77777777-7777-4777-8777-777777777778");
        let session = state
            .ideation_session_repo
            .create(
                IdeationSession::builder()
                    .project_id(project_id.clone())
                    .session_flow(IdeationSessionFlow::Planning)
                    .build(),
            )
            .await
            .expect("planning session persisted");
        let mut workspace = mode_lock_test_workspace(conversation_id, project_id);
        workspace.mode = AgentConversationWorkspaceMode::Plan;
        workspace.linked_ideation_session_id = Some(session.id);

        let response = agent_workspace_response_for_state(&state, workspace)
            .await
            .expect("workspace response resolves mode lock");

        assert!(!response.mode_switch_locked);
        assert!(response.mode_switch_lock_reason.is_none());
    }

    #[tokio::test]
    async fn workspace_response_projects_superseded_execution_plan_as_unlocked() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-superseded-mode-lock".to_string());
        let conversation_id =
            ChatConversationId::from_string("88888888-8888-4888-8888-888888888888");
        let session = state
            .ideation_session_repo
            .create(IdeationSession::new(project_id.clone()))
            .await
            .expect("ideation session persisted");
        let mut execution_plan = ExecutionPlan::new(session.id.clone());
        execution_plan.status = ExecutionPlanStatus::Superseded;
        let execution_plan = state
            .execution_plan_repo
            .create(execution_plan)
            .await
            .expect("execution plan persisted");
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-superseded-lock"),
            session.id.clone(),
            project_id.clone(),
            "plan-superseded-lock".to_string(),
            "main".to_string(),
        );
        plan_branch.execution_plan_id = Some(execution_plan.id);
        let plan_branch = state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch persisted");
        let mut workspace = mode_lock_test_workspace(conversation_id, project_id);
        workspace.linked_ideation_session_id = Some(session.id);
        workspace.linked_plan_branch_id = Some(plan_branch.id);

        let response = agent_workspace_response_for_state(&state, workspace)
            .await
            .expect("workspace response resolves mode lock");

        assert!(!response.mode_switch_locked);
        assert!(response.mode_switch_lock_reason.is_none());
    }

    #[tokio::test]
    async fn workspace_response_treats_missing_mode_owner_links_as_unlocked() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-missing-mode-lock".to_string());
        let plan_conversation_id =
            ChatConversationId::from_string("99999999-9999-4999-8999-999999999999");
        let mut plan_workspace = mode_lock_test_workspace(plan_conversation_id, project_id.clone());
        plan_workspace.linked_plan_branch_id =
            Some(PlanBranchId::from_string("missing-plan-branch".to_string()));

        let plan_response = agent_workspace_response_for_state(&state, plan_workspace)
            .await
            .expect("missing plan branch resolves as unlocked");
        assert!(!plan_response.mode_switch_locked);
        assert!(plan_response.mode_switch_lock_reason.is_none());

        let session_conversation_id =
            ChatConversationId::from_string("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
        let mut session_workspace = mode_lock_test_workspace(session_conversation_id, project_id);
        session_workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string(
            "missing-ideation-session".to_string(),
        ));

        let session_response = agent_workspace_response_for_state(&state, session_workspace)
            .await
            .expect("missing ideation session resolves as unlocked");
        assert!(!session_response.mode_switch_locked);
        assert!(session_response.mode_switch_lock_reason.is_none());
    }

    #[tokio::test]
    async fn switching_to_chat_without_existing_workspace_keeps_workspace_absent() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-chat-no-workspace".to_string());
        let conversation_id =
            ChatConversationId::from_string("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
        let mut conversation = ChatConversation::new_project(project_id);
        conversation.id = conversation_id;
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation persisted");

        let response = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "chat".to_string(),
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            &state,
        )
        .await
        .expect("chat mode switch succeeds without workspace");

        assert_eq!(response.conversation.agent_mode.as_deref(), Some("chat"));
        assert!(response.workspace.is_none());
    }

    #[tokio::test]
    async fn switching_to_edit_without_existing_workspace_creates_workspace() {
        let state = AppState::new_test();
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_publish_repo(&repo_path);
        let project_id = ProjectId::from_string("project-edit-new-workspace".to_string());
        let conversation_id =
            ChatConversationId::from_string("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
        let mut project = Project::new(
            "Mode Switch Project".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project)
            .await
            .expect("project persisted");
        state
            .execution_settings_repo
            .update_settings(
                Some(&project_id),
                &ExecutionSettings {
                    agent_workspace_pr_autofix_default: true,
                    agent_workspace_pr_auto_merge_default: true,
                    ..ExecutionSettings::default()
                },
            )
            .await
            .expect("settings persisted");
        let mut conversation = ChatConversation::new_project(project_id);
        conversation.id = conversation_id;
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation persisted");

        let response = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "edit".to_string(),
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            &state,
        )
        .await
        .expect("edit mode switch creates workspace");

        assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
        let workspace = response.workspace.expect("workspace should be returned");
        assert_eq!(workspace.mode.as_str(), "edit");
        assert!(workspace.pr_autofix_enabled);
        assert!(workspace.pr_auto_merge_desired);
    }

    #[tokio::test]
    async fn switching_branchless_chat_to_edit_persists_source_pull_request_metadata() {
        let state = AppState::new_test();
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_publish_repo(&repo_path);
        git(&repo_path, &["checkout", "-b", "feature/source-pr"]);
        std::fs::write(repo_path.join("README.md"), "source pr\n")
            .expect("fixture update should be written");
        git(&repo_path, &["add", "README.md"]);
        git(&repo_path, &["commit", "-m", "source pr"]);
        let source_sha = git(&repo_path, &["rev-parse", "HEAD"]);
        git(&repo_path, &["checkout", "main"]);

        let project_id = ProjectId::from_string("project-source-pr-switch".to_string());
        let conversation_id =
            ChatConversationId::from_string("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee");
        let mut project = Project::new(
            "Mode Switch Source PR".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project)
            .await
            .expect("project persisted");
        let mut conversation = ChatConversation::new_project(project_id);
        conversation.id = conversation_id.clone();
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation persisted");

        let response = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "edit".to_string(),
                base_ref_kind: Some("local_branch".to_string()),
                base_branch_mode: None,
                base_ref: Some("feature/source-pr".to_string()),
                base_display_name: Some("PR #456: Source PR".to_string()),
                base_source_pull_request: Some(AgentWorkspaceSourcePullRequestInput {
                    number: 456,
                    url: Some("https://github.com/owner/repo/pull/456".to_string()),
                    title: Some("Source PR".to_string()),
                    head_ref_name: "feature/source-pr".to_string(),
                    base_ref_name: Some("main".to_string()),
                    head_ref_oid: Some(source_sha.clone()),
                }),
            },
            &state,
        )
        .await
        .expect("edit mode switch should create source PR workspace");

        let workspace = response.workspace.expect("workspace should be returned");
        assert_eq!(workspace.mode, "edit");
        assert_eq!(workspace.branch_mode, "isolated");
        assert_eq!(workspace.base_ref_kind, "local_branch");
        assert_eq!(workspace.base_ref, "feature/source-pr");
        assert_ne!(workspace.branch_name, "feature/source-pr");
        assert!(workspace.branch_name.contains("/agent-"));
        assert_eq!(workspace.publication_pr_number, None);
        assert_eq!(workspace.publication_pr_status.as_deref(), None);
        let source = workspace
            .source_pull_request
            .expect("source PR metadata should be returned");
        assert_eq!(source.number, 456);
        assert_eq!(source.head_ref_name, "feature/source-pr");
        assert_eq!(source.base_ref_name.as_deref(), Some("main"));
        assert_eq!(source.head_ref_oid.as_deref(), Some(source_sha.as_str()));

        let persisted = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup succeeds")
            .expect("workspace should persist");
        assert_eq!(
            persisted.branch_mode,
            AgentConversationWorkspaceBranchMode::Isolated
        );
        assert_eq!(
            persisted.base_ref_kind,
            IdeationAnalysisBaseRefKind::LocalBranch
        );
        assert_eq!(persisted.base_ref, "feature/source-pr");
        assert_ne!(persisted.branch_name, "feature/source-pr");
        assert!(persisted.branch_name.contains("/agent-"));
        assert_eq!(persisted.publication_pr_number, None);
        assert_eq!(
            persisted.publication_pr_url.as_deref(),
            None
        );
        assert_eq!(persisted.publication_pr_status.as_deref(), None);
        assert_eq!(
            persisted
                .source_pull_request
                .as_ref()
                .map(|source| source.number),
            Some(456)
        );
        assert_eq!(
            persisted
                .source_pull_request
                .as_ref()
                .and_then(|source| source.base_ref_name.as_deref()),
            Some("main")
        );
    }

    #[tokio::test]
    async fn accepted_plan_proposal_switch_can_bypass_running_agent_guard() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-running-plan-switch".to_string());
        let conversation_id =
            ChatConversationId::from_string("12121212-1212-4121-8121-121212121212");
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.id = conversation_id.clone();
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation persisted");

        let workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project_id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::CurrentBranch,
            "feature/agent-screen".to_string(),
            Some("Current branch (feature/agent-screen)".to_string()),
            Some("base-sha".to_string()),
            "ralphx/project/agent-12121212".to_string(),
            "/tmp/ralphx-agent-12121212".to_string(),
        );
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace persisted");

        let running_key = RunningAgentKey::new(
            ChatContextType::Project.to_string(),
            conversation_id.as_str(),
        );
        state
            .running_agent_registry
            .register(
                running_key,
                123,
                conversation_id.as_str(),
                "run-plan-proposal".to_string(),
                None,
                None,
            )
            .await;

        let public_result = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "plan".to_string(),
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            &state,
        )
        .await;
        assert_eq!(
            public_result.expect_err("public switch should reject running agents"),
            "Cannot change mode while the agent is running"
        );

        let response = switch_agent_conversation_mode_for_state_allowing_running(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "plan".to_string(),
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            &state,
            ModeSwitchInitiator::User,
        )
        .await
        .expect("accepted proposal switch should bypass running guard");

        assert_eq!(response.conversation.agent_mode.as_deref(), Some("plan"));
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup succeeds")
            .expect("workspace exists");
        assert_eq!(stored.mode, AgentConversationWorkspaceMode::Plan);
    }

    #[tokio::test]
    async fn switching_unlocked_linked_plan_ideation_to_edit_uses_plan_worktree() {
        let state = AppState::new_test();
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        let main_sha = setup_publish_repo(&repo_path);
        let plan_branch_name = "plan/manual-agent-handoff";
        git(&repo_path, &["branch", plan_branch_name]);

        let project_id = ProjectId::from_string("project-linked-plan-mode-switch".to_string());
        let conversation_id =
            ChatConversationId::from_string("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee");
        let mut project = Project::new(
            "Linked Plan Mode Switch".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project persisted");

        let session = state
            .ideation_session_repo
            .create(IdeationSession::new(project_id.clone()))
            .await
            .expect("ideation session persisted");
        let mut execution_plan = ExecutionPlan::new(session.id.clone());
        execution_plan.status = ExecutionPlanStatus::Superseded;
        let execution_plan = state
            .execution_plan_repo
            .create(execution_plan)
            .await
            .expect("execution plan persisted");
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-linked-plan-mode-switch"),
            session.id.clone(),
            project_id.clone(),
            plan_branch_name.to_string(),
            "main".to_string(),
        );
        plan_branch.status = PlanBranchStatus::Active;
        plan_branch.execution_plan_id = Some(execution_plan.id);
        plan_branch.pr_number = Some(123);
        plan_branch.pr_url = Some("https://github.com/mock/repo/pull/123".to_string());
        plan_branch.pr_status = Some(PrStatus::Open);
        plan_branch.pr_push_status = PrPushStatus::Pushed;
        let plan_branch_id = plan_branch.id.clone();
        let expected_plan_worktree =
            resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
                .expect("expected plan worktree path should resolve");
        state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch persisted");

        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.id = conversation_id.clone();
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Ideation));
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation persisted");

        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project_id,
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(main_sha),
            "agent-shell-linked-plan".to_string(),
            temp.path()
                .join("agent-shell-linked-plan")
                .to_string_lossy()
                .to_string(),
        );
        workspace.linked_ideation_session_id = Some(session.id);
        workspace.linked_plan_branch_id = Some(plan_branch_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace persisted");

        let response = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "edit".to_string(),
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            &state,
        )
        .await
        .expect("linked plan ideation workspace should switch to edit");

        assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
        let switched = response.workspace.expect("workspace should be returned");

        assert_eq!(switched.mode, "edit");
        assert_eq!(switched.branch_name, plan_branch_name);
        assert_eq!(
            switched.worktree_path,
            expected_plan_worktree.to_string_lossy()
        );
        assert_eq!(switched.linked_ideation_session_id, None);
        assert_eq!(switched.linked_plan_branch_id, None);
        assert_eq!(switched.publication_pr_number, Some(123));
        assert_eq!(
            switched.publication_pr_url.as_deref(),
            Some("https://github.com/mock/repo/pull/123")
        );
        assert_eq!(switched.publication_pr_status.as_deref(), Some("open"));
        assert_eq!(switched.publication_push_status.as_deref(), Some("pushed"));
        assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
        assert_eq!(
            GitService::get_current_branch(&expected_plan_worktree)
                .await
                .expect("plan worktree branch should be readable"),
            plan_branch_name
        );
    }

    #[tokio::test]
    async fn switching_to_plan_defers_planning_session_until_first_send_and_edit_preserves_it() {
        let state = AppState::new_test();
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        setup_publish_repo(&repo_path);
        let project_id = ProjectId::from_string("project-plan-new-workspace".to_string());
        let conversation_id =
            ChatConversationId::from_string("dddddddd-dddd-4ddd-8ddd-dddddddddddd");
        let mut project = Project::new(
            "Mode Switch Plan Project".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project)
            .await
            .expect("project persisted");
        let mut conversation = ChatConversation::new_project(project_id);
        conversation.id = conversation_id.clone();
        conversation.title = Some("Review CLI gaps".to_string());
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation persisted");

        let plan_response = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "plan".to_string(),
                base_ref_kind: Some("project_default".to_string()),
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            &state,
        )
        .await
        .expect("plan mode switch creates workspace");

        let plan_workspace = plan_response
            .workspace
            .as_ref()
            .expect("plan workspace should be returned");
        assert_eq!(plan_workspace.mode, "plan");
        assert!(
            plan_workspace.linked_ideation_session_id.is_none(),
            "idle Plan mode should not create an empty planning session"
        );
        assert!(plan_workspace.linked_plan_branch_id.is_none());

        let created_for_send =
            ensure_plan_workspace_planning_session_link_for_send(&state, &conversation_id)
                .await
                .expect("first Plan send should ensure a planning session");
        assert!(created_for_send);
        let second_ensure =
            ensure_plan_workspace_planning_session_link_for_send(&state, &conversation_id)
                .await
                .expect("existing planning session should be reused");
        assert!(!second_ensure);

        let plan_workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup succeeds")
            .expect("plan workspace should persist");
        let session_id = plan_workspace
            .linked_ideation_session_id
            .as_ref()
            .expect("first Plan send should link a planning session")
            .clone();
        let session = state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .expect("planning session lookup succeeds")
            .expect("planning session should exist");
        let conversation_id_string = conversation_id.as_str();
        assert_eq!(session.session_flow, IdeationSessionFlow::Planning);
        assert_eq!(session.title.as_deref(), Some("Review CLI gaps"));
        assert_eq!(session.title_source.as_deref(), Some("auto"));
        assert_eq!(
            session.source_context_type.as_deref(),
            Some("agent_conversation")
        );
        assert_eq!(
            session.source_context_id.as_deref(),
            Some(conversation_id_string.as_str())
        );
        assert_eq!(
            session.analysis.workspace_path.as_deref(),
            Some(plan_workspace.worktree_path.as_str())
        );
        assert!(plan_workspace.linked_plan_branch_id.is_none());

        let edit_response = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "edit".to_string(),
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            &state,
        )
        .await
        .expect("edit mode switch preserves planning link");

        let edit_workspace = edit_response
            .workspace
            .as_ref()
            .expect("edit workspace should be returned");
        assert_eq!(edit_workspace.mode, "edit");
        assert_eq!(
            edit_workspace.linked_ideation_session_id.as_deref(),
            Some(session_id.as_str())
        );
        assert!(edit_workspace.linked_plan_branch_id.is_none());
    }

    #[tokio::test]
    async fn switching_agent_mode_preserves_provider_session_for_native_resume() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-mode-switch".to_string());
        let conversation_id =
            ChatConversationId::from_string("11111111-1111-4111-8111-111111111111");
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.id = conversation_id;
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
        conversation.set_provider_session_ref(ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: "codex-thread-existing".to_string(),
        });
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation persisted");

        let workspace = AgentConversationWorkspace::new(
            conversation_id,
            project_id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::CurrentBranch,
            "feature/agent-screen".to_string(),
            Some("Current branch (feature/agent-screen)".to_string()),
            Some("base-sha".to_string()),
            "ralphx/project/agent-11111111".to_string(),
            "/tmp/ralphx-agent-11111111".to_string(),
        );
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace persisted");

        let response = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "ideation".to_string(),
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            &state,
        )
        .await
        .expect("mode switch succeeds");

        assert_eq!(
            response.conversation.agent_mode.as_deref(),
            Some("ideation")
        );
        assert_eq!(
            response.conversation.provider_session_id.as_deref(),
            Some("codex-thread-existing")
        );
        assert_eq!(
            response.conversation.provider_harness.as_deref(),
            Some("codex")
        );

        let stored = state
            .chat_conversation_repo
            .get_by_id(&conversation_id)
            .await
            .expect("conversation load succeeds")
            .expect("conversation exists");
        assert_eq!(
            stored
                .provider_session_ref()
                .map(|session_ref| session_ref.provider_session_id),
            Some("codex-thread-existing".to_string())
        );
    }

    #[test]
    fn parse_wrapped_mcp_result_object_extracts_embedded_json_payload() {
        let result = json!({
            "content": [
                {
                    "type": "text",
                    "text": "{\"delegated_session_id\":\"delegated-1\",\"status\":\"running\"}"
                }
            ]
        });

        let parsed = parse_wrapped_mcp_result_object(&result).expect("parsed result");

        assert_eq!(
            parsed
                .get("delegated_session_id")
                .and_then(|value| value.as_str()),
            Some("delegated-1")
        );
        assert_eq!(
            parsed.get("status").and_then(|value| value.as_str()),
            Some("running")
        );
    }

    #[test]
    fn merge_delegated_snapshot_overrides_running_result_with_terminal_runtime_state() {
        let mut result = json!({
            "delegated_session_id": "delegated-1",
            "status": "running",
            "job_status": "running"
        });
        let snapshot = DelegatedToolRuntimeSnapshot {
            session_id: "delegated-1".to_string(),
            conversation_id: Some("conversation-1".to_string()),
            agent_run_id: Some("run-1".to_string()),
            agent_name: "ralphx-plan-critic-completeness".to_string(),
            title: Some("Completeness critic".to_string()),
            harness: "codex".to_string(),
            provider_session_id: Some("provider-1".to_string()),
            session_status: "completed".to_string(),
            session_error: None,
            created_at: "2026-04-13T10:00:00Z".to_string(),
            updated_at: "2026-04-13T10:01:00Z".to_string(),
            completed_at: Some("2026-04-13T10:01:30Z".to_string()),
            latest_run: Some(json!({
                "agent_run_id": "run-1",
                "status": "completed"
            })),
            recent_messages: vec![json!({
                "role": "assistant",
                "content": "Completeness: no critical blockers found.",
                "created_at": "2026-04-13T10:01:20Z"
            })],
        };

        merge_delegated_snapshot_into_result(&mut result, &snapshot);

        assert_eq!(
            result.get("status").and_then(|value| value.as_str()),
            Some("completed")
        );
        assert_eq!(
            result.get("job_status").and_then(|value| value.as_str()),
            Some("completed")
        );
        assert_eq!(
            result
                .get("delegated_status")
                .and_then(|value| value.get("latest_run"))
                .and_then(|value| value.get("status"))
                .and_then(|value| value.as_str()),
            Some("completed")
        );
        assert_eq!(
            result
                .get("delegated_status")
                .and_then(|value| value.get("recent_messages"))
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(1)
        );
    }

    #[tokio::test]
    async fn conversation_timeline_page_limits_visible_items_not_message_rows() {
        let state = AppState::new_test();
        let conversation = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(ProjectId::new()))
            .await
            .expect("create conversation");
        let message_id = ChatMessageId::from_string("assistant-message-1");

        for index in 0..3 {
            let mut item = ChatTimelineItem::for_message_block(
                message_id.clone(),
                conversation.id,
                index,
                MessageRole::Orchestrator,
                ChatTimelineItemKind::Text,
            );
            item.status = ChatTimelineItemStatus::Finalized;
            item.text = Some(format!("block {index}"));
            state
                .chat_timeline_repo
                .upsert_item(item)
                .await
                .expect("upsert timeline item");
        }

        let newest_page =
            get_agent_conversation_timeline_page_for_app_state(&state, conversation.id, 2, None)
                .await
                .expect("timeline page")
                .expect("conversation exists");

        assert_eq!(newest_page.items.len(), 2);
        assert_eq!(newest_page.total_item_count, 3);
        assert!(newest_page.has_older);
        assert_eq!(
            newest_page
                .items
                .iter()
                .map(|item| item.content.as_str())
                .collect::<Vec<_>>(),
            vec!["block 1", "block 2"]
        );

        let older_page = get_agent_conversation_timeline_page_for_app_state(
            &state,
            conversation.id,
            2,
            newest_page.oldest_loaded_sequence,
        )
        .await
        .expect("older timeline page")
        .expect("conversation exists");

        assert_eq!(older_page.items.len(), 1);
        assert!(!older_page.has_older);
        assert_eq!(older_page.items[0].content, "block 0");
    }

    #[test]
    fn timeline_item_response_builds_text_message_block() {
        let conversation_id = ChatConversationId::new();
        let message_id = ChatMessageId::from_string("assistant-message-text");
        let mut item = ChatTimelineItem::for_message_block(
            message_id.clone(),
            conversation_id,
            0,
            MessageRole::Orchestrator,
            ChatTimelineItemKind::Text,
        );
        item.sequence = 12;
        item.status = ChatTimelineItemStatus::Finalized;
        item.text = Some("final answer".to_string());
        item.metadata = Some(r#"{"source":"test"}"#.to_string());

        let response = AgentTimelineItemResponse::from(item);

        assert_eq!(response.message_id.as_deref(), Some(message_id.as_str()));
        assert_eq!(response.content, "final answer");
        assert_eq!(response.kind, "text");
        assert!(response.tool_call.is_none());
        assert_eq!(
            response.content_blocks,
            json!([{ "type": "text", "text": "final answer" }])
        );
        assert_eq!(response.metadata.as_deref(), Some(r#"{"source":"test"}"#));
    }

    #[test]
    fn timeline_item_response_builds_tool_block_with_detail_ref_and_diff_context() {
        let conversation_id = ChatConversationId::new();
        let message_id = ChatMessageId::from_string("assistant-message-tool");
        let mut item = ChatTimelineItem::for_message_block(
            message_id.clone(),
            conversation_id,
            3,
            MessageRole::Orchestrator,
            ChatTimelineItemKind::ToolUse,
        );
        item.sequence = 22;
        item.status = ChatTimelineItemStatus::Finalized;
        item.tool_call_id = Some("tool-1".to_string());
        item.tool_name = Some("bash".to_string());
        item.input_json = Some(r#"{"command":"cargo test"}"#.to_string());
        item.result_json = Some(r#""ok""#.to_string());
        item.raw_block_json =
            Some(r#"{"type":"tool_use","diff_context":{"file_path":"src/lib.rs"}}"#.to_string());
        item.provider_harness = Some(AgentHarnessKind::Codex);
        item.provider_session_id = Some("thread-1".to_string());

        let response = AgentTimelineItemResponse::from(item);
        let tool = response.tool_call.expect("tool response");

        assert_eq!(response.kind, "tool_use");
        assert_eq!(response.provider_harness.as_deref(), Some("codex"));
        assert_eq!(response.provider_session_id.as_deref(), Some("thread-1"));
        assert_eq!(tool["id"], "tool-1");
        assert_eq!(tool["name"], "bash");
        assert_eq!(tool["arguments"]["command"], "cargo test");
        assert_eq!(tool["result"], "ok");
        assert_eq!(
            tool["detail_ref"]["timeline_item_id"].as_str(),
            Some(response.id.as_str())
        );
        assert_eq!(
            tool["detail_ref"]["message_id"].as_str(),
            Some(message_id.as_str())
        );
        assert_eq!(tool["diff_context"]["file_path"], "src/lib.rs");
    }

    #[test]
    fn preview_tool_payloads_preserves_parseable_mcp_artifact_preview() {
        let artifact_content = "Detailed artifact line.\n".repeat(600);
        let artifact = json!({
            "id": "artifact-preview-1",
            "title": "Previewable Artifact",
            "artifact_type": "design_doc",
            "content": artifact_content,
            "version": 3
        });
        let tool_calls = json!([{
            "id": "tool-artifact-1",
            "name": "mcp__ralphx__get_artifact",
            "arguments": { "artifact_id": "artifact-preview-1" },
            "result": {
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string(&artifact).expect("artifact json")
                }]
            }
        }]);

        let (tool_calls, _) = preview_tool_payloads_for_message(
            "conversation-1",
            "message-1",
            Some(tool_calls),
            None,
        );
        let tool_calls = tool_calls.expect("previewed tool calls");
        let tool = &tool_calls.as_array().expect("tool call array")[0];
        let preview_text = tool["result"]["content"][0]["text"]
            .as_str()
            .expect("mcp text content preview");
        let parsed_preview: serde_json::Value =
            serde_json::from_str(preview_text).expect("preview text remains valid JSON");

        assert_eq!(tool["result_preview_truncated"], true);
        assert_eq!(parsed_preview["title"], "Previewable Artifact");
        assert_eq!(parsed_preview["artifact_type"], "design_doc");
        assert_eq!(parsed_preview["version"], 3);
        assert!(
            parsed_preview["content"]
                .as_str()
                .expect("content preview string")
                .len()
                < artifact_content.len(),
            "artifact content should stay bounded in the paginated preview"
        );
        assert_eq!(
            tool["detail_ref"],
            json!({
                "conversation_id": "conversation-1",
                "message_id": "message-1",
                "tool_call_id": "tool-artifact-1",
                "content_block_index": null
            })
        );
    }

    #[test]
    fn preview_tool_payloads_replaces_edit_arguments_with_first_diff_hunk() {
        let old_content = [
            "line 1", "line 2", "line 3", "line 4", "line 5", "line 6", "line 7", "line 8",
            "line 9", "line 10", "line 11", "line 12",
        ]
        .join("\n");
        let new_content = [
            "line 1",
            "line 2 changed",
            "line 3",
            "line 4",
            "line 5",
            "line 6",
            "line 7",
            "line 8",
            "line 9",
            "line 10 changed",
            "line 11",
            "line 12",
        ]
        .join("\n");
        let tool_calls = json!([{
            "id": "tool-edit-1",
            "name": "edit",
            "arguments": {
                "file_path": "src/example.ts",
                "old_string": old_content,
                "new_string": new_content,
                "replace_all": false
            },
            "result": { "status": "ok" }
        }]);

        let (tool_calls, _) = preview_tool_payloads_for_message(
            "conversation-1",
            "message-1",
            Some(tool_calls),
            None,
        );
        let tool_calls = tool_calls.expect("previewed tool calls");
        let tool = &tool_calls.as_array().expect("tool call array")[0];
        let diff_preview_text =
            serde_json::to_string(&tool["diff_preview"]).expect("diff preview serializes");

        assert_eq!(tool["arguments_preview_truncated"], true);
        assert_eq!(tool["arguments"]["file_path"], "src/example.ts");
        assert_eq!(tool["arguments"]["replace_all"], false);
        assert!(tool["arguments"]["old_string"].is_null());
        assert!(tool["arguments"]["new_string"].is_null());
        assert_eq!(
            tool["detail_ref"],
            json!({
                "conversation_id": "conversation-1",
                "message_id": "message-1",
                "tool_call_id": "tool-edit-1",
                "content_block_index": null
            })
        );
        assert_eq!(tool["diff_preview"]["file_path"], "src/example.ts");
        assert_eq!(tool["diff_preview"]["language"], "typescript");
        assert!(diff_preview_text.contains("line 2 changed"));
        assert!(!diff_preview_text.contains("line 10 changed"));
    }

    #[test]
    fn preview_tool_payloads_replaces_write_content_and_diff_context_with_diff_preview() {
        let content_blocks = json!([{
            "type": "tool_use",
            "id": "tool-write-1",
            "name": "write",
            "arguments": {
                "file_path": "src/lib.rs",
                "content": "fn main() {\n    println!(\"new\");\n}"
            },
            "diff_context": {
                "file_path": "src/lib.rs",
                "old_content": "fn main() {\n    println!(\"old\");\n}"
            },
            "result": { "status": "ok" }
        }]);

        let (_, content_blocks) = preview_tool_payloads_for_message(
            "conversation-1",
            "message-1",
            None,
            Some(content_blocks),
        );
        let content_blocks = content_blocks.expect("previewed content blocks");
        let tool = &content_blocks.as_array().expect("content block array")[0];

        assert_eq!(tool["arguments_preview_truncated"], true);
        assert_eq!(tool["arguments"]["file_path"], "src/lib.rs");
        assert!(tool["arguments"]["content"].is_null());
        assert_eq!(tool["diff_context"]["file_path"], "src/lib.rs");
        assert!(tool["diff_context"]["old_content"].is_null());
        assert_eq!(tool["diff_preview"]["file_path"], "src/lib.rs");
        assert_eq!(
            tool["detail_ref"]["content_block_index"],
            serde_json::json!(0)
        );
    }

    #[test]
    fn preview_tool_payloads_renders_new_write_as_added_diff() {
        let content_blocks = json!([{
            "type": "tool_use",
            "id": "tool-write-new",
            "name": "write",
            "arguments": {
                "file_path": "src/new.rs",
                "content": "pub fn new() {}\n"
            },
            "diff_context": {
                "file_path": "src/new.rs",
                "old_file_exists": false
            },
            "result": { "status": "ok" }
        }]);

        let (_, content_blocks) = preview_tool_payloads_for_message(
            "conversation-1",
            "message-1",
            None,
            Some(content_blocks),
        );
        let content_blocks = content_blocks.expect("previewed content blocks");
        let tool = &content_blocks.as_array().expect("content block array")[0];

        assert_eq!(tool["arguments_preview_truncated"], true);
        assert_eq!(tool["arguments"]["file_path"], "src/new.rs");
        assert!(tool["arguments"]["content"].is_null());
        assert_eq!(tool["diff_context"]["old_file_exists"], false);
        assert_eq!(tool["diff_preview"]["old_total_lines"], 0);
        assert_eq!(tool["diff_preview"]["new_total_lines"], 2);
        assert_eq!(
            tool["diff_preview"]["hunks"][0]["lines"][0]["kind"],
            "addition"
        );
    }

    #[tokio::test]
    async fn timeline_item_response_previews_edit_arguments_but_detail_returns_full_payload() {
        let state = AppState::new_test();
        let conversation_id = ChatConversationId::new();
        let message_id = ChatMessageId::from_string("assistant-message-edit");
        let mut item = ChatTimelineItem::for_message_block(
            message_id.clone(),
            conversation_id,
            0,
            MessageRole::Orchestrator,
            ChatTimelineItemKind::ToolUse,
        );
        item.tool_call_id = Some("tool-edit-timeline".to_string());
        item.tool_name = Some("edit".to_string());
        item.input_json = Some(
            json!({
                "file_path": "src/example.ts",
                "old_string": "old line",
                "new_string": "new line"
            })
            .to_string(),
        );

        let response = AgentTimelineItemResponse::from(item.clone());
        let preview_tool = response.tool_call.expect("timeline tool preview");
        assert_eq!(preview_tool["arguments_preview_truncated"], true);
        assert!(preview_tool["arguments"]["old_string"].is_null());
        assert_eq!(
            preview_tool["detail_ref"]["timeline_item_id"].as_str(),
            Some(response.id.as_str())
        );

        let item = state
            .chat_timeline_repo
            .upsert_item(item)
            .await
            .expect("insert timeline edit item");
        let detail = get_agent_timeline_item_tool_call_detail_for_app_state(
            &state,
            conversation_id,
            item.id,
        )
        .await
        .expect("timeline edit detail lookup")
        .expect("timeline edit detail");

        assert_eq!(detail.tool_call["arguments"]["old_string"], "old line");
        assert_eq!(detail.tool_call["arguments"]["new_string"], "new line");
        assert!(detail.tool_call["arguments_preview_truncated"].is_null());
    }

    #[test]
    fn chat_timeline_domain_values_cover_all_variants_from_app_crate_tests() {
        let generated_id = ChatTimelineItemId::new();
        assert!(!generated_id.as_str().is_empty());
        assert!(!ChatTimelineItemId::default().as_str().is_empty());

        for (raw, kind) in [
            ("text", ChatTimelineItemKind::Text),
            ("tool_use", ChatTimelineItemKind::ToolUse),
            ("task", ChatTimelineItemKind::Task),
            ("system_notice", ChatTimelineItemKind::SystemNotice),
            ("error", ChatTimelineItemKind::Error),
        ] {
            assert_eq!(kind.to_string(), raw);
            assert_eq!(ChatTimelineItemKind::from_str(raw), Ok(kind));
        }

        for (raw, status) in [
            ("streaming", ChatTimelineItemStatus::Streaming),
            ("finalized", ChatTimelineItemStatus::Finalized),
            ("error", ChatTimelineItemStatus::Error),
        ] {
            assert_eq!(status.to_string(), raw);
            assert_eq!(ChatTimelineItemStatus::from_str(raw), Ok(status));
        }

        assert!(ChatTimelineItemKind::from_str("bogus").is_err());
        assert!(ChatTimelineItemStatus::from_str("bogus").is_err());
    }

    #[tokio::test]
    async fn timeline_item_detail_returns_none_for_missing_or_mismatched_item() {
        let state = AppState::new_test();
        let conversation_id = ChatConversationId::new();

        let missing = get_agent_timeline_item_tool_call_detail_for_app_state(
            &state,
            conversation_id,
            ChatTimelineItemId::from_string("missing"),
        )
        .await
        .expect("missing detail lookup");
        assert!(missing.is_none());

        let other_conversation_id = ChatConversationId::new();
        let message_id = ChatMessageId::from_string("assistant-message-tool");
        let mut item = ChatTimelineItem::for_message_block(
            message_id,
            other_conversation_id,
            0,
            MessageRole::Orchestrator,
            ChatTimelineItemKind::ToolUse,
        );
        item.tool_call_id = Some("tool-other".to_string());
        item.tool_name = Some("Read".to_string());
        item.input_json = Some(r#"{"file_path":"src/lib.rs"}"#.to_string());
        let item = state
            .chat_timeline_repo
            .upsert_item(item)
            .await
            .expect("insert mismatched timeline item");

        let mismatched = get_agent_timeline_item_tool_call_detail_for_app_state(
            &state,
            conversation_id,
            item.id,
        )
        .await
        .expect("mismatched detail lookup");
        assert!(mismatched.is_none());
    }

    #[tokio::test]
    async fn timeline_item_detail_uses_preview_fallbacks_for_partial_tool_payload() {
        let state = AppState::new_test();
        let conversation_id = ChatConversationId::new();
        let mut item = ChatTimelineItem {
            id: ChatTimelineItemId::from_string("timeline-tool-preview"),
            conversation_id,
            message_id: None,
            run_id: None,
            sequence: 4,
            block_index: 2,
            role: MessageRole::Orchestrator,
            kind: ChatTimelineItemKind::ToolUse,
            status: ChatTimelineItemStatus::Streaming,
            text: None,
            tool_call_id: None,
            tool_name: None,
            tool_status: Some("pending".to_string()),
            tool_input_preview: Some(r#"{"path":"src/lib.rs"}"#.to_string()),
            tool_result_preview: Some("preview result".to_string()),
            input_json: None,
            result_json: None,
            raw_block_json: Some(r#"{"type":"tool_use","extra":true}"#.to_string()),
            metadata: None,
            provider_harness: None,
            provider_session_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            finalized_at: None,
        };
        item = state
            .chat_timeline_repo
            .upsert_item(item)
            .await
            .expect("insert preview timeline item");

        let detail = get_agent_timeline_item_tool_call_detail_for_app_state(
            &state,
            conversation_id,
            item.id.clone(),
        )
        .await
        .expect("preview detail lookup")
        .expect("preview detail");
        let tool = detail.tool_call;

        assert_eq!(tool["id"], item.id.to_string());
        assert_eq!(tool["name"], "unknown");
        assert_eq!(tool["arguments"]["path"], "src/lib.rs");
        assert_eq!(tool["result"], "preview result");
        assert_eq!(tool["detail_ref"]["message_id"], item.id.to_string());
        assert_eq!(tool["detail_ref"]["content_block_index"], 2);
    }
}
