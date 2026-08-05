//! Spawn-free automation action intent twin. This module validates and persists identifiers only;
//! it has no AppHandle, ExecutionState, or ChatService access.

use crate::{
    application::AppState,
    domain::entities::{
        AutomationId, AutomationJudgeState, AutomationStatus, RemoteAutomationRunKind,
        RemoteAutomationRunRequest, RemoteAutomationRunRequestStatus,
    },
};
use serde::{Deserialize, Serialize};

pub const REMOTE_AUTOMATION_RUN_LOOKUP_FAILED: &str = "REMOTE_AUTOMATION_RUN_LOOKUP_FAILED";
pub const REMOTE_AUTOMATION_RUN_NOT_FOUND: &str = "REMOTE_AUTOMATION_RUN_NOT_FOUND";
pub const REMOTE_AUTOMATION_RUN_AUTHORITY_CHANGED: &str = "REMOTE_AUTOMATION_RUN_AUTHORITY_CHANGED";
pub const REMOTE_AUTOMATION_RUN_PLAN_GATE_PAUSED: &str = "REMOTE_AUTOMATION_RUN_PLAN_GATE_PAUSED";
pub const REMOTE_AUTOMATION_RUN_RUN_CHANGED: &str = "REMOTE_AUTOMATION_RUN_RUN_CHANGED";
pub const REMOTE_AUTOMATION_RUN_RUN_IN_FLIGHT: &str = "REMOTE_AUTOMATION_RUN_RUN_IN_FLIGHT";
pub const REMOTE_AUTOMATION_RUN_ALREADY_SETTLED: &str = "REMOTE_AUTOMATION_RUN_ALREADY_SETTLED";
pub const REMOTE_AUTOMATION_RUN_HOST_FAILED: &str = "REMOTE_AUTOMATION_RUN_HOST_FAILED";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAutomationRunInput {
    pub automation_id: String,
    pub kind: RemoteAutomationRunKind,
    pub expected_run_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAutomationRunIntentResponse {
    pub request_id: String,
    pub status: RemoteAutomationRunRequestStatus,
    pub deduplicated: bool,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAutomationRunRequestView {
    pub request_id: String,
    pub status: RemoteAutomationRunRequestStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn request_remote_automation_run_for_state(
    state: &AppState,
    input: RequestRemoteAutomationRunInput,
) -> Result<RemoteAutomationRunIntentResponse, String> {
    let id = AutomationId::from_string(input.automation_id.trim().to_string());
    let automation = state
        .automation_repo
        .get_by_id(&id)
        .await
        .map_err(|_| REMOTE_AUTOMATION_RUN_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_AUTOMATION_RUN_NOT_FOUND.to_string())?;
    validate_automation_status(&automation)?;
    let latest = state
        .automation_run_repo
        .latest_for_automation(&id)
        .await
        .map_err(|_| REMOTE_AUTOMATION_RUN_LOOKUP_FAILED.to_string())?;
    if let Some(expected) = input.expected_run_id.as_deref() {
        if latest.as_ref().map(|run| run.id.as_str()).as_deref() != Some(expected) {
            return Err(REMOTE_AUTOMATION_RUN_RUN_CHANGED.into());
        }
    }
    if input.kind == RemoteAutomationRunKind::RetryJudge
        && latest
            .as_ref()
            .is_none_or(|run| run.judge_state != AutomationJudgeState::Failed)
    {
        return Err(REMOTE_AUTOMATION_RUN_ALREADY_SETTLED.into());
    }
    if let Some(existing) = state
        .remote_automation_run_request_repo
        .find_unsettled(id.as_str(), input.kind)
        .await
        .map_err(|_| REMOTE_AUTOMATION_RUN_LOOKUP_FAILED.to_string())?
    {
        return Ok(response(existing, true));
    }
    let now = chrono::Utc::now();
    let row = RemoteAutomationRunRequest {
        id: uuid::Uuid::new_v4().to_string(),
        automation_id: id.as_str().to_string(),
        kind: input.kind,
        expected_run_id: input.expected_run_id,
        status: RemoteAutomationRunRequestStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    state
        .remote_automation_run_request_repo
        .create_remote_automation_run_request(row)
        .await
        .map(|row| response(row, false))
        .map_err(|_| REMOTE_AUTOMATION_RUN_LOOKUP_FAILED.into())
}
pub(crate) fn validate_automation_status(
    automation: &crate::domain::entities::Automation,
) -> Result<(), String> {
    match automation.status {
        AutomationStatus::Active => Ok(()),
        AutomationStatus::Paused
            if crate::application::automation::plan_gate::is_plan_gate_pause_reason(
                automation.paused_reason_code.as_deref(),
            ) =>
        {
            Err(REMOTE_AUTOMATION_RUN_PLAN_GATE_PAUSED.into())
        }
        AutomationStatus::Paused => Ok(()),
        _ => Err(REMOTE_AUTOMATION_RUN_AUTHORITY_CHANGED.into()),
    }
}
fn response(
    row: RemoteAutomationRunRequest,
    deduplicated: bool,
) -> RemoteAutomationRunIntentResponse {
    RemoteAutomationRunIntentResponse {
        request_id: row.id,
        status: row.status,
        deduplicated,
        created_at: row.created_at.to_rfc3339(),
    }
}
pub async fn get_remote_automation_run_request_for_state(
    state: &AppState,
    id: String,
) -> Result<RemoteAutomationRunRequestView, String> {
    let row = state
        .remote_automation_run_request_repo
        .get(&id)
        .await
        .map_err(|_| REMOTE_AUTOMATION_RUN_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_AUTOMATION_RUN_NOT_FOUND.to_string())?;
    Ok(RemoteAutomationRunRequestView {
        request_id: row.id,
        status: row.status,
        error_code: row.error_code,
        result: row.result,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    })
}
