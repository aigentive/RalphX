use axum::{extract::State, http::StatusCode, Json};
use std::sync::Arc;
use tracing::error;

use super::*;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{ProjectSkillId, ProjectSkillLifecycleStatus};
use crate::domain::repositories::ProjectSkillListOptions;
use crate::domain::services::ProjectSkillService;
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

pub async fn approve_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ProjectSkillLifecycleRequest>,
) -> Result<Json<ProjectSkillLifecycleResponse>, HttpError> {
    update_project_skill_lifecycle(
        state,
        scope,
        req.project_skill_id,
        ProjectSkillLifecycleStatus::Approved,
    )
    .await
}

pub async fn reject_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ProjectSkillLifecycleRequest>,
) -> Result<Json<ProjectSkillLifecycleResponse>, HttpError> {
    update_project_skill_lifecycle(
        state,
        scope,
        req.project_skill_id,
        ProjectSkillLifecycleStatus::Rejected,
    )
    .await
}

pub async fn archive_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ProjectSkillLifecycleRequest>,
) -> Result<Json<ProjectSkillLifecycleResponse>, HttpError> {
    update_project_skill_lifecycle(
        state,
        scope,
        req.project_skill_id,
        ProjectSkillLifecycleStatus::Archived,
    )
    .await
}

async fn update_project_skill_lifecycle(
    state: HttpServerState,
    scope: ProjectScope,
    project_skill_id: String,
    status: ProjectSkillLifecycleStatus,
) -> Result<Json<ProjectSkillLifecycleResponse>, HttpError> {
    let skill_id = ProjectSkillId::from_string(project_skill_id);
    let existing = state
        .app_state
        .project_skill_repo
        .get_by_id(&skill_id)
        .await
        .map_err(|error| {
            error!("failed to get project skill before lifecycle update: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to get project skill".to_string()),
            }
        })?;

    let Some(existing) = existing else {
        return Ok(Json(ProjectSkillLifecycleResponse { skill: None }));
    };
    existing.assert_project_scope(&scope)?;

    let service = ProjectSkillService::new(Arc::clone(&state.app_state.project_skill_repo));
    let updated = match status {
        ProjectSkillLifecycleStatus::Approved => service.approve_skill(&skill_id).await,
        ProjectSkillLifecycleStatus::Rejected => service.reject_skill(&skill_id).await,
        ProjectSkillLifecycleStatus::Archived => service.archive_skill(&skill_id).await,
        ProjectSkillLifecycleStatus::Staged | ProjectSkillLifecycleStatus::Retired => {
            return Err(HttpError {
                status: StatusCode::BAD_REQUEST,
                message: Some("unsupported project skill lifecycle transition".to_string()),
            });
        }
    }
    .map_err(|error| {
        error!("failed to update project skill lifecycle: {}", error);
        HttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: Some("failed to update project skill lifecycle".to_string()),
        }
    })?;

    Ok(Json(ProjectSkillLifecycleResponse {
        skill: updated.map(ProjectSkillResponse::from),
    }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{AppState, TeamService, TeamStateTracker};
    use crate::commands::ExecutionState;
    use crate::domain::entities::{ProjectSkill, ProjectSkillLifecycleStatus};

    fn test_state(app_state: Arc<AppState>) -> HttpServerState {
        let tracker = TeamStateTracker::new();
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
        HttpServerState {
            app_state,
            execution_state: Arc::new(ExecutionState::new()),
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        }
    }

    fn staged_skill(project_id: ProjectId) -> ProjectSkill {
        let now = chrono::Utc::now();
        ProjectSkill {
            id: ProjectSkillId::new(),
            project_id,
            title: "Review repeat failures".to_string(),
            bucket: "review".to_string(),
            stage: "review".to_string(),
            status: ProjectSkillLifecycleStatus::Staged,
            pinned: false,
            archived: false,
            scope_paths: Vec::new(),
            compact_guidance: "Check repeated review failures before approving.".to_string(),
            body_markdown: "Detailed guidance".to_string(),
            predicted_effect: Some("Reduces repeated review changes.".to_string()),
            provenance_json: serde_json::json!({ "test": true }),
            companion_of_skill_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn approve_project_skill_handler_requires_scope_and_updates_status() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-skill-test".to_string());
        let skill = staged_skill(project_id.clone());
        let skill_id = skill.id.clone();
        app_state.project_skill_repo.create(skill).await.unwrap();

        let response = approve_project_skill(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(ProjectSkillLifecycleRequest {
                project_skill_id: skill_id.as_str().to_string(),
            }),
        )
        .await
        .unwrap();

        let updated = response.0.skill.expect("updated skill");
        assert_eq!(updated.status, "approved");
    }

    #[tokio::test]
    async fn approve_project_skill_handler_rejects_cross_project_scope() {
        let app_state = Arc::new(AppState::new_test());
        let skill = staged_skill(ProjectId::from_string("project-a".to_string()));
        let skill_id = skill.id.clone();
        app_state.project_skill_repo.create(skill).await.unwrap();

        let error = approve_project_skill(
            State(test_state(app_state)),
            ProjectScope(Some(vec![ProjectId::from_string("project-b".to_string())])),
            Json(ProjectSkillLifecycleRequest {
                project_skill_id: skill_id.as_str().to_string(),
            }),
        )
        .await
        .expect_err("cross-project approval should fail");

        assert_eq!(error.status, StatusCode::FORBIDDEN);
    }
}
