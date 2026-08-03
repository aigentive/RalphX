//! Spawn-free remote question resolution for the remote facade.
//!
//! The local resolver carries `AppHandle`, `ExecutionState`, and `ChatService` authority for
//! accepted Plan-mode proposals and runtime handoff. This twin carries none of them: it refuses
//! Plan-mode acceptance before commit and otherwise uses only `AppState` question persistence,
//! notification resolution, and event delivery.

use serde::Deserialize;
use tauri::State;

use crate::application::interactive_notification_producer::question_notification_key;
use crate::application::QuestionAnswer;
use crate::commands::question_commands::{accepted_plan_mode_proposal, ResolveQuestionResponse};
use crate::AppState;

pub const REMOTE_PLAN_MODE_PROPOSAL_REQUIRES_HOST: &str =
    "accepting a plan-mode proposal prepares a workspace on the host — answer it from the host";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveRemoteUserQuestionInput {
    pub request_id: String,
    pub selected_options: Vec<String>,
    pub custom_response: Option<String>,
    #[serde(default)]
    pub skipped: bool,
}

#[tauri::command]
pub async fn resolve_remote_user_question(
    input: ResolveRemoteUserQuestionInput,
    state: State<'_, AppState>,
) -> Result<ResolveQuestionResponse, String> {
    resolve_remote_user_question_for_state(&state, input).await
}

#[doc(hidden)]
pub async fn resolve_remote_user_question_for_state(
    state: &AppState,
    input: ResolveRemoteUserQuestionInput,
) -> Result<ResolveQuestionResponse, String> {
    let request_id = input.request_id;
    let answer = QuestionAnswer {
        selected_options: input.selected_options,
        text: input.custom_response,
        skipped: input.skipped,
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

    if accepted_plan_mode_proposal(Some(claim.pending_question()), &answer).is_some() {
        state.question_state.release_claim(claim).await;
        return Err(REMOTE_PLAN_MODE_PROPOSAL_REQUIRES_HOST.to_string());
    }

    let result = state.question_state.commit_claim(claim, answer).await;
    if !result.resolved {
        return Err(format!(
            "Question request '{}' could not be resolved",
            request_id
        ));
    }

    state
        .notification_service()
        .resolve_workflow_notification(&question_notification_key(&request_id))
        .await;
    if let Some(session_id) = result.session_id {
        state.events.emit(
            "agent:question_resolved",
            serde_json::json!({
                "sessionId": session_id,
                "requestId": &request_id,
            }),
        );
    }

    Ok(ResolveQuestionResponse {
        success: true,
        message: Some(format!("Question {} resolved", request_id)),
        delivered_to_waiting_agent: result.delivered_to_waiting_agent,
        plan_mode_proposal_handled: false,
    })
}
