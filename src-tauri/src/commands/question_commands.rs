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
use crate::application::memory_orchestration::{
    schedule_explicit_project_skill_distillation, ProjectSkillDistillationScheduleStatus,
};
use crate::application::plan_verdict_ledger::length_prefixed_component;
use crate::application::project_skill_distillation_service::ProjectSkillDistillationSelection;
use crate::application::{PendingQuestionInfo, QuestionAnswer};
use crate::commands::unified_chat_commands::{
    create_chat_service, ensure_plan_workspace_planning_session_link_for_send,
    switch_agent_conversation_mode_for_state_allowing_running, ModeSwitchInitiator,
    SwitchAgentConversationModeInput,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    ChatContextType, ChatConversationId, ProjectId, TaskOutcome, TaskOutcomeClass, TaskOutcomeId,
    TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::services::learned_skill_adapters::{
    capture_plan_mode_verdict, PlanModeVerdict, PlanModeVerdictCaptureInput, PlanModeVerdictOutcome,
};
use crate::domain::services::OutcomeLedgerService;
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

pub(crate) fn declined_plan_mode_proposal(
    question: Option<&PendingQuestionInfo>,
    answer: &QuestionAnswer,
) -> Option<AcceptedPlanModeProposal> {
    if answer.skipped
        || answer
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
    Some(AcceptedPlanModeProposal {
        conversation_id: ChatConversationId::from_string(conversation_id),
        reason: answer.text.clone(),
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
    plan_mode_proposal_continuation_metadata_with_outcome(request_id, None)
}

pub(crate) fn plan_mode_proposal_continuation_metadata_with_outcome(
    request_id: &str,
    outcome: Option<&PlanModeVerdictOutcome>,
) -> String {
    let mut metadata = serde_json::json!({
        "source": "accepted_plan_mode_proposal",
        "source_request_id": request_id,
        "required_workspace_mode": "plan",
        "resume_in_place": true,
        "persist_hidden_marker": true,
    });
    if let Some(outcome) = outcome {
        metadata["plan_mode_verdict_outcome"] = serde_json::json!(outcome);
    }
    metadata.to_string()
}

/// Dedupe identity for a Plan-mode proposal verdict row.
///
/// The verdict class is part of the key so an accept and a decline in the same
/// planning session are distinct historical rows; `PlanMode` outcomes are outside
/// the terminal-PR rank lattice, so a shared key would overwrite the earlier
/// verdict in place.
pub(crate) fn plan_mode_proposal_source_ref_id(
    planning_session_id: &str,
    outcome_class: &str,
) -> String {
    format!(
        "{}{}",
        length_prefixed_component('s', planning_session_id),
        length_prefixed_component('c', outcome_class),
    )
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
    let source = outcome.source.parse::<TaskOutcomeSource>().ok()?;
    if !source.is_live() {
        return None;
    }
    let outcome_class = TaskOutcomeClass::from(outcome.outcome_class.as_str());
    let now = chrono::Utc::now();
    Some(TaskOutcome {
        id: TaskOutcomeId::new(),
        project_id: ProjectId::from_string(outcome.project_id.clone()),
        source,
        source_ref_kind: "planning_session".to_string(),
        source_ref_id: plan_mode_proposal_source_ref_id(
            planning_session_id,
            outcome_class.as_str(),
        ),
        task_id: None,
        conversation_id: outcome.refs.get("conversation_id").cloned(),
        agent_run_id: None,
        pull_request_id: None,
        proposal_id: None,
        verification_id: None,
        review_id: None,
        outcome_class: Some(outcome_class),
        status,
        evidence_json: serde_json::to_value(outcome).unwrap_or_else(|_| serde_json::json!({})),
        failure_fingerprint: None,
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
        conversation_id: conversation_id.as_str().to_string(),
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

async fn capture_declined_plan_mode_proposal_outcome(
    state: &AppState,
    proposal: &AcceptedPlanModeProposal,
) {
    let workspace = match state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&proposal.conversation_id)
        .await
    {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(
                conversation_id = %proposal.conversation_id,
                error = %error,
                "Could not read the workspace for a declined Plan-mode proposal; no verdict recorded"
            );
            return;
        }
    };
    let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
        return;
    };
    let session = match state.ideation_session_repo.get_by_id(session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(
                conversation_id = %proposal.conversation_id,
                session_id = %session_id.as_str(),
                error = %error,
                "Could not read the planning session for a declined Plan-mode proposal; no verdict recorded"
            );
            return;
        }
    };
    // Derive the project the same way the accept path does (the project conversation's
    // context id) so accept and decline rows for one planning session share a project
    // scope and therefore share the dedupe keyspace they are distinguished within.
    let project_id = match state
        .chat_conversation_repo
        .get_by_id(&proposal.conversation_id)
        .await
    {
        Ok(Some(conversation)) if conversation.context_type == ChatContextType::Project => {
            conversation.context_id
        }
        Ok(_) => return,
        Err(error) => {
            tracing::warn!(
                conversation_id = %proposal.conversation_id,
                error = %error,
                "Could not read the conversation for a declined Plan-mode proposal; no verdict recorded"
            );
            return;
        }
    };
    if project_id != session.project_id.as_str() {
        tracing::warn!(
            conversation_id = %proposal.conversation_id,
            session_id = %session_id.as_str(),
            "Declined Plan-mode proposal conversation and planning session disagree on project"
        );
    }
    let plan_artifact_id = session
        .plan_artifact_id
        .or(session.inherited_plan_artifact_id)
        .map(|artifact_id| artifact_id.as_str().to_string());
    // `reason` is the declining user's free text here, while the accept path takes the
    // agent-supplied `metadata["reason"]`. Both answer "why this verdict"; they are kept
    // in the same `evidence_summary` field on purpose and are never merged.
    let Some(outcome) = capture_plan_mode_verdict(PlanModeVerdictCaptureInput {
        project_id: project_id.clone(),
        conversation_id: proposal.conversation_id.as_str().to_string(),
        planning_session_id: Some(session_id.as_str().to_string()),
        accepted_session_id: None,
        plan_artifact_id,
        verdict: PlanModeVerdict::Declined,
        reason: proposal.reason.clone(),
    }) else {
        return;
    };
    let Some(task_outcome) = task_outcome_from_plan_mode_verdict(&outcome) else {
        return;
    };
    let service = OutcomeLedgerService::new(Arc::clone(&state.task_outcome_repo));
    if let Err(error) = service.record_outcome(task_outcome).await {
        tracing::warn!(
            conversation_id = %proposal.conversation_id,
            error = %error,
            "Plan-mode proposal decline committed but outcome ledger capture failed"
        );
    }
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

    let plan_mode_outcome = capture_accepted_plan_mode_proposal_outcome(
        state,
        &conversation_id,
        &conversation.context_id,
        proposal.reason.as_deref(),
    )
    .await;

    let continuation = build_plan_mode_proposal_continuation(proposal.reason.as_deref());
    let continuation_id = format!("plan-mode-handoff:{request_id}");
    let mut queued = QueuedMessage::with_id(continuation_id.clone(), continuation);
    queued.metadata_override = Some(match plan_mode_outcome.as_ref() {
        Some(outcome) => {
            plan_mode_proposal_continuation_metadata_with_outcome(request_id, Some(outcome))
        }
        None => plan_mode_proposal_continuation_metadata(request_id),
    });

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
    let declined_plan_mode_proposal =
        declined_plan_mode_proposal(Some(claim.pending_question()), &answer);

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
        if let Some(proposal) = declined_plan_mode_proposal.as_ref() {
            capture_declined_plan_mode_proposal_outcome(state.inner(), proposal).await;
        }
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
