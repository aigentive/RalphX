use chrono::{DateTime, Utc};
use serde::Deserialize;
use tauri::State;

pub(crate) use crate::application::automation::actions::{
    retry_automation_judge_for_state, retry_automation_plan_judge_for_state,
    trigger_automation_run_now_for_state,
};
use crate::application::automation::api::{
    automation_detail_response_for_state, automation_run_response_for_state,
    automation_service_for_state, AutomationDetailResponse, AutomationResponse,
    AutomationRunResponse, AutomationScheduleResponse, CreateAutomationDraftResponse,
};
use crate::application::automation::delete::{
    delete_automation_run_with_archive, delete_automation_with_archive,
};
use crate::application::automation::reopen::reopen_automation_run;
use crate::application::automation::resume_orchestrator::resume_automation_smart;
use crate::application::automation::service::{
    AutomationService, UpdateAutomationConfigInput as ServiceUpdateConfigInput,
    UpdateAutomationSettingsInput as ServiceUpdateSettingsInput,
};
pub use crate::application::automation_draft_creation::{
    create_automation_draft_for_state, CreateAutomationDraftInput,
};
use crate::application::AppState;
use crate::domain::entities::{
    AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode, AutomationRunId, ProjectId,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAutomationsInput {
    #[serde(default)]
    pub project_id: Option<String>,
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

pub const REMOTE_AUTOMATION_CONFIG_LOOKUP_FAILED: &str = "REMOTE_AUTOMATION_CONFIG_LOOKUP_FAILED";
pub const REMOTE_AUTOMATION_CONFIG_NOT_FOUND: &str = "REMOTE_AUTOMATION_CONFIG_NOT_FOUND";
pub const REMOTE_AUTOMATION_CONFIG_VERSION_CONFLICT: &str =
    "REMOTE_AUTOMATION_CONFIG_VERSION_CONFLICT";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAutomationSettingsPatchInput {
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAutomationConfigPatchInput {
    #[serde(default)]
    pub goal_prompt: Option<String>,
    #[serde(default)]
    pub first_run_prompt: Option<String>,
    #[serde(default)]
    pub provider_harness: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub logical_effort: Option<String>,
    #[serde(default)]
    pub run_mode: Option<String>,
    #[serde(default)]
    pub base_ref_kind: Option<String>,
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub base_display_name: Option<String>,
    #[serde(default)]
    pub goal_items_json: Option<String>,
    #[serde(default)]
    pub chain_mode: Option<String>,
    #[serde(default)]
    pub completion_signal: Option<String>,
    #[serde(default)]
    pub setup_analysis_summary: Option<String>,
    #[serde(default)]
    pub spec_artifact_id: Option<String>,
    #[serde(default)]
    pub spec_content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAutomationConfigInput {
    pub automation_id: String,
    pub expected_updated_at: DateTime<Utc>,
    #[serde(default)]
    pub settings: Option<UpdateAutomationSettingsPatchInput>,
    #[serde(default)]
    pub config: Option<UpdateAutomationConfigPatchInput>,
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
pub async fn update_automation_config(
    input: UpdateAutomationConfigInput,
    state: State<'_, AppState>,
) -> Result<AutomationResponse, String> {
    update_automation_config_for_state(&state, input).await
}

#[doc(hidden)]
pub async fn update_automation_config_for_state(
    state: &AppState,
    input: UpdateAutomationConfigInput,
) -> Result<AutomationResponse, String> {
    let id = parse_automation_id(&input.automation_id)?;
    let current = state
        .automation_repo
        .get_by_id(&id)
        .await
        .map_err(|_| REMOTE_AUTOMATION_CONFIG_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_AUTOMATION_CONFIG_NOT_FOUND.to_string())?;
    if current.updated_at != input.expected_updated_at {
        return Err(REMOTE_AUTOMATION_CONFIG_VERSION_CONFLICT.to_string());
    }

    let service = automation_service(state);
    let mut updated = current;
    if let Some(settings) = input.settings {
        let plan_approval_mode = parse_plan_approval_mode(settings.plan_approval_mode)?;
        let pr_merge_mode = parse_pr_merge_mode(settings.pr_merge_mode)?;
        updated = service
            .update_settings(ServiceUpdateSettingsInput {
                id: id.clone(),
                name: settings.name,
                max_runs: settings.max_runs,
                max_consecutive_failures: settings.max_consecutive_failures,
                plan_approval_mode,
                pr_merge_mode,
                plan_deep_verification: settings.plan_deep_verification,
            })
            .await
            .map_err(|error| error.to_string())?;
    }
    if let Some(config) = input.config {
        updated = service
            .update_config(ServiceUpdateConfigInput {
                id,
                goal_prompt: config.goal_prompt,
                first_run_prompt: config.first_run_prompt,
                provider_harness: config.provider_harness,
                model_id: config.model_id,
                logical_effort: config.logical_effort,
                run_mode: config.run_mode,
                base_ref_kind: config.base_ref_kind,
                base_ref: config.base_ref,
                base_display_name: config.base_display_name,
                goal_items_json: config.goal_items_json,
                chain_mode: config.chain_mode,
                completion_signal: config.completion_signal,
                setup_analysis_summary: config.setup_analysis_summary,
                spec_artifact_id: config.spec_artifact_id,
                spec_content: config.spec_content,
            })
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(AutomationResponse::from(updated))
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

#[doc(hidden)]
pub fn parse_project_id(value: &str) -> Result<ProjectId, String> {
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
