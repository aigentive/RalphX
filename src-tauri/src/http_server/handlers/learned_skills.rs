use axum::{extract::State, http::StatusCode, Json};
use std::sync::Arc;
use tracing::error;

use super::*;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::ChatConversationId;
use crate::domain::entities::{
    ChatContextType, MemoryEntryId, ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus,
    TaskOutcomeStatus,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, SkillUsageListOptions, UpsertTaskOutcomeInput,
};
use crate::domain::services::{
    new_empty_task_outcome, DistillEligibleOutcomesInput, MemoryToProjectSkillPromotionService,
    ProjectSkillDistillationOrigin, ProjectSkillDistillerService, ProjectSkillImportApplyInput,
    ProjectSkillImportCandidate, ProjectSkillImportPreview, ProjectSkillImportPreviewInput,
    ProjectSkillImportPreviewRow, ProjectSkillImportPreviewService, ProjectSkillReportOptions,
    ProjectSkillReportService, ProjectSkillService, PromoteMemoryToProjectSkillInput,
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

pub async fn list_conversation_project_skills(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ListConversationProjectSkillsRequest>,
) -> Result<Json<ListConversationProjectSkillsResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let conversation_id = req.conversation_id.trim().to_string();
    if conversation_id.is_empty() {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("conversation_id is required".to_string()),
        });
    }

    let skills = state
        .app_state
        .project_skill_repo
        .list_by_project(
            &project_id,
            ProjectSkillListOptions {
                include_archived: true,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| {
            error!("failed to list project skills for conversation: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to list project skills".to_string()),
            }
        })?;

    let usage_events = state
        .app_state
        .skill_usage_event_repo
        .list_by_project(&project_id, SkillUsageListOptions::default())
        .await
        .map_err(|error| {
            error!("failed to list skill usage for conversation: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to list skill usage events".to_string()),
            }
        })?;

    let mut usage_counts = std::collections::HashMap::<ProjectSkillId, usize>::new();
    for event in usage_events {
        if event.conversation_id.as_deref() == Some(conversation_id.as_str()) {
            *usage_counts.entry(event.project_skill_id).or_default() += 1;
        }
    }

    let mut rows = skills
        .into_iter()
        .filter_map(|skill| {
            let generated = project_skill_mentions_conversation(&skill, conversation_id.as_str());
            let usage_count = usage_counts.get(&skill.id).copied().unwrap_or_default();
            if !generated && usage_count == 0 {
                return None;
            }
            Some(ConversationProjectSkillResponse {
                skill: ProjectSkillResponse::from(skill),
                generated_by_conversation: generated,
                used_by_conversation: usage_count > 0,
                usage_count,
            })
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| {
        right
            .skill
            .updated_at
            .cmp(&left.skill.updated_at)
            .then_with(|| left.skill.title.cmp(&right.skill.title))
    });

    let count = rows.len();
    Ok(Json(ListConversationProjectSkillsResponse {
        skills: rows,
        count,
    }))
}

pub async fn process_conversation_project_skills(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ProcessConversationProjectSkillsRequest>,
) -> Result<Json<ProcessConversationProjectSkillsResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let conversation_id = req.conversation_id.trim().to_string();
    if conversation_id.is_empty() {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("conversation_id is required".to_string()),
        });
    }

    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&ChatConversationId::from_string(conversation_id.clone()))
        .await
        .map_err(|error| {
            error!("failed to get conversation for skill processing: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to get conversation".to_string()),
            }
        })?;
    let Some(conversation) = conversation else {
        return Err(HttpError {
            status: StatusCode::NOT_FOUND,
            message: Some("conversation not found".to_string()),
        });
    };
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != project_id.as_str()
    {
        return Err(HttpError {
            status: StatusCode::FORBIDDEN,
            message: Some("conversation is outside project scope".to_string()),
        });
    }

    let messages = state
        .app_state
        .chat_message_repo
        .get_by_conversation(&conversation.id)
        .await
        .map_err(|error| {
            error!(
                "failed to list conversation messages for skill processing: {}",
                error
            );
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to list conversation messages".to_string()),
            }
        })?;

    let message_count = messages.len();
    if message_count == 0 {
        return Ok(Json(ProcessConversationProjectSkillsResponse {
            staged_skills: Vec::new(),
            skipped_existing: 0,
            message_count,
        }));
    }
    let evidence = build_conversation_skill_evidence(&conversation_id, &messages);
    let mut outcome = new_empty_task_outcome(
        project_id.clone(),
        "agent_conversation",
        "conversation",
        conversation_id.clone(),
    );
    outcome.conversation_id = Some(conversation_id);
    outcome.outcome_class = Some("conversation_skill_candidate".to_string());
    outcome.status = TaskOutcomeStatus::Eligible;
    outcome.evidence_json = evidence;
    if let Some(harness) = conversation.provider_harness {
        outcome.provider_harness = Some(harness.to_string());
    }
    outcome.provider_session_id = conversation.provider_session_id;

    state
        .app_state
        .task_outcome_repo
        .upsert(UpsertTaskOutcomeInput { outcome })
        .await
        .map_err(|error| {
            error!("failed to upsert conversation skill outcome: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to record conversation skill outcome".to_string()),
            }
        })?;

    let distiller = ProjectSkillDistillerService::new(
        Arc::clone(&state.app_state.task_outcome_repo),
        Arc::clone(&state.app_state.project_skill_repo),
    );
    let result = distiller
        .distill_eligible_outcomes(DistillEligibleOutcomesInput {
            project_id,
            source: Some("agent_conversation".to_string()),
            limit: 10,
            origin: ProjectSkillDistillationOrigin::ManualCurator,
        })
        .await
        .map_err(|error| {
            error!("failed to process conversation project skills: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to process conversation project skills".to_string()),
            }
        })?;

    Ok(Json(ProcessConversationProjectSkillsResponse {
        staged_skills: result
            .staged_skills
            .into_iter()
            .map(ProjectSkillResponse::from)
            .collect(),
        skipped_existing: result.skipped_existing,
        message_count,
    }))
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

pub async fn list_project_skill_report_cards(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ListProjectSkillReportCardsRequest>,
) -> Result<Json<ListProjectSkillReportCardsResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;

    let service = ProjectSkillReportService::new(
        Arc::clone(&state.app_state.project_skill_repo),
        Arc::clone(&state.app_state.skill_usage_event_repo),
        Arc::clone(&state.app_state.task_outcome_repo),
    );
    let cards = service
        .list_report_cards(
            &project_id,
            ProjectSkillReportOptions {
                min_linked_outcomes: req.min_linked_outcomes.unwrap_or(5),
                stale_after_days: req.stale_after_days.unwrap_or(30),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| {
            error!("failed to list project skill report cards: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to list project skill report cards".to_string()),
            }
        })?
        .into_iter()
        .map(ProjectSkillReportCardResponse::from)
        .collect::<Vec<_>>();
    let count = cards.len();

    Ok(Json(ListProjectSkillReportCardsResponse { cards, count }))
}

pub async fn preview_project_skill_import(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<PreviewProjectSkillImportRequest>,
) -> Result<Json<PreviewProjectSkillImportResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;

    let service =
        ProjectSkillImportPreviewService::new(Arc::clone(&state.app_state.project_skill_repo));
    let preview = service
        .preview_import(ProjectSkillImportPreviewInput {
            project_id,
            candidates: req
                .candidates
                .into_iter()
                .map(import_candidate_from_request)
                .collect(),
        })
        .await
        .map_err(|error| {
            error!("failed to preview project skill import: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to preview project skill import".to_string()),
            }
        })?;

    Ok(Json(PreviewProjectSkillImportResponse {
        rows: preview_response_rows(preview.rows),
        eligible_count: preview.eligible_count,
        invalid_count: preview.invalid_count,
        duplicate_count: preview.duplicate_count,
    }))
}

pub async fn apply_project_skill_import(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ApplyProjectSkillImportRequest>,
) -> Result<Json<ApplyProjectSkillImportResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;

    let service =
        ProjectSkillImportPreviewService::new(Arc::clone(&state.app_state.project_skill_repo));
    let result = service
        .apply_import(ProjectSkillImportApplyInput {
            project_id,
            candidates: req
                .candidates
                .into_iter()
                .map(import_candidate_from_request)
                .collect(),
            confirm_import: req.confirm_import,
        })
        .await
        .map_err(|error| match error {
            AppError::Validation(message) => HttpError {
                status: StatusCode::BAD_REQUEST,
                message: Some(message),
            },
            other => {
                error!("failed to apply project skill import: {}", other);
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some("failed to apply project skill import".to_string()),
                }
            }
        })?;
    let imported_skills = result
        .imported_skills
        .into_iter()
        .map(ProjectSkillResponse::from)
        .collect::<Vec<_>>();
    let imported_count = imported_skills.len();

    Ok(Json(ApplyProjectSkillImportResponse {
        preview: preview_response(result.preview),
        imported_skills,
        imported_count,
    }))
}

pub async fn promote_memory_to_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<PromoteMemoryToProjectSkillRequest>,
) -> Result<Json<PromoteMemoryToProjectSkillResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;

    let service = MemoryToProjectSkillPromotionService::new(
        Arc::clone(&state.app_state.memory_entry_repo),
        Arc::clone(&state.app_state.project_skill_repo),
    );
    let result = service
        .promote_memory(PromoteMemoryToProjectSkillInput {
            project_id,
            memory_id: MemoryEntryId::from_string(req.memory_id),
            title: req.title,
            bucket: req.bucket,
            stage: req.stage,
            compact_guidance: req.compact_guidance,
            body_markdown: req.body_markdown,
            predicted_effect: req.predicted_effect,
        })
        .await
        .map_err(|error| match error {
            AppError::Validation(message) => HttpError {
                status: StatusCode::BAD_REQUEST,
                message: Some(message),
            },
            other => {
                error!("failed to promote memory to project skill: {}", other);
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some("failed to promote memory to project skill".to_string()),
                }
            }
        })?;

    Ok(Json(PromoteMemoryToProjectSkillResponse {
        skill: ProjectSkillResponse::from(result.skill),
    }))
}

fn project_skill_mentions_conversation(skill: &ProjectSkill, conversation_id: &str) -> bool {
    json_value_contains_string(&skill.provenance_json, conversation_id)
}

fn build_conversation_skill_evidence(
    conversation_id: &str,
    messages: &[crate::domain::entities::ChatMessage],
) -> serde_json::Value {
    let excerpts = messages
        .iter()
        .rev()
        .filter(|message| !message.content.trim().is_empty())
        .take(12)
        .map(|message| {
            serde_json::json!({
                "role": message.role.to_string(),
                "message_id": message.id.as_str(),
                "content": truncate_text(message.content.trim(), 900),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "conversation_id": conversation_id,
        "message_count": messages.len(),
        "recent_messages": excerpts,
    })
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut result = value.chars().take(max_chars).collect::<String>();
    result.push_str("...");
    result
}

fn json_value_contains_string(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value == needle,
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| json_value_contains_string(item, needle)),
        serde_json::Value::Object(map) => map
            .values()
            .any(|item| json_value_contains_string(item, needle)),
        _ => false,
    }
}

fn import_candidate_from_request(
    candidate: PreviewProjectSkillImportCandidateRequest,
) -> ProjectSkillImportCandidate {
    ProjectSkillImportCandidate {
        external_id: candidate.external_id,
        title: candidate.title,
        bucket: candidate.bucket,
        stage: candidate.stage,
        scope_paths: candidate.scope_paths,
        compact_guidance: candidate.compact_guidance,
        body_markdown: candidate.body_markdown,
        predicted_effect: candidate.predicted_effect,
        provenance_json: candidate.provenance_json,
        source_snapshot_json: candidate.source_snapshot_json,
    }
}

fn preview_response(preview: ProjectSkillImportPreview) -> PreviewProjectSkillImportResponse {
    PreviewProjectSkillImportResponse {
        rows: preview_response_rows(preview.rows),
        eligible_count: preview.eligible_count,
        invalid_count: preview.invalid_count,
        duplicate_count: preview.duplicate_count,
    }
}

fn preview_response_rows(
    rows: Vec<ProjectSkillImportPreviewRow>,
) -> Vec<ProjectSkillImportPreviewRowResponse> {
    rows.into_iter()
        .map(ProjectSkillImportPreviewRowResponse::from)
        .collect()
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
    use crate::domain::entities::{
        ChatConversation, ChatMessage, MemoryBucket, MemoryEntry, ProjectSkill,
        ProjectSkillLifecycleStatus, SkillUsageEvent, SkillUsageEventId, TaskOutcomeStatus,
    };
    use crate::domain::repositories::{ProjectSkillListOptions, UpsertTaskOutcomeInput};
    use crate::domain::services::{new_empty_task_outcome, new_skill_usage_event};
    use serde_json::json;

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

    fn import_preview_request(project_id: &str) -> PreviewProjectSkillImportRequest {
        PreviewProjectSkillImportRequest {
            project_id: project_id.to_string(),
            candidates: vec![PreviewProjectSkillImportCandidateRequest {
                external_id: Some("manifest-skill-1".to_string()),
                title: "Check review branch before export".to_string(),
                bucket: "review".to_string(),
                stage: "review".to_string(),
                scope_paths: vec!["src-tauri/src/domain".to_string()],
                compact_guidance: "Preview branch state before exporting skills.".to_string(),
                body_markdown: "Detailed guidance".to_string(),
                predicted_effect: "Prevents direct writes from unsafe branches.".to_string(),
                provenance_json: json!({
                    "source": "import_manifest",
                    "source_ref": "manifest-skill-1"
                }),
                source_snapshot_json: json!({
                    "kind": "project_skill_manifest",
                    "captured_at": "2026-06-15T00:00:00Z"
                }),
            }],
        }
    }

    fn promote_memory_request(
        project_id: &str,
        memory_id: &str,
    ) -> PromoteMemoryToProjectSkillRequest {
        PromoteMemoryToProjectSkillRequest {
            project_id: project_id.to_string(),
            memory_id: memory_id.to_string(),
            title: Some("Promoted review procedure".to_string()),
            bucket: "review".to_string(),
            stage: "review".to_string(),
            compact_guidance: "Turn the memory into a repeatable review check.".to_string(),
            body_markdown: "## Procedure\n\nApply the remembered fact as a review checklist item."
                .to_string(),
            predicted_effect: "Reduces repeated review misses.".to_string(),
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
    async fn list_conversation_project_skills_scopes_generated_and_used_skills() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-a".to_string());
        let conversation_id = "conversation-a";

        let mut generated = staged_skill(project_id.clone());
        generated.title = "Generated conversation skill".to_string();
        generated.provenance_json = json!({
            "source": "memory_to_skill",
            "conversation": {
                "id": conversation_id
            }
        });
        let generated_id = generated.id.clone();
        app_state
            .project_skill_repo
            .create(generated)
            .await
            .unwrap();

        let mut used = staged_skill(project_id.clone());
        used.title = "Used conversation skill".to_string();
        used.provenance_json = json!({ "source": "import" });
        let used_id = used.id.clone();
        app_state.project_skill_repo.create(used).await.unwrap();

        let mut unrelated = staged_skill(project_id.clone());
        unrelated.title = "Unrelated skill".to_string();
        app_state
            .project_skill_repo
            .create(unrelated)
            .await
            .unwrap();

        let now = chrono::Utc::now();
        app_state
            .skill_usage_event_repo
            .record(SkillUsageEvent {
                id: SkillUsageEventId::new(),
                project_id: project_id.clone(),
                project_skill_id: used_id.clone(),
                conversation_id: Some(conversation_id.to_string()),
                agent_run_id: Some("run-a".to_string()),
                provider_harness: Some("codex".to_string()),
                stage: Some("review".to_string()),
                bucket: Some("review".to_string()),
                injection_kind: "composer_directive".to_string(),
                outcome_id: None,
                metadata_json: json!({}),
                created_at: now,
            })
            .await
            .unwrap();

        app_state
            .skill_usage_event_repo
            .record(SkillUsageEvent {
                id: SkillUsageEventId::new(),
                project_id: project_id.clone(),
                project_skill_id: generated_id.clone(),
                conversation_id: Some("other-conversation".to_string()),
                agent_run_id: Some("run-b".to_string()),
                provider_harness: Some("claude".to_string()),
                stage: Some("review".to_string()),
                bucket: Some("review".to_string()),
                injection_kind: "composer_directive".to_string(),
                outcome_id: None,
                metadata_json: json!({}),
                created_at: now,
            })
            .await
            .unwrap();

        let response = list_conversation_project_skills(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id.clone()])),
            Json(ListConversationProjectSkillsRequest {
                project_id: project_id.as_str().to_string(),
                conversation_id: conversation_id.to_string(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.0.count, 2);
        let generated_row = response
            .0
            .skills
            .iter()
            .find(|row| row.skill.id == generated_id.as_str())
            .expect("generated skill row");
        assert!(generated_row.generated_by_conversation);
        assert!(!generated_row.used_by_conversation);
        assert_eq!(generated_row.usage_count, 0);

        let used_row = response
            .0
            .skills
            .iter()
            .find(|row| row.skill.id == used_id.as_str())
            .expect("used skill row");
        assert!(!used_row.generated_by_conversation);
        assert!(used_row.used_by_conversation);
        assert_eq!(used_row.usage_count, 1);
    }

    #[tokio::test]
    async fn process_conversation_project_skills_stages_from_existing_chat() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-process".to_string());
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.title = Some("Older bugfix chat".to_string());
        let conversation_id = conversation.id.clone();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .unwrap();

        let mut user_message = ChatMessage::user_in_project(
            project_id.clone(),
            "We keep missing the proposal rejection dependency rows.",
        );
        user_message.conversation_id = Some(conversation_id.clone());
        app_state
            .chat_message_repo
            .create(user_message)
            .await
            .unwrap();

        let response = process_conversation_project_skills(
            State(test_state(Arc::clone(&app_state))),
            ProjectScope(Some(vec![project_id.clone()])),
            Json(ProcessConversationProjectSkillsRequest {
                project_id: project_id.as_str().to_string(),
                conversation_id: conversation_id.as_str().to_string(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.0.message_count, 1);
        assert_eq!(response.0.staged_skills.len(), 1);
        let staged = &response.0.staged_skills[0];
        assert_eq!(staged.status, "staged");
        assert_eq!(staged.project_id, project_id.as_str());

        let scoped = list_conversation_project_skills(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id.clone()])),
            Json(ListConversationProjectSkillsRequest {
                project_id: project_id.as_str().to_string(),
                conversation_id: conversation_id.as_str().to_string(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(scoped.0.count, 1);
        assert!(scoped.0.skills[0].generated_by_conversation);
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

    #[tokio::test]
    async fn list_project_skill_report_cards_returns_descriptive_counts() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-report".to_string());
        let mut skill = staged_skill(project_id.clone());
        skill.status = ProjectSkillLifecycleStatus::Approved;
        let skill_id = skill.id.clone();
        app_state.project_skill_repo.create(skill).await.unwrap();

        let mut outcome =
            new_empty_task_outcome(project_id.clone(), "review", "review_note", "review-1");
        outcome.status = TaskOutcomeStatus::Succeeded;
        let outcome = app_state
            .task_outcome_repo
            .upsert(UpsertTaskOutcomeInput { outcome })
            .await
            .unwrap();
        let mut usage =
            new_skill_usage_event(project_id.clone(), skill_id.clone(), "compact_index");
        usage.outcome_id = Some(outcome.id);
        app_state
            .skill_usage_event_repo
            .record(usage)
            .await
            .unwrap();

        let response = list_project_skill_report_cards(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(ListProjectSkillReportCardsRequest {
                project_id: "project-report".to_string(),
                min_linked_outcomes: Some(2),
                stale_after_days: Some(30),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.0.count, 1);
        let card = &response.0.cards[0];
        assert_eq!(card.project_skill_id, skill_id.as_str());
        assert_eq!(card.usage_count, 1);
        assert_eq!(card.linked_outcome_count, 1);
        assert_eq!(card.succeeded_outcome_count, 1);
        assert_eq!(card.evidence_level, "insufficient_data");
    }

    #[tokio::test]
    async fn preview_project_skill_import_returns_fail_closed_decisions() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-import".to_string());
        let mut request = import_preview_request("project-import");
        request.candidates[0].source_snapshot_json = json!(null);
        request.candidates[0].scope_paths = vec!["../outside".to_string()];

        let response = preview_project_skill_import(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(request),
        )
        .await
        .unwrap();

        assert_eq!(response.0.eligible_count, 0);
        assert_eq!(response.0.invalid_count, 1);
        assert_eq!(response.0.rows[0].decision, "invalid");
        assert!(response.0.rows[0]
            .reasons
            .iter()
            .any(|reason| reason == "source snapshot is required before import"));
        assert!(response.0.rows[0]
            .reasons
            .iter()
            .any(|reason| reason.starts_with("invalid scope path")));
    }

    #[tokio::test]
    async fn preview_project_skill_import_rejects_cross_project_scope() {
        let app_state = Arc::new(AppState::new_test());
        let error = preview_project_skill_import(
            State(test_state(app_state)),
            ProjectScope(Some(vec![ProjectId::from_string("project-b".to_string())])),
            Json(import_preview_request("project-a")),
        )
        .await
        .expect_err("cross-project import preview should fail");

        assert_eq!(error.status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn apply_project_skill_import_requires_confirmation() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-import".to_string());

        let error = apply_project_skill_import(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(ApplyProjectSkillImportRequest {
                project_id: "project-import".to_string(),
                candidates: import_preview_request("project-import").candidates,
                confirm_import: false,
            }),
        )
        .await
        .expect_err("unconfirmed import should fail");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_project_skill_import_stages_eligible_rows() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-import".to_string());

        let response = apply_project_skill_import(
            State(test_state(Arc::clone(&app_state))),
            ProjectScope(Some(vec![project_id.clone()])),
            Json(ApplyProjectSkillImportRequest {
                project_id: "project-import".to_string(),
                candidates: import_preview_request("project-import").candidates,
                confirm_import: true,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.0.imported_count, 1);
        assert_eq!(response.0.preview.eligible_count, 1);
        assert_eq!(response.0.imported_skills[0].status, "staged");

        let written = app_state
            .project_skill_repo
            .list_by_project(&project_id, ProjectSkillListOptions::default())
            .await
            .unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(
            written[0]
                .provenance_json
                .get("source")
                .and_then(serde_json::Value::as_str),
            Some("project_skill_import")
        );
    }

    #[tokio::test]
    async fn promote_memory_to_project_skill_stages_skill() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-memory".to_string());
        let memory = MemoryEntry::new(
            project_id.clone(),
            MemoryBucket::OperationalPlaybooks,
            "Review memory".to_string(),
            "Remember this review fact.".to_string(),
            "Factual memory details.".to_string(),
            vec!["src-tauri".to_string()],
            "memory-hash".to_string(),
        );
        let memory = app_state.memory_entry_repo.create(memory).await.unwrap();

        let response = promote_memory_to_project_skill(
            State(test_state(Arc::clone(&app_state))),
            ProjectScope(Some(vec![project_id.clone()])),
            Json(promote_memory_request("project-memory", memory.id.as_str())),
        )
        .await
        .unwrap();

        assert_eq!(response.0.skill.status, "staged");
        assert_eq!(response.0.skill.scope_paths, vec!["src-tauri".to_string()]);
        assert_eq!(
            response
                .0
                .skill
                .provenance_json
                .get("source")
                .and_then(serde_json::Value::as_str),
            Some("memory_to_project_skill_promotion")
        );

        let written = app_state
            .project_skill_repo
            .list_by_project(&project_id, ProjectSkillListOptions::default())
            .await
            .unwrap();
        assert_eq!(written.len(), 1);
    }

    #[tokio::test]
    async fn promote_memory_to_project_skill_rejects_cross_project_scope() {
        let app_state = Arc::new(AppState::new_test());
        let error = promote_memory_to_project_skill(
            State(test_state(app_state)),
            ProjectScope(Some(vec![ProjectId::from_string("project-b".to_string())])),
            Json(promote_memory_request("project-a", "memory-1")),
        )
        .await
        .expect_err("cross-project promotion should fail");

        assert_eq!(error.status, StatusCode::FORBIDDEN);
    }
}
