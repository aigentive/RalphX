use std::time::Instant;

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
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow, Project,
    ProjectId,
};

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
