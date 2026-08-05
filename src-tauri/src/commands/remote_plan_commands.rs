//! Thin Tauri adapters for application-owned remote plan-approval intents.

use tauri::State;

use crate::application::remote_plan_approval_intent;
use crate::application::remote_plan_edit_intent;
use crate::application::AppState;

pub use remote_plan_approval_intent::*;
pub use remote_plan_edit_intent::*;

#[tauri::command]
pub async fn request_remote_plan_artifact_edit(
    input: RequestRemotePlanEditInput,
    state: State<'_, AppState>,
) -> Result<RemotePlanEditIntentResponse, String> {
    request_remote_plan_edit_for_state(&state, input).await
}

#[tauri::command]
pub async fn get_remote_plan_edit_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemotePlanEditRequestView, String> {
    get_remote_plan_edit_request_for_state(&state, request_id).await
}

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
