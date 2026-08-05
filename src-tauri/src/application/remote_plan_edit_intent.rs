//! Spawn-free remote intent. The host pins caller/provenance fields absent when dispatching.
use crate::{
    application::AppState,
    domain::entities::{RemotePlanEditRequest, RemotePlanEditRequestStatus},
    infrastructure::sqlite::{
        SqliteArtifactRepository as ArtifactRepo, SqliteIdeationSessionRepository as SessionRepo,
    },
};
use serde::{Deserialize, Serialize};
pub const REMOTE_PLAN_EDIT_LOOKUP_FAILED: &str = "REMOTE_PLAN_EDIT_LOOKUP_FAILED";
pub const REMOTE_PLAN_EDIT_NOT_FOUND: &str = "REMOTE_PLAN_EDIT_NOT_FOUND";
pub const REMOTE_PLAN_EDIT_AUTHORITY_CHANGED: &str = "REMOTE_PLAN_EDIT_AUTHORITY_CHANGED";
pub const REMOTE_PLAN_EDIT_CONTENT_REQUIRED: &str = "REMOTE_PLAN_EDIT_CONTENT_REQUIRED";
pub const REMOTE_PLAN_EDIT_VERSION_CONFLICT: &str = "REMOTE_PLAN_EDIT_VERSION_CONFLICT";
pub const REMOTE_PLAN_EDIT_ENQUEUE_FAILED: &str = "REMOTE_PLAN_EDIT_ENQUEUE_FAILED";
pub const REMOTE_PLAN_EDIT_REQUEST_NOT_FOUND: &str = "REMOTE_PLAN_EDIT_REQUEST_NOT_FOUND";
pub const REMOTE_PLAN_EDIT_HOST_FAILED: &str = "REMOTE_PLAN_EDIT_HOST_FAILED";
pub const REMOTE_PLAN_EDIT_FROZEN: &str = "REMOTE_PLAN_EDIT_FROZEN";
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemotePlanEditInput {
    pub artifact_id: String,
    pub content: String,
    pub expected_version: u32,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemotePlanEditIntentResponse {
    pub request_id: String,
    pub status: RemotePlanEditRequestStatus,
    pub deduplicated: bool,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemotePlanEditRequestView {
    pub request_id: String,
    pub status: RemotePlanEditRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}
pub async fn request_remote_plan_edit_for_state(
    state: &AppState,
    input: RequestRemotePlanEditInput,
) -> Result<RemotePlanEditIntentResponse, String> {
    if input.content.trim().is_empty() {
        return Err(REMOTE_PLAN_EDIT_CONTENT_REQUIRED.into());
    }
    let input_id = input.artifact_id.clone();
    let (resolved, current_version, mutable) = state
        .db
        .run(move |conn| {
            let resolved = ArtifactRepo::resolve_latest_sync(conn, &input_id)?;
            let artifact = ArtifactRepo::get_by_id_sync(conn, &resolved)?
                .ok_or_else(|| crate::error::AppError::NotFound(resolved.clone()))?;
            let mut sessions = SessionRepo::get_by_plan_artifact_id_sync(conn, &resolved)?;
            sessions.extend(SessionRepo::get_by_plan_blueprint_artifact_id_sync(
                conn, &resolved,
            )?);
            let mutable = sessions.first().is_some_and(|s| {
                s.status == crate::domain::entities::IdeationSessionStatus::Active
            });
            Ok((resolved, artifact.metadata.version, mutable))
        })
        .await
        .map_err(|e| {
            if matches!(e, crate::error::AppError::NotFound(_)) {
                REMOTE_PLAN_EDIT_NOT_FOUND.to_string()
            } else {
                REMOTE_PLAN_EDIT_LOOKUP_FAILED.to_string()
            }
        })?;
    if !mutable {
        return Err(REMOTE_PLAN_EDIT_AUTHORITY_CHANGED.into());
    }
    if current_version != input.expected_version {
        return Err(REMOTE_PLAN_EDIT_VERSION_CONFLICT.into());
    }
    if let Some(existing) = state
        .remote_plan_edit_request_repo
        .find_unsettled_for_artifact(&resolved)
        .await
        .map_err(|_| REMOTE_PLAN_EDIT_LOOKUP_FAILED.to_string())?
    {
        if existing.content == input.content && existing.expected_version == input.expected_version
        {
            return Ok(response(existing, true));
        }
        return Err(REMOTE_PLAN_EDIT_AUTHORITY_CHANGED.into());
    }
    let now = chrono::Utc::now();
    let row = RemotePlanEditRequest {
        id: uuid::Uuid::new_v4().to_string(),
        artifact_id: resolved,
        content: input.content,
        expected_version: input.expected_version,
        status: RemotePlanEditRequestStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    state
        .remote_plan_edit_request_repo
        .create_remote_plan_edit_request(row)
        .await
        .map(|v| response(v, false))
        .map_err(|_| REMOTE_PLAN_EDIT_ENQUEUE_FAILED.into())
}
fn response(row: RemotePlanEditRequest, deduplicated: bool) -> RemotePlanEditIntentResponse {
    RemotePlanEditIntentResponse {
        request_id: row.id,
        status: row.status,
        deduplicated,
        created_at: row.created_at.to_rfc3339(),
    }
}
pub async fn get_remote_plan_edit_request_for_state(
    state: &AppState,
    request_id: String,
) -> Result<RemotePlanEditRequestView, String> {
    let row = state
        .remote_plan_edit_request_repo
        .get(&request_id)
        .await
        .map_err(|_| REMOTE_PLAN_EDIT_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_PLAN_EDIT_REQUEST_NOT_FOUND.to_string())?;
    Ok(RemotePlanEditRequestView {
        request_id: row.id,
        status: row.status,
        error_code: row.error_code,
        result: row.result,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    })
}
