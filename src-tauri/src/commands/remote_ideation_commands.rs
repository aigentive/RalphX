//! Thin Tauri adapters for application-owned remote ideation finalize-decision intents.

use tauri::State;

use crate::application::remote_finalize_decision_intent;
use crate::application::AppState;

pub use remote_finalize_decision_intent::*;

#[tauri::command]
pub async fn request_remote_ideation_finalize_decision(
    input: RequestRemoteFinalizeDecisionInput,
    state: State<'_, AppState>,
) -> Result<RemoteFinalizeDecisionIntentResponse, String> {
    request_remote_finalize_decision_for_state(&state, input).await
}

#[tauri::command]
pub async fn get_remote_ideation_finalize_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteFinalizeDecisionRequestView, String> {
    get_remote_finalize_decision_request_for_state(&state, request_id).await
}
