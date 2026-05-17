use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use super::*;
use crate::domain::entities::{
    Artifact, ArtifactContent, ArtifactId, ArtifactSummary, ArtifactType, TaskContext, TaskId,
};
use crate::http_server::project_scope::{ProjectScope, ProjectScopeGuard};
use tracing::error;

pub async fn get_task_context(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Path(task_id): Path<String>,
) -> Result<Json<TaskContext>, StatusCode> {
    let task_id = TaskId::from_string(task_id);

    // Get task context using helper function
    let context = get_task_context_impl(&state.app_state, &task_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Enforce project scope (no-op for internal requests without the header)
    context
        .task
        .assert_project_scope(&scope)
        .map_err(|e| e.status)?;

    Ok(Json(context))
}

pub async fn get_artifact_full(
    State(state): State<HttpServerState>,
    Path(artifact_id): Path<String>,
) -> Result<Json<ArtifactResponse>, StatusCode> {
    let artifact_id = ArtifactId::from_string(artifact_id);

    let artifact = state
        .app_state
        .artifact_repo
        .get_by_id(&artifact_id)
        .await
        .map_err(|e| {
            error!("Failed to get artifact {}: {}", artifact_id.as_str(), e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ArtifactResponse::from(artifact)))
}

pub async fn get_artifact_version(
    State(state): State<HttpServerState>,
    Path((artifact_id, version)): Path<(String, u32)>,
) -> Result<Json<ArtifactResponse>, StatusCode> {
    let artifact_id = ArtifactId::from_string(artifact_id);

    let artifact = state
        .app_state
        .artifact_repo
        .get_by_id_at_version(&artifact_id, version)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ArtifactResponse::from(artifact)))
}

pub async fn get_related_artifacts(
    State(state): State<HttpServerState>,
    Path(artifact_id): Path<String>,
) -> Result<Json<Vec<ArtifactSummary>>, StatusCode> {
    let artifact_id = ArtifactId::from_string(artifact_id);

    let related = state
        .app_state
        .artifact_repo
        .get_related(&artifact_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Convert to ArtifactSummary with preview
    let summaries: Vec<ArtifactSummary> = related
        .into_iter()
        .map(|artifact| {
            let content_preview = create_artifact_preview(&artifact);
            ArtifactSummary {
                id: artifact.id.clone(),
                title: artifact.name.clone(),
                artifact_type: artifact.artifact_type,
                current_version: artifact.metadata.version,
                content_preview,
            }
        })
        .collect();

    Ok(Json(summaries))
}

pub async fn search_artifacts(
    State(state): State<HttpServerState>,
    Json(req): Json<SearchArtifactsRequest>,
) -> Result<Json<Vec<ArtifactSummary>>, StatusCode> {
    // For MVP, implement basic search by getting all artifacts and filtering
    // Get all artifacts (we don't have project filtering yet, so get by type)
    let all_artifacts: Vec<Artifact> = if let Some(types) = req.artifact_types {
        let mut results = Vec::new();
        for type_str in types {
            if let Ok(artifact_type) = parse_artifact_type(&type_str) {
                let artifacts = state
                    .app_state
                    .artifact_repo
                    .get_by_type(artifact_type)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                results.extend(artifacts);
            }
        }
        results
    } else {
        // If no type filter, we need to get all types
        let mut results = Vec::new();
        for artifact_type in [
            ArtifactType::Prd,
            ArtifactType::ResearchDocument,
            ArtifactType::DesignDoc,
            ArtifactType::Specification,
            ArtifactType::CodeChange,
            ArtifactType::Diff,
            ArtifactType::TestResult,
            ArtifactType::TaskSpec,
            ArtifactType::ReviewFeedback,
            ArtifactType::Approval,
            ArtifactType::Findings,
            ArtifactType::Recommendations,
            ArtifactType::Context,
            ArtifactType::PreviousWork,
            ArtifactType::ResearchBrief,
        ] {
            let artifacts = state
                .app_state
                .artifact_repo
                .get_by_type(artifact_type)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            results.extend(artifacts);
        }
        results
    };

    // Filter by query (case-insensitive search in name and content)
    let query_lower = req.query.to_lowercase();
    let filtered: Vec<Artifact> = all_artifacts
        .into_iter()
        .filter(|artifact| {
            let name_matches = artifact.name.to_lowercase().contains(&query_lower);
            let content_matches = match &artifact.content {
                ArtifactContent::Inline { text } => text.to_lowercase().contains(&query_lower),
                ArtifactContent::File { path } => path.to_lowercase().contains(&query_lower),
            };
            name_matches || content_matches
        })
        .collect();

    // Convert to ArtifactSummary
    let summaries: Vec<ArtifactSummary> = filtered
        .into_iter()
        .map(|artifact| {
            let content_preview = create_artifact_preview(&artifact);
            ArtifactSummary {
                id: artifact.id.clone(),
                title: artifact.name.clone(),
                artifact_type: artifact.artifact_type,
                current_version: artifact.metadata.version,
                content_preview,
            }
        })
        .collect();

    Ok(Json(summaries))
}
