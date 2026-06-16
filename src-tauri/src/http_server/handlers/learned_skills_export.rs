use axum::{extract::State, http::StatusCode, Json};
use std::sync::Arc;
use tracing::error;

use super::*;
use crate::application::project_skill_export_service::{
    ProjectSkillExportPreview, ProjectSkillExportService,
};
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::ProjectSkillSettings;
use crate::error::AppError;
use crate::http_server::project_scope::ProjectScope;
use crate::http_server::types::HttpError;

pub async fn preview_project_skill_export(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<PreviewProjectSkillExportRequest>,
) -> Result<Json<ProjectSkillExportResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let service = ProjectSkillExportService::new(
        Arc::clone(&state.app_state.project_repo),
        Arc::clone(&state.app_state.project_skill_repo),
        Arc::clone(&state.app_state.project_skill_settings_repo),
    );
    let preview = service
        .preview_export(&project_id)
        .await
        .map_err(export_error)?;
    Ok(Json(export_response(preview)))
}

pub async fn apply_project_skill_export(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ApplyProjectSkillExportRequest>,
) -> Result<Json<ProjectSkillExportResponse>, HttpError> {
    if !req.confirm_export {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("project skill export requires confirm_export=true".to_string()),
        });
    }

    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let service = ProjectSkillExportService::new(
        Arc::clone(&state.app_state.project_repo),
        Arc::clone(&state.app_state.project_skill_repo),
        Arc::clone(&state.app_state.project_skill_settings_repo),
    );
    let preview = service
        .apply_export(&project_id)
        .await
        .map_err(export_error)?;
    Ok(Json(export_response(preview)))
}

pub async fn get_project_skill_settings(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<GetProjectSkillSettingsRequest>,
) -> Result<Json<ProjectSkillSettingsResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let settings = state
        .app_state
        .project_skill_settings_repo
        .get_for_project(&project_id)
        .await
        .map_err(export_error)?
        .unwrap_or_else(|| ProjectSkillSettings::default_for_project(project_id));
    Ok(Json(settings_response(settings)))
}

pub async fn update_project_skill_settings(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<UpdateProjectSkillSettingsRequest>,
) -> Result<Json<ProjectSkillSettingsResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let settings = ProjectSkillSettings {
        project_id,
        export_enabled: req.export_enabled,
    };
    let settings = state
        .app_state
        .project_skill_settings_repo
        .upsert(settings)
        .await
        .map_err(export_error)?;
    Ok(Json(settings_response(settings)))
}

pub(crate) fn assert_project_id_scope(
    project_id: &ProjectId,
    scope: &ProjectScope,
) -> Result<(), HttpError> {
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

fn export_response(preview: ProjectSkillExportPreview) -> ProjectSkillExportResponse {
    let files = preview
        .files
        .into_iter()
        .map(|file| ProjectSkillExportFileResponse {
            project_skill_id: file.project_skill_id,
            title: file.title,
            relative_path: file.relative_path,
            pinned: file.pinned,
            status: file.status.to_string(),
            will_write: file.will_write,
        })
        .collect::<Vec<_>>();
    ProjectSkillExportResponse {
        project_id: preview.project_id.as_str().to_string(),
        target_root: preview.target_root.to_string_lossy().to_string(),
        count: files.len(),
        files,
    }
}

fn settings_response(settings: ProjectSkillSettings) -> ProjectSkillSettingsResponse {
    ProjectSkillSettingsResponse {
        project_id: settings.project_id.as_str().to_string(),
        export_enabled: settings.export_enabled,
    }
}

fn export_error(error: AppError) -> HttpError {
    match error {
        AppError::Validation(message) => HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(message),
        },
        AppError::NotFound(message) => HttpError {
            status: StatusCode::NOT_FOUND,
            message: Some(message),
        },
        other => {
            error!("failed to export project skills: {}", other);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to export project skills".to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{AppState, TeamService, TeamStateTracker};
    use crate::commands::ExecutionState;
    use crate::domain::entities::{
        Project, ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus,
    };

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

    fn temp_project_dir() -> tempfile::TempDir {
        let cwd = std::env::current_dir().expect("current dir");
        tempfile::tempdir_in(cwd).expect("temp project dir")
    }

    fn approved_skill(project_id: ProjectId) -> ProjectSkill {
        let now = chrono::Utc::now();
        ProjectSkill {
            id: ProjectSkillId::new(),
            project_id,
            title: "Review repeat failures".to_string(),
            bucket: "review".to_string(),
            stage: "review".to_string(),
            status: ProjectSkillLifecycleStatus::Approved,
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
    async fn preview_project_skill_export_lists_approved_skill_files() {
        let project_dir = temp_project_dir();
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-export".to_string());
        let mut project = Project::new(
            "Export Test".to_string(),
            project_dir.path().to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        app_state.project_repo.create(project).await.unwrap();
        app_state
            .project_skill_repo
            .create(approved_skill(project_id.clone()))
            .await
            .unwrap();

        let response = preview_project_skill_export(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(PreviewProjectSkillExportRequest {
                project_id: "project-export".to_string(),
            }),
        )
        .await
        .unwrap()
        .0;

        // One skill exported into both provider roots (.claude/skills + .agents/skills).
        assert_eq!(response.count, 2);
        assert!(response
            .files
            .iter()
            .any(|file| file.relative_path.starts_with(".claude/skills/")));
        assert!(response
            .files
            .iter()
            .any(|file| file.relative_path.starts_with(".agents/skills/")));
        assert!(response.files.iter().all(|file| file.will_write));
    }

    #[tokio::test]
    async fn project_skill_settings_default_to_export_disabled() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-export".to_string());

        let response = get_project_skill_settings(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(GetProjectSkillSettingsRequest {
                project_id: "project-export".to_string(),
            }),
        )
        .await
        .unwrap()
        .0;

        assert!(!response.export_enabled);
    }

    #[tokio::test]
    async fn update_project_skill_settings_persists_export_opt_in() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-export".to_string());

        let response = update_project_skill_settings(
            State(test_state(Arc::clone(&app_state))),
            ProjectScope(Some(vec![project_id.clone()])),
            Json(UpdateProjectSkillSettingsRequest {
                project_id: "project-export".to_string(),
                export_enabled: true,
            }),
        )
        .await
        .unwrap()
        .0;

        assert!(response.export_enabled);
        let loaded = app_state
            .project_skill_settings_repo
            .get_for_project(&project_id)
            .await
            .unwrap()
            .expect("saved settings");
        assert!(loaded.export_enabled);
    }

    #[tokio::test]
    async fn apply_project_skill_export_requires_explicit_confirmation() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-export".to_string());

        let error = apply_project_skill_export(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(ApplyProjectSkillExportRequest {
                project_id: "project-export".to_string(),
                confirm_export: false,
            }),
        )
        .await
        .expect_err("export apply should require confirmation");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }
}
