use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use tracing::error;

use crate::domain::entities::{ProjectId, ProjectSkillId};
use crate::domain::repositories::ProjectSkillResolutionOutcome;
use crate::domain::services::{
    ProjectSkillPipelineContext, ProjectSkillPipelineInput, ProjectSkillPipelineService,
    PROJECT_SKILL_PIPELINE_PROJECT_SCOPE_ERROR,
};
use crate::error::AppError;
use crate::http_server::handlers::learned_skills_export::assert_project_id_scope;
use crate::http_server::project_scope::ProjectScope;
use crate::http_server::types::{
    HttpError, HttpServerState, PatchProjectSkillRequest, ProjectSkillPipelineResponse,
    ProjectSkillResponse, RetireProjectSkillRequest, UpsertProjectSkillRequest,
};

pub async fn upsert_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    headers: HeaderMap,
    Json(req): Json<UpsertProjectSkillRequest>,
) -> Result<Json<ProjectSkillPipelineResponse>, HttpError> {
    let context = trusted_pipeline_context(&headers)?;
    let input = pipeline_input(req);
    assert_project_id_scope(&input.project_id, &scope)?;
    let result = ProjectSkillPipelineService::new(Arc::clone(
        &state.app_state.project_skill_repo,
    ))
    .upsert(context, input)
    .await
    .map_err(pipeline_error)?;
    Ok(Json(ProjectSkillPipelineResponse {
        outcome: resolution_outcome(result.outcome).to_string(),
        skill: ProjectSkillResponse::from(result.skill),
    }))
}

pub async fn patch_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    headers: HeaderMap,
    Json(req): Json<PatchProjectSkillRequest>,
) -> Result<Json<ProjectSkillPipelineResponse>, HttpError> {
    let context = trusted_pipeline_context(&headers)?;
    let project_id = ProjectId::from_string(req.project_id.clone());
    assert_project_id_scope(&project_id, &scope)?;
    let target_id = ProjectSkillId::from_string(req.project_skill_id);
    let input = ProjectSkillPipelineInput {
        project_id,
        title: req.title,
        bucket: req.bucket,
        stage: req.stage,
        scope_paths: req.scope_paths,
        compact_guidance: req.compact_guidance,
        body_markdown: req.body_markdown,
        predicted_effect: req.predicted_effect,
    };
    let result = ProjectSkillPipelineService::new(Arc::clone(
        &state.app_state.project_skill_repo,
    ))
    .patch(context, target_id, input)
    .await
    .map_err(pipeline_error)?;
    Ok(Json(ProjectSkillPipelineResponse {
        outcome: resolution_outcome(result.outcome).to_string(),
        skill: ProjectSkillResponse::from(result.skill),
    }))
}

pub async fn retire_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    headers: HeaderMap,
    Json(req): Json<RetireProjectSkillRequest>,
) -> Result<Json<ProjectSkillPipelineResponse>, HttpError> {
    let context = trusted_pipeline_context(&headers)?;
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let target_id = ProjectSkillId::from_string(req.project_skill_id);
    let result = ProjectSkillPipelineService::new(Arc::clone(
        &state.app_state.project_skill_repo,
    ))
    .retire(context, &project_id, &target_id)
    .await
    .map_err(pipeline_error)?;
    Ok(Json(ProjectSkillPipelineResponse {
        outcome: if result.changed {
            "retired".to_string()
        } else {
            "unchanged".to_string()
        },
        skill: ProjectSkillResponse::from(result.skill),
    }))
}

fn pipeline_input(req: UpsertProjectSkillRequest) -> ProjectSkillPipelineInput {
    ProjectSkillPipelineInput {
        project_id: ProjectId::from_string(req.project_id),
        title: req.title,
        bucket: req.bucket,
        stage: req.stage,
        scope_paths: req.scope_paths,
        compact_guidance: req.compact_guidance,
        body_markdown: req.body_markdown,
        predicted_effect: req.predicted_effect,
    }
}

fn trusted_pipeline_context(headers: &HeaderMap) -> Result<ProjectSkillPipelineContext, HttpError> {
    let required = |name: &'static str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(unauthorized_pipeline)
    };
    let optional = |name: &'static str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let context = ProjectSkillPipelineContext {
        agent_name: required("x-ralphx-agent-name")?,
        pipeline_role: required("x-ralphx-pipeline-role")?,
        project_id: ProjectId::from_string(required("x-ralphx-project-id")?),
        context_type: required("x-ralphx-context-type")?,
        context_id: required("x-ralphx-context-id")?,
        conversation_id: required("x-ralphx-conversation-id")?,
        agent_run_id: optional("x-ralphx-agent-run-id"),
        task_id: optional("x-ralphx-task-id"),
    };
    context.validate().map_err(|_| unauthorized_pipeline())?;
    Ok(context)
}

fn unauthorized_pipeline() -> HttpError {
    HttpError {
        status: StatusCode::UNAUTHORIZED,
        message: Some("project skill pipeline runtime authority is required".to_string()),
    }
}

fn pipeline_error(error: AppError) -> HttpError {
    match error {
        AppError::Validation(message) if message == PROJECT_SKILL_PIPELINE_PROJECT_SCOPE_ERROR => {
            HttpError {
                status: StatusCode::FORBIDDEN,
                message: Some("project skill does not belong to the active project".to_string()),
            }
        }
        AppError::Validation(message) => HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(message),
        },
        AppError::NotFound(_) => HttpError {
            status: StatusCode::NOT_FOUND,
            message: Some("project skill was not found".to_string()),
        },
        AppError::Conflict(message) => HttpError {
            status: StatusCode::CONFLICT,
            message: Some(message),
        },
        error => {
            error!("project skill pipeline write failed: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("project skill pipeline write failed".to_string()),
            }
        }
    }
}

fn resolution_outcome(outcome: ProjectSkillResolutionOutcome) -> &'static str {
    match outcome {
        ProjectSkillResolutionOutcome::Duplicate => "duplicate",
        ProjectSkillResolutionOutcome::PatchExisting => "patch_existing",
        ProjectSkillResolutionOutcome::AppendEvidence => "append_evidence",
        ProjectSkillResolutionOutcome::CreateNew => "create_new",
    }
}
