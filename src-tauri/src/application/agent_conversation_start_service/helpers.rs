use std::{path::Path, time::Instant};

use serde::Serialize;
use tauri::{Emitter, Runtime};

use super::AgentWorkspaceSourcePullRequestInput;
use crate::application::agent_conversation_workspace::AgentConversationWorkspacePrAutomationDefaults;
use crate::application::agent_planning_session_titles::hydrate_agent_conversation_planning_session_title;
use crate::application::ideation_workspace::prepare_ideation_analysis_state_from_agent_workspace;
use crate::application::AppState;
use crate::domain::agents::{
    default_effort_for_provider, default_efforts_for_provider, AgentHarnessKind,
    AgentModelRegistrySnapshot, LogicalEffort,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationAnalysisState, IdeationSession, IdeationSessionFlow,
    Project, ProjectId,
};
use crate::domain::services::ComposerIntegrationReference;

pub(crate) fn parse_agent_workspace_mode(
    mode: Option<&str>,
) -> Result<AgentConversationWorkspaceMode, String> {
    mode.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("edit")
        .parse::<AgentConversationWorkspaceMode>()
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TicketStartBaseReference {
    pub provider: String,
    pub issue_key: String,
}

pub(crate) fn first_ticket_start_base_reference(
    references: &[ComposerIntegrationReference],
) -> Option<TicketStartBaseReference> {
    references
        .iter()
        .find_map(ticket_start_base_reference_from_composer_reference)
}

fn ticket_start_base_reference_from_composer_reference(
    reference: &ComposerIntegrationReference,
) -> Option<TicketStartBaseReference> {
    let provider = match (
        reference.provider.trim().to_ascii_lowercase().as_str(),
        reference.kind.trim().to_ascii_lowercase().as_str(),
    ) {
        ("atlassian", "jira") | ("jira", "jira") => "jira",
        ("linear", "linear") => "linear",
        ("clickup", "clickup") => "clickup",
        _ => return None,
    };
    let issue_key = reference
        .key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| reference.id.trim());
    if issue_key.is_empty() {
        return None;
    }
    Some(TicketStartBaseReference {
        provider: provider.to_string(),
        issue_key: issue_key.to_string(),
    })
}

pub(crate) fn base_selection_allows_ticket_canonical_branch(
    base_ref_kind: Option<IdeationAnalysisBaseRefKind>,
    source_pull_request: Option<&AgentWorkspaceSourcePullRequest>,
) -> bool {
    source_pull_request.is_none()
        && matches!(
            base_ref_kind,
            None | Some(IdeationAnalysisBaseRefKind::ProjectDefault)
        )
}

pub(crate) fn apply_ticket_canonical_branch_base_selection(
    base_ref_kind: &mut Option<IdeationAnalysisBaseRefKind>,
    base_ref: &mut Option<String>,
    base_display_name: &mut Option<String>,
    issue_key: &str,
    canonical_branch_name: &str,
) {
    *base_ref_kind = Some(IdeationAnalysisBaseRefKind::LocalBranch);
    *base_ref = Some(canonical_branch_name.to_string());
    *base_display_name = Some(format!("Ticket {issue_key} ({canonical_branch_name})"));
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
) -> bool {
    agent_mode_requires_workspace(mode)
        || (mode == AgentConversationWorkspaceMode::Chat && source_pull_request.is_some())
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
    if let Some(conflict) = active_workspaces.into_iter().find(|workspace| {
        current_conversation_id != Some(&workspace.conversation_id)
    }) {
        return Err(format!(
            "Selected branch '{}' is already linked to active conversation {}; choose isolated branch mode or continue in that conversation",
            branch_name, conflict.conversation_id
        ));
    }

    Ok(())
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
    let analysis = prepare_ideation_analysis_state_from_agent_workspace(project, workspace)
        .await
        .map_err(|error| error.to_string())?;
    ensure_plan_workspace_planning_session_link_with_analysis(state, workspace, analysis).await
}

pub(crate) async fn ensure_plan_workspace_planning_session_link_with_analysis(
    state: &AppState,
    workspace: &mut AgentConversationWorkspace,
    analysis: IdeationAnalysisState,
) -> Result<bool, String> {
    if workspace.mode != AgentConversationWorkspaceMode::Plan {
        return Ok(false);
    }

    if linked_ideation_session_is_planning(state, workspace).await? {
        return Ok(false);
    }

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
