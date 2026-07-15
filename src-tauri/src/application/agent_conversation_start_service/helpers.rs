use std::{path::Path, time::Instant};

use serde::Serialize;
use tauri::{Emitter, Runtime};

use super::AgentWorkspaceSourcePullRequestInput;
use crate::application::agent_conversation_workspace::{
    reject_persona_builder_workspace_mode, AgentConversationWorkspaceBranchNameHint,
    AgentConversationWorkspacePrAutomationDefaults,
};
use crate::application::agent_planning_session_titles::hydrate_agent_conversation_planning_session_title;
use crate::application::ideation_workspace::prepare_ideation_analysis_state_from_agent_workspace;
use crate::application::AppState;
use crate::domain::agents::{
    default_effort_for_provider, default_efforts_for_provider, AgentHarnessKind,
    AgentModelRegistrySnapshot, LogicalEffort,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceSourcePullRequest, ChatContextType,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession,
    IdeationSessionFlow, Project, ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::domain::services::ComposerIntegrationReference;

pub(crate) fn parse_agent_workspace_mode(
    mode: Option<&str>,
) -> Result<AgentConversationWorkspaceMode, String> {
    let mode = mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("edit");
    reject_persona_builder_workspace_mode(mode)?;
    mode.parse::<AgentConversationWorkspaceMode>()
}

pub(crate) fn parse_agent_workspace_base_kind(
    kind: Option<&str>,
) -> Result<Option<IdeationAnalysisBaseRefKind>, String> {
    kind.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<IdeationAnalysisBaseRefKind>)
        .transpose()
}

pub(crate) fn parse_agent_workspace_branch_mode(
    branch_mode: Option<&str>,
) -> Result<Option<AgentConversationWorkspaceBranchMode>, String> {
    branch_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<AgentConversationWorkspaceBranchMode>)
        .transpose()
}

pub(crate) fn trim_optional_input(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn normalize_agent_workspace_source_pull_request(
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

pub(crate) fn first_ticket_branch_name_hint(
    references: &[ComposerIntegrationReference],
) -> Option<AgentConversationWorkspaceBranchNameHint> {
    references
        .iter()
        .find_map(ticket_branch_name_hint_from_composer_reference)
}

fn ticket_branch_name_hint_from_composer_reference(
    reference: &ComposerIntegrationReference,
) -> Option<AgentConversationWorkspaceBranchNameHint> {
    let provider = match (
        reference.provider.trim().to_ascii_lowercase().as_str(),
        reference.kind.trim().to_ascii_lowercase().as_str(),
    ) {
        ("atlassian", "jira") | ("jira", "jira") => "jira",
        ("linear", "linear") => "linear",
        ("clickup", "clickup") => "clickup",
        _ => return None,
    };
    let ticket_token = reference
        .key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| ticket_id_fallback_token(provider, reference.id.trim()))?;

    Some(AgentConversationWorkspaceBranchNameHint {
        provider: provider.to_string(),
        ticket_token,
    })
}

fn ticket_id_fallback_token(provider: &str, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    if provider == "clickup" && !id.to_ascii_uppercase().starts_with("CU-") {
        return Some(format!("CU-{id}"));
    }
    Some(id.to_string())
}

pub(crate) fn agent_mode_requires_workspace(mode: AgentConversationWorkspaceMode) -> bool {
    matches!(
        mode,
        AgentConversationWorkspaceMode::Edit
            | AgentConversationWorkspaceMode::Plan
            | AgentConversationWorkspaceMode::Ideation
            | AgentConversationWorkspaceMode::ReviewPr
    )
}

pub(crate) fn agent_mode_should_create_workspace(
    mode: AgentConversationWorkspaceMode,
    source_pull_request: Option<&AgentWorkspaceSourcePullRequest>,
    has_plan_reference: bool,
) -> bool {
    if matches!(
        mode,
        AgentConversationWorkspaceMode::Automation | AgentConversationWorkspaceMode::PersonaBuilder
    ) {
        return false;
    }
    agent_mode_requires_workspace(mode)
        || (mode == AgentConversationWorkspaceMode::Chat
            && (source_pull_request.is_some() || has_plan_reference))
}

pub(crate) fn review_pr_monitor_for_workspace(
    workspace: &AgentConversationWorkspace,
) -> Result<Option<AgentWorkspacePrReviewMonitor>, String> {
    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr {
        return Ok(None);
    }

    let pr_number = workspace
        .source_pull_request
        .as_ref()
        .map(|pull_request| pull_request.number)
        .or(workspace.publication_pr_number)
        .ok_or_else(|| "Review PR workspace requires a linked pull request".to_string())?;
    let head_sha = workspace
        .source_pull_request
        .as_ref()
        .and_then(|pull_request| pull_request.head_ref_oid.clone());
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
        pr_number,
        head_sha,
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    Ok(Some(monitor))
}

pub(crate) async fn ensure_review_pr_monitor_for_workspace(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    workspace: Option<&AgentConversationWorkspace>,
) -> Result<(), String> {
    let Some(monitor) = workspace
        .map(review_pr_monitor_for_workspace)
        .transpose()?
        .flatten()
    else {
        return Ok(());
    };

    if workspace_repo
        .get_pr_review_monitor(&monitor.conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .is_none()
    {
        workspace_repo
            .upsert_pr_review_monitor(monitor)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) async fn ensure_linked_branch_workspace_available(
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

pub(crate) const LINKED_SETUP_FAILURE_MARKER: &str = "[ralphx:linked_setup_failure]";

pub(crate) fn linked_setup_failure_error(message: impl AsRef<str>) -> String {
    let message = message.as_ref().trim();
    if message.contains(LINKED_SETUP_FAILURE_MARKER) {
        return message.to_string();
    }
    if message.is_empty() {
        LINKED_SETUP_FAILURE_MARKER.to_string()
    } else {
        format!("{LINKED_SETUP_FAILURE_MARKER} {message}")
    }
}

pub(crate) async fn archive_empty_seeded_draft_after_setup_failure(
    state: &AppState,
    conversation: &ChatConversation,
) -> Result<bool, String> {
    if conversation.context_type != ChatContextType::Project
        || conversation.message_count != 0
        || conversation.archived_at.is_some()
        || conversation.provider_session_ref().is_some()
    {
        return Ok(false);
    }

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .map_err(|error| error.to_string())?;
    if workspace.is_some() {
        return Ok(false);
    }

    state
        .chat_conversation_repo
        .archive(&conversation.id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(true)
}

pub(crate) async fn archive_supplied_seeded_draft_after_setup_failure(
    state: &AppState,
    project_id: &str,
    conversation_id: &ChatConversationId,
) -> Result<bool, String> {
    let lookup = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|error| error.to_string());

    archive_seeded_draft_lookup_after_setup_failure(state, project_id, lookup).await
}

pub(crate) async fn archive_seeded_draft_lookup_after_setup_failure(
    state: &AppState,
    project_id: &str,
    conversation: Result<Option<ChatConversation>, String>,
) -> Result<bool, String> {
    let Some(conversation) = conversation? else {
        return Ok(false);
    };
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != project_id
    {
        return Ok(false);
    }

    archive_empty_seeded_draft_after_setup_failure(state, &conversation).await
}

pub(crate) async fn hydrate_linked_branch_source_pull_request(
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
                "Linked branch PR lookup failed; continuing without PR linkage"
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

pub(crate) async fn ensure_plan_workspace_planning_session_link(
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

pub(super) async fn agent_workspace_pr_automation_defaults_for_project(
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

fn normalized_effort_for_supported(
    requested: Option<LogicalEffort>,
    supported_efforts: &[LogicalEffort],
    default_effort: LogicalEffort,
) -> LogicalEffort {
    requested
        .filter(|effort| supported_efforts.contains(effort))
        .unwrap_or(default_effort)
}

pub(crate) async fn normalize_agent_runtime_selection(
    state: &AppState,
    provider: Option<AgentHarnessKind>,
    model_override: Option<String>,
    effort_override: Option<LogicalEffort>,
) -> Result<(Option<String>, Option<LogicalEffort>), String> {
    let Some(provider) = provider else {
        return Ok((model_override, effort_override));
    };

    let custom_models = state
        .agent_model_registry_repo
        .list_custom_models()
        .await
        .map_err(|error| format!("Failed to fetch custom agent models: {error}"))?;
    let snapshot = AgentModelRegistrySnapshot::merged(custom_models);
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

pub(super) fn log_start_agent_conversation_phase(
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

#[derive(Clone, Debug, Serialize)]
struct AgentStartupProgressPayload<'a> {
    conversation_id: String,
    context_type: &'static str,
    context_id: &'a str,
    stage: &'static str,
    label: &'static str,
}

pub(super) fn emit_start_agent_conversation_progress<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_id: &str,
    conversation_id: &ChatConversationId,
    stage: &'static str,
    label: &'static str,
) {
    let _ = app.emit(
        "agent:startup_progress",
        AgentStartupProgressPayload {
            conversation_id: conversation_id.as_str(),
            context_type: "project",
            context_id: project_id,
            stage,
            label,
        },
    );
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod tests;
