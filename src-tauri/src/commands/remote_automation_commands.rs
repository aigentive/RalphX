//! Thin Tauri adapters for application-owned remote automation run intents.
use crate::application::{remote_automation_draft_intent, remote_automation_run_intent, AppState};
pub use remote_automation_draft_intent::*;
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

#[tauri::command]
pub async fn request_remote_automation_draft(
    project_id: String,
    name: String,
    authoring_mode: String,
    base_ref_kind: String,
    base_branch_mode: String,
    base_branch: Option<String>,
    state: State<'_, AppState>,
) -> Result<RemoteAutomationDraftIntentResponse, String> {
    request_remote_automation_draft_for_state(
        &state,
        RequestRemoteAutomationDraftInput {
            project_id,
            name,
            authoring_mode,
            base_ref_kind,
            base_branch_mode,
            base_branch,
        },
    )
    .await
}

#[tauri::command]
pub async fn get_remote_automation_draft_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteAutomationDraftRequestView, String> {
    get_remote_automation_draft_request_for_state(&state, request_id).await
}
