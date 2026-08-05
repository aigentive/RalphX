//! Spawn-free remote automation draft intent validation and persistence.

use serde::{Deserialize, Serialize};

use crate::application::automation::decomposition_verifier::AutomationAuthoringMode;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceBranchMode, AutomationId, IdeationAnalysisBaseRefKind, ProjectId,
    RemoteAutomationDraftRequest, RemoteAutomationDraftRequestStatus,
};

pub const REMOTE_AUTOMATION_DRAFT_LOOKUP_FAILED: &str = "REMOTE_AUTOMATION_DRAFT_LOOKUP_FAILED";
pub const REMOTE_AUTOMATION_DRAFT_PROJECT_NOT_FOUND: &str =
    "REMOTE_AUTOMATION_DRAFT_PROJECT_NOT_FOUND";
pub const REMOTE_AUTOMATION_DRAFT_INVALID_INPUT: &str = "REMOTE_AUTOMATION_DRAFT_INVALID_INPUT";
pub const REMOTE_AUTOMATION_DRAFT_HOST_FAILED: &str = "REMOTE_AUTOMATION_DRAFT_HOST_FAILED";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAutomationDraftInput {
    pub project_id: String,
    pub name: String,
    pub authoring_mode: String,
    pub base_ref_kind: String,
    pub base_branch_mode: String,
    pub base_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAutomationDraftIntentResponse {
    pub request_id: String,
    pub automation_id: String,
    pub status: RemoteAutomationDraftRequestStatus,
    pub deduplicated: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAutomationDraftRequestView {
    pub request_id: String,
    pub automation_id: String,
    pub status: RemoteAutomationDraftRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn request_remote_automation_draft_for_state(
    state: &AppState,
    input: RequestRemoteAutomationDraftInput,
) -> Result<RemoteAutomationDraftIntentResponse, String> {
    let project_id = ProjectId::from_string(input.project_id.trim().to_string());
    if input.project_id.trim().is_empty() || input.name.trim().is_empty() {
        return Err(REMOTE_AUTOMATION_DRAFT_INVALID_INPUT.into());
    }
    let authoring_mode = AutomationAuthoringMode::parse(&input.authoring_mode)
        .ok_or_else(|| REMOTE_AUTOMATION_DRAFT_INVALID_INPUT.to_string())?;
    let base_ref_kind = input
        .base_ref_kind
        .trim()
        .parse::<IdeationAnalysisBaseRefKind>()
        .map_err(|_| REMOTE_AUTOMATION_DRAFT_INVALID_INPUT.to_string())?;
    let base_branch_mode = input
        .base_branch_mode
        .trim()
        .parse::<AgentConversationWorkspaceBranchMode>()
        .map_err(|_| REMOTE_AUTOMATION_DRAFT_INVALID_INPUT.to_string())?;
    state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|_| REMOTE_AUTOMATION_DRAFT_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_AUTOMATION_DRAFT_PROJECT_NOT_FOUND.to_string())?;
    let name = input.name.trim().to_string();
    if let Some(existing) = state
        .remote_automation_draft_request_repo
        .find_unsettled(project_id.as_str(), &name)
        .await
        .map_err(|_| REMOTE_AUTOMATION_DRAFT_LOOKUP_FAILED.to_string())?
    {
        return Ok(response(existing, true));
    }
    let now = chrono::Utc::now();
    let row = RemoteAutomationDraftRequest {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project_id.as_str().to_string(),
        automation_id: AutomationId::new().as_str().to_string(),
        name,
        authoring_mode: authoring_mode.as_str().to_string(),
        base_ref_kind: base_ref_kind.to_string(),
        base_branch_mode: base_branch_mode.to_string(),
        base_branch: input
            .base_branch
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        status: RemoteAutomationDraftRequestStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    state
        .remote_automation_draft_request_repo
        .create_remote_automation_draft_request(row)
        .await
        .map(|row| response(row, false))
        .map_err(|_| REMOTE_AUTOMATION_DRAFT_LOOKUP_FAILED.into())
}

fn response(
    row: RemoteAutomationDraftRequest,
    deduplicated: bool,
) -> RemoteAutomationDraftIntentResponse {
    RemoteAutomationDraftIntentResponse {
        request_id: row.id,
        automation_id: row.automation_id,
        status: row.status,
        deduplicated,
        created_at: row.created_at.to_rfc3339(),
    }
}

pub async fn get_remote_automation_draft_request_for_state(
    state: &AppState,
    id: String,
) -> Result<RemoteAutomationDraftRequestView, String> {
    let row = state
        .remote_automation_draft_request_repo
        .get(&id)
        .await
        .map_err(|_| REMOTE_AUTOMATION_DRAFT_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_AUTOMATION_DRAFT_LOOKUP_FAILED.to_string())?;
    Ok(RemoteAutomationDraftRequestView {
        request_id: row.id,
        automation_id: row.automation_id,
        status: row.status,
        error_code: row.error_code,
        result: row.result,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    })
}
