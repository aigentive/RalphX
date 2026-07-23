use axum::{extract::State, http::StatusCode, Json};
use std::sync::Arc;
use tracing::error;

use super::*;
use crate::application::project_skill_export_service::{
    ProjectSkillExportPreview, ProjectSkillExportService,
};
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{ProjectSkillSettings, ProjectSkillSettingsPatch};
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
    let settings = state
        .app_state
        .project_skill_settings_repo
        .patch(
            &project_id,
            ProjectSkillSettingsPatch {
                enabled: req.enabled,
                auto_inject: req.auto_inject,
                auto_distill: req.auto_distill,
                injection_max_skills: req.injection_max_skills,
                injection_max_chars: req.injection_max_chars,
                injection_guidance_max_chars: req.injection_guidance_max_chars,
                report_min_outcomes: req.report_min_outcomes,
                verification_corpus_gate: req.verification_corpus_gate,
                export_enabled: req.export_enabled,
            },
        )
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
        enabled: settings.enabled,
        auto_inject: settings.auto_inject,
        auto_distill: settings.auto_distill,
        injection_max_skills: settings.injection_max_skills,
        injection_max_chars: settings.injection_max_chars,
        injection_guidance_max_chars: settings.injection_guidance_max_chars,
        report_min_outcomes: settings.report_min_outcomes,
        verification_corpus_gate: settings.verification_corpus_gate,
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
