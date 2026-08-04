//! Thin Tauri adapters for application-owned remote plan-approval intents.

use tauri::State;

use crate::application::remote_plan_approval_intent;
use crate::application::AppState;

pub use remote_plan_approval_intent::*;

#[tauri::command]
pub async fn request_remote_plan_approval(
    input: RequestRemotePlanApprovalInput,
    state: State<'_, AppState>,
) -> Result<RemotePlanApprovalIntentResponse, String> {
    request_remote_plan_approval_for_state(&state, input).await
}

#[tauri::command]
pub async fn get_remote_plan_approval_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemotePlanApprovalRequestView, String> {
    get_remote_plan_approval_request_for_state(&state, request_id).await
}
