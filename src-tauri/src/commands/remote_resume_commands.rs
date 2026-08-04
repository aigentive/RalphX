//! Spawn-free intent twins for host-owned execution and task resume workflows.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::entities::{
    InternalStatus, ProjectId, RemoteExecutionResumeRequest, RemoteResumeRequestStatus,
    RemoteTaskAction, RemoteTaskActionRequest, TaskId,
};

pub const REMOTE_RESUME_LOOKUP_FAILED: &str = "REMOTE_RESUME_LOOKUP_FAILED";
pub const REMOTE_RESUME_PROJECT_NOT_FOUND: &str = "REMOTE_RESUME_PROJECT_NOT_FOUND";
pub const REMOTE_RESUME_TASK_NOT_FOUND: &str = "REMOTE_RESUME_TASK_NOT_FOUND";
pub const REMOTE_RESUME_TASK_NOT_PAUSED: &str = "REMOTE_RESUME_TASK_NOT_PAUSED";
pub const REMOTE_RESTART_TASK_NOT_RESTARTABLE: &str = "REMOTE_RESTART_TASK_NOT_RESTARTABLE";
pub const REMOTE_RESUME_INVALID_GROUP: &str = "REMOTE_RESUME_INVALID_GROUP";
pub const REMOTE_RESUME_ENQUEUE_FAILED: &str = "REMOTE_RESUME_ENQUEUE_FAILED";
pub const REMOTE_RESUME_REQUEST_NOT_FOUND: &str = "REMOTE_RESUME_REQUEST_NOT_FOUND";
pub const REMOTE_RESUME_AUTHORITY_CHANGED: &str = "REMOTE_RESUME_AUTHORITY_CHANGED";
pub const REMOTE_RESUME_HOST_FAILED: &str = "REMOTE_RESUME_HOST_FAILED";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteExecutionResumeInput {
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteTaskResumeInput {
    pub task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteTaskRestartInput {
    pub task_id: String,
    #[serde(default)]
    pub force: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteGroupResumeInput {
    pub group_kind: String,
    pub group_id: String,
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteResumeIntentResponse {
    pub request_id: String,
    pub status: RemoteResumeRequestStatus,
    pub deduplicated: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteResumeRequestView {
    pub request_id: String,
    pub status: RemoteResumeRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[tauri::command]
pub async fn request_remote_execution_resume(
    input: RequestRemoteExecutionResumeInput,
    state: State<'_, AppState>,
) -> Result<RemoteResumeIntentResponse, String> {
    request_remote_execution_resume_for_state(&state, input).await
}

pub async fn request_remote_execution_resume_for_state(
    state: &AppState,
    input: RequestRemoteExecutionResumeInput,
) -> Result<RemoteResumeIntentResponse, String> {
    let project_id = input.project_id.map(ProjectId::from_string);
    if let Some(project_id) = &project_id {
        if state
            .project_repo
            .get_by_id(project_id)
            .await
            .map_err(|_| REMOTE_RESUME_LOOKUP_FAILED.to_string())?
            .is_none()
        {
            return Err(REMOTE_RESUME_PROJECT_NOT_FOUND.to_string());
        }
    }
    if let Some(existing) = state
        .remote_execution_resume_request_repo
        .find_unsettled(project_id.as_ref())
        .await
        .map_err(|_| REMOTE_RESUME_LOOKUP_FAILED.to_string())?
    {
        return Ok(execution_response(existing, true));
    }
    let now = chrono::Utc::now();
    let row = RemoteExecutionResumeRequest {
        id: uuid::Uuid::new_v4().to_string(),
        project_id,
        status: RemoteResumeRequestStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    state
        .remote_execution_resume_request_repo
        .create_execution_resume_request(row)
        .await
        .map(execution_response_new)
        .map_err(|_| REMOTE_RESUME_ENQUEUE_FAILED.to_string())
}

fn execution_response(
    row: RemoteExecutionResumeRequest,
    deduplicated: bool,
) -> RemoteResumeIntentResponse {
    RemoteResumeIntentResponse {
        request_id: row.id,
        status: row.status,
        deduplicated,
        created_at: row.created_at.to_rfc3339(),
    }
}
fn execution_response_new(row: RemoteExecutionResumeRequest) -> RemoteResumeIntentResponse {
    execution_response(row, false)
}
fn task_response(row: RemoteTaskActionRequest, deduplicated: bool) -> RemoteResumeIntentResponse {
    RemoteResumeIntentResponse {
        request_id: row.id,
        status: row.status,
        deduplicated,
        created_at: row.created_at.to_rfc3339(),
    }
}

async fn validated_task(
    state: &AppState,
    task_id: String,
    action: RemoteTaskAction,
) -> Result<(TaskId, ProjectId), String> {
    let task_id = TaskId::from_string(task_id);
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|_| REMOTE_RESUME_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_RESUME_TASK_NOT_FOUND.to_string())?;
    match action {
        RemoteTaskAction::Resume if task.internal_status != InternalStatus::Paused => {
            return Err(REMOTE_RESUME_TASK_NOT_PAUSED.to_string())
        }
        RemoteTaskAction::Restart
            if !matches!(
                task.internal_status,
                InternalStatus::Stopped | InternalStatus::Failed
            ) =>
        {
            return Err(REMOTE_RESTART_TASK_NOT_RESTARTABLE.to_string())
        }
        _ => {}
    }
    Ok((task_id, task.project_id))
}

async fn persist_task(
    state: &AppState,
    row: RemoteTaskActionRequest,
) -> Result<RemoteResumeIntentResponse, String> {
    if let Some(task_id) = &row.task_id {
        if let Some(existing) = state
            .remote_task_action_request_repo
            .find_unsettled_for_task(task_id)
            .await
            .map_err(|_| REMOTE_RESUME_LOOKUP_FAILED.to_string())?
        {
            if existing.action == row.action {
                return Ok(task_response(existing, true));
            }
            return Err(REMOTE_RESUME_AUTHORITY_CHANGED.to_string());
        }
    }
    if let (Some(kind), Some(group)) = (&row.group_kind, &row.group_id) {
        if let Some(existing) = state
            .remote_task_action_request_repo
            .find_unsettled_for_group(&row.project_id, kind, group)
            .await
            .map_err(|_| REMOTE_RESUME_LOOKUP_FAILED.to_string())?
        {
            return Ok(task_response(existing, true));
        }
    }
    state
        .remote_task_action_request_repo
        .create_task_action_request(row)
        .await
        .map(|row| task_response(row, false))
        .map_err(|_| REMOTE_RESUME_ENQUEUE_FAILED.to_string())
}

#[tauri::command]
pub async fn request_remote_task_resume(
    input: RequestRemoteTaskResumeInput,
    state: State<'_, AppState>,
) -> Result<RemoteResumeIntentResponse, String> {
    request_remote_task_resume_for_state(&state, input).await
}
pub async fn request_remote_task_resume_for_state(
    state: &AppState,
    input: RequestRemoteTaskResumeInput,
) -> Result<RemoteResumeIntentResponse, String> {
    let (task_id, project_id) =
        validated_task(state, input.task_id, RemoteTaskAction::Resume).await?;
    persist_task(
        state,
        new_task_row(
            RemoteTaskAction::Resume,
            Some(task_id),
            project_id,
            None,
            None,
            false,
            None,
        ),
    )
    .await
}

#[tauri::command]
pub async fn request_remote_task_restart(
    input: RequestRemoteTaskRestartInput,
    state: State<'_, AppState>,
) -> Result<RemoteResumeIntentResponse, String> {
    request_remote_task_restart_for_state(&state, input).await
}
pub async fn request_remote_task_restart_for_state(
    state: &AppState,
    input: RequestRemoteTaskRestartInput,
) -> Result<RemoteResumeIntentResponse, String> {
    let (task_id, project_id) =
        validated_task(state, input.task_id, RemoteTaskAction::Restart).await?;
    persist_task(
        state,
        new_task_row(
            RemoteTaskAction::Restart,
            Some(task_id),
            project_id,
            None,
            None,
            input.force,
            input.note,
        ),
    )
    .await
}

#[tauri::command]
pub async fn request_remote_group_resume(
    input: RequestRemoteGroupResumeInput,
    state: State<'_, AppState>,
) -> Result<RemoteResumeIntentResponse, String> {
    request_remote_group_resume_for_state(&state, input).await
}
pub async fn request_remote_group_resume_for_state(
    state: &AppState,
    input: RequestRemoteGroupResumeInput,
) -> Result<RemoteResumeIntentResponse, String> {
    if !matches!(
        input.group_kind.as_str(),
        "status" | "session" | "uncategorized"
    ) || input.group_id.trim().is_empty()
    {
        return Err(REMOTE_RESUME_INVALID_GROUP.to_string());
    }
    let project_id = ProjectId::from_string(input.project_id);
    if state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|_| REMOTE_RESUME_LOOKUP_FAILED.to_string())?
        .is_none()
    {
        return Err(REMOTE_RESUME_PROJECT_NOT_FOUND.to_string());
    }
    persist_task(
        state,
        new_task_row(
            RemoteTaskAction::GroupResume,
            None,
            project_id,
            Some(input.group_kind),
            Some(input.group_id),
            false,
            None,
        ),
    )
    .await
}

fn new_task_row(
    action: RemoteTaskAction,
    task_id: Option<TaskId>,
    project_id: ProjectId,
    group_kind: Option<String>,
    group_id: Option<String>,
    force: bool,
    note: Option<String>,
) -> RemoteTaskActionRequest {
    let now = chrono::Utc::now();
    RemoteTaskActionRequest {
        id: uuid::Uuid::new_v4().to_string(),
        action,
        task_id,
        project_id,
        group_kind,
        group_id,
        force,
        note,
        status: RemoteResumeRequestStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    }
}

#[tauri::command]
pub async fn get_remote_execution_resume_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteResumeRequestView, String> {
    let row = state
        .remote_execution_resume_request_repo
        .get(&request_id)
        .await
        .map_err(|_| REMOTE_RESUME_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_RESUME_REQUEST_NOT_FOUND.to_string())?;
    Ok(view(
        row.id,
        row.status,
        row.error_code,
        row.result,
        row.created_at,
        row.updated_at,
    ))
}

#[tauri::command]
pub async fn get_remote_task_action_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteResumeRequestView, String> {
    let row = state
        .remote_task_action_request_repo
        .get(&request_id)
        .await
        .map_err(|_| REMOTE_RESUME_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_RESUME_REQUEST_NOT_FOUND.to_string())?;
    Ok(view(
        row.id,
        row.status,
        row.error_code,
        row.result,
        row.created_at,
        row.updated_at,
    ))
}

fn view(
    id: String,
    status: RemoteResumeRequestStatus,
    error_code: Option<String>,
    result: Option<serde_json::Value>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
) -> RemoteResumeRequestView {
    RemoteResumeRequestView {
        request_id: id,
        status,
        error_code,
        result,
        created_at: created_at.to_rfc3339(),
        updated_at: updated_at.to_rfc3339(),
    }
}
