use std::{path::PathBuf, sync::Arc, time::Instant};

use serde::Deserialize;
use tauri::{Emitter, Runtime};

use crate::application::agent_conversation_workspace::{
    agent_name_for_workspace_mode,
    prepare_agent_conversation_workspace_with_setup_mode_and_defaults,
    AgentConversationWorkspaceBaseSelection, AgentConversationWorkspaceSetupMode,
};
use crate::application::chat_service::{AgentConversationCreatedPayload, SendMessageOptions};
use crate::application::plan_reference_import::{
    import_agent_conversation_plan_reference, rewrite_imported_plan_reference,
    selected_plan_reference,
};
use crate::application::ticket_canonical_branch::ensure_ticket_canonical_branch;
use crate::application::{AppState, ChatService, SendResult, TeamService};
use crate::commands::ExecutionState;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{
    AgentConversationWorkspace, ChatContextType, ChatConversation, ChatConversationId, ProjectId,
    TeamIntent,
};
use crate::domain::services::{
    ComposerArtifactReference, ComposerIntegrationReference, ComposerProjectReference,
};

mod helpers;

use self::helpers::{
    agent_mode_should_create_workspace, agent_workspace_pr_automation_defaults_for_project,
    apply_ticket_canonical_branch_base_selection, base_selection_allows_ticket_canonical_branch,
    emit_start_agent_conversation_progress, ensure_linked_branch_workspace_available,
    ensure_plan_workspace_planning_session_link, first_ticket_start_base_reference,
    hydrate_linked_branch_source_pull_request, log_start_agent_conversation_phase,
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
    pub project_id: String,
    pub content: String,
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
    /// Optional native team-mode overlay request. Disabled by default until the
    /// managed-team runtime is authoritative.
    pub team_intent: Option<TeamIntent>,
}

#[derive(Debug)]
pub struct AgentConversationStartResult {
    pub conversation: ChatConversation,
    pub workspace: Option<AgentConversationWorkspace>,
    pub send_result: SendResult,
}

pub struct AgentConversationStartDeps<'a, R: Runtime + 'static> {
    pub state: &'a AppState,
    pub execution_state: &'a Arc<ExecutionState>,
    pub team_service: Option<Arc<TeamService>>,
    pub app_handle: tauri::AppHandle<R>,
}

pub struct AgentConversationStartService<'a, R: Runtime + 'static> {
    deps: AgentConversationStartDeps<'a, R>,
}

impl<'a, R: Runtime + 'static> AgentConversationStartService<'a, R> {
    pub fn new(deps: AgentConversationStartDeps<'a, R>) -> Self {
        Self { deps }
    }

    pub async fn start(
        self,
        input: StartAgentConversationInput,
    ) -> Result<AgentConversationStartResult, String> {
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
            self.deps.state,
            ChatContextType::Project,
            &input.project_id,
            "start_agent_conversation",
            harness_override,
        )
        .await?;
        crate::application::managed_team::validate_native_team_intent(
            input.team_intent.as_ref(),
            harness_override.unwrap_or(DEFAULT_AGENT_HARNESS),
        )
        .map_err(|error| error.to_string())?;
        log_start_agent_conversation_phase(
            &input.project_id,
            None,
            "validate_chat_runtime",
            validate_runtime_started,
        );

        let parse_input_started = Instant::now();
        let mode = parse_agent_workspace_mode(input.mode.as_deref())?;
        let mut base_ref_kind = parse_agent_workspace_base_kind(input.base_ref_kind.as_deref())?;
        let base_branch_mode =
            parse_agent_workspace_branch_mode(input.base_branch_mode.as_deref())?;
        let mut base_ref = trim_optional_input(input.base_ref);
        let mut base_display_name = trim_optional_input(input.base_display_name);
        let parent_conversation_id = trim_optional_input(input.parent_conversation_id);
        let conversation_title = trim_optional_input(input.title);
        let draft_conversation_id = input
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|conversation_id| !conversation_id.is_empty())
            .map(ChatConversationId::from_string);
        let ticket_start_base_reference =
            first_ticket_start_base_reference(&input.composer_integration_references);
        let mut source_pull_request = normalize_agent_workspace_source_pull_request(
            input.base_source_pull_request,
            base_ref_kind,
            base_ref.as_deref(),
        )?;
        let selected_plan_reference = selected_plan_reference(&input.composer_artifact_references)?;
        let should_create_workspace = agent_mode_should_create_workspace(
            mode,
            source_pull_request.as_ref(),
            selected_plan_reference.is_some(),
        );
        let project_id = ProjectId::from_string(input.project_id.clone());
        log_start_agent_conversation_phase(
            &input.project_id,
            None,
            "parse_input",
            parse_input_started,
        );

        let project_lookup_started = Instant::now();
        let project = self
            .deps
            .state
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

        if should_create_workspace
            && base_selection_allows_ticket_canonical_branch(
                base_ref_kind,
                source_pull_request.as_ref(),
            )
        {
            if let Some(ticket_reference) = ticket_start_base_reference.as_ref() {
                let ticket_base_started = Instant::now();
                let canonical = ensure_ticket_canonical_branch(
                    self.deps.state,
                    &project_id,
                    &ticket_reference.provider,
                    &ticket_reference.issue_key,
                )
                .await
                .map_err(|error| error.to_string())?;
                apply_ticket_canonical_branch_base_selection(
                    &mut base_ref_kind,
                    &mut base_ref,
                    &mut base_display_name,
                    &ticket_reference.issue_key,
                    &canonical.branch_name,
                );
                log_start_agent_conversation_phase(
                    &input.project_id,
                    None,
                    "resolve_ticket_base",
                    ticket_base_started,
                );
            }
        }
        if should_create_workspace {
            ensure_linked_branch_workspace_available(
                self.deps.state,
                &project_id,
                draft_conversation_id.as_ref(),
                base_branch_mode,
                base_ref.as_deref(),
                source_pull_request.as_ref(),
            )
            .await?;
        }
        source_pull_request = hydrate_linked_branch_source_pull_request(
            self.deps.state,
            &project,
            base_branch_mode,
            base_ref.as_deref(),
            source_pull_request,
        )
        .await?;

        let conversation_resolve_started = Instant::now();
        let mut conversation = if let Some(conversation_id) = draft_conversation_id {
            let conversation = self
                .deps
                .state
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
        let should_create_conversation = draft_conversation_id.is_none();
        if let Some(parent_conversation_id) = parent_conversation_id.as_deref() {
            if should_create_conversation {
                let parent_id = ChatConversationId::from_string(parent_conversation_id.to_string());
                let parent = self
                    .deps
                    .state
                    .chat_conversation_repo
                    .get_by_id(&parent_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Parent conversation not found: {}", parent_id))?;
                if parent.context_type != ChatContextType::Project
                    || parent.context_id != input.project_id
                {
                    return Err(format!(
                        "Parent conversation {} does not belong to project {}",
                        parent.id, input.project_id
                    ));
                }
                conversation.parent_conversation_id = Some(parent.id.as_str());
            }
        }
        if should_create_conversation {
            if let Some(title) = conversation_title {
                conversation.set_title(title);
            }
        }
        log_start_agent_conversation_phase(
            &input.project_id,
            Some(&conversation.id),
            "resolve_conversation",
            conversation_resolve_started,
        );

        let workspace_prepare_started = Instant::now();
        if should_create_conversation {
            emit_start_agent_conversation_progress(
                &self.deps.app_handle,
                &input.project_id,
                &conversation.id,
                "resolve_conversation",
                "Creating chat",
            );
        }
        if should_create_workspace {
            emit_start_agent_conversation_progress(
                &self.deps.app_handle,
                &input.project_id,
                &conversation.id,
                "prepare_workspace",
                "Setup workspace",
            );
        }
        let mut composer_artifact_references = input.composer_artifact_references.clone();
        let workspace = if should_create_workspace {
            let pr_automation_defaults =
                agent_workspace_pr_automation_defaults_for_project(self.deps.state, &project.id)
                    .await?;
            let mut workspace = prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
                &project,
                &conversation.id,
                mode,
                AgentConversationWorkspaceBaseSelection {
                    kind: base_ref_kind,
                    branch_mode: base_branch_mode,
                    base_ref,
                    display_name: base_display_name,
                    source_pull_request,
                },
                AgentConversationWorkspaceSetupMode::Deferred,
                pr_automation_defaults,
            )
            .await
            .map_err(|error| error.to_string())?;
            if let Some(plan_reference) = selected_plan_reference.as_ref() {
                let import = import_agent_conversation_plan_reference(
                    self.deps.state,
                    &project,
                    &mut workspace,
                    plan_reference,
                )
                .await?;
                composer_artifact_references = rewrite_imported_plan_reference(
                    &composer_artifact_references,
                    plan_reference,
                    &import.composer_reference,
                );
            } else {
                ensure_plan_workspace_planning_session_link(
                    self.deps.state,
                    &project,
                    &mut workspace,
                )
                .await?;
            }
            Some(workspace)
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
        emit_start_agent_conversation_progress(
            &self.deps.app_handle,
            &input.project_id,
            &conversation.id,
            "persist_conversation",
            "Saving chat",
        );
        let conversation = if should_create_conversation {
            self.deps
                .state
                .chat_conversation_repo
                .create(conversation)
                .await
                .map_err(|error| error.to_string())?
        } else {
            self.deps
                .state
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
        if workspace.is_some() {
            emit_start_agent_conversation_progress(
                &self.deps.app_handle,
                &input.project_id,
                &conversation.id,
                "persist_workspace",
                "Saving chat",
            );
        }
        let workspace = match workspace {
            Some(workspace) => match self
                .deps
                .state
                .agent_conversation_workspace_repo
                .create_or_update(workspace)
                .await
            {
                Ok(workspace) => Some(workspace),
                Err(error) => {
                    if should_create_conversation {
                        let _ = self
                            .deps
                            .state
                            .chat_conversation_repo
                            .delete(&conversation.id)
                            .await;
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
            let _ = self.deps.app_handle.emit(
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
        let mut service = self.deps.state.build_chat_service_for_runtime(
            Some(Arc::clone(self.deps.execution_state)),
            Some(self.deps.app_handle.clone()),
        );
        if let Some(team_service) = self.deps.team_service {
            service = service.with_team_service(team_service);
        }
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
            self.deps.state,
            harness_override,
            model_override,
            input.logical_effort,
        )
        .await?;
        let service_tier_override =
            crate::application::chat_service::codex_fast_mode_service_tier_override(
                input.codex_fast_mode,
            );
        log_start_agent_conversation_phase(
            &input.project_id,
            Some(&conversation.id),
            "normalize_runtime_selection",
            runtime_normalize_started,
        );

        let send_message_started = Instant::now();
        emit_start_agent_conversation_progress(
            &self.deps.app_handle,
            &input.project_id,
            &conversation.id,
            "send_message",
            "Starting agent",
        );
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
                    service_tier_override,
                    conversation_id_override: Some(conversation.id),
                    working_directory_override,
                    composer_project_references: input.composer_project_references.clone(),
                    composer_integration_references: input.composer_integration_references.clone(),
                    composer_artifact_references,
                    team_intent: input.team_intent.clone(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        log_start_agent_conversation_phase(
            &input.project_id,
            Some(&conversation.id),
            "send_message",
            send_message_started,
        );
        log_start_agent_conversation_phase(
            &input.project_id,
            Some(&conversation.id),
            "command_total",
            command_started,
        );

        Ok(AgentConversationStartResult {
            conversation,
            workspace,
            send_result,
        })
    }
}
