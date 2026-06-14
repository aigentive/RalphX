use axum::{extract::State, http::StatusCode, Json};
use tracing::error;

use super::*;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{ProjectSkillId, ProjectSkillLifecycleStatus};
use crate::domain::repositories::ProjectSkillListOptions;
use crate::http_server::project_scope::{ProjectScope, ProjectScopeGuard};
use crate::http_server::types::HttpError;

pub async fn list_project_skills(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ListProjectSkillsRequest>,
) -> Result<Json<ListProjectSkillsResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let status = req
        .status
        .as_deref()
        .map(str::parse::<ProjectSkillLifecycleStatus>)
        .transpose()
        .map_err(|_| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("invalid project skill status".to_string()),
        })?;

    let skills = state
        .app_state
        .project_skill_repo
        .list_by_project(
            &project_id,
            ProjectSkillListOptions {
                status,
                include_archived: req.include_archived,
                stage: req.stage,
                bucket: req.bucket,
                scope_path: req.scope_path,
            },
        )
        .await
        .map_err(|error| {
            error!("failed to list project skills: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to list project skills".to_string()),
            }
        })?;

    let skills = skills
        .into_iter()
        .map(ProjectSkillResponse::from)
        .collect::<Vec<_>>();
    let count = skills.len();
    Ok(Json(ListProjectSkillsResponse { skills, count }))
}

pub async fn get_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<GetProjectSkillRequest>,
) -> Result<Json<GetProjectSkillResponse>, HttpError> {
    let skill_id = ProjectSkillId::from_string(req.project_skill_id);
    let skill = state
        .app_state
        .project_skill_repo
        .get_by_id(&skill_id)
        .await
        .map_err(|error| {
            error!("failed to get project skill: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to get project skill".to_string()),
            }
        })?;

    if let Some(skill) = skill {
        skill.assert_project_scope(&scope)?;
        return Ok(Json(GetProjectSkillResponse {
            skill: Some(ProjectSkillResponse::from(skill)),
        }));
    }

    Ok(Json(GetProjectSkillResponse { skill: None }))
}

fn assert_project_id_scope(project_id: &ProjectId, scope: &ProjectScope) -> Result<(), HttpError> {
    if let ProjectScope(Some(allowed)) = scope {
        if !allowed.contains(project_id) {
            return Err(HttpError {
                status: StatusCode::FORBIDDEN,
                message: Some("API key does not have access to this project".to_string()),
            });
        }
    }
    Ok(())
}
