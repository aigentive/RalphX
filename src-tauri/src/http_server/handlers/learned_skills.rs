use axum::{extract::State, http::StatusCode, Json};
use std::sync::Arc;
use tracing::error;

use super::*;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{ProjectSkillId, ProjectSkillLifecycleStatus};
use crate::domain::repositories::ProjectSkillListOptions;
use crate::domain::services::{
    DistillEligibleOutcomesInput, ProjectSkillDistillationOrigin, ProjectSkillDistillerService,
    ProjectSkillService,
};
use crate::error::AppError;
use crate::http_server::handlers::learned_skills_export::assert_project_id_scope;
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

pub async fn pin_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ProjectSkillLifecycleRequest>,
) -> Result<Json<ProjectSkillLifecycleResponse>, HttpError> {
    update_project_skill_pin(state, scope, req.project_skill_id, true).await
}

pub async fn unpin_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ProjectSkillLifecycleRequest>,
) -> Result<Json<ProjectSkillLifecycleResponse>, HttpError> {
    update_project_skill_pin(state, scope, req.project_skill_id, false).await
}

pub async fn distill_project_skills(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<DistillProjectSkillsRequest>,
) -> Result<Json<DistillProjectSkillsResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;

    let distiller = ProjectSkillDistillerService::new(
        Arc::clone(&state.app_state.task_outcome_repo),
        Arc::clone(&state.app_state.project_skill_repo),
    );
    let result = distiller
        .distill_eligible_outcomes(DistillEligibleOutcomesInput {
            project_id,
            source: req.source,
            limit: req.limit.unwrap_or(25),
            origin: ProjectSkillDistillationOrigin::ManualCurator,
        })
        .await
        .map_err(|error| {
            error!("failed to distill project skills: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to distill project skills".to_string()),
            }
        })?;

    Ok(Json(DistillProjectSkillsResponse {
        staged_skills: result
            .staged_skills
            .into_iter()
            .map(ProjectSkillResponse::from)
            .collect(),
        skipped_existing: result.skipped_existing,
    }))
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
            error!(
                "failed to get project skill before lifecycle update: {}",
                error
            );
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

async fn update_project_skill_pin(
    state: HttpServerState,
    scope: ProjectScope,
    project_skill_id: String,
    pinned: bool,
) -> Result<Json<ProjectSkillLifecycleResponse>, HttpError> {
    let skill_id = ProjectSkillId::from_string(project_skill_id);
    let existing = state
        .app_state
        .project_skill_repo
        .get_by_id(&skill_id)
        .await
        .map_err(|error| {
            error!("failed to get project skill before pin update: {}", error);
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
    let updated = if pinned {
        service.pin_skill(&skill_id).await
    } else {
        service.unpin_skill(&skill_id).await
    }
    .map_err(|error| match error {
        AppError::Validation(message) => HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(message),
        },
        other => {
            error!("failed to update project skill pin state: {}", other);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to update project skill pin state".to_string()),
            }
        }
    })?;

    Ok(Json(ProjectSkillLifecycleResponse {
        skill: updated.map(ProjectSkillResponse::from),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{AppState, TeamService, TeamStateTracker};
    use crate::commands::ExecutionState;
    use crate::domain::entities::{ProjectSkill, ProjectSkillLifecycleStatus, TaskOutcomeStatus};
    use crate::domain::repositories::UpsertTaskOutcomeInput;
    use crate::domain::services::new_empty_task_outcome;

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

    #[tokio::test]
    async fn pin_project_skill_handler_requires_approved_skill() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-pin".to_string());
        let staged = staged_skill(project_id.clone());
        let skill_id = staged.id.clone();
        app_state.project_skill_repo.create(staged).await.unwrap();

        let error = pin_project_skill(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(ProjectSkillLifecycleRequest {
                project_skill_id: skill_id.as_str().to_string(),
            }),
        )
        .await
        .expect_err("unapproved pin should fail");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn pin_project_skill_handler_updates_pin_state() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-pin".to_string());
        let mut skill = staged_skill(project_id.clone());
        skill.status = ProjectSkillLifecycleStatus::Approved;
        let skill_id = skill.id.clone();
        app_state.project_skill_repo.create(skill).await.unwrap();

        let pinned = pin_project_skill(
            State(test_state(Arc::clone(&app_state))),
            ProjectScope(Some(vec![project_id.clone()])),
            Json(ProjectSkillLifecycleRequest {
                project_skill_id: skill_id.as_str().to_string(),
            }),
        )
        .await
        .unwrap()
        .0
        .skill
        .expect("pinned skill");
        assert!(pinned.pinned);

        let unpinned = unpin_project_skill(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(ProjectSkillLifecycleRequest {
                project_skill_id: skill_id.as_str().to_string(),
            }),
        )
        .await
        .unwrap()
        .0
        .skill
        .expect("unpinned skill");
        assert!(!unpinned.pinned);
    }

    #[tokio::test]
    async fn distill_project_skills_stages_eligible_outcomes() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-distill".to_string());
        let mut outcome =
            new_empty_task_outcome(project_id.clone(), "review", "review_note", "review-1");
        outcome.status = TaskOutcomeStatus::Eligible;
        outcome.outcome_class = Some("review_changes_requested".to_string());
        app_state
            .task_outcome_repo
            .upsert(UpsertTaskOutcomeInput { outcome })
            .await
            .unwrap();

        let response = distill_project_skills(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(DistillProjectSkillsRequest {
                project_id: "project-distill".to_string(),
                source: Some("review".to_string()),
                limit: Some(5),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.0.staged_skills.len(), 1);
        assert_eq!(response.0.staged_skills[0].status, "staged");
        assert_eq!(response.0.staged_skills[0].bucket, "review");
        assert_eq!(response.0.skipped_existing, 0);
    }

    #[tokio::test]
    async fn distill_project_skills_rejects_cross_project_scope() {
        let app_state = Arc::new(AppState::new_test());
        let error = distill_project_skills(
            State(test_state(app_state)),
            ProjectScope(Some(vec![ProjectId::from_string("project-b".to_string())])),
            Json(DistillProjectSkillsRequest {
                project_id: "project-a".to_string(),
                source: None,
                limit: None,
            }),
        )
        .await
        .expect_err("cross-project distill should fail");

        assert_eq!(error.status, StatusCode::FORBIDDEN);
    }
}
