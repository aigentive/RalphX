//! Thin Tauri adapters for application-owned remote resume intents.

use tauri::State;

use crate::application::remote_resume_intent;
use crate::application::AppState;

pub use remote_resume_intent::*;

#[tauri::command]
pub async fn request_remote_execution_resume(
    input: RequestRemoteExecutionResumeInput,
    state: State<'_, AppState>,
) -> Result<RemoteResumeIntentResponse, String> {
    request_remote_execution_resume_for_state(&state, input).await
}

#[tauri::command]
pub async fn request_remote_task_resume(
    input: RequestRemoteTaskResumeInput,
    state: State<'_, AppState>,
) -> Result<RemoteResumeIntentResponse, String> {
    request_remote_task_resume_for_state(&state, input).await
}

#[tauri::command]
pub async fn request_remote_task_restart(
    input: RequestRemoteTaskRestartInput,
    state: State<'_, AppState>,
) -> Result<RemoteResumeIntentResponse, String> {
    request_remote_task_restart_for_state(&state, input).await
}

#[tauri::command]
pub async fn request_remote_group_resume(
    input: RequestRemoteGroupResumeInput,
    state: State<'_, AppState>,
) -> Result<RemoteResumeIntentResponse, String> {
    request_remote_group_resume_for_state(&state, input).await
}

#[tauri::command]
pub async fn get_remote_execution_resume_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteResumeRequestView, String> {
    get_remote_execution_resume_request_for_state(&state, request_id).await
}

#[tauri::command]
pub async fn get_remote_task_action_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteResumeRequestView, String> {
    get_remote_task_action_request_for_state(&state, request_id).await
}
