//! Thin Tauri adapters for application-owned remote automation run intents.
use crate::application::{remote_automation_run_intent, AppState};
pub use remote_automation_run_intent::*;
use tauri::State;

#[tauri::command]
pub async fn request_remote_automation_run(
    automation_id: String,
    kind: crate::domain::entities::RemoteAutomationRunKind,
    expected_run_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<RemoteAutomationRunIntentResponse, String> {
    request_remote_automation_run_for_state(
        &state,
        RequestRemoteAutomationRunInput {
            automation_id,
            kind,
            expected_run_id,
        },
    )
    .await
}
#[tauri::command]
pub async fn get_remote_automation_run_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteAutomationRunRequestView, String> {
    get_remote_automation_run_request_for_state(&state, request_id).await
}
