// Tauri commands for question resolution
// Allows frontend to resolve pending questions from agents (AskUserQuestion)

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Emitter, Runtime, State};

use crate::application::chat_service::{ChatService, SendMessageOptions};
use crate::application::interactive_process_registry::InteractiveProcessKey;
use crate::application::{PendingQuestionInfo, QuestionAnswer};
use crate::commands::unified_chat_commands::{
    create_chat_service, ensure_plan_workspace_planning_session_link_for_send,
    switch_agent_conversation_mode_for_state_allowing_running, SwitchAgentConversationModeInput,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{ChatContextType, ChatConversationId};
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
    serde_json::json!({
        "source": "accepted_plan_mode_proposal",
        "resume_in_place": true,
        "persist_hidden_marker": true,
    })
    .to_string()
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
            base_ref: None,
            base_display_name: None,
        },
        state,
    )
    .await?;
    ensure_plan_workspace_planning_session_link_for_send(state, &conversation_id).await?;

    let _ = app.emit(
        "agent:workspace_changed",
        serde_json::json!({
            "conversation_id": conversation_id.as_str(),
            "mode": "plan",
        }),
    );

    let continuation = build_plan_mode_proposal_continuation(proposal.reason.as_deref());
    if delivered_to_waiting_agent {
        let queued = state.message_queue.queue_with_overrides(
            ChatContextType::Project,
            conversation_id.as_str(),
            continuation,
            Some(plan_mode_proposal_continuation_metadata()),
            None,
            None,
        );

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
