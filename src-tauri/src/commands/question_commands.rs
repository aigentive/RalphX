// Tauri commands for question resolution
// Allows frontend to resolve pending questions from agents (AskUserQuestion)

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::application::{PendingQuestionInfo, QuestionAnswer};
use crate::AppState;

/// Arguments for resolving a question
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveQuestionArgs {
    pub request_id: String,
    pub selected_options: Vec<String>,
    pub custom_response: Option<String>,
}

/// Response for resolve_user_question command
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveQuestionResponse {
    pub success: bool,
    pub message: Option<String>,
    pub delivered_to_waiting_agent: bool,
}

/// Resolve a pending question with the user's answer
///
/// Called by the frontend AskUserQuestionCard when the user submits their answer.
/// Signals the waiting MCP long-poll request with the answer.
#[tauri::command]
pub async fn resolve_user_question(
    state: State<'_, AppState>,
    args: ResolveQuestionArgs,
) -> Result<ResolveQuestionResponse, String> {
    let answer = QuestionAnswer {
        selected_options: args.selected_options,
        text: args.custom_response,
    };

    let result = state.question_state.resolve(&args.request_id, answer).await;

    if result.resolved {
        if let Some(ref sid) = result.session_id {
            if let Some(ref app_handle) = state.app_handle {
                let _ = app_handle.emit(
                    "agent:question_resolved",
                    serde_json::json!({
                        "sessionId": sid,
                        "requestId": &args.request_id,
                    }),
                );
            }
        }
        Ok(ResolveQuestionResponse {
            success: true,
            message: Some(format!("Question {} resolved", args.request_id)),
            delivered_to_waiting_agent: result.delivered_to_waiting_agent,
        })
    } else {
        Err(format!("Question request '{}' not found", args.request_id))
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
