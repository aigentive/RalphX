// Tauri commands for question resolution
// Allows frontend to resolve pending questions from agents (AskUserQuestion)

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Emitter, Runtime, State};

use crate::application::chat_service::{
    ChatService, RuntimeHandoffCapture, RuntimeHandoffKickOutcome, RuntimeHandoffOutcome,
    RuntimeHandoffOwner, RuntimeHandoffReleaseOutcome, RuntimeHandoffReservation,
};
use crate::application::interactive_notification_producer::question_notification_key;
use crate::application::{PendingQuestionInfo, QuestionAnswer};
use crate::commands::unified_chat_commands::{
    create_chat_service, ensure_plan_workspace_planning_session_link_for_send,
    switch_agent_conversation_mode_for_state_allowing_running, ModeSwitchInitiator,
    SwitchAgentConversationModeInput,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{ChatContextType, ChatConversationId};
use crate::domain::services::{QueueKey, QueuedMessage};
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

pub(crate) fn plan_mode_proposal_continuation_metadata(request_id: &str) -> String {
    serde_json::json!({
        "source": "accepted_plan_mode_proposal",
        "source_request_id": request_id,
        "required_workspace_mode": "plan",
        "resume_in_place": true,
        "persist_hidden_marker": true,
    })
    .to_string()
}

struct PreparedPlanModeHandoff {
    conversation_id: ChatConversationId,
    runtime_owner: Option<RuntimeHandoffOwner>,
    no_owner_reservation: Option<RuntimeHandoffReservation>,
    continuation_id: String,
    outcome: RuntimeHandoffOutcome,
}

async fn release_no_owner_plan_mode_handoff_reservation(
    service: &dyn ChatService,
    reservation: Option<&RuntimeHandoffReservation>,
) -> RuntimeHandoffReleaseOutcome {
    if let Some(reservation) = reservation {
        service.release_no_owner_runtime_handoff(reservation).await
    } else {
        RuntimeHandoffReleaseOutcome::Released
    }
}

async fn compensate_precommit_plan_mode_handoff<R: Runtime + 'static>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app: tauri::AppHandle<R>,
    prepared: &PreparedPlanModeHandoff,
) {
    let service = create_chat_service(state, app, execution_state);
    if let Some(owner) = prepared.runtime_owner.clone() {
        let _ = service
            .compensate_runtime_handoff(owner, &prepared.continuation_id)
            .await;
    } else {
        let queue_key = QueueKey::new(ChatContextType::Project, prepared.conversation_id.as_str());
        match state
            .queued_message_repo
            .delete(&queue_key, &prepared.continuation_id)
            .await
        {
            Ok(_) => {
                let _ = state
                    .message_queue
                    .delete_with_key(&queue_key, &prepared.continuation_id);
            }
            Err(error) => tracing::warn!(
                conversation_id = %prepared.conversation_id,
                queued_message_id = %prepared.continuation_id,
                error = %error,
                "Could not compensate pre-commit Plan-mode handoff row"
            ),
        }
    }
    let release_outcome = release_no_owner_plan_mode_handoff_reservation(
        &service,
        prepared.no_owner_reservation.as_ref(),
    )
    .await;
    if release_outcome == RuntimeHandoffReleaseOutcome::FailedOrUncertain {
        tracing::warn!(
            conversation_id = %prepared.conversation_id,
            queued_message_id = %prepared.continuation_id,
            "Could not verify no-owner Plan-mode handoff reservation release during compensation"
        );
    }
}

async fn handle_accepted_plan_mode_proposal<R: Runtime + 'static>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app: tauri::AppHandle<R>,
    proposal: AcceptedPlanModeProposal,
    request_id: &str,
) -> Result<PreparedPlanModeHandoff, String> {
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
    let service = create_chat_service(state, app.clone(), execution_state);
    let runtime_capture = service
        .capture_runtime_handoff_owner(ChatContextType::Project, &conversation_id.as_str())
        .await;
    let runtime_owner = match runtime_capture {
        RuntimeHandoffCapture::Captured(owner) => Some(owner),
        RuntimeHandoffCapture::NoOwner => None,
        RuntimeHandoffCapture::FailedOrUncertain => {
            return Err("Could not establish stable runtime-handoff ownership".to_string());
        }
    };
    let mut no_owner_reservation = match runtime_owner.is_none() {
        true => Some(
            service
                .reserve_no_owner_runtime_handoff(
                    ChatContextType::Project,
                    &conversation_id.as_str(),
                    request_id,
                )
                .await
                .map_err(|_| "Could not establish stable runtime-handoff ownership".to_string())?,
        ),
        false => None,
    };

    if let Err(error) = switch_agent_conversation_mode_for_state_allowing_running(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "plan".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        state,
        ModeSwitchInitiator::User,
    )
    .await
    {
        let _ =
            release_no_owner_plan_mode_handoff_reservation(&service, no_owner_reservation.as_ref())
                .await;
        return Err(error);
    }
    if let Err(error) =
        ensure_plan_workspace_planning_session_link_for_send(state, &conversation_id).await
    {
        let _ =
            release_no_owner_plan_mode_handoff_reservation(&service, no_owner_reservation.as_ref())
                .await;
        return Err(error);
    }

    let continuation = build_plan_mode_proposal_continuation(proposal.reason.as_deref());
    let continuation_id = format!("plan-mode-handoff:{request_id}");
    let mut queued = QueuedMessage::with_id(continuation_id.clone(), continuation);
    queued.metadata_override = Some(plan_mode_proposal_continuation_metadata(request_id));

    let outcome = if let Some(owner) = runtime_owner.as_ref() {
        service.stage_runtime_handoff(owner.clone(), queued).await
    } else {
        let queue_key = QueueKey::new(ChatContextType::Project, conversation_id.as_str());
        if let Err(error) = state
            .queued_message_repo
            .enqueue_back(&queue_key, &queued)
            .await
        {
            let _ = release_no_owner_plan_mode_handoff_reservation(
                &service,
                no_owner_reservation.as_ref(),
            )
            .await;
            return Err(error.to_string());
        }
        state.message_queue.queue_back_existing(
            ChatContextType::Project,
            conversation_id.as_str(),
            queued,
        );
        RuntimeHandoffOutcome::DurablyRecoverable
    };

    if outcome == RuntimeHandoffOutcome::Failed {
        if let Some(owner) = runtime_owner {
            let _ = service
                .compensate_runtime_handoff(owner, &continuation_id)
                .await;
        }
        let _ =
            release_no_owner_plan_mode_handoff_reservation(&service, no_owner_reservation.as_ref())
                .await;
        return Err("Could not establish durable Plan-mode handoff authority".to_string());
    }

    let _ = app.emit(
        "agent:workspace_changed",
        serde_json::json!({
            "conversation_id": conversation_id.as_str(),
            "mode": "plan",
        }),
    );

    Ok(PreparedPlanModeHandoff {
        conversation_id,
        runtime_owner,
        no_owner_reservation: no_owner_reservation.take(),
        continuation_id,
        outcome,
    })
}

/// Resolve a pending question with the user's answer
///
/// Called by the frontend AskUserQuestionCard when the user submits their answer.
/// Signals the waiting MCP long-poll request with the answer.
#[tauri::command]
pub async fn resolve_user_question<R: Runtime + 'static>(
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle<R>,
    args: ResolveQuestionArgs,
) -> Result<ResolveQuestionResponse, String> {
    let request_id = args.request_id;
    let answer = QuestionAnswer {
        selected_options: args.selected_options,
        text: args.custom_response,
        skipped: args.skipped,
    };
    let claim = state
        .question_state
        .claim_pending(&request_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Question request '{}' not found", request_id))?;
    match state.question_state.get_resolved_answer(&request_id).await {
        Ok(Some(_)) => {
            state.question_state.release_claim(claim).await;
            return Err(format!(
                "Question request '{}' is already resolved",
                request_id
            ));
        }
        Ok(None) => {}
        Err(error) => {
            state.question_state.release_claim(claim).await;
            return Err(error.to_string());
        }
    }
    let accepted_plan_mode_proposal =
        accepted_plan_mode_proposal(Some(claim.pending_question()), &answer);

    let prepared = if let Some(proposal) = accepted_plan_mode_proposal {
        match handle_accepted_plan_mode_proposal(
            state.inner(),
            execution_state.inner(),
            app.clone(),
            proposal,
            &request_id,
        )
        .await
        {
            Ok(prepared) => Some(prepared),
            Err(error) => {
                state.question_state.release_claim(claim).await;
                tracing::warn!(
                    request_id = %request_id,
                    error = %error,
                    "Accepted Plan-mode proposal could not establish handoff authority"
                );
                return Err(error);
            }
        }
    } else {
        None
    };

    let result = state.question_state.commit_claim(claim, answer).await;

    if result.resolved {
        state
            .notification_service()
            .resolve_workflow_notification(&question_notification_key(&request_id))
            .await;
        let plan_mode_proposal_handled = if let Some(prepared) = prepared {
            let service = create_chat_service(state.inner(), app.clone(), execution_state.inner());
            let release_outcome = release_no_owner_plan_mode_handoff_reservation(
                &service,
                prepared.no_owner_reservation.as_ref(),
            )
            .await;
            if release_outcome == RuntimeHandoffReleaseOutcome::FailedOrUncertain {
                tracing::warn!(
                    conversation_id = %prepared.conversation_id,
                    queued_message_id = %prepared.continuation_id,
                    "No-owner Plan-mode handoff reservation release was not verified; leaving recovery to the durable row"
                );
            }
            match prepared.outcome {
                RuntimeHandoffOutcome::AwaitingRetirement => {
                    if let Some(owner) = prepared.runtime_owner {
                        if service.finalize_idle_runtime_handoff(owner.clone()).await {
                            matches!(
                                service
                                    .kick_runtime_handoff(
                                        &prepared.conversation_id,
                                        &prepared.continuation_id,
                                    )
                                    .await,
                                RuntimeHandoffKickOutcome::Started { .. }
                                    | RuntimeHandoffKickOutcome::DurablyRecoverable
                            )
                        } else {
                            service.activate_runtime_handoff_watchdog(owner);
                            true
                        }
                    } else {
                        false
                    }
                }
                RuntimeHandoffOutcome::DurablyRecoverable
                    if release_outcome == RuntimeHandoffReleaseOutcome::Released =>
                {
                    matches!(
                        service
                            .kick_runtime_handoff(
                                &prepared.conversation_id,
                                &prepared.continuation_id,
                            )
                            .await,
                        RuntimeHandoffKickOutcome::Started { .. }
                            | RuntimeHandoffKickOutcome::DurablyRecoverable
                    )
                }
                RuntimeHandoffOutcome::DurablyRecoverable => false,
                RuntimeHandoffOutcome::Failed => false,
            }
        } else {
            false
        };

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
        if let Some(prepared) = prepared.as_ref() {
            compensate_precommit_plan_mode_handoff(
                state.inner(),
                execution_state.inner(),
                app,
                prepared,
            )
            .await;
        }
        Err(format!(
            "Question request '{}' could not be resolved",
            request_id
        ))
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
