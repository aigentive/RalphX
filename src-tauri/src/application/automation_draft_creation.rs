use serde::Deserialize;

use crate::application::agent_conversation_start_service::AgentWorkspaceSourcePullRequestInput;
use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace_with_setup_mode_and_defaults,
    AgentConversationWorkspaceBaseSelection, AgentConversationWorkspacePrAutomationDefaults,
    AgentConversationWorkspaceSetupMode,
};
use crate::application::automation::api::{
    automation_service_for_state, CreateAutomationDraftResponse,
};
use crate::application::automation::decomposition_verifier::AutomationAuthoringMode;
use crate::application::automation::service::CreateAutomationDraftInput as ServiceCreateDraftInput;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceBranchMode, AgentConversationWorkspaceMode, AutomationId,
    ChatConversation, IdeationAnalysisBaseRefKind, ProjectId,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAutomationDraftInput {
    pub project_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub authoring_mode: Option<String>,
    #[serde(default)]
    pub base_ref_kind: Option<String>,
    #[serde(default)]
    pub base_branch_mode: Option<String>,
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub base_display_name: Option<String>,
    #[serde(default)]
    pub base_source_pull_request: Option<AgentWorkspaceSourcePullRequestInput>,
}

pub async fn create_automation_draft_for_state(
    input: CreateAutomationDraftInput,
    state: &AppState,
) -> Result<CreateAutomationDraftResponse, String> {
    create_automation_draft_with_id_for_state(input, None, state).await
}

pub async fn create_automation_draft_with_id_for_state(
    input: CreateAutomationDraftInput,
    automation_id: Option<AutomationId>,
    state: &AppState,
) -> Result<CreateAutomationDraftResponse, String> {
    let project_id = parse_project_id(&input.project_id)?;
    let authoring_mode = input
        .authoring_mode
        .as_deref()
        .map(|value| {
            AutomationAuthoringMode::parse(value)
                .ok_or_else(|| format!("invalid automation authoring mode: {value}"))
        })
        .transpose()?;
    if input
        .name
        .as_deref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err("automation name cannot be empty".to_string());
    }
    let base_ref_kind = input
        .base_ref_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<IdeationAnalysisBaseRefKind>)
        .transpose()?;
    let base_branch_mode = input
        .base_branch_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<AgentConversationWorkspaceBranchMode>)
        .transpose()?;
    let base_ref = trim_optional(input.base_ref);
    let base_display_name = trim_optional(input.base_display_name);
    let base_source_pull_request = input
        .base_source_pull_request
        .map(|pull_request| pull_request.normalize(base_ref_kind, base_ref.as_deref()))
        .transpose()?;
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", project_id))?;
    let automation_id = automation_id.unwrap_or_else(AutomationId::new);
    let mut setup_conversation = ChatConversation::new_project(project_id.clone());
    let setup_title = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Automation setup");
    setup_conversation.set_title(setup_title.to_string());
    setup_conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Automation));
    setup_conversation.automation_id = Some(automation_id.clone());
    let setup_conversation = state
        .chat_conversation_repo
        .create(setup_conversation)
        .await
        .map_err(|error| error.to_string())?;
    let setup_conversation_id = setup_conversation.id;
    let setup_workspace = match prepare_agent_conversation_workspace_with_setup_mode_and_defaults(
        &project,
        &setup_conversation_id,
        AgentConversationWorkspaceMode::Automation,
        AgentConversationWorkspaceBaseSelection {
            kind: base_ref_kind.or(Some(IdeationAnalysisBaseRefKind::ProjectDefault)),
            branch_mode: base_branch_mode.or(Some(AgentConversationWorkspaceBranchMode::Isolated)),
            base_ref,
            display_name: base_display_name,
            source_pull_request: base_source_pull_request,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
        AgentConversationWorkspacePrAutomationDefaults::default(),
        false,
    )
    .await
    {
        Ok(workspace) => workspace,
        Err(error) => {
            let _ = state
                .chat_conversation_repo
                .delete(&setup_conversation_id)
                .await;
            return Err(error.to_string());
        }
    };
    let setup_branch = setup_workspace.branch_name.clone();
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .create_or_update(setup_workspace)
        .await
    {
        let _ = state
            .chat_conversation_repo
            .delete(&setup_conversation_id)
            .await;
        return Err(error.to_string());
    }
    let result = automation_service_for_state(state)
        .create_draft(ServiceCreateDraftInput {
            id: Some(automation_id),
            project_id,
            name: input.name,
            setup_conversation_id: Some(setup_conversation_id),
            base_ref_kind: Some(IdeationAnalysisBaseRefKind::LocalBranch.to_string()),
            base_ref: Some(setup_branch.clone()),
            base_display_name: Some(format!("Automation branch ({setup_branch})")),
            authoring_mode,
        })
        .await;
    match result {
        Ok(automation) => Ok(CreateAutomationDraftResponse::from(automation)),
        Err(error) => {
            let _ = state
                .chat_conversation_repo
                .delete(&setup_conversation_id)
                .await;
            Err(error.to_string())
        }
    }
}

fn parse_project_id(value: &str) -> Result<ProjectId, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("project id cannot be empty".to_string());
    }
    Ok(ProjectId::from_string(value.to_string()))
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
