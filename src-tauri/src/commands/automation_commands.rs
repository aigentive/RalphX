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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::application::automation::service::AutomationDetail;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, AgentRun, Automation, AutomationJudgeState,
        AutomationPromptAuthor, AutomationRun, AutomationRunId, AutomationRunStatus,
        AutomationStatus, ChatContextType, ChatConversationId, ProjectId,
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

    #[test]
    fn command_helpers_trim_inputs_and_reject_empty_ids() {
        assert_eq!(
            trim_optional(Some("  project-1  ".to_string())).as_deref(),
            Some("project-1")
        );
        assert!(trim_optional(Some("   ".to_string())).is_none());
        assert_eq!(
            parse_automation_id(" automation-1 ").unwrap().as_str(),
            "automation-1"
        );
        assert_eq!(
            parse_automation_run_id(" run-1 ").unwrap().as_str(),
            "run-1"
        );
        assert_eq!(
            parse_project_id(" project-1 ").unwrap().as_str(),
            "project-1"
        );
        assert_eq!(
            parse_automation_id(" ").unwrap_err(),
            "automation id is required"
        );
        assert_eq!(
            parse_automation_run_id("").unwrap_err(),
            "automation run id is required"
        );
        assert_eq!(parse_project_id("").unwrap_err(), "project id is required");
    }

    #[tokio::test]
    async fn automation_detail_response_aggregates_usage_from_run_conversations() {
        let state = AppState::new_test();
        let automation = automation();
        let conversation_id = ChatConversationId::from_string("conversation-1");
        let mut run = automation_run(&automation.id);
        run.conversation_id = Some(conversation_id);

        let mut first_agent_run = AgentRun::new(conversation_id.clone());
        first_agent_run.input_tokens = Some(120);
        first_agent_run.output_tokens = Some(30);
        first_agent_run.cache_creation_tokens = Some(7);
        first_agent_run.cache_read_tokens = Some(9);
        first_agent_run.estimated_usd = Some(0.04);
        state.agent_run_repo.create(first_agent_run).await.unwrap();

        let mut second_agent_run = AgentRun::new(conversation_id);
        second_agent_run.input_tokens = Some(80);
        second_agent_run.output_tokens = Some(20);
        second_agent_run.estimated_usd = Some(0.02);
        state.agent_run_repo.create(second_agent_run).await.unwrap();

        let response = automation_detail_response_for_state(
            AutomationDetail {
                automation,
                runs: vec![run],
            },
            &state,
        )
        .await
        .unwrap();

        assert_eq!(response.usage.input_tokens, 200);
        assert_eq!(response.usage.output_tokens, 50);
        assert_eq!(response.usage.cache_creation_tokens, 7);
        assert_eq!(response.usage.cache_read_tokens, 9);
        assert_eq!(response.usage.estimated_usd, Some(0.06));
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
