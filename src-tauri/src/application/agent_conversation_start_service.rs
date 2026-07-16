use std::{path::PathBuf, sync::Arc, time::Instant};

use serde::Deserialize;
use tauri::{Emitter, Runtime};

use crate::application::agent_conversation_workspace::{
    agent_name_for_workspace_mode,
    prepare_agent_conversation_workspace_with_setup_mode_defaults_and_branch_name_hint,
    resolve_valid_agent_conversation_workspace_path,
    validate_review_pr_workspace_source_pull_request, AgentConversationWorkspaceBaseSelection,
    AgentConversationWorkspaceBranchNameHint, AgentConversationWorkspaceSetupMode,
};
use crate::application::chat_service::{AgentConversationCreatedPayload, SendMessageOptions};
use crate::application::clickup_git_association::{
    clickup_identity_from_task, resolve_clickup_ticket_start, ClickUpTicketStartResolution,
};
use crate::application::external_issue_link_service::TicketConversationLinkInput;
use crate::application::git_service::GitService;
use crate::application::personas::PersonaService;
use crate::application::plan_reference_import::{
    import_agent_conversation_plan_reference, rewrite_imported_plan_reference,
    selected_plan_reference,
};
use crate::application::{AppState, ChatService, SendResult, TeamService};
use crate::commands::ExecutionState;
use crate::domain::agents::{
    AgentHarnessKind, LogicalEffort, ManualServiceTier, DEFAULT_AGENT_HARNESS,
};
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
use crate::infrastructure::agents::claude::agent_personas_enabled;

mod helpers;

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
    pub project_id: String,
    pub content: String,
    /// Optional active persona to bind before the first project-conversation send.
    pub persona_id: Option<String>,
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

pub struct AgentConversationStartDeps<'a, R: Runtime + 'static> {
    pub state: &'a AppState,
    pub execution_state: &'a Arc<ExecutionState>,
    pub team_service: Option<Arc<TeamService>>,
    pub app_handle: tauri::AppHandle<R>,
}

pub struct AgentConversationStartService<'a, R: Runtime + 'static> {
    deps: AgentConversationStartDeps<'a, R>,
}

const PERSONA_BINDING_PROJECT_CONTEXT_ERROR: &str =
    "Persona bindings require Project conversation context";

fn ensure_persona_binding_project_context(context_type: ChatContextType) -> Result<(), AppError> {
    if context_type == ChatContextType::Project {
        Ok(())
    } else {
        Err(AppError::Validation(
            PERSONA_BINDING_PROJECT_CONTEXT_ERROR.to_string(),
        ))
    }
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

        let parse_input_started = Instant::now();
        let mode = parse_agent_workspace_mode(input.mode.as_deref())?;
        let mut base_ref_kind = parse_agent_workspace_base_kind(input.base_ref_kind.as_deref())?;
        let mut base_branch_mode =
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
        let mut ticket_branch_name_hint =
            first_ticket_branch_name_hint(&input.composer_integration_references);
        let mut source_pull_request = normalize_agent_workspace_source_pull_request(
            input.base_source_pull_request,
            base_ref_kind,
            base_ref.as_deref(),
        )?;
        validate_review_pr_workspace_source_pull_request(mode, source_pull_request.as_ref())
            .map_err(|error| error.to_string())?;
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

        let has_explicit_composer_override = input.provider_harness.is_some()
            || input.model_override.is_some()
            || input.logical_effort.is_some()
            || input.codex_fast_mode.is_some()
            || input.persona_id.is_some()
            || input.team_intent.is_some();
        let role_default = if has_explicit_composer_override {
            None
        } else {
            let role = crate::application::agent_lane_resolution::routing_role_for_chat_launch(
                agent_name_for_workspace_mode(mode),
                ChatContextType::Project,
                None,
                Some(mode),
                false,
            );
            Some(
                self.deps
                    .state
                    .manual_role_default_service()
                    .resolve(
                        Some(&input.project_id),
                        Some(std::path::Path::new(&project.working_directory)),
                        role,
                    )
                    .await
                    .map_err(|error| {
                        format!("Failed to resolve manual default for {role}: {error}")
                    })?,
            )
        };
        let role_value = role_default.as_ref().map(|resolved| &resolved.value);
        let harness_override = match input.provider_harness.as_deref() {
            Some(provider) => Some(provider.parse::<AgentHarnessKind>()?),
            None => role_value.map(|value| value.harness),
        };
        let effective_model_override = input
            .model_override
            .clone()
            .or_else(|| role_value.and_then(|value| value.model.clone()));
        let effective_logical_effort = input
            .logical_effort
            .or_else(|| role_value.and_then(|value| value.effort));
        let effective_service_tier_override = match input.codex_fast_mode {
            Some(fast) => {
                crate::application::chat_service::codex_fast_mode_service_tier_override(Some(fast))
            }
            None => role_value.and_then(|value| match value.service_tier {
                ManualServiceTier::ProviderDefault => None,
                ManualServiceTier::Standard => Some("standard".to_string()),
                ManualServiceTier::Fast => Some("fast".to_string()),
            }),
        };
        let effective_team_intent = input.team_intent.clone().or_else(|| {
            role_value
                .and_then(|value| value.coordination_mode)
                .map(|coordination_mode| TeamIntent {
                    coordination_mode,
                    strategy: None,
                })
        });
        let effective_persona_id = input.persona_id.clone().or_else(|| {
            agent_personas_enabled()
                .then(|| role_value.and_then(|value| value.persona_id.as_ref()))
                .flatten()
                .map(ToString::to_string)
        });
        let persona_id = trim_optional_input(effective_persona_id).map(PersonaId::from_string);
        let requested_coordination_mode = effective_team_intent
            .as_ref()
            .map(|intent| intent.coordination_mode);

        let validate_runtime_started = Instant::now();
        crate::application::validate_chat_runtime_for_context_with_override(
            self.deps.state,
            ChatContextType::Project,
            &input.project_id,
            "start_agent_conversation",
            harness_override,
        )
        .await?;
        let requested_capability = requested_coordination_mode.unwrap_or_default();
        let requested_harness = harness_override.unwrap_or(DEFAULT_AGENT_HARNESS);
        let codex_ultra_supported = (requested_capability == CoordinationMode::CodexNativeUltra)
            .then(|| {
                crate::application::agent_capability_validation::codex_ultra_support_for_model(
                    requested_harness,
                    effective_model_override.as_deref(),
                )
            })
            .flatten();
        crate::application::agent_capability_validation::validate_agent_capability(
            requested_capability,
            requested_harness,
            &self.deps.state.agent_capability_gate,
            codex_ultra_supported,
        )
        .map_err(|error| error.to_string())?;
        crate::application::managed_team::validate_native_team_intent(
            effective_team_intent.as_ref(),
            requested_harness,
        )
        .map_err(|error| error.to_string())?;
        log_start_agent_conversation_phase(
            &input.project_id,
            None,
            "validate_chat_runtime",
            validate_runtime_started,
        );

        let validated_clickup_task = if let Some(lookup_key) =
            clickup_task_lookup_key_from_references(&input.composer_integration_references)?
        {
            let task = self
                .deps
                .state
                .clickup_integration_service
                .fetch_task(&lookup_key)
                .await?;
            let identity = clickup_identity_from_task(&task);
            ticket_branch_name_hint = Some(AgentConversationWorkspaceBranchNameHint {
                provider: "clickup".to_string(),
                ticket_token: identity.preferred_token(),
            });

            let should_auto_resolve_ticket_base = should_create_workspace
                && matches!(
                    mode,
                    AgentConversationWorkspaceMode::Edit
                        | AgentConversationWorkspaceMode::Plan
                        | AgentConversationWorkspaceMode::Ideation
                )
                && matches!(
                    base_ref_kind,
                    None | Some(IdeationAnalysisBaseRefKind::ProjectDefault)
                        | Some(IdeationAnalysisBaseRefKind::CurrentBranch)
                );
            if should_auto_resolve_ticket_base {
                match resolve_clickup_ticket_start(
                    &identity,
                    std::path::Path::new(&project.working_directory),
                    self.deps.state.github_service.as_deref(),
                )
                .await?
                {
                    ClickUpTicketStartResolution::NoMatch => {}
                    ClickUpTicketStartResolution::Unique(candidate) => {
                        base_ref_kind = Some(IdeationAnalysisBaseRefKind::LocalBranch);
                        base_branch_mode = Some(AgentConversationWorkspaceBranchMode::Linked);
                        base_ref = Some(candidate.branch_name.clone());
                        base_display_name = Some(format!(
                            "ClickUp {} ({})",
                            identity.preferred_token(),
                            candidate.branch_name
                        ));
                        source_pull_request = candidate.pull_request.map(|pull_request| {
                            AgentWorkspaceSourcePullRequest {
                                number: pull_request.number,
                                url: Some(pull_request.url),
                                title: Some(pull_request.title),
                                head_ref_name: pull_request.head_ref_name,
                                base_ref_name: Some(pull_request.base_ref_name),
                                head_ref_oid: pull_request.head_ref_oid,
                            }
                        });
                    }
                    ClickUpTicketStartResolution::Ambiguous { branch_names } => {
                        return Err(format!(
                            "ClickUp task {} matches multiple open PRs or branches ({}); select the intended branch explicitly",
                            identity.preferred_token(),
                            branch_names.join(", ")
                        ));
                    }
                }
            }
            Some(task)
        } else {
            None
        };

        if let Some(persona_id) = persona_id.as_ref() {
            if let Some(conversation_id) = draft_conversation_id.as_ref() {
                let existing = self
                    .deps
                    .state
                    .chat_conversation_repo
                    .get_by_id(conversation_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Conversation not found: {conversation_id}"))?;
                ensure_persona_binding_project_context(existing.context_type)
                    .map_err(|error| error.to_string())?;
            } else {
                ensure_persona_binding_project_context(ChatContextType::Project)
                    .map_err(|error| error.to_string())?;
            }

            PersonaService::new(
                self.deps.state.db.clone(),
                Arc::clone(&self.deps.state.persona_repo),
                Arc::clone(&self.deps.state.chat_conversation_repo),
            )
            .ensure_bindable(agent_personas_enabled(), persona_id)
            .await
            .map_err(|error| error.to_string())?;
        }

        if should_create_workspace {
            if let Err(error) = ensure_linked_branch_workspace_available(
                self.deps.state,
                &project_id,
                draft_conversation_id.as_ref(),
                base_branch_mode,
                base_ref.as_deref(),
                source_pull_request.as_ref(),
            )
            .await
            {
                if let Some(conversation_id) = draft_conversation_id.as_ref() {
                    if let Err(archive_error) = archive_supplied_seeded_draft_after_setup_failure(
                        self.deps.state,
                        &input.project_id,
                        conversation_id,
                    )
                    .await
                    {
                        return Err(linked_setup_failure_error(format!(
                            "{error}; failed to archive failed draft: {archive_error}",
                        )));
                    }
                }
                return Err(linked_setup_failure_error(error));
            }
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
        if let Some(coordination_mode) = requested_coordination_mode {
            conversation.set_coordination_mode(coordination_mode);
        }
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
            let mut workspace =
                match prepare_agent_conversation_workspace_with_setup_mode_defaults_and_branch_name_hint(
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
                    // Automation runs (setup + successors) prefer the advanced
                    // remote-tracking base so successor worktrees build on merged work
                    // (integration-branch model). Non-automation chats keep the local
                    // start-point.
                    conversation.automation_id.is_some(),
                    ticket_branch_name_hint.clone(),
                )
                .await
                {
                    Ok(workspace) => workspace,
                    Err(error) => {
                        let mut error = error.to_string();
                        if !should_create_conversation {
                            if let Err(archive_error) =
                                archive_empty_seeded_draft_after_setup_failure(
                                    self.deps.state,
                                    &conversation,
                                )
                                .await
                            {
                                error = format!(
                                    "{error}; failed to archive failed draft: {archive_error}",
                                );
                            }
                        }
                        return Err(
                            if base_branch_mode
                                == Some(AgentConversationWorkspaceBranchMode::Linked)
                            {
                                linked_setup_failure_error(error)
                            } else {
                                error
                            },
                        );
                    }
                };
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
        let mut conversation = if should_create_conversation {
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
            if let Some(coordination_mode) = requested_coordination_mode {
                self.deps
                    .state
                    .chat_conversation_repo
                    .update_coordination_mode(&conversation.id, coordination_mode)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            conversation
        };
        if let Some(persona_id) = persona_id.as_ref() {
            self.deps
                .state
                .chat_conversation_repo
                .update_persona_binding(&conversation.id, Some(persona_id.as_str()))
                .await
                .map_err(|error| error.to_string())?;
            conversation.persona_id = Some(persona_id.to_string());
        }
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
        ensure_review_pr_monitor_for_workspace(
            self.deps.state.agent_conversation_workspace_repo.as_ref(),
            workspace.as_ref(),
        )
        .await?;
        log_start_agent_conversation_phase(
            &input.project_id,
            Some(&conversation.id),
            "persist_workspace",
            workspace_persist_started,
        );

        if let Some(clickup_task) = validated_clickup_task.as_ref() {
            let head_sha = match workspace.as_ref() {
                Some(workspace) => {
                    match resolve_valid_agent_conversation_workspace_path(&project, workspace).await
                    {
                        Ok(worktree_path) => GitService::get_head_sha(&worktree_path).await.ok(),
                        Err(_) => None,
                    }
                }
                None => None,
            };
            let metadata_json = serde_json::json!({
                "source": "ticket_start",
                "title": clickup_task.name,
                "branch": workspace.as_ref().map(|workspace| workspace.branch_name.as_str()),
                "pr_number": workspace.as_ref().and_then(|workspace| {
                    workspace.source_pull_request.as_ref().map(|pull_request| pull_request.number)
                }),
                "validated_at": chrono::Utc::now().to_rfc3339(),
            })
            .to_string();
            self.deps
                .state
                .external_issue_link_service
                .upsert_ticket_conversation_link(TicketConversationLinkInput {
                    provider: "clickup".to_string(),
                    external_kind: "clickup".to_string(),
                    external_id: clickup_task.id.clone(),
                    external_key: clickup_task.custom_id.clone(),
                    external_url: clickup_task.url.clone(),
                    conversation_id: conversation.id.as_str(),
                    project_id: project.id.to_string(),
                    local_sha: head_sha,
                    local_state: Some("active".to_string()),
                    metadata_json: Some(metadata_json),
                })
                .await
                .map_err(|error| error.to_string())?;
            let _ = self.deps.app_handle.emit(
                "ticketing:cache_invalidated",
                serde_json::json!({
                    "provider": "clickup",
                    "ticketId": clickup_task.id,
                    "ticketKey": clickup_task.custom_id,
                    "projectId": project.id.to_string(),
                    "reason": "conversation_started",
                    "invalidatedAt": chrono::Utc::now().to_rfc3339(),
                }),
            );
        }

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
        let model_override = effective_model_override
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
            effective_logical_effort,
        )
        .await?;
        let service_tier_override = effective_service_tier_override;
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
                    persona_directive: persona_id
                        .as_ref()
                        .map(|persona_id| PersonaDirective::Explicit(persona_id.clone()))
                        .unwrap_or_default(),
                    model_override,
                    logical_effort_override,
                    service_tier_override,
                    conversation_id_override: Some(conversation.id),
                    working_directory_override,
                    composer_project_references: input.composer_project_references.clone(),
                    composer_integration_references: input.composer_integration_references.clone(),
                    composer_artifact_references,
                    composer_selection_snapshot: input.composer_selection_snapshot.clone(),
                    team_intent: effective_team_intent,
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
