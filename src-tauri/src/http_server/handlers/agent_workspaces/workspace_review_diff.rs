use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};

use super::workspace_review_context::workspace_review_runtime_header;
use super::*;

use crate::application::agent_workspace_review::{
    apply_workspace_review_runtime_authority, load_current_workspace_review_eligible,
    lock_workspace_review_lifecycle,
};
use crate::application::agent_workspace_review_diff::{
    get_workspace_review_diff_page, list_workspace_review_files, AgentWorkspaceReviewDiffPage,
    AgentWorkspaceReviewFilePage,
};
use crate::domain::entities::{AgentWorkspaceReviewRuntimeState, Project};

#[derive(Debug, serde::Deserialize, Default)]
pub struct ListAgentWorkspaceReviewFilesQuery {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct GetAgentWorkspaceReviewDiffPageQuery {
    pub cursor: Option<String>,
    pub path: Option<String>,
    pub source: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, serde::Serialize)]
pub struct ListAgentWorkspaceReviewFilesResponse {
    pub success: bool,
    #[serde(flatten)]
    pub page: AgentWorkspaceReviewFilePage,
}

#[derive(Debug, serde::Serialize)]
pub struct GetAgentWorkspaceReviewDiffPageResponse {
    pub success: bool,
    #[serde(flatten)]
    pub diff: AgentWorkspaceReviewDiffPage,
}

/// GET /api/agent-workspaces/{conversation_id}/workspace-review-files
pub async fn list_agent_workspace_review_files(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<ListAgentWorkspaceReviewFilesQuery>,
) -> Result<Json<ListAgentWorkspaceReviewFilesResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let _lifecycle_guard = lock_workspace_review_lifecycle(&conversation_id).await;
    let (workspace, project) =
        authorized_workspace_review_diff_context(&state, &conversation_id, &headers).await?;
    let page =
        list_workspace_review_files(&workspace, &project, query.cursor.as_deref(), query.limit)
            .await
            .map_err(workspace_review_diff_error)?;
    Ok(Json(ListAgentWorkspaceReviewFilesResponse {
        success: true,
        page,
    }))
}

/// GET /api/agent-workspaces/{conversation_id}/workspace-review-diff-page
pub async fn get_agent_workspace_review_diff_page(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<GetAgentWorkspaceReviewDiffPageQuery>,
) -> Result<Json<GetAgentWorkspaceReviewDiffPageResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let _lifecycle_guard = lock_workspace_review_lifecycle(&conversation_id).await;
    let (workspace, project) =
        authorized_workspace_review_diff_context(&state, &conversation_id, &headers).await?;
    let diff = get_workspace_review_diff_page(
        &workspace,
        &project,
        query.cursor.as_deref(),
        query.path.as_deref(),
        query.source.as_deref(),
        query.limit,
    )
    .await
    .map_err(workspace_review_diff_error)?;
    Ok(Json(GetAgentWorkspaceReviewDiffPageResponse {
        success: true,
        diff,
    }))
}

async fn authorized_workspace_review_diff_context(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    headers: &HeaderMap,
) -> Result<(AgentConversationWorkspace, Project), JsonError> {
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), conversation_id).await?;
    let workspace = load_current_workspace_review_eligible(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_action_error)?;
    let project = state
        .app_state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                format!("Project not found: {}", workspace.project_id),
                None,
            )
        })?;
    let mut context = load_agent_workspace_review_context(state.app_state.as_ref(), &workspace)
        .await
        .map_err(workspace_review_diff_error)?;
    let caller_run_id = workspace_review_runtime_header(headers, "x-ralphx-agent-run-id");
    let caller_conversation_id =
        workspace_review_runtime_header(headers, "x-ralphx-conversation-id");
    apply_workspace_review_runtime_authority(
        state.app_state.as_ref(),
        &mut context,
        caller_run_id.as_deref(),
        caller_conversation_id.as_deref(),
    )
    .await
    .map_err(workspace_review_diff_error)?;
    if !context.can_mutate_review_state
        || context.review_runtime_state != AgentWorkspaceReviewRuntimeState::ActiveOwned
    {
        return Err(json_error(
            StatusCode::CONFLICT,
            format!(
                "Workspace Review diff access requires the active owned reviewer runtime (state: {})",
                context.review_runtime_state
            ),
            None,
        ));
    }
    if context.target.is_none() {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Workspace Review target is no longer current",
            None,
        ));
    }
    Ok((workspace, project))
}

fn workspace_review_diff_error(error: AppError) -> JsonError {
    let status = match &error {
        AppError::Validation(_) => StatusCode::BAD_REQUEST,
        AppError::Conflict(_) => StatusCode::CONFLICT,
        AppError::NotFound(_) | AppError::ProjectNotFound(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    json_error(status, error.to_string(), None)
}
