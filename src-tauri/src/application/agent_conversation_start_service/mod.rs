use std::{path::PathBuf, sync::Arc, time::Instant};

use ralphx_events::EventSink;
use serde::Deserialize;

use crate::application::agent_conversation_workspace::{
    agent_name_for_workspace_mode,
    prepare_agent_conversation_workspace_with_setup_mode_defaults_and_branch_name_hint,
    resolve_valid_agent_conversation_workspace_path,
    validate_review_pr_workspace_source_pull_request, AgentConversationWorkspaceBaseSelection,
    AgentConversationWorkspaceBranchNameHint, AgentConversationWorkspaceSetupMode,
};
use crate::application::app_state::ApplicationExecutionState;
use crate::application::builder_attachment_materializer::sync_builder_attachments;
use crate::application::chat_service::{AgentConversationCreatedPayload, SendMessageOptions};
use crate::application::clickup_git_association::{
    clickup_identity_from_task, resolve_clickup_ticket_start, ClickUpTicketStartResolution,
};
use crate::application::external_issue_link_service::TicketConversationLinkInput;
use crate::application::git_service::GitService;
use crate::application::personas::PersonaService;
use crate::application::plan_reference_import::{
    import_agent_conversation_plan_reference, rewrite_imported_plan_references,
    selected_plan_reference,
};
use crate::application::seeded_agent_conversation_abort::abort_seeded_agent_conversation;
use crate::application::standalone_workspace::{
    create_workspace, remove_workspace_if_present, resolve_workspace,
};
use crate::application::{AppState, ChatService, SendResult};
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, ManualServiceTier};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest, ChatContextType,
    ChatConversation, ChatConversationId, CoordinationMode, IdeationAnalysisBaseRefKind,
    PersonaDirective, PersonaId, ProjectId, TeamIntent,
};
use crate::domain::services::{
    ComposerArtifactReference, ComposerIntegrationReference, ComposerProjectReference,
    ComposerSelectionSnapshot,
};
use crate::error::AppError;
use crate::infrastructure::agents::{agent_personas_enabled, standalone_conversations_enabled};

mod finish_flow;
mod helpers;
mod project_setup;
mod start;

use self::finish_flow::FinishFlow;
use self::project_setup::{ProjectSetupInput, ProjectSetupOutput};

use self::helpers::{
    agent_mode_should_create_workspace, agent_workspace_pr_automation_defaults_for_project,
    archive_empty_seeded_draft_after_setup_failure,
    archive_supplied_seeded_draft_after_setup_failure, clickup_task_lookup_key_from_references,
    emit_start_agent_conversation_progress, ensure_linked_branch_workspace_available,
    ensure_plan_workspace_planning_session_link, ensure_review_pr_monitor_for_workspace,
    first_ticket_branch_name_hint, hydrate_linked_branch_source_pull_request,
    linked_setup_failure_error, log_start_agent_conversation_phase,
    normalize_agent_runtime_selection, normalize_agent_workspace_source_pull_request,
    parse_agent_workspace_base_kind, parse_agent_workspace_branch_mode, parse_agent_workspace_mode,
    trim_optional_input,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkspaceSourcePullRequestInput {
    pub number: i64,
    pub url: Option<String>,
    pub title: Option<String>,
    pub head_ref_name: String,
    pub base_ref_name: Option<String>,
    pub head_ref_oid: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartAgentConversationInput {
    /// `None` (including an omitted `projectId` key) starts a standalone
    /// (projectless) conversation — requires the `standalone_conversations`
    /// flag, `mode == "chat"` or `mode == "persona_builder"`, and a solo team intent.
    #[serde(default)]
    pub project_id: Option<String>,
    pub content: String,
    /// Optional active persona to bind before the first project-conversation send.
    pub persona_id: Option<String>,
    /// Optional active persona whose content seeds a scope-locked builder draft.
    #[serde(default)]
    pub source_persona_id: Option<String>,
    /// Optional draft conversation to use after uploading pending attachments.
    pub conversation_id: Option<String>,
    /// Optional visible parent conversation for follow-up/branch conversations.
    pub parent_conversation_id: Option<String>,
    /// Optional initial title for a newly created conversation.
    pub title: Option<String>,
    /// Optional provider harness selected for the initial conversation send.
    pub provider_harness: Option<String>,
    /// Optional explicit model override for the spawned agent.
    pub model_override: Option<String>,
    /// Optional provider-neutral reasoning effort override for the spawned agent.
    pub logical_effort: Option<LogicalEffort>,
    /// Optional Codex Fast Mode override for this initial send.
    pub codex_fast_mode: Option<bool>,
    /// Agent mode: "chat" routes to a read-only explorer in the project root;
    /// edit/plan/ideation modes create a selected-base workspace for runtime CWD.
    pub mode: Option<String>,
    /// Optional base ref kind using ideation naming: project_default, current_branch, local_branch.
    pub base_ref_kind: Option<String>,
    /// Optional branch work policy: isolated creates a new RalphX branch; linked uses the selected branch.
    pub base_branch_mode: Option<String>,
    /// Optional selected branch/ref name for the base.
    pub base_ref: Option<String>,
    /// Optional user-facing base ref label.
    pub base_display_name: Option<String>,
    /// Optional source pull request metadata when the selected base came from a PR head branch.
    pub base_source_pull_request: Option<AgentWorkspaceSourcePullRequestInput>,
    /// Structured composer project references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_project_references: Vec<ComposerProjectReference>,
    /// Structured external integration references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_integration_references: Vec<ComposerIntegrationReference>,
    /// Structured artifact references for runtime-only prompt expansion.
    #[serde(default)]
    pub composer_artifact_references: Vec<ComposerArtifactReference>,
    /// Immutable whole-line artifact or ticket excerpt selected for the first turn.
    pub composer_selection_snapshot: Option<ComposerSelectionSnapshot>,
    /// Optional Team request for the Agent conversation.
    #[serde(alias = "capabilityIntent")]
    pub team_intent: Option<TeamIntent>,
}

#[derive(Debug)]
pub struct AgentConversationStartResult {
    pub conversation: ChatConversation,
    pub workspace: Option<AgentConversationWorkspace>,
    pub send_result: SendResult,
}

pub struct AgentConversationStartDeps<'a> {
    pub state: &'a AppState,
    pub execution_state: &'a Arc<ApplicationExecutionState>,
    pub events: Arc<dyn EventSink>,
}

pub struct AgentConversationStartService<'a> {
    deps: AgentConversationStartDeps<'a>,
}

const PERSONA_BINDING_PROJECT_CONTEXT_ERROR: &str =
    "Persona bindings require Project conversation context";
const STANDALONE_CONVERSATIONS_DISABLED_ERROR: &str =
    "Standalone conversations are disabled (flag: standalone_conversations)";
const STANDALONE_MODE_NOT_ALLOWED_ERROR: &str =
    "Standalone conversations only support mode=\"chat\" or mode=\"persona_builder\"";
const STANDALONE_TEAM_INTENT_REJECTED_ERROR: &str =
    "Team mode is not supported for standalone conversations";
const STANDALONE_PARENT_CONVERSATION_REJECTED_ERROR: &str =
    "Standalone conversations do not support parent_conversation_id";
const STANDALONE_CONTEXT_LOG_LABEL: &str = "standalone";
const PERSONA_BUILDER_TEAM_INTENT_REJECTED_ERROR: &str =
    "Team mode is not supported for persona builder conversations";
const PERSONA_BUILDER_SOURCE_MODE_ERROR: &str =
    "source_persona_id is valid only with mode=\"persona_builder\"";
const SEEDED_CONVERSATION_MODE_LOCKED_ERROR_CODE: &str = "[ralphx:conversation_mode_locked]";

fn ensure_persona_binding_project_context(context_type: ChatContextType) -> Result<(), AppError> {
    if context_type == ChatContextType::Project {
        Ok(())
    } else {
        Err(AppError::Validation(
            PERSONA_BINDING_PROJECT_CONTEXT_ERROR.to_string(),
        ))
    }
}
