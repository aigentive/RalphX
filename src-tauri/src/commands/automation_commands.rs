use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::automation::scheduler::{
    automation_judge_lease_expires_at, spawn_automation_judge_task, AutomationSchedulerConfig,
    HarnessAutomationJudgeInvoker,
};
use crate::application::automation::service::{
    AutomationDetail, AutomationRunNowAction, AutomationScheduleOutcome, AutomationService,
    CreateAutomationDraftInput as ServiceCreateDraftInput,
    UpdateAutomationSettingsInput as ServiceUpdateSettingsInput,
};
use crate::application::automation::transition::{
    AutomationTransitionService, NoopAutomationEventEmitter,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, Automation, AutomationId, AutomationJudgeState, AutomationRun,
    AutomationRunId, ChatConversation, ProjectId,
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

#[derive(Debug, Clone, Serialize)]
pub struct AutomationResponse {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub status: String,
    pub paused_reason_code: Option<String>,
    pub paused_reason_detail: Option<String>,
    pub goal_prompt: String,
    pub setup_conversation_id: Option<String>,
    pub provider_harness: String,
    pub model_id: String,
    pub logical_effort: Option<String>,
    pub run_mode: String,
    pub base_ref_kind: String,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub base_source_pull_request_json: Option<String>,
    pub goal_items_json: Option<String>,
    pub chain_mode: String,
    pub completion_signal: String,
    pub max_runs: i64,
    pub max_consecutive_failures: i64,
    pub first_run_prompt: Option<String>,
    pub setup_analysis_summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationRunResponse {
    pub id: String,
    pub automation_id: String,
    pub run_index: i64,
    pub status: String,
    pub judge_state: String,
    pub judge_lease_expires_at: Option<String>,
    pub conversation_id: Option<String>,
    pub run_prompt: String,
    pub prompt_author: String,
    pub base_ref_kind: String,
    pub base_ref_used: String,
    pub base_from_run_id: Option<String>,
    pub branch_name: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
    pub pr_head_ref_name: Option<String>,
    pub pr_base_ref_name: Option<String>,
    pub pr_merged_at: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub diff_stats_json: Option<String>,
    pub agent_summary: Option<String>,
    pub judge_verdict_json: Option<String>,
    pub judge_model_id: Option<String>,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub signal_check_failures: i64,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationDetailResponse {
    pub automation: AutomationResponse,
    pub runs: Vec<AutomationRunResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateAutomationDraftResponse {
    pub automation: AutomationResponse,
    pub setup_conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationScheduleResponse {
    pub scheduled: bool,
    pub reason: Option<String>,
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
    automation_service(&state)
        .get_automation_detail(&id)
        .await
        .map(AutomationDetailResponse::from)
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
    AutomationService::new(
        state.automation_repo.clone(),
        state.automation_run_repo.clone(),
        Arc::new(NoopAutomationEventEmitter),
    )
}

fn automation_transition_service(state: &AppState) -> AutomationTransitionService {
    AutomationTransitionService::new(
        state.automation_repo.clone(),
        state.automation_run_repo.clone(),
        Arc::new(NoopAutomationEventEmitter),
    )
}

fn parse_automation_id(value: &str) -> Result<AutomationId, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("automation id is required".to_string());
    }
    Ok(AutomationId::from_string(trimmed.to_string()))
}

fn parse_automation_run_id(value: &str) -> Result<AutomationRunId, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("automation run id is required".to_string());
    }
    Ok(AutomationRunId::from_string(trimmed.to_string()))
}

fn parse_project_id(value: &str) -> Result<ProjectId, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("project id is required".to_string());
    }
    Ok(ProjectId::from_string(trimmed.to_string()))
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl From<Automation> for AutomationResponse {
    fn from(automation: Automation) -> Self {
        Self {
            id: automation.id.as_str().to_string(),
            project_id: automation.project_id.as_str().to_string(),
            name: automation.name,
            status: automation.status.as_str().to_string(),
            paused_reason_code: automation.paused_reason_code,
            paused_reason_detail: automation.paused_reason_detail,
            goal_prompt: automation.goal_prompt,
            setup_conversation_id: automation.setup_conversation_id.map(|id| id.as_str()),
            provider_harness: automation.provider_harness,
            model_id: automation.model_id,
            logical_effort: automation.logical_effort,
            run_mode: automation.run_mode,
            base_ref_kind: automation.base_ref_kind,
            base_ref: automation.base_ref,
            base_display_name: automation.base_display_name,
            base_source_pull_request_json: automation.base_source_pull_request_json,
            goal_items_json: automation.goal_items_json,
            chain_mode: automation.chain_mode,
            completion_signal: automation.completion_signal,
            max_runs: automation.max_runs,
            max_consecutive_failures: automation.max_consecutive_failures,
            first_run_prompt: automation.first_run_prompt,
            setup_analysis_summary: automation.setup_analysis_summary,
            created_at: automation.created_at.to_rfc3339(),
            updated_at: automation.updated_at.to_rfc3339(),
        }
    }
}

impl From<AutomationRun> for AutomationRunResponse {
    fn from(run: AutomationRun) -> Self {
        Self {
            id: run.id.as_str().to_string(),
            automation_id: run.automation_id.as_str().to_string(),
            run_index: run.run_index,
            status: run.status.as_str().to_string(),
            judge_state: run.judge_state.as_str().to_string(),
            judge_lease_expires_at: run.judge_lease_expires_at.map(|dt| dt.to_rfc3339()),
            conversation_id: run.conversation_id.map(|id| id.as_str()),
            run_prompt: run.run_prompt,
            prompt_author: run.prompt_author.as_str().to_string(),
            base_ref_kind: run.base_ref_kind,
            base_ref_used: run.base_ref_used,
            base_from_run_id: run.base_from_run_id.map(|id| id.as_str().to_string()),
            branch_name: run.branch_name,
            pr_number: run.pr_number,
            pr_url: run.pr_url,
            pr_title: run.pr_title,
            pr_head_ref_name: run.pr_head_ref_name,
            pr_base_ref_name: run.pr_base_ref_name,
            pr_merged_at: run.pr_merged_at.map(|dt| dt.to_rfc3339()),
            merge_commit_sha: run.merge_commit_sha,
            diff_stats_json: run.diff_stats_json,
            agent_summary: run.agent_summary,
            judge_verdict_json: run.judge_verdict_json,
            judge_model_id: run.judge_model_id,
            error_code: run.error_code,
            error_detail: run.error_detail,
            signal_check_failures: run.signal_check_failures,
            started_at: run.started_at.map(|dt| dt.to_rfc3339()),
            finished_at: run.finished_at.map(|dt| dt.to_rfc3339()),
            created_at: run.created_at.to_rfc3339(),
            updated_at: run.updated_at.to_rfc3339(),
        }
    }
}

impl From<AutomationDetail> for AutomationDetailResponse {
    fn from(detail: AutomationDetail) -> Self {
        Self {
            automation: AutomationResponse::from(detail.automation),
            runs: detail
                .runs
                .into_iter()
                .map(AutomationRunResponse::from)
                .collect(),
        }
    }
}

impl From<Automation> for CreateAutomationDraftResponse {
    fn from(automation: Automation) -> Self {
        let setup_conversation_id = automation
            .setup_conversation_id
            .as_ref()
            .map(|id| id.as_str());
        Self {
            automation: AutomationResponse::from(automation),
            setup_conversation_id,
        }
    }
}

impl From<AutomationScheduleOutcome> for AutomationScheduleResponse {
    fn from(outcome: AutomationScheduleOutcome) -> Self {
        Self {
            scheduled: outcome.scheduled,
            reason: outcome.reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, AutomationJudgeState, AutomationPromptAuthor,
        AutomationRunId, AutomationRunStatus, AutomationStatus, ChatContextType,
        ChatConversationId, ProjectId,
    };

    fn automation() -> Automation {
        let now = Utc::now();
        Automation {
            id: AutomationId::from_string("automation-1"),
            project_id: ProjectId::from_string("project-1".to_string()),
            name: "Automation 1".to_string(),
            status: AutomationStatus::Draft,
            paused_reason_code: None,
            paused_reason_detail: None,
            goal_prompt: "Goal".to_string(),
            setup_conversation_id: None,
            provider_harness: "claude".to_string(),
            model_id: "sonnet".to_string(),
            logical_effort: None,
            run_mode: "edit".to_string(),
            base_ref_kind: "project_default".to_string(),
            base_ref: String::new(),
            base_display_name: None,
            base_source_pull_request_json: None,
            goal_items_json: None,
            chain_mode: "merged_base".to_string(),
            completion_signal: "pr_merged".to_string(),
            max_runs: 25,
            max_consecutive_failures: 3,
            first_run_prompt: Some("Run 1".to_string()),
            setup_analysis_summary: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn automation_run(automation_id: &AutomationId) -> AutomationRun {
        let now = Utc::now();
        AutomationRun {
            id: AutomationRunId::from_string("run-1"),
            automation_id: automation_id.clone(),
            run_index: 1,
            status: AutomationRunStatus::Merged,
            judge_state: AutomationJudgeState::Done,
            judge_lease_expires_at: None,
            conversation_id: None,
            run_prompt: "Run 1 prompt".to_string(),
            prompt_author: AutomationPromptAuthor::SetupAgent,
            base_ref_kind: "project_default".to_string(),
            base_ref_used: String::new(),
            base_from_run_id: None,
            branch_name: None,
            pr_number: Some(593),
            pr_url: None,
            pr_title: None,
            pr_head_ref_name: None,
            pr_base_ref_name: Some("main".to_string()),
            pr_merged_at: None,
            merge_commit_sha: None,
            diff_stats_json: None,
            agent_summary: None,
            judge_verdict_json: Some(continue_verdict(
                "Implement the next automation item with focused tests and publish the follow-up PR.",
            )),
            judge_model_id: Some("haiku".to_string()),
            error_code: None,
            error_detail: None,
            signal_check_failures: 0,
            started_at: Some(now),
            finished_at: Some(now),
            created_at: now,
            updated_at: now,
        }
    }

    fn continue_verdict(next_prompt: &str) -> String {
        json!({
            "decision": "continue",
            "goalMet": false,
            "reason": "The next item remains and should be implemented in a scoped PR.",
            "confidence": 0.87,
            "goalProgress": { "completedItems": 1, "totalItems": 2, "summary": "One item complete." },
            "updatedItemStatuses": null,
            "nextRunPrompt": next_prompt,
            "nextBaseBranch": "automation_base"
        })
        .to_string()
    }

    #[test]
    fn command_inputs_accept_camel_case_wrapped_payloads() {
        let input: UpdateAutomationSettingsInput = serde_json::from_value(json!({
            "id": "automation-1",
            "maxRuns": 12,
            "maxConsecutiveFailures": 4
        }))
        .unwrap();

        assert_eq!(input.max_runs, Some(12));
        assert_eq!(input.max_consecutive_failures, Some(4));

        let run_input: AutomationRunScopedInput = serde_json::from_value(json!({
            "id": "automation-1",
            "runId": "automation-run-1"
        }))
        .unwrap();
        assert_eq!(run_input.run_id, "automation-run-1");
    }

    #[test]
    fn automation_response_serializes_with_api_layer_snake_case() {
        let value = serde_json::to_value(AutomationResponse::from(automation())).unwrap();

        assert_eq!(value["project_id"], "project-1");
        assert_eq!(value["max_runs"], 25);
        assert!(value.get("projectId").is_none());
        assert!(value.get("maxRuns").is_none());
    }

    #[test]
    fn automation_schedule_response_serializes_with_api_layer_snake_case() {
        let value = serde_json::to_value(AutomationScheduleResponse::from(
            AutomationScheduleOutcome {
                scheduled: false,
                reason: Some("deferred".to_string()),
            },
        ))
        .unwrap();

        assert_eq!(value["scheduled"], false);
        assert_eq!(value["reason"], "deferred");
    }

    #[tokio::test]
    async fn create_draft_creates_bound_setup_conversation_without_worktree() {
        let state = AppState::new_test();

        let response = create_automation_draft_for_state(
            CreateAutomationDraftInput {
                project_id: "project-1".to_string(),
                name: Some("Nightly cleanup".to_string()),
            },
            &state,
        )
        .await
        .unwrap();

        let setup_conversation_id = response
            .setup_conversation_id
            .as_deref()
            .expect("draft response should expose setup conversation id");
        assert_eq!(
            response.automation.setup_conversation_id.as_deref(),
            Some(setup_conversation_id)
        );

        let automation_id = AutomationId::from_string(response.automation.id.clone());
        let persisted = state
            .automation_repo
            .get_by_id(&automation_id)
            .await
            .unwrap()
            .expect("automation should be persisted");
        let setup_conversation_id =
            ChatConversationId::from_string(setup_conversation_id.to_string());
        assert_eq!(persisted.setup_conversation_id, Some(setup_conversation_id));

        let setup_conversation = state
            .chat_conversation_repo
            .get_by_id(&setup_conversation_id)
            .await
            .unwrap()
            .expect("setup conversation should be persisted");
        assert_eq!(setup_conversation.context_type, ChatContextType::Project);
        assert_eq!(setup_conversation.context_id, "project-1");
        assert_eq!(
            setup_conversation.agent_mode,
            Some(AgentConversationWorkspaceMode::Automation)
        );
        assert_eq!(setup_conversation.automation_id, Some(automation_id));
        assert!(setup_conversation.automation_run_id.is_none());

        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&setup_conversation_id)
            .await
            .unwrap();
        assert!(
            workspace.is_none(),
            "setup conversations must not create a worktree"
        );
    }

    #[tokio::test]
    async fn create_draft_cleans_setup_conversation_when_draft_validation_fails() {
        let state = AppState::new_test();

        let error = create_automation_draft_for_state(
            CreateAutomationDraftInput {
                project_id: "project-1".to_string(),
                name: Some("   ".to_string()),
            },
            &state,
        )
        .await
        .unwrap_err();

        assert!(error.contains("automation name cannot be empty"));
        let conversations = state
            .chat_conversation_repo
            .get_by_context(ChatContextType::Project, "project-1")
            .await
            .unwrap();
        assert!(conversations.is_empty());
        let automations = state
            .automation_repo
            .list(Some(ProjectId::from_string("project-1".to_string())))
            .await
            .unwrap();
        assert!(automations.is_empty());
    }

    #[tokio::test]
    async fn run_now_command_applies_stored_verdict_without_deferred_placeholder() {
        let state = AppState::new_test();
        let mut automation = automation();
        automation.status = AutomationStatus::Active;
        state
            .automation_repo
            .create(automation.clone())
            .await
            .unwrap();
        state
            .automation_run_repo
            .create_run(automation_run(&automation.id))
            .await
            .unwrap();

        let outcome = trigger_automation_run_now_for_state(&automation.id, &state)
            .await
            .unwrap();

        assert!(outcome.scheduled);
        assert!(outcome.reason.is_none());
        let runs = state
            .automation_run_repo
            .list_for_automation(&automation.id)
            .await
            .unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1].prompt_author, AutomationPromptAuthor::Judge);
        assert_eq!(
            runs[1].run_prompt,
            "Implement the next automation item with focused tests and publish the follow-up PR."
        );
    }
}
