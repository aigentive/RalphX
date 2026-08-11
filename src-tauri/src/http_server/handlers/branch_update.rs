use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::HttpServerState;
use crate::domain::entities::{
    BranchUpdateFailureKind, BranchUpdatePhase, GitTargetLeaseOwner, InternalStatus, TaskId,
};
use crate::domain::repositories::{BlockBranchUpdate, BranchUpdateCasOutcome};

type HandlerError = (StatusCode, Json<serde_json::Value>);

#[derive(Debug, Serialize)]
pub struct BranchUpdateContextResponse {
    pub operation_id: String,
    pub direction: String,
    pub phase: String,
    pub source_branch: String,
    pub target_branch: String,
    pub workspace_path: Option<String>,
    pub conflict_files: Vec<String>,
    pub continuation: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct CompleteBranchUpdateRequest {}

#[derive(Debug, Deserialize)]
pub struct ReportBranchUpdateRequest {
    pub reason: String,
    #[serde(default)]
    pub conflict_files: Vec<String>,
    pub diagnostic_info: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BranchUpdateResponse {
    pub success: bool,
    pub new_status: String,
}

fn error(status: StatusCode, message: impl Into<String>) -> HandlerError {
    (status, Json(serde_json::json!({ "error": message.into() })))
}

fn is_update_status(status: InternalStatus) -> bool {
    matches!(
        status,
        InternalStatus::UpdatingPlanBranch | InternalStatus::UpdatingTaskBranch
    )
}

async fn active(
    state: &HttpServerState,
    task_id: &TaskId,
) -> Result<
    (
        crate::domain::entities::Task,
        crate::domain::entities::BranchUpdateOperation,
    ),
    HandlerError,
> {
    let task = state
        .app_state
        .task_repo
        .get_by_id(task_id)
        .await
        .map_err(|source_error| error(StatusCode::INTERNAL_SERVER_ERROR, source_error.to_string()))?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Task not found"))?;
    let operation = state
        .app_state
        .branch_update_repo
        .get_active_operation(task_id)
        .await
        .map_err(|source_error| error(StatusCode::INTERNAL_SERVER_ERROR, source_error.to_string()))?
        .ok_or_else(|| error(StatusCode::CONFLICT, "No active branch update"))?;
    Ok((task, operation))
}

async fn authorize_bound_run(
    state: &HttpServerState,
    operation: &crate::domain::entities::BranchUpdateOperation,
    headers: &HeaderMap,
) -> Result<(), HandlerError> {
    let run_id = headers
        .get("x-ralphx-agent-run-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error(
                StatusCode::UNAUTHORIZED,
                "Missing branch-updater run authority",
            )
        })?;
    let conversation_id = headers
        .get("x-ralphx-conversation-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error(
                StatusCode::UNAUTHORIZED,
                "Missing branch-updater conversation authority",
            )
        })?;
    if operation.agent_run_id.as_deref() != Some(run_id)
        || operation.conversation_id.as_deref() != Some(conversation_id)
    {
        return Err(error(
            StatusCode::CONFLICT,
            "Stale branch-updater run authority",
        ));
    }
    let run = state
        .app_state
        .agent_run_repo
        .get_by_id(&crate::domain::entities::AgentRunId::from_string(run_id))
        .await
        .map_err(|source_error| error(StatusCode::INTERNAL_SERVER_ERROR, source_error.to_string()))?
        .ok_or_else(|| error(StatusCode::CONFLICT, "Bound branch-updater run is missing"))?;
    if run.conversation_id.as_str() != conversation_id
        || run.status != crate::domain::entities::AgentRunStatus::Running
    {
        return Err(error(
            StatusCode::CONFLICT,
            "Branch-updater run is not current and running",
        ));
    }
    Ok(())
}

pub async fn get_branch_update_context(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<BranchUpdateContextResponse>, HandlerError> {
    let (_, operation) = active(&state, &TaskId::from_string(task_id)).await?;
    authorize_bound_run(&state, &operation, &headers).await?;
    Ok(Json(BranchUpdateContextResponse {
        operation_id: operation.id.as_str().to_string(),
        direction: operation.direction.as_str().to_string(),
        phase: operation.phase.as_str().to_string(),
        source_branch: operation.source_branch,
        target_branch: operation.target_branch,
        workspace_path: operation
            .workspace_path
            .map(|path| path.to_string_lossy().into_owned()),
        conflict_files: operation
            .conflict_files
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        continuation: operation.continuation.as_str().to_string(),
    }))
}

pub async fn complete_branch_update(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
    headers: HeaderMap,
    Json(_request): Json<CompleteBranchUpdateRequest>,
) -> Result<Json<BranchUpdateResponse>, HandlerError> {
    let task_id = TaskId::from_string(task_id);
    let (task, operation) = active(&state, &task_id).await?;
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state.app_state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await
        .map_err(|source_error| error(StatusCode::CONFLICT, source_error.to_string()))?;
    authorize_bound_run(&state, &operation, &headers).await?;
    if !is_update_status(task.internal_status) || operation.phase != BranchUpdatePhase::Resolving {
        return Err(error(
            StatusCode::CONFLICT,
            "Branch update is not completable",
        ));
    }
    let project = state
        .app_state
        .project_repo
        .get_by_id(&task.project_id)
        .await
        .map_err(|source_error| error(StatusCode::INTERNAL_SERVER_ERROR, source_error.to_string()))?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Project not found"))?;
    let mut next_status =
        crate::application::branch_update_executor::complete_resolved_branch_update(
            state.app_state.branch_update_repo.clone(),
            state.app_state.task_repo.clone(),
            std::path::Path::new(&project.working_directory),
            &operation,
            task.internal_status,
        )
        .await
        .map_err(|source_error| error(StatusCode::CONFLICT, source_error.to_string()))?;
    if operation.continuation
        == crate::domain::entities::BranchUpdateContinuation::FinalizePostMergePrPublication
    {
        let pending = state
            .app_state
            .branch_update_repo
            .get_operation(&operation.id)
            .await
            .map_err(|source_error| {
                error(StatusCode::INTERNAL_SERVER_ERROR, source_error.to_string())
            })?
            .ok_or_else(|| error(StatusCode::CONFLICT, "Branch update operation disappeared"))?;
        next_status = crate::application::branch_update_executor::publish_post_merge_branch_update(
            state.app_state.branch_update_repo.clone(),
            std::path::Path::new(&project.working_directory),
            &pending,
            task.internal_status,
        )
        .await
        .map_err(|source_error| error(StatusCode::CONFLICT, source_error.to_string()))?;
        if let Some(plan_branch) =
            crate::domain::state_machine::transition_handler::resolve_task_plan_branch_record(
                &task,
                &state.app_state.plan_branch_repo,
            )
            .await
        {
            let _ = state
                .app_state
                .plan_branch_repo
                .update_pr_push_status(
                    &plan_branch.id,
                    crate::domain::entities::plan_branch::PrPushStatus::Pushed,
                )
                .await;
        }
    }
    if next_status != InternalStatus::Merged {
        let continued = state
            .app_state
            .task_repo
            .get_by_id(&task.id)
            .await
            .map_err(|source_error| {
                error(StatusCode::INTERNAL_SERVER_ERROR, source_error.to_string())
            })?
            .ok_or_else(|| error(StatusCode::CONFLICT, "Continued task disappeared"))?;
        let transition_service = state
            .app_state
            .build_transition_service_with_execution_state(Arc::clone(&state.execution_state));
        transition_service
            .execute_entry_actions(&continued.id, &continued, next_status)
            .await;
    }
    Ok(Json(BranchUpdateResponse {
        success: true,
        new_status: next_status.as_str().to_string(),
    }))
}

async fn report(
    state: HttpServerState,
    task_id: String,
    headers: HeaderMap,
    request: ReportBranchUpdateRequest,
    failure_kind: BranchUpdateFailureKind,
) -> Result<Json<BranchUpdateResponse>, HandlerError> {
    let task_id = TaskId::from_string(task_id);
    let (task, operation) = active(&state, &task_id).await?;
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state.app_state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await
        .map_err(|source_error| error(StatusCode::CONFLICT, source_error.to_string()))?;
    authorize_bound_run(&state, &operation, &headers).await?;
    if !is_update_status(task.internal_status) {
        return Err(error(StatusCode::CONFLICT, "Task is not updating a branch"));
    }
    let owner = GitTargetLeaseOwner::branch_update(task_id.as_str(), operation.id.as_str());
    let result = state
        .app_state
        .branch_update_repo
        .block_operation(BlockBranchUpdate {
            operation_id: operation.id,
            task_id,
            originating_history_id: operation.originating_history_id,
            update_status: task.internal_status,
            owner,
            fencing_epoch: operation.target_lease_epoch,
            failure_kind,
            diagnostics: request
                .diagnostic_info
                .map(|diagnostics| format!("{}\n{}", request.reason, diagnostics))
                .unwrap_or(request.reason),
            conflict_files: request.conflict_files.into_iter().map(Into::into).collect(),
        })
        .await
        .map_err(|source_error| {
            error(StatusCode::INTERNAL_SERVER_ERROR, source_error.to_string())
        })?;
    if result != BranchUpdateCasOutcome::Applied {
        return Err(error(
            StatusCode::CONFLICT,
            format!("Stale branch update: {result:?}"),
        ));
    }
    Ok(Json(BranchUpdateResponse {
        success: true,
        new_status: InternalStatus::BranchUpdateBlocked.as_str().to_string(),
    }))
}

pub async fn report_branch_update_conflict(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ReportBranchUpdateRequest>,
) -> Result<Json<BranchUpdateResponse>, HandlerError> {
    report(
        state,
        task_id,
        headers,
        request,
        BranchUpdateFailureKind::Conflict,
    )
    .await
}

pub async fn report_branch_update_incomplete(
    State(state): State<HttpServerState>,
    Path(task_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ReportBranchUpdateRequest>,
) -> Result<Json<BranchUpdateResponse>, HandlerError> {
    report(
        state,
        task_id,
        headers,
        request,
        BranchUpdateFailureKind::Incomplete,
    )
    .await
}
