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

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use tauri::{Emitter, Manager, Runtime, State};

use crate::application::agent_conversation_workspace::{
    agent_name_for_workspace_mode, prepare_agent_conversation_workspace,
    prepare_agent_conversation_workspace_with_setup_mode,
    resolve_agent_conversation_workspace_path_for_send,
    resolve_valid_agent_conversation_workspace_path, AgentConversationWorkspaceBaseSelection,
    AgentConversationWorkspaceSetupMode,
};
use crate::application::agent_conversation_workspace_base::{
    apply_workspace_base_resolution, resolve_workspace_base, BaseResolutionResult, BaseStatus,
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
use crate::application::agent_workspace_publish_recovery::recover_stale_publish_repair_for_workspace_in_state;
use crate::application::chat_service::tool_result_preview::{
    preview_tool_result_object, tool_detail_ref,
};
use crate::application::chat_service::{AgentConversationCreatedPayload, SendMessageOptions};
use crate::application::git_service::{
    git_cmd::{self, GitCommandLane},
    GitService,
};
use crate::application::publish_resilience::{
    classify_publish_failure, count_publish_reviewable_commits, count_unpublished_publish_commits,
    ensure_plan_publish_branch_fresh, ensure_publish_branch_fresh,
    inspect_publish_branch_freshness_for_source_after_fetch, publish_push_status_for_failure,
    push_publish_branch, remote_tracking_ref_for_publish, review_base_for_publish,
    PublishBranchFreshnessOutcome, PublishBranchFreshnessStatus, PublishFailureClass,
};
use crate::application::{AppChatService, AppState, ChatService, ChatServiceError, SendResult};
use crate::commands::agent_model_commands::load_agent_model_registry;
use crate::commands::ExecutionState;
use crate::domain::agents::{
    default_effort_for_provider, default_efforts_for_provider, AgentHarnessKind, LogicalEffort,
};
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus, AgentRunId,
    AgentRunStatus, ChatContextType, ChatConversation, ChatConversationId, ChatMessageId,
    ChatTimelineItem, DelegatedSessionId, ExecutionPlanStatus, IdeationAnalysisBaseRefKind,
    IdeationSessionId, PlanBranch, PlanBranchStatus, Project, ProjectId, TaskId,
};
use crate::domain::services::{
    AgentWorkspacePrPublisher, ComposerProjectReference, QueuedMessage, RunningAgentKey,
    RunningAgentRegistry,
};
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
    /// Optional provider harness override for the first spawn of a conversation.
    pub provider_harness: Option<String>,
    /// Optional explicit model override for the spawned agent.
    pub model_override: Option<String>,
    /// Optional provider-neutral reasoning effort override for the spawned agent.
    pub logical_effort: Option<LogicalEffort>,
    /// Structured composer project references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_project_references: Vec<ComposerProjectReference>,
    /// Optional target for team message routing.
    /// When set to a teammate name, the message is routed to that teammate's stdin
    /// instead of the lead's. "lead" or None routes to the lead (default behavior).
    pub target: Option<String>,
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

/// Input for creating a project-backed agent conversation with an isolated workspace.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartAgentConversationInput {
    pub project_id: String,
    pub content: String,
    /// Optional draft conversation to use after uploading pending attachments.
    pub conversation_id: Option<String>,
    /// Optional provider harness override for the first spawn of the conversation.
    pub provider_harness: Option<String>,
    /// Optional explicit model override for the spawned agent.
    pub model_override: Option<String>,
    /// Optional provider-neutral reasoning effort override for the spawned agent.
    pub logical_effort: Option<LogicalEffort>,
    /// Agent mode: "chat" routes to a read-only explorer in the project root;
    /// edit/ideation modes create a selected-base workspace for runtime CWD.
    pub mode: Option<String>,
    /// Optional base ref kind using ideation naming: project_default, current_branch, local_branch.
    pub base_ref_kind: Option<String>,
    /// Optional selected branch/ref name for the base.
    pub base_ref: Option<String>,
    /// Optional user-facing base ref label.
    pub base_display_name: Option<String>,
    /// Structured composer project references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_project_references: Vec<ComposerProjectReference>,
}

/// Response for an agent conversation workspace.
#[derive(Debug, Serialize)]
pub struct AgentConversationWorkspaceResponse {
    pub conversation_id: String,
    pub project_id: String,
    pub mode: String,
    pub base_ref_kind: String,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub base_commit: Option<String>,
    pub branch_name: String,
    pub worktree_path: String,
    pub linked_ideation_session_id: Option<String>,
    pub linked_plan_branch_id: Option<String>,
    pub publication_pr_number: Option<i64>,
    pub publication_pr_url: Option<String>,
    pub publication_pr_status: Option<String>,
    pub publication_push_status: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub mode_switch_locked: bool,
    pub mode_switch_lock_reason: Option<String>,
}

impl From<AgentConversationWorkspace> for AgentConversationWorkspaceResponse {
    fn from(workspace: AgentConversationWorkspace) -> Self {
        Self {
            conversation_id: workspace.conversation_id.as_str(),
            project_id: workspace.project_id.as_str().to_string(),
            mode: workspace.mode.to_string(),
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
            publication_pr_number: workspace.publication_pr_number,
            publication_pr_url: workspace.publication_pr_url,
            publication_pr_status: workspace.publication_pr_status,
            publication_push_status: workspace.publication_push_status,
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

        if session.is_active() && session.archived_at.is_none() && session.converted_at.is_none() {
            return Ok(AgentConversationWorkspaceModeLock::locked(
                "Ideation session is still active",
            ));
        }
    }

    Ok(AgentConversationWorkspaceModeLock::unlocked())
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
        return Ok(AgentConversationWorkspacePublishTarget {
            worktree_path: PathBuf::from(&project.working_directory),
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
    /// Optional selected branch/ref name for the base.
    pub base_ref: Option<String>,
    /// Optional user-facing base ref label.
    pub base_display_name: Option<String>,
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
}

impl From<QueuedMessage> for QueuedMessageResponse {
    fn from(msg: QueuedMessage) -> Self {
        Self {
            id: msg.id,
            content: msg.content,
            created_at: msg.created_at,
            is_editing: msg.is_editing,
        }
    }
}

/// Response for conversation listing
#[derive(Debug, Serialize)]
pub struct AgentConversationResponse {
    pub id: String,
    pub context_type: String,
    pub context_id: String,
    pub claude_session_id: Option<String>,
    pub provider_session_id: Option<String>,
    pub provider_harness: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub agent_mode: Option<String>,
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
            agent_mode: c.agent_mode.map(|mode| mode.to_string()),
            title: c.title,
            message_count: c.message_count,
            last_message_at: c.last_message_at.map(|dt| dt.to_rfc3339()),
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
            archived_at: c.archived_at.map(|dt| dt.to_rfc3339()),
        }
    }
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
            timeline_item_content_block(&item, &conversation_id, message_id.as_deref());
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
    preview_tool_result_object(object, Some(detail_ref));
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

fn agent_mode_requires_workspace(mode: AgentConversationWorkspaceMode) -> bool {
    matches!(
        mode,
        AgentConversationWorkspaceMode::Edit | AgentConversationWorkspaceMode::Ideation
    )
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
            AgentConversationWorkspaceMode::Ideation
        ));
    }

    #[test]
    fn active_agent_conversations_support_expected_valid_mode_transition_matrix() {
        let valid_transitions = [
            (
                AgentConversationWorkspaceMode::Chat,
                AgentConversationWorkspaceMode::Chat,
            ),
            (
                AgentConversationWorkspaceMode::Chat,
                AgentConversationWorkspaceMode::Edit,
            ),
            (
                AgentConversationWorkspaceMode::Chat,
                AgentConversationWorkspaceMode::Ideation,
            ),
            (
                AgentConversationWorkspaceMode::Edit,
                AgentConversationWorkspaceMode::Chat,
            ),
            (
                AgentConversationWorkspaceMode::Edit,
                AgentConversationWorkspaceMode::Edit,
            ),
            (
                AgentConversationWorkspaceMode::Edit,
                AgentConversationWorkspaceMode::Ideation,
            ),
            (
                AgentConversationWorkspaceMode::Ideation,
                AgentConversationWorkspaceMode::Ideation,
            ),
            (
                AgentConversationWorkspaceMode::Ideation,
                AgentConversationWorkspaceMode::Chat,
            ),
            (
                AgentConversationWorkspaceMode::Ideation,
                AgentConversationWorkspaceMode::Edit,
            ),
        ];

        for (current_mode, target_mode) in valid_transitions {
            assert!(
                validate_agent_conversation_mode_transition(
                    current_mode,
                    target_mode,
                    &AgentConversationWorkspaceModeLock::unlocked()
                )
                .is_ok(),
                "{current_mode} -> {target_mode} should be allowed"
            );
        }
    }

    #[test]
    fn active_state_owned_conversations_cannot_leave_ideation_mode() {
        for target_mode in [
            AgentConversationWorkspaceMode::Chat,
            AgentConversationWorkspaceMode::Edit,
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

fn log_start_agent_conversation_phase(
    project_id: &str,
    conversation_id: Option<&ChatConversationId>,
    phase: &'static str,
    started: Instant,
) {
    tracing::info!(
        project_id,
        conversation_id = ?conversation_id.map(ChatConversationId::as_str),
        phase,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "start_agent_conversation phase completed"
    );
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
    let command_started = Instant::now();
    tracing::info!(
        project_id = %input.project_id,
        content_len = input.content.len(),
        mode = ?input.mode,
        base_ref_kind = ?input.base_ref_kind,
        base_ref = ?input.base_ref,
        "[START_AGENT_CONVERSATION] command invoked"
    );

    let parse_runtime_started = Instant::now();
    let harness_override = input
        .provider_harness
        .as_deref()
        .map(str::parse::<AgentHarnessKind>)
        .transpose()?;
    log_start_agent_conversation_phase(
        &input.project_id,
        None,
        "parse_runtime_selection",
        parse_runtime_started,
    );

    let validate_runtime_started = Instant::now();
    crate::application::validate_chat_runtime_for_context_with_override(
        &state,
        ChatContextType::Project,
        &input.project_id,
        "start_agent_conversation",
        harness_override,
    )
    .await?;
    log_start_agent_conversation_phase(
        &input.project_id,
        None,
        "validate_chat_runtime",
        validate_runtime_started,
    );

    let parse_input_started = Instant::now();
    let mode = parse_agent_workspace_mode(input.mode.as_deref())?;
    let base_ref_kind = parse_agent_workspace_base_kind(input.base_ref_kind.as_deref())?;
    let project_id = ProjectId::from_string(input.project_id.clone());
    log_start_agent_conversation_phase(&input.project_id, None, "parse_input", parse_input_started);

    let project_lookup_started = Instant::now();
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", input.project_id))?;
    log_start_agent_conversation_phase(
        &input.project_id,
        None,
        "load_project",
        project_lookup_started,
    );

    let conversation_resolve_started = Instant::now();
    let draft_conversation_id = input
        .conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|conversation_id| !conversation_id.is_empty())
        .map(ChatConversationId::from_string);
    let mut conversation = if let Some(conversation_id) = draft_conversation_id {
        let conversation = state
            .chat_conversation_repo
            .get_by_id(&conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;
        if conversation.context_type != ChatContextType::Project
            || conversation.context_id != input.project_id
        {
            return Err(format!(
                "Conversation {} does not belong to project {}",
                conversation.id, input.project_id
            ));
        }
        conversation
    } else {
        ChatConversation::new_project(project_id)
    };
    conversation.set_agent_mode(Some(mode));
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "resolve_conversation",
        conversation_resolve_started,
    );

    let should_create_conversation = draft_conversation_id.is_none();
    let workspace_prepare_started = Instant::now();
    let workspace = if agent_mode_requires_workspace(mode) {
        Some(
            prepare_agent_conversation_workspace_with_setup_mode(
                &project,
                &conversation.id,
                mode,
                AgentConversationWorkspaceBaseSelection {
                    kind: base_ref_kind,
                    base_ref: input
                        .base_ref
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                    display_name: input
                        .base_display_name
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                },
                AgentConversationWorkspaceSetupMode::Deferred,
            )
            .await
            .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "prepare_workspace",
        workspace_prepare_started,
    );

    let conversation_persist_started = Instant::now();
    let conversation = if should_create_conversation {
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .map_err(|error| error.to_string())?
    } else {
        state
            .chat_conversation_repo
            .update_agent_mode(&conversation.id, Some(mode))
            .await
            .map_err(|error| error.to_string())?;
        conversation
    };
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "persist_conversation",
        conversation_persist_started,
    );

    let workspace_persist_started = Instant::now();
    let workspace = match workspace {
        Some(workspace) => match state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
        {
            Ok(workspace) => Some(workspace),
            Err(error) => {
                if should_create_conversation {
                    let _ = state.chat_conversation_repo.delete(&conversation.id).await;
                }
                return Err(error.to_string());
            }
        },
        None => None,
    };
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "persist_workspace",
        workspace_persist_started,
    );

    let event_emit_started = Instant::now();
    if should_create_conversation {
        let _ = app.emit(
            "agent:conversation_created",
            AgentConversationCreatedPayload {
                conversation_id: conversation.id.as_str(),
                context_type: ChatContextType::Project.to_string(),
                context_id: input.project_id.clone(),
            },
        );
    }
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "emit_conversation_created",
        event_emit_started,
    );

    let service_create_started = Instant::now();
    let service = create_chat_service(
        &state,
        app,
        &execution_state,
        Some(team_service.inner().clone()),
    );
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "create_chat_service",
        service_create_started,
    );

    let runtime_override_prepare_started = Instant::now();
    let model_override = input
        .model_override
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    let working_directory_override = workspace
        .as_ref()
        .map(|workspace| PathBuf::from(&workspace.worktree_path));
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "prepare_runtime_overrides",
        runtime_override_prepare_started,
    );

    let runtime_normalize_started = Instant::now();
    let (model_override, logical_effort_override) = normalize_agent_runtime_selection(
        &state,
        harness_override,
        model_override,
        input.logical_effort,
    )
    .await?;
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "normalize_runtime_selection",
        runtime_normalize_started,
    );

    let send_message_started = Instant::now();
    let send_result = service
        .send_message(
            ChatContextType::Project,
            &input.project_id,
            &input.content,
            SendMessageOptions {
                harness_override,
                agent_name_override: Some(agent_name_for_workspace_mode(mode).to_string()),
                model_override,
                logical_effort_override,
                conversation_id_override: Some(conversation.id),
                working_directory_override,
                composer_project_references: input.composer_project_references.clone(),
                ..Default::default()
            },
        )
        .await
        .map(SendAgentMessageResponse::from)
        .map_err(|error| error.to_string())?;
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "send_message",
        send_message_started,
    );

    let workspace_response_started = Instant::now();
    let workspace_response = match workspace {
        Some(workspace) => {
            Some(agent_workspace_response_for_state(state.inner(), workspace).await?)
        }
        None => None,
    };
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "build_workspace_response",
        workspace_response_started,
    );
    log_start_agent_conversation_phase(
        &input.project_id,
        Some(&conversation.id),
        "command_total",
        command_started,
    );

    Ok(StartAgentConversationResponse {
        conversation: AgentConversationResponse::from(conversation),
        workspace: workspace_response,
        send_result,
    })
}

/// Switch a project-backed agent conversation between chat/edit/ideation modes.
#[tauri::command]
pub async fn switch_agent_conversation_mode(
    input: SwitchAgentConversationModeInput,
    state: State<'_, AppState>,
) -> Result<SwitchAgentConversationModeResponse, String> {
    switch_agent_conversation_mode_for_state(input, state.inner()).await
}

#[doc(hidden)]
pub async fn switch_agent_conversation_mode_for_state(
    input: SwitchAgentConversationModeInput,
    state: &AppState,
) -> Result<SwitchAgentConversationModeResponse, String> {
    let conversation_id = ChatConversationId::from_string(input.conversation_id.clone());
    let target_mode = parse_agent_workspace_mode(Some(input.mode.as_str()))?;
    let base_ref_kind = parse_agent_workspace_base_kind(input.base_ref_kind.as_deref())?;

    let mut conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;
    if conversation.context_type != ChatContextType::Project {
        return Err("Only project agent conversations can change mode".to_string());
    }

    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    if state.running_agent_registry.is_running(&running_key).await {
        return Err("Cannot change mode while the agent is running".to_string());
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

    validate_agent_conversation_mode_transition(
        current_mode,
        target_mode,
        &workspace_mode_lock,
    )?;

    let workspace = match existing_workspace {
        Some(mut workspace) => {
            let should_detach_inactive_owner =
                target_mode != AgentConversationWorkspaceMode::Ideation
                    && !workspace_mode_lock.locked
                    && (workspace.linked_ideation_session_id.is_some()
                        || workspace.linked_plan_branch_id.is_some());
            if workspace.mode != target_mode || should_detach_inactive_owner {
                workspace.mode = target_mode;
                if should_detach_inactive_owner {
                    workspace.linked_ideation_session_id = None;
                    workspace.linked_plan_branch_id = None;
                }
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
            if agent_mode_requires_workspace(target_mode) {
                let project_id = ProjectId::from_string(conversation.context_id.clone());
                let project = state
                    .project_repo
                    .get_by_id(&project_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Project not found: {}", conversation.context_id))?;
                let workspace = prepare_agent_conversation_workspace(
                    &project,
                    &conversation.id,
                    target_mode,
                    AgentConversationWorkspaceBaseSelection {
                        kind: base_ref_kind,
                        base_ref: input
                            .base_ref
                            .map(|value| value.trim().to_string())
                            .filter(|value| !value.is_empty()),
                        display_name: input
                            .base_display_name
                            .map(|value| value.trim().to_string())
                            .filter(|value| !value.is_empty()),
                    },
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
        conversation: AgentConversationResponse::from(conversation),
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

    let mut service = create_chat_service(
        &state,
        app,
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
    let conversation_id_override = input
        .conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|conversation_id| !conversation_id.is_empty())
        .map(ChatConversationId::from_string);
    if let Some(conversation_id) = conversation_id_override.as_ref() {
        invalidate_agent_workspace_pr_description_cache(conversation_id);
    }

    service
        .send_message(
            context_type,
            &input.context_id,
            &input.content,
            SendMessageOptions {
                harness_override,
                model_override,
                logical_effort_override,
                conversation_id_override,
                composer_project_references: input.composer_project_references,
                ..Default::default()
            },
        )
        .await
        .map(SendAgentMessageResponse::from)
        .map_err(|e| e.to_string())
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
        .map(|msgs| msgs.into_iter().map(QueuedMessageResponse::from).collect())
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

    Ok(conversations
        .into_iter()
        .map(AgentConversationResponse::from)
        .collect())
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

    let page = state
        .chat_conversation_repo
        .get_by_context_page_filtered(
            context_type_enum,
            &context_id,
            include_archived,
            archived_only,
            offset,
            limit,
            search.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;
    let has_more = page.has_more();

    Ok(AgentConversationListPageResponse {
        conversations: page
            .conversations
            .into_iter()
            .map(AgentConversationResponse::from)
            .collect(),
        total: page.total_count,
        limit: page.limit,
        offset: page.offset,
        has_more,
    })
}

/// Core archive logic, testable without Tauri `State` wrapper.
#[doc(hidden)]
pub async fn archive_agent_conversation_inner(
    conversation_id: &ChatConversationId,
    state: &AppState,
) -> Result<(), String> {
    state
        .chat_conversation_repo
        .archive(conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    if let Ok(Some(workspace)) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
    {
        state
            .agent_conversation_workspace_repo
            .update_status(conversation_id, AgentConversationWorkspaceStatus::Archived)
            .await
            .map_err(|e| e.to_string())?;

        let has_open_pr = workspace.publication_pr_number.is_some()
            && workspace.publication_pr_status.as_deref() != Some("closed")
            && workspace.publication_pr_status.as_deref() != Some("merged");

        if has_open_pr {
            if let Ok(Some(project)) = state.project_repo.get_by_id(&workspace.project_id).await {
                let pr_number = workspace.publication_pr_number.unwrap();
                let working_dir = std::path::Path::new(&project.working_directory);

                if let Some(github_svc) = &state.github_service {
                    if let Err(e) = github_svc.close_pr(working_dir, pr_number).await {
                        tracing::warn!(
                            pr_number,
                            error = %e,
                            "archive_agent_conversation: failed to close PR on remote"
                        );
                    }
                }

                state
                    .agent_conversation_workspace_repo
                    .update_publication(
                        conversation_id,
                        Some(pr_number),
                        workspace.publication_pr_url.as_deref(),
                        Some("closed"),
                        workspace.publication_push_status.as_deref(),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
            }

            if let Some(plan_branch_id) = &workspace.linked_plan_branch_id {
                if let Ok(Some(plan_branch)) =
                    state.plan_branch_repo.get_by_id(plan_branch_id).await
                {
                    if plan_branch.pr_number.is_some()
                        && plan_branch.pr_status != Some(PrStatus::Closed)
                        && plan_branch.pr_status != Some(PrStatus::Merged)
                    {
                        let _ = state
                            .plan_branch_repo
                            .update_pr_status(plan_branch_id, PrStatus::Closed)
                            .await;
                    }
                }
            }
        }
    }

    Ok(())
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

    state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .map(AgentConversationResponse::from)
        .ok_or_else(|| "Conversation not found".to_string())
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
    state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .map(AgentConversationResponse::from)
        .ok_or_else(|| "Conversation not found".to_string())
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

/// Schedule a background publication reconciliation for a project-backed agent conversation.
#[tauri::command]
pub async fn reconcile_agent_conversation_workspace_publication(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    schedule_external_pr_reconciliation_for_conversation_id(
        state.inner(),
        ChatConversationId::from_string(conversation_id),
        AgentWorkspaceExternalPrReconciliationTrigger::AgentRunCompleted,
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
        count_unpublished_publish_commits(&worktree_path, &workspace.branch_name),
    );
    let has_uncommitted_changes = has_uncommitted_changes.map_err(|e| e.to_string())?;
    let unpublished_commit_count = unpublished_commit_count.map_err(|e| e.to_string())?;

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
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    team_service: State<'_, std::sync::Arc<crate::application::TeamService>>,
    app: tauri::AppHandle,
) -> Result<UpdateAgentConversationWorkspaceFromBaseResponse, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let _workspace_changed_event = emit_workspace_changed_when_done(&app, &conversation_id);
    let selection = AgentConversationWorkspaceBaseSelection {
        kind: parse_agent_workspace_base_kind(base_ref_kind.as_deref())?,
        base_ref,
        display_name: base_display_name,
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
                }
                return Err(error);
            }
        };

    let mut repair_service =
        state.build_chat_service_with_execution_state(Arc::clone(execution_state));
    if let Some(team_service) = team_service {
        repair_service = repair_service.with_team_service(team_service);
    }

    let base_resolution = if let Some(explicit_base) = explicit_base.as_ref() {
        publish_target.base_ref = explicit_base.base_ref.clone();
        publish_target.base_display_name = Some(explicit_base.display_name.clone());
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

        let review_base =
            match review_base_for_publish(workspace.base_commit.as_deref(), &workspace.base_ref) {
                Ok(review_base) => review_base.to_string(),
                Err(_) => return Ok(skip("missing_review_base")),
            };

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

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;

    let linked_plan_branch = if workspace.mode == AgentConversationWorkspaceMode::Ideation {
        match workspace.linked_plan_branch_id.as_ref() {
            Some(plan_branch_id) => state
                .plan_branch_repo
                .get_by_id(plan_branch_id)
                .await
                .map_err(|e| e.to_string())?,
            None => None,
        }
    } else {
        None
    };
    let pr_number = linked_plan_branch
        .as_ref()
        .and_then(|branch| branch.pr_number)
        .or(workspace.publication_pr_number)
        .ok_or_else(|| "No PR associated with this workspace".to_string())?;

    let working_dir = std::path::Path::new(&project.working_directory);

    if let Some(github_svc) = &state.github_service {
        if let Err(e) = github_svc.close_pr(working_dir, pr_number).await {
            tracing::warn!(
                pr_number = pr_number,
                error = %e,
                "close_agent_workspace_pr: failed to close PR on remote (continuing with local status update)"
            );
        }
    }

    if let Some(plan_branch) = linked_plan_branch.as_ref() {
        state
            .plan_branch_repo
            .update_pr_status(&plan_branch.id, PrStatus::Closed)
            .await
            .map_err(|e| e.to_string())?;
    }

    state
        .agent_conversation_workspace_repo
        .update_publication(
            &conversation_id,
            Some(pr_number),
            linked_plan_branch
                .as_ref()
                .and_then(|branch| branch.pr_url.as_deref())
                .or(workspace.publication_pr_url.as_deref()),
            Some("closed"),
            workspace.publication_push_status.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;

    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Workspace disappeared after update".to_string())?;

    agent_workspace_response_for_state(&state, updated).await
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

    if workspace.mode != AgentConversationWorkspaceMode::Edit {
        return Err(
            "Ideation-mode agent conversations are published through the execution pipeline"
                .to_string(),
        );
    }
    if workspace.is_execution_owned() {
        return Err(
            "This agent conversation workspace is owned by an execution plan and cannot be directly published"
                .to_string(),
        );
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

    let publisher = AgentWorkspacePrPublisher::new(github);
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

    let review_chat_service: Arc<dyn ChatService> = Arc::new(repair_service);
    state.pr_poller_registry.start_agent_workspace_polling(
        workspace.conversation_id,
        outcome.pr_number,
        project.clone(),
        worktree_path.clone(),
        Arc::clone(&state.agent_conversation_workspace_repo),
        review_chat_service,
    );

    let refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(workspace);

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
    let push_status = publish_push_status_for_failure(error);
    let failure_class = classify_publish_failure(error);
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
    should_defer_agent_workspace_repair_message_for_registry(
        state.app_handle.is_some(),
        &state.running_agent_registry,
        workspace,
    )
    .await
}

async fn should_defer_agent_workspace_repair_message_for_registry(
    app_handle_available: bool,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    workspace: &AgentConversationWorkspace,
) -> bool {
    if !app_handle_available {
        return false;
    }

    let key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        workspace.conversation_id.as_str(),
    );
    running_agent_registry.is_running(&key).await
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
        let wait_started = Instant::now();
        loop {
            let Some(state) = app_handle.try_state::<AppState>() else {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    "Deferred agent workspace repair could not access AppState"
                );
                return;
            };
            if !state.running_agent_registry.is_running(&key).await {
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
        conversation: AgentConversationResponse::from(cwm.conversation),
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
    state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map(|conversation| conversation.map(AgentConversationResponse::from))
        .map_err(|e| e.to_string())
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
        conversation: AgentConversationResponse::from(conversation),
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
        conversation: AgentConversationResponse::from(conversation),
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
) -> Result<HashMap<String, bool>, String> {
    let service =
        state.build_chat_service_with_execution_state(Arc::clone(execution_state.inner()));

    get_agent_running_states_for_service(&service, context_type, context_ids).await
}

#[doc(hidden)]
pub async fn get_agent_running_states_for_service(
    service: &dyn ChatService,
    context_type: String,
    context_ids: Vec<String>,
) -> Result<HashMap<String, bool>, String> {
    let context_type = parse_context_type(&context_type)?;

    Ok(service
        .get_agent_running_states(context_type, &context_ids)
        .await)
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

    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .map(AgentConversationResponse::from)
        .map_err(|e| e.to_string())
}

/// Update an existing conversation title.
#[tauri::command]
pub async fn update_agent_conversation_title(
    input: UpdateAgentConversationTitleInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationResponse, String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err("Conversation title cannot be empty".to_string());
    }

    let conversation_id = ChatConversationId::from_string(input.conversation_id);
    state
        .chat_conversation_repo
        .update_title(&conversation_id, title)
        .await
        .map_err(|e| e.to_string())?;

    state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .map(AgentConversationResponse::from)
        .ok_or_else(|| "Conversation not found".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        agent_workspace_freshness_cache, agent_workspace_freshness_cache_key,
        agent_workspace_post_repair_action_from_events, apply_base_resolution_to_publish_target,
        agent_workspace_response_for_state,
        build_agent_workspace_publish_repair_message_for_target,
        build_agent_workspace_repair_message_for_target, cached_agent_workspace_freshness,
        existing_pr_retarget_block_reason, get_agent_conversation_timeline_page_for_app_state,
        get_agent_conversation_workspace_freshness,
        get_agent_timeline_item_tool_call_detail_for_app_state,
        invalidate_agent_workspace_freshness_cache,
        mark_agent_workspace_failure_with_routing_and_action, merge_delegated_snapshot_into_result,
        normalize_agent_runtime_selection, normalize_explicit_publish_base_selection,
        normalized_effort_for_supported, parse_wrapped_mcp_result_object,
        persist_workspace_base_resolution_if_retargeted,
        precompute_agent_conversation_workspace_pr_description_for_app_state,
        project_plan_branch_publication_into_workspace_response,
        publication_event_status_for_push_status, publication_event_summary_for_push_status,
        publish_agent_conversation_workspace_for_app_state,
        retarget_existing_workspace_pr_base_if_needed,
        schedule_external_pr_reconciliation_for_conversation_id,
        schedule_external_pr_reconciliation_for_workspace,
        send_agent_workspace_publish_repair_message_for_target,
        should_defer_agent_workspace_repair_message_for_registry,
        spawn_deferred_agent_workspace_repair_message, store_agent_workspace_freshness,
        switch_agent_conversation_mode_for_state, try_acquire_agent_workspace_publish_guard,
        update_agent_conversation_workspace_from_base_for_app_state,
        validate_explicit_publish_base_ref, AgentConversationResponse,
        AgentConversationWorkspaceFreshnessResponse, AgentConversationWorkspacePublishTarget,
        AgentConversationWorkspaceRepairTarget, AgentConversationWorkspaceResponse,
        AgentTimelineItemResponse, AgentWorkspaceExternalPrReconciliationTrigger,
        AgentWorkspaceFreshnessCacheEntry, AgentWorkspaceFreshnessCacheStatus,
        AgentWorkspaceFreshnessInvalidationGuard, AgentWorkspaceFreshnessScope,
        AgentWorkspacePostRepairAction, AgentWorkspacePrDescriptionInvalidationGuard,
        AgentWorkspaceRepairRuntimeOverrides, DelegatedToolRuntimeSnapshot,
        SwitchAgentConversationModeInput, AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE,
    };
    use crate::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use crate::application::agent_conversation_workspace_base::{
        BaseResolutionResult, BaseStatus, BLOCK_REASON_MISSING_BASE_COMMIT,
    };
    use crate::application::publish_resilience::PublishBranchFreshnessStatus;
    use crate::application::{
        chat_service::MockChatService, AppState, TeamService, TeamStateTracker,
    };
    use crate::commands::ExecutionState;
    use crate::domain::agents::{
        AgentConfig, AgentHandle, AgentHarnessKind, AgentModelDefinition, AgentOutput,
        AgentResponse, AgentResult, AgenticClient, ClientCapabilities, LogicalEffort,
        ProviderSessionRef, ResponseChunk,
    };
    use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus};
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentConversationWorkspacePublicationEvent, AgentWorkspacePrDescription, ArtifactId,
        ChatContextType, ChatConversation, ChatConversationId, ChatMessageId, ChatTimelineItem,
        ChatTimelineItemId, ChatTimelineItemKind, ChatTimelineItemStatus, ExecutionPlan,
        ExecutionPlanStatus, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionId,
        MessageRole, PlanBranch, PlanBranchId, PlanBranchStatus, Project, ProjectId,
    };
    use crate::domain::repositories::AgentConversationWorkspaceRepository;
    use crate::domain::services::{
        GithubServiceTrait, MemoryRunningAgentRegistry, PrBranchMatch, PrStatus as GithubPrStatus,
        RunningAgentKey, RunningAgentRegistry,
    };
    use crate::error::AppError;
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
        for model_id in ["sonnet", "opus", "haiku"] {
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
            base_ref_kind: "project_default".to_string(),
            base_ref: "main".to_string(),
            base_display_name: Some("Project default (main)".to_string()),
            base_commit: None,
            branch_name: "agent-d619a9fd".to_string(),
            worktree_path: "/tmp/workspace".to_string(),
            linked_ideation_session_id: Some("session-1".to_string()),
            linked_plan_branch_id: Some("plan-branch-1".to_string()),
            publication_pr_number: None,
            publication_pr_url: None,
            publication_pr_status: None,
            publication_push_status: None,
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
            base_ref_kind: "project_default".to_string(),
            base_ref: "main".to_string(),
            base_display_name: Some("Project default (main)".to_string()),
            base_commit: None,
            branch_name: "agent-shell-branch".to_string(),
            worktree_path: "/tmp/workspace".to_string(),
            linked_ideation_session_id: Some("session-1".to_string()),
            linked_plan_branch_id: Some("plan-branch-1".to_string()),
            publication_pr_number: Some(12),
            publication_pr_url: Some("https://github.com/mock/project/pull/12".to_string()),
            publication_pr_status: Some("open".to_string()),
            publication_push_status: Some("needs_agent".to_string()),
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
                &workspace
            )
            .await
        );
        assert!(
            !should_defer_agent_workspace_repair_message_for_registry(
                false,
                &registry_trait,
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
                &workspace
            )
            .await
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
                base_ref: Some("  ".to_string()),
                display_name: Some("ignored".to_string()),
            }
        )
        .expect("blank base ref should be allowed as no explicit selection")
        .is_none());

        let local =
            normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
                kind: None,
                base_ref: Some("  release/0.8  ".to_string()),
                display_name: None,
            })
            .expect("local branch should normalize")
            .expect("local branch should produce a selection");
        assert_eq!(local.kind, IdeationAnalysisBaseRefKind::LocalBranch);
        assert_eq!(local.base_ref, "release/0.8");
        assert_eq!(local.display_name, "release/0.8");

        let project =
            normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                base_ref: Some("main".to_string()),
                display_name: Some("  ".to_string()),
            })
            .expect("project default should normalize")
            .expect("project default should produce a selection");
        assert_eq!(project.display_name, "Project default (main)");

        let current =
            normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::CurrentBranch),
                base_ref: Some("feature/base".to_string()),
                display_name: None,
            })
            .expect("current branch should normalize")
            .expect("current branch should produce a selection");
        assert_eq!(current.display_name, "Current branch (feature/base)");

        let error =
            normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::PullRequest),
                base_ref: Some("123".to_string()),
                display_name: None,
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
    }

    #[async_trait]
    impl AgenticClient for SubmittingPrDescriptionClient {
        async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
            *self.spawned.lock().await += 1;
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
                base_ref: Some("main".to_string()),
                display_name: None,
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

        let client = Arc::new(SubmittingPrDescriptionClient {
            repo: Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id: conversation_id.clone(),
            spawned: tokio::sync::Mutex::new(0),
        });
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
        assert_eq!(*client.spawned.lock().await, 1);
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
                base_ref: Some("release/0.8".to_string()),
                display_name: Some("release/0.8".to_string()),
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
                base_ref: None,
                display_name: None,
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
                base_ref: None,
                display_name: None,
            },
        )
        .await
        .expect_err("primary checkout plan branch should not be updated in place");

        assert!(error.contains("Refusing to update plan branch"));
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
                base_ref: None,
                display_name: None,
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
                base_ref: Some("release/0.8".to_string()),
                display_name: Some("release/0.8".to_string()),
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
                base_ref: Some("release/missing".to_string()),
                display_name: Some("release/missing".to_string()),
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

        assert_eq!(error, BLOCK_REASON_MISSING_BASE_COMMIT);
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
        let mut plan_workspace =
            mode_lock_test_workspace(plan_conversation_id, project_id.clone());
        plan_workspace.linked_plan_branch_id =
            Some(PlanBranchId::from_string("missing-plan-branch".to_string()));

        let plan_response = agent_workspace_response_for_state(&state, plan_workspace)
            .await
            .expect("missing plan branch resolves as unlocked");
        assert!(!plan_response.mode_switch_locked);
        assert!(plan_response.mode_switch_lock_reason.is_none());

        let session_conversation_id =
            ChatConversationId::from_string("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
        let mut session_workspace =
            mode_lock_test_workspace(session_conversation_id, project_id);
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
                base_ref: None,
                base_display_name: None,
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
                base_ref: None,
                base_display_name: None,
            },
            &state,
        )
        .await
        .expect("edit mode switch creates workspace");

        assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
        assert_eq!(
            response.workspace.as_ref().map(|workspace| workspace.mode.as_str()),
            Some("edit")
        );
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
                base_ref: None,
                base_display_name: None,
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
