use serde::Deserialize;
use tauri::State;

use crate::application::agent_conversation_start_service::AgentWorkspaceSourcePullRequestInput;
use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace_with_setup_mode_and_defaults,
    AgentConversationWorkspaceBaseSelection, AgentConversationWorkspacePrAutomationDefaults,
    AgentConversationWorkspaceSetupMode,
};
pub(crate) use crate::application::automation::actions::{
    retry_automation_judge_for_state, retry_automation_plan_judge_for_state,
    trigger_automation_run_now_for_state,
};
use crate::application::automation::api::{
    automation_detail_response_for_state, automation_run_response_for_state,
    automation_service_for_state, AutomationDetailResponse, AutomationResponse,
    AutomationRunResponse, AutomationScheduleResponse, CreateAutomationDraftResponse,
};
use crate::application::automation::decomposition_verifier::AutomationAuthoringMode;
use crate::application::automation::delete::{
    delete_automation_run_with_archive, delete_automation_with_archive,
};
use crate::application::automation::reopen::reopen_automation_run;
use crate::application::automation::resume_orchestrator::resume_automation_smart;
use crate::application::automation::service::{
    AutomationService, CreateAutomationDraftInput as ServiceCreateDraftInput,
    UpdateAutomationSettingsInput as ServiceUpdateSettingsInput,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceBranchMode, AgentConversationWorkspaceMode, AutomationId,
    AutomationPlanApprovalMode, AutomationPrMergeMode, AutomationRunId, ChatConversation,
    IdeationAnalysisBaseRefKind, ProjectId,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAutomationsInput {
    #[serde(default)]
    pub project_id: Option<String>,
}

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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationIdInput {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAutomationSettingsInput {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub max_runs: Option<i64>,
    #[serde(default)]
    pub max_consecutive_failures: Option<i64>,
    #[serde(default)]
    pub plan_approval_mode: Option<String>,
    #[serde(default)]
    pub pr_merge_mode: Option<String>,
    #[serde(default)]
    pub plan_deep_verification: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseAutomationInput {
    pub id: String,
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub reason_detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRunScopedInput {
    pub id: String,
    pub run_id: String,
}

#[tauri::command]
pub async fn list_automations(
    input: Option<ListAutomationsInput>,
    state: State<'_, AppState>,
) -> Result<Vec<AutomationResponse>, String> {
    let project_id = input
        .and_then(|input| trim_optional(input.project_id))
        .map(ProjectId::from_string);
    automation_service(&state)
        .list_automations(project_id)
        .await
        .map(|automations| {
            automations
                .into_iter()
                .map(AutomationResponse::from)
                .collect()
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_automation(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<AutomationDetailResponse, String> {
    let id = parse_automation_id(&input.id)?;
    let detail = automation_service(&state)
        .get_automation_detail(&id)
        .await
        .map_err(|error| error.to_string())?;
    automation_detail_response_for_state(detail, &state)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_automation_draft(
    input: CreateAutomationDraftInput,
    state: State<'_, AppState>,
) -> Result<CreateAutomationDraftResponse, String> {
    create_automation_draft_for_state(input, &state).await
}

pub(crate) async fn create_automation_draft_for_state(
    input: CreateAutomationDraftInput,
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
    let automation_id = AutomationId::new();
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

    let result = automation_service(state)
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

#[tauri::command]
pub async fn update_automation_settings(
    input: UpdateAutomationSettingsInput,
    state: State<'_, AppState>,
) -> Result<AutomationResponse, String> {
    let id = parse_automation_id(&input.id)?;
    let plan_approval_mode = parse_plan_approval_mode(input.plan_approval_mode)?;
    let pr_merge_mode = parse_pr_merge_mode(input.pr_merge_mode)?;
    automation_service(&state)
        .update_settings(ServiceUpdateSettingsInput {
            id,
            name: input.name,
            max_runs: input.max_runs,
            max_consecutive_failures: input.max_consecutive_failures,
            plan_approval_mode,
            pr_merge_mode,
            plan_deep_verification: input.plan_deep_verification,
        })
        .await
        .map(AutomationResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn pause_automation(
    input: PauseAutomationInput,
    state: State<'_, AppState>,
) -> Result<AutomationResponse, String> {
    let id = parse_automation_id(&input.id)?;
    let reason_code = input.reason_code.as_deref().unwrap_or("user_paused");
    automation_service(&state)
        .pause(&id, reason_code, input.reason_detail.as_deref())
        .await
        .map(AutomationResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn resume_automation(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<AutomationResponse, String> {
    let id = parse_automation_id(&input.id)?;
    resume_automation_smart(&state, &id)
        .await
        .map(AutomationResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn finalize_automation(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<AutomationResponse, String> {
    let id = parse_automation_id(&input.id)?;
    automation_service(&state)
        .finalize(&id)
        .await
        .map(AutomationResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_automation(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<AutomationResponse, String> {
    let id = parse_automation_id(&input.id)?;
    automation_service(&state)
        .stop(&id)
        .await
        .map(AutomationResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn restart_automation(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<AutomationScheduleResponse, String> {
    let id = parse_automation_id(&input.id)?;
    automation_service(&state)
        .restart(&id)
        .await
        .map(AutomationScheduleResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn trigger_automation_run_now(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<AutomationScheduleResponse, String> {
    let id = parse_automation_id(&input.id)?;
    trigger_automation_run_now_for_state(&id, &state)
        .await
        .map(AutomationScheduleResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn retry_automation_judge(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<AutomationScheduleResponse, String> {
    let id = parse_automation_id(&input.id)?;
    retry_automation_judge_for_state(&id, &state)
        .await
        .map(AutomationScheduleResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn retry_automation_plan_judge(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<AutomationScheduleResponse, String> {
    let id = parse_automation_id(&input.id)?;
    retry_automation_plan_judge_for_state(&id, &state)
        .await
        .map(AutomationScheduleResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn skip_automation_judge(
    input: AutomationRunScopedInput,
    state: State<'_, AppState>,
) -> Result<AutomationScheduleResponse, String> {
    let id = parse_automation_id(&input.id)?;
    let run_id = parse_automation_run_id(&input.run_id)?;
    automation_service(&state)
        .skip_judge(&id, &run_id)
        .await
        .map(AutomationScheduleResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cancel_automation_run(
    input: AutomationRunScopedInput,
    state: State<'_, AppState>,
) -> Result<AutomationRunResponse, String> {
    let id = parse_automation_id(&input.id)?;
    let run_id = parse_automation_run_id(&input.run_id)?;
    let run = automation_service(&state)
        .cancel_run(&id, &run_id)
        .await
        .map_err(|error| error.to_string())?;
    automation_run_response_for_state(run, state.inner())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_automation_run(
    input: AutomationRunScopedInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = parse_automation_id(&input.id)?;
    let run_id = parse_automation_run_id(&input.run_id)?;
    delete_automation_run_with_archive(&state, &id, &run_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn resume_automation_run(
    input: AutomationRunScopedInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = parse_automation_id(&input.id)?;
    let run_id = parse_automation_run_id(&input.run_id)?;
    reopen_automation_run(&state, &id, &run_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_automation(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = parse_automation_id(&input.id)?;
    delete_automation_with_archive(&state, &id)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) fn automation_service(state: &AppState) -> AutomationService {
    automation_service_for_state(state)
}

pub(crate) fn parse_automation_id(value: &str) -> Result<AutomationId, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("automation id is required".to_string());
    }
    Ok(AutomationId::from_string(trimmed.to_string()))
}

pub(crate) fn parse_automation_run_id(value: &str) -> Result<AutomationRunId, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("automation run id is required".to_string());
    }
    Ok(AutomationRunId::from_string(trimmed.to_string()))
}

pub(crate) fn parse_project_id(value: &str) -> Result<ProjectId, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("project id is required".to_string());
    }
    Ok(ProjectId::from_string(trimmed.to_string()))
}

fn parse_plan_approval_mode(
    value: Option<String>,
) -> Result<Option<AutomationPlanApprovalMode>, String> {
    value
        .map(|value| {
            let trimmed = value.trim();
            AutomationPlanApprovalMode::parse(trimmed)
                .ok_or_else(|| format!("invalid planApprovalMode: {trimmed}"))
        })
        .transpose()
}

fn parse_pr_merge_mode(value: Option<String>) -> Result<Option<AutomationPrMergeMode>, String> {
    value
        .map(|value| {
            let trimmed = value.trim();
            AutomationPrMergeMode::parse(trimmed)
                .ok_or_else(|| format!("invalid prMergeMode: {trimmed}"))
        })
        .transpose()
}

pub(crate) fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
