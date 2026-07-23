// Tauri commands for question resolution
// Allows frontend to resolve pending questions from agents (AskUserQuestion)

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Emitter, Runtime, State};

use crate::application::chat_service::{ChatService, SendMessageOptions};
use crate::application::interactive_notification_producer::question_notification_key;
use crate::application::interactive_process_registry::InteractiveProcessKey;
use crate::application::memory_orchestration::{
    schedule_explicit_project_skill_distillation, ProjectSkillDistillationScheduleStatus,
};
use crate::application::project_skill_distillation_service::ProjectSkillDistillationSelection;
use crate::application::{PendingQuestionInfo, QuestionAnswer};
use crate::commands::unified_chat_commands::{
    create_chat_service, ensure_plan_workspace_planning_session_link_for_send,
    switch_agent_conversation_mode_for_state_allowing_running, ModeSwitchInitiator,
    SwitchAgentConversationModeInput,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    ChatContextType, ChatConversationId, ProjectId, TaskOutcome, TaskOutcomeId, TaskOutcomeStatus,
};
use crate::domain::services::learned_skill_adapters::{
    capture_plan_mode_verdict, PlanModeVerdict, PlanModeVerdictCaptureInput, PlanModeVerdictOutcome,
};
use crate::domain::services::OutcomeLedgerService;
use crate::domain::services::QueueKey;
use crate::AppState;

pub(crate) const PLAN_MODE_PROPOSAL_KIND: &str = "plan_mode_proposal";
pub(crate) const PLAN_MODE_PROPOSAL_ACCEPT_VALUE: &str = "switch_to_plan";
pub(crate) const PLAN_MODE_PROPOSAL_CONTINUATION_BASE: &str =
    "Continue in Plan mode from the accepted proposal. Work with me on a concrete plan before implementation.";

/// Arguments for resolving a question
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveQuestionArgs {
    pub request_id: String,
    pub selected_options: Vec<String>,
    pub custom_response: Option<String>,
    #[serde(default)]
    pub skipped: bool,
}

/// Response for resolve_user_question command
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveQuestionResponse {
    pub success: bool,
    pub message: Option<String>,
    pub delivered_to_waiting_agent: bool,
    #[serde(default)]
    pub plan_mode_proposal_handled: bool,
}

pub(crate) struct AcceptedPlanModeProposal {
    pub(crate) conversation_id: ChatConversationId,
    pub(crate) reason: Option<String>,
}

pub(crate) fn accepted_plan_mode_proposal(
    question: Option<&PendingQuestionInfo>,
    answer: &QuestionAnswer,
) -> Option<AcceptedPlanModeProposal> {
    if answer.skipped
        || !answer
            .selected_options
            .iter()
            .any(|option| option == PLAN_MODE_PROPOSAL_ACCEPT_VALUE)
    {
        return None;
    }

    let question = question?;
    let metadata = question.metadata.as_ref()?;
    if metadata.get("kind").and_then(|value| value.as_str()) != Some(PLAN_MODE_PROPOSAL_KIND) {
        return None;
    }

    let conversation_id = metadata
        .get("conversation_id")
        .and_then(|value| value.as_str())
        .unwrap_or(question.session_id.as_str())
        .trim();
    if conversation_id.is_empty() {
        return None;
    }

    let reason = metadata
        .get("reason")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    Some(AcceptedPlanModeProposal {
        conversation_id: ChatConversationId::from_string(conversation_id.to_string()),
        reason,
    })
}

pub(crate) fn build_plan_mode_proposal_continuation(reason: Option<&str>) -> String {
    match reason.map(str::trim).filter(|value| !value.is_empty()) {
        Some(reason) => {
            format!("{PLAN_MODE_PROPOSAL_CONTINUATION_BASE}\n\nPlanning focus: {reason}")
        }
        None => PLAN_MODE_PROPOSAL_CONTINUATION_BASE.to_string(),
    }
}

pub(crate) fn plan_mode_proposal_continuation_metadata() -> String {
    plan_mode_proposal_continuation_metadata_with_outcome(None)
}

pub(crate) fn plan_mode_proposal_continuation_metadata_with_outcome(
    outcome: Option<&PlanModeVerdictOutcome>,
) -> String {
    let mut metadata = serde_json::json!({
        "source": "accepted_plan_mode_proposal",
        "resume_in_place": true,
        "persist_hidden_marker": true,
    });
    if let Some(outcome) = outcome {
        metadata["plan_mode_verdict_outcome"] = serde_json::json!(outcome);
    }
    metadata.to_string()
}

pub(crate) fn task_outcome_from_plan_mode_verdict(
    outcome: &PlanModeVerdictOutcome,
) -> Option<TaskOutcome> {
    let planning_session_id = outcome.refs.get("planning_session_id")?.trim();
    if planning_session_id.is_empty() {
        return None;
    }
    let status = outcome
        .status
        .parse::<TaskOutcomeStatus>()
        .unwrap_or(TaskOutcomeStatus::Unknown);
    let now = chrono::Utc::now();
    Some(TaskOutcome {
        id: TaskOutcomeId::new(),
        project_id: ProjectId::from_string(outcome.project_id.clone()),
        source: outcome.source.clone(),
        source_ref_kind: "planning_session".to_string(),
        source_ref_id: planning_session_id.to_string(),
        task_id: None,
        conversation_id: outcome.refs.get("conversation_id").cloned(),
        agent_run_id: None,
        pull_request_id: None,
        proposal_id: None,
        verification_id: None,
        review_id: None,
        outcome_class: Some(outcome.outcome_class.clone()),
        status,
        evidence_json: serde_json::to_value(outcome).unwrap_or_else(|_| serde_json::json!({})),
        provider_harness: None,
        provider_session_id: None,
        created_at: now,
        updated_at: now,
    })
}

async fn capture_accepted_plan_mode_proposal_outcome(
    state: &AppState,
    conversation_id: &ChatConversationId,
    project_id: &str,
    reason: Option<&str>,
) -> Option<PlanModeVerdictOutcome> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .ok()
        .flatten()?;
    let planning_session_id = workspace.linked_ideation_session_id.as_ref()?;
    let planning_session = state
        .ideation_session_repo
        .get_by_id(planning_session_id)
        .await
        .ok()
        .flatten();
    let plan_artifact_id = planning_session.and_then(|session| {
        session
            .plan_artifact_id
            .or(session.inherited_plan_artifact_id)
            .map(|artifact_id| artifact_id.as_str().to_string())
    });

    let outcome = capture_plan_mode_verdict(PlanModeVerdictCaptureInput {
        project_id: project_id.to_string(),
        conversation_id: conversation_id.as_str(),
        planning_session_id: Some(planning_session_id.0.clone()),
        accepted_session_id: None,
        plan_artifact_id,
        verdict: PlanModeVerdict::Accepted,
        reason: reason.map(str::to_string),
    })?;

    if let Some(task_outcome) = task_outcome_from_plan_mode_verdict(&outcome) {
        let service = OutcomeLedgerService::new(Arc::clone(&state.task_outcome_repo));
        match service.record_outcome(task_outcome).await {
            Ok(recorded_outcome) => {
                let outcome_id = recorded_outcome.id.clone();
                let schedule = schedule_explicit_project_skill_distillation(
                    state,
                    &recorded_outcome.project_id,
                    ProjectSkillDistillationSelection::ExactOutcomes(vec![outcome_id.clone()]),
                    Some(conversation_id),
                    ChatContextType::Project,
                    project_id,
                )
                .await;
                if matches!(
                    schedule.status,
                    ProjectSkillDistillationScheduleStatus::Failed
                        | ProjectSkillDistillationScheduleStatus::Unavailable
                ) {
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        outcome_id = %outcome_id.as_str(),
                        status = schedule.status.as_str(),
                        "Accepted Plan-mode evidence was queued but the distiller did not start"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(
                    conversation_id = %conversation_id,
                    error = %error,
                    "Failed to persist accepted Plan-mode proposal outcome"
                );
            }
        }
    }

    Some(outcome)
}

async fn handle_accepted_plan_mode_proposal<R: Runtime + 'static>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    team_service: Arc<crate::application::TeamService>,
    app: tauri::AppHandle<R>,
    proposal: AcceptedPlanModeProposal,
    delivered_to_waiting_agent: bool,
) -> Result<(), String> {
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&proposal.conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Conversation not found: {}", proposal.conversation_id))?;
    if conversation.context_type != ChatContextType::Project {
        return Err("Plan-mode proposals are only supported for project conversations".to_string());
    }

    let conversation_id = proposal.conversation_id.clone();
    switch_agent_conversation_mode_for_state_allowing_running(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "plan".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        state,
        ModeSwitchInitiator::User,
    )
    .await?;
    ensure_plan_workspace_planning_session_link_for_send(state, &conversation_id).await?;
    let plan_mode_outcome = capture_accepted_plan_mode_proposal_outcome(
        state,
        &conversation_id,
        &conversation.context_id,
        proposal.reason.as_deref(),
    )
    .await;

    let _ = app.emit(
        "agent:workspace_changed",
        serde_json::json!({
            "conversation_id": conversation_id.as_str(),
            "mode": "plan",
        }),
    );

    let continuation = build_plan_mode_proposal_continuation(proposal.reason.as_deref());
    let continuation_metadata = match plan_mode_outcome.as_ref() {
        Some(outcome) => plan_mode_proposal_continuation_metadata_with_outcome(Some(outcome)),
        None => plan_mode_proposal_continuation_metadata(),
    };
    if delivered_to_waiting_agent {
        let queued = state.message_queue.queue_with_overrides(
            ChatContextType::Project,
            conversation_id.as_str(),
            continuation,
            Some(continuation_metadata),
            None,
            None,
        );
        let queue_key = QueueKey::new(ChatContextType::Project, conversation_id.as_str());
        state
            .queued_message_repo
            .enqueue_back(&queue_key, &queued)
            .await
            .map_err(|error| error.to_string())?;

        let ipr_key = InteractiveProcessKey::new(
            ChatContextType::Project.to_string(),
            conversation_id.as_str(),
        );
        let removed = state
            .interactive_process_registry
            .remove(&ipr_key)
            .await
            .is_some();
        tracing::info!(
            conversation_id = %conversation_id,
            queued_message_id = %queued.id,
            removed_interactive_process = removed,
            "Accepted Plan-mode proposal queued hidden continuation and invalidated current interactive process"
        );
        return Ok(());
    }

    let service = create_chat_service(state, app, execution_state, Some(team_service));
    service
        .send_message(
            ChatContextType::Project,
            &conversation.context_id,
            &continuation,
            SendMessageOptions {
                metadata: Some(continuation_metadata),
                conversation_id_override: Some(conversation_id),
                ..Default::default()
            },
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Resolve a pending question with the user's answer
///
/// Called by the frontend AskUserQuestionCard when the user submits their answer.
/// Signals the waiting MCP long-poll request with the answer.
#[tauri::command]
pub async fn resolve_user_question<R: Runtime + 'static>(
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    team_service: State<'_, Arc<crate::application::TeamService>>,
    app: tauri::AppHandle<R>,
    args: ResolveQuestionArgs,
) -> Result<ResolveQuestionResponse, String> {
    let request_id = args.request_id;
    let answer = QuestionAnswer {
        selected_options: args.selected_options,
        text: args.custom_response,
        skipped: args.skipped,
    };
    let pending_question = state
        .question_state
        .get_pending_info()
        .await
        .into_iter()
        .find(|question| question.request_id == request_id);
    let accepted_plan_mode_proposal =
        accepted_plan_mode_proposal(pending_question.as_ref(), &answer);

    let result = state.question_state.resolve(&request_id, answer).await;

    if result.resolved {
        state
            .notification_service()
            .resolve_workflow_notification(&question_notification_key(&request_id))
            .await;
        let mut plan_mode_proposal_handled = false;
        if let Some(proposal) = accepted_plan_mode_proposal {
            match handle_accepted_plan_mode_proposal(
                state.inner(),
                execution_state.inner(),
                Arc::clone(team_service.inner()),
                app.clone(),
                proposal,
                result.delivered_to_waiting_agent,
            )
            .await
            {
                Ok(()) => {
                    plan_mode_proposal_handled = true;
                }
                Err(error) => {
                    tracing::warn!(
                        request_id = %request_id,
                        error = %error,
                        "Accepted Plan-mode proposal could not be handled by backend"
                    );
                }
            }
        }

        if let Some(ref sid) = result.session_id {
            if let Some(ref app_handle) = state.app_handle {
                let _ = app_handle.emit(
                    "agent:question_resolved",
                    serde_json::json!({
                        "sessionId": sid,
                        "requestId": &request_id,
                    }),
                );
            }
        }
        Ok(ResolveQuestionResponse {
            success: true,
            message: Some(format!("Question {} resolved", request_id)),
            delivered_to_waiting_agent: result.delivered_to_waiting_agent,
            plan_mode_proposal_handled,
        })
    } else {
        Err(format!("Question request '{}' not found", request_id))
    }
}

/// Get information about all pending questions
///
/// Used by the frontend to display any pending questions that might have been
/// missed (e.g., if the chat view was just opened while an agent was asking).
#[tauri::command]
pub async fn get_pending_questions(
    state: State<'_, AppState>,
) -> Result<Vec<PendingQuestionInfo>, String> {
    let pending = state.question_state.get_pending_info().await;
    Ok(pending)
}
