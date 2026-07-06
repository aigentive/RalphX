use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::application::automation::api::{
    automation_detail_response_for_state, automation_service_for_state, AutomationDetailResponse,
    AutomationResponse, AutomationRunResponse, AutomationScheduleResponse,
    CreateAutomationDraftResponse,
};
use crate::application::automation::scheduler::{
    automation_judge_lease_expires_at, spawn_automation_judge_task, AutomationSchedulerConfig,
    HarnessAutomationJudgeInvoker,
};
use crate::application::automation::service::{
    AutomationRunNowAction, AutomationScheduleOutcome, AutomationService,
    CreateAutomationDraftInput as ServiceCreateDraftInput,
    UpdateAutomationSettingsInput as ServiceUpdateSettingsInput,
};
use crate::application::automation::transition::{
    AutomationTransitionService, NoopAutomationEventEmitter,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AutomationId, AutomationJudgeState, AutomationRunId,
    ChatConversation, ProjectId,
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
    let automation_id = AutomationId::new();
    let mut setup_conversation = ChatConversation::new_project(project_id.clone());
    setup_conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Automation));
    setup_conversation.automation_id = Some(automation_id.clone());
    let setup_conversation = state
        .chat_conversation_repo
        .create(setup_conversation)
        .await
        .map_err(|error| error.to_string())?;

    let setup_conversation_id = setup_conversation.id;
    let result = automation_service(state)
        .create_draft(ServiceCreateDraftInput {
            id: Some(automation_id),
            project_id,
            name: input.name,
            setup_conversation_id: Some(setup_conversation_id),
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
    automation_service(&state)
        .update_settings(ServiceUpdateSettingsInput {
            id,
            name: input.name,
            max_runs: input.max_runs,
            max_consecutive_failures: input.max_consecutive_failures,
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
    automation_service(&state)
        .resume(&id)
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

pub(crate) async fn trigger_automation_run_now_for_state(
    id: &AutomationId,
    state: &AppState,
) -> crate::error::AppResult<AutomationScheduleOutcome> {
    let service = automation_service(state);
    match service.trigger_run_now_action(id).await? {
        AutomationRunNowAction::Outcome(outcome) => Ok(outcome),
        AutomationRunNowAction::StartJudge {
            automation,
            runs,
            run,
        } => {
            let automation = *automation;
            let run = *run;
            let config = AutomationSchedulerConfig::default();
            let transition_service = automation_transition_service(state);
            let changed = transition_service
                .transition_judge_state(
                    &run.id,
                    run.judge_state,
                    AutomationJudgeState::InProgress,
                    None,
                    None,
                    Some(automation_judge_lease_expires_at(config.judge_timeout)),
                    None,
                )
                .await?;
            if !changed {
                return Ok(AutomationScheduleOutcome {
                    scheduled: false,
                    reason: Some("run in flight".to_string()),
                });
            }
            spawn_automation_judge_task(
                service,
                transition_service,
                Arc::new(HarnessAutomationJudgeInvoker::new(state.clone())),
                config,
                automation,
                runs,
                run,
            );
            Ok(AutomationScheduleOutcome {
                scheduled: true,
                reason: None,
            })
        }
    }
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
    automation_service(&state)
        .cancel_run(&id, &run_id)
        .await
        .map(AutomationRunResponse::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_automation(
    input: AutomationIdInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = parse_automation_id(&input.id)?;
    automation_service(&state)
        .delete(&id)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) fn automation_service(state: &AppState) -> AutomationService {
    automation_service_for_state(state)
}

fn automation_transition_service(state: &AppState) -> AutomationTransitionService {
    AutomationTransitionService::new(
        state.automation_repo.clone(),
        state.automation_run_repo.clone(),
        Arc::new(NoopAutomationEventEmitter),
    )
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

pub(crate) fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
