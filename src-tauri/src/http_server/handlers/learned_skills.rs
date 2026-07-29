use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::error;

use super::*;
use crate::application::memory_orchestration::{
    schedule_explicit_project_skill_distillation, ProjectSkillDistillationScheduleResult,
    ProjectSkillDistillationScheduleStatus,
};
use crate::application::project_skill_distillation_service::ProjectSkillDistillationSelection;
use crate::application::project_skill_export_service::MAX_SKILL_DESCRIPTION_CHARS;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{AgentRunId, ChatConversationId};
use crate::domain::entities::{
    ChatContextType, MemoryEntryId, ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus,
    SkillUsageInjectionKind, TaskOutcomeClass, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, SkillUsageListOptions, UpsertTaskOutcomeInput,
};
use crate::domain::services::{
    new_c2_skill_usage_event, new_empty_task_outcome, MemoryToProjectSkillPromotionService,
    ProjectSkillImportApplyInput, ProjectSkillImportCandidate, ProjectSkillImportPreview,
    ProjectSkillImportPreviewInput, ProjectSkillImportPreviewRow, ProjectSkillImportPreviewService,
    ProjectSkillReportOptions, ProjectSkillReportService, ProjectSkillService,
    PromoteMemoryToProjectSkillInput, SkillUsageAttribution, SkillUsageService,
    UpdateProjectSkillContentInput,
};
use crate::error::{AppError, AppResult};
use crate::http_server::handlers::learned_skills_export::assert_project_id_scope;
use crate::http_server::project_scope::{ProjectScope, ProjectScopeGuard};
use crate::http_server::types::HttpError;
use crate::infrastructure::tool_paths::{resolve_gh_cli_path, resolve_git_cli_path};
use crate::utils::path_safety::validate_absolute_non_root_path;

const GIT_HISTORY_SCAN_LIMIT: usize = 50;
const GIT_HISTORY_DISTILL_SOURCE: TaskOutcomeSource = TaskOutcomeSource::GitCommitHistory;
const GITHUB_PR_HISTORY_SCAN_LIMIT: usize = 25;
const GITHUB_PR_DISTILL_SOURCE: TaskOutcomeSource = TaskOutcomeSource::GithubPrHistory;

#[derive(Debug, Clone, Default)]
struct GitHistoryIngestSummary {
    ingested_outcomes: usize,
    scanned_git_commits: usize,
    scanned_github_prs: usize,
    outcome_ids: Vec<crate::domain::entities::TaskOutcomeId>,
}

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
            message_count,
            status: ProjectSkillDistillationScheduleStatus::Skipped
                .as_str()
                .to_string(),
            selected_outcomes: 0,
            batch_count: 0,
            started_batches: 0,
            message: Some("No conversation evidence was available to queue.".to_string()),
        }));
    }
    let mut evidence = build_conversation_skill_evidence(&conversation_id, &messages);
    let recurrence_text = messages
        .iter()
        .filter(|message| !message.content.trim().is_empty())
        .map(|message| message.content.trim())
        .collect::<Vec<_>>()
        .join("\n");
    let recurrence_session = conversation
        .provider_session_id
        .as_deref()
        .unwrap_or(conversation_id.as_str())
        .to_string();
    crate::domain::services::failure_fingerprint::attach_recurrence_evidence(
        &mut evidence,
        &recurrence_text,
        Some(&recurrence_session),
    );
    let mut outcome = new_empty_task_outcome(
        project_id.clone(),
        TaskOutcomeSource::AgentConversation,
        "conversation",
        conversation_id.clone(),
    );
    outcome.conversation_id = Some(conversation_id);
    outcome.outcome_class = Some(TaskOutcomeClass::ConversationSkillCandidate);
    outcome.status = TaskOutcomeStatus::Eligible;
    outcome.evidence_json = evidence;
    if let Some(harness) = conversation.provider_harness {
        outcome.provider_harness = Some(harness.to_string());
    }
    outcome.provider_session_id = conversation.provider_session_id;

    let recorded_outcome = state
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
    let schedule = schedule_explicit_project_skill_distillation(
        &state.app_state,
        &project_id,
        ProjectSkillDistillationSelection::ExactOutcomes(vec![recorded_outcome.id]),
        Some(&conversation.id),
        ChatContextType::Project,
        project_id.as_str(),
    )
    .await;

    Ok(Json(ProcessConversationProjectSkillsResponse {
        message_count,
        status: schedule.status.as_str().to_string(),
        selected_outcomes: schedule.selected_outcomes,
        batch_count: schedule.batch_count,
        started_batches: schedule.started_batches,
        message: schedule.message,
    }))
}

pub async fn get_project_skill(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    scope: ProjectScope,
    Json(req): Json<GetProjectSkillRequest>,
) -> Result<Json<GetProjectSkillResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
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
        if skill.project_id != project_id {
            return Err(HttpError {
                status: StatusCode::FORBIDDEN,
                message: Some("project skill does not belong to the requested project".to_string()),
            });
        }
        record_full_load_skill_usage(&state, &headers, &skill).await;
        return Ok(Json(GetProjectSkillResponse {
            skill: Some(ProjectSkillResponse::from(skill)),
        }));
    }

    Ok(Json(GetProjectSkillResponse { skill: None }))
}

async fn record_full_load_skill_usage(
    state: &HttpServerState,
    headers: &HeaderMap,
    skill: &ProjectSkill,
) {
    let attribution = match trusted_full_load_attribution(state, headers, skill).await {
        Ok(Some(attribution)) => attribution,
        Ok(None) => return,
        Err(reason) => {
            tracing::warn!(
                project_skill_id = skill.id.as_str(),
                reason,
                "Suppressing learned skill full-load telemetry"
            );
            return;
        }
    };
    let mut event = match new_c2_skill_usage_event(
        skill.project_id.clone(),
        skill.id.clone(),
        SkillUsageInjectionKind::FullLoad,
        attribution,
    ) {
        Ok(event) => event,
        Err(error) => {
            tracing::warn!(error = %error, "Suppressing invalid learned skill full-load telemetry");
            return;
        }
    };
    event.metadata_json["source"] = serde_json::json!("get_project_skill");
    let service = SkillUsageService::new(Arc::clone(&state.app_state.skill_usage_event_repo));
    if let Err(error) = service.record_usage_batch(vec![event]).await {
        tracing::warn!(
            project_skill_id = skill.id.as_str(),
            error = %error,
            "Failed to record learned skill full-load telemetry"
        );
    }
}

async fn trusted_full_load_attribution(
    state: &HttpServerState,
    headers: &HeaderMap,
    skill: &ProjectSkill,
) -> Result<Option<SkillUsageAttribution>, String> {
    let conversation_header = headers.get("x-ralphx-conversation-id");
    let run_header = headers.get("x-ralphx-agent-run-id");
    if conversation_header.is_none() && run_header.is_none() {
        return Ok(None);
    }
    let conversation_id = conversation_header
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "run identity requires a valid conversation identity".to_string())?
        .parse::<ChatConversationId>()
        .map_err(|_| "conversation identity is malformed".to_string())?;
    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| format!("conversation identity lookup failed: {error}"))?
        .ok_or_else(|| "conversation identity is stale".to_string())?;
    let resolved_project_id =
        crate::application::chat_service::chat_service_context::resolve_project_id(
            conversation.context_type,
            &conversation.context_id,
            Arc::clone(&state.app_state.task_repo),
            Arc::clone(&state.app_state.ideation_session_repo),
            Arc::clone(&state.app_state.delegated_session_repo),
        )
        .await
        .ok_or_else(|| "conversation has no resolvable project authority".to_string())?;
    if resolved_project_id != skill.project_id.as_str() {
        return Err("conversation belongs to a different project".to_string());
    }

    let Some(run_value) = run_header else {
        return Ok(Some(SkillUsageAttribution::BoundedConversation {
            conversation_id: conversation_id.as_str().to_string(),
            reason: "agent_run_header_absent".to_string(),
            stage: Some(skill.stage.clone()),
            bucket: Some(skill.bucket.clone()),
        }));
    };
    let run_id = run_value
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "run identity is malformed".to_string())?
        .parse::<AgentRunId>()
        .map_err(|_| "run identity is malformed".to_string())?;
    let run = state
        .app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .map_err(|error| format!("run identity lookup failed: {error}"))?
        .ok_or_else(|| "run identity is stale".to_string())?;
    if run.conversation_id != conversation_id {
        return Err("run identity belongs to a different conversation".to_string());
    }
    let active_run = state
        .app_state
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await
        .map_err(|error| format!("active run lookup failed: {error}"))?;
    if active_run.as_ref().map(|active| &active.id) != Some(&run_id) {
        return Err("run identity is stale or no longer active".to_string());
    }
    let harness = run
        .harness
        .ok_or_else(|| "run identity has no authoritative harness".to_string())?;
    Ok(Some(SkillUsageAttribution::ExactRun {
        conversation_id: conversation_id.as_str().to_string(),
        agent_run_id: run_id.as_str().to_string(),
        provider_harness: harness.to_string(),
        stage: Some(skill.stage.clone()),
        bucket: Some(skill.bucket.clone()),
    }))
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

pub async fn update_project_skill(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<UpdateProjectSkillRequest>,
) -> Result<Json<ProjectSkillLifecycleResponse>, HttpError> {
    let skill_id = ProjectSkillId::from_string(req.project_skill_id);
    let existing = state
        .app_state
        .project_skill_repo
        .get_by_id(&skill_id)
        .await
        .map_err(|error| {
            error!("failed to get project skill before update: {}", error);
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
    let updated = service
        .update_skill_content(UpdateProjectSkillContentInput {
            project_id: existing.project_id,
            project_skill_id: skill_id,
            title: req.title,
            bucket: req.bucket,
            stage: req.stage,
            scope_paths: req.scope_paths,
            compact_guidance: req.compact_guidance,
            body_markdown: req.body_markdown,
            predicted_effect: req.predicted_effect,
            source_sync_enabled: req.source_sync_enabled,
        })
        .await
        .map_err(|error| match error {
            AppError::Validation(message) => HttpError {
                status: StatusCode::BAD_REQUEST,
                message: Some(message),
            },
            AppError::Conflict(message) => HttpError {
                status: StatusCode::CONFLICT,
                message: Some(message),
            },
            other => {
                error!("failed to update project skill: {}", other);
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some("failed to update project skill".to_string()),
                }
            }
        })?;

    Ok(Json(ProjectSkillLifecycleResponse {
        skill: updated.map(ProjectSkillResponse::from),
    }))
}

pub async fn distill_project_skills(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<DistillProjectSkillsRequest>,
) -> Result<Json<DistillProjectSkillsResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let requested_source = req
        .source
        .as_deref()
        .map(str::parse::<TaskOutcomeSource>)
        .transpose()
        .map_err(|error| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(error.to_string()),
        })?;
    let limit = req.limit.unwrap_or(10).clamp(1, 10);
    let include_git_history = req.include_git_history.unwrap_or(false);
    let include_github_pr_history = req.include_github_pr_history.unwrap_or(false);
    let mut schedules = vec![
        schedule_explicit_project_skill_distillation(
            &state.app_state,
            &project_id,
            ProjectSkillDistillationSelection::EligibleOutcomes {
                source: requested_source,
                limit,
            },
            None,
            ChatContextType::Project,
            project_id.as_str(),
        )
        .await,
    ];
    let mut ingested_outcomes = 0;
    let mut scanned_git_commits = 0;
    let mut scanned_github_prs = 0;

    if include_git_history {
        match ingest_recent_git_history_outcomes(&state, &project_id).await {
            Ok(summary) => {
                ingested_outcomes = summary.ingested_outcomes;
                scanned_git_commits = summary.scanned_git_commits;
                scanned_github_prs = summary.scanned_github_prs;
                schedules.push(
                    schedule_explicit_project_skill_distillation(
                        &state.app_state,
                        &project_id,
                        ProjectSkillDistillationSelection::ExactOutcomes(summary.outcome_ids),
                        None,
                        ChatContextType::Project,
                        project_id.as_str(),
                    )
                    .await,
                );
            }
            Err(error) => {
                error!(
                    "failed to ingest git history for skill candidates: {}",
                    error
                );
            }
        }
    }
    if include_github_pr_history {
        match ingest_recent_github_pr_outcomes(&state, &project_id).await {
            Ok(summary) => {
                ingested_outcomes += summary.ingested_outcomes;
                scanned_git_commits += summary.scanned_git_commits;
                scanned_github_prs += summary.scanned_github_prs;
                schedules.push(
                    schedule_explicit_project_skill_distillation(
                        &state.app_state,
                        &project_id,
                        ProjectSkillDistillationSelection::ExactOutcomes(summary.outcome_ids),
                        None,
                        ChatContextType::Project,
                        project_id.as_str(),
                    )
                    .await,
                );
            }
            Err(error) => {
                error!(
                    "failed to ingest GitHub PR history for skill candidates: {}",
                    error
                );
            }
        }
    }

    let schedule = combine_distillation_schedules(&schedules);
    Ok(Json(DistillProjectSkillsResponse {
        status: schedule.status.as_str().to_string(),
        selected_outcomes: schedule.selected_outcomes,
        batch_count: schedule.batch_count,
        started_batches: schedule.started_batches,
        message: schedule.message,
        ingested_outcomes,
        scanned_git_commits,
        scanned_github_prs,
    }))
}

fn combine_distillation_schedules(
    schedules: &[ProjectSkillDistillationScheduleResult],
) -> ProjectSkillDistillationScheduleResult {
    let selected_outcomes = schedules
        .iter()
        .map(|result| result.selected_outcomes)
        .sum();
    let batch_count = schedules.iter().map(|result| result.batch_count).sum();
    let started_batches = schedules.iter().map(|result| result.started_batches).sum();
    let status = if started_batches > 0 {
        ProjectSkillDistillationScheduleStatus::Started
    } else if schedules
        .iter()
        .any(|result| result.status == ProjectSkillDistillationScheduleStatus::Failed)
    {
        ProjectSkillDistillationScheduleStatus::Failed
    } else if schedules
        .iter()
        .any(|result| result.status == ProjectSkillDistillationScheduleStatus::Queued)
    {
        ProjectSkillDistillationScheduleStatus::Queued
    } else if schedules
        .iter()
        .any(|result| result.status == ProjectSkillDistillationScheduleStatus::Unavailable)
    {
        ProjectSkillDistillationScheduleStatus::Unavailable
    } else {
        ProjectSkillDistillationScheduleStatus::Skipped
    };
    ProjectSkillDistillationScheduleResult {
        status,
        selected_outcomes,
        batch_count,
        started_batches,
        message: Some(match status {
            ProjectSkillDistillationScheduleStatus::Started if started_batches < batch_count => {
                "Some evidence batches started; the remainder stay queued for retry.".to_string()
            }
            ProjectSkillDistillationScheduleStatus::Started => {
                "Evidence queued and the skill distiller started.".to_string()
            }
            ProjectSkillDistillationScheduleStatus::Failed => {
                "Evidence remains queued because the skill distiller could not start.".to_string()
            }
            ProjectSkillDistillationScheduleStatus::Queued => {
                "Evidence is already queued or being processed by the distiller.".to_string()
            }
            ProjectSkillDistillationScheduleStatus::Unavailable => {
                "Evidence remains queued until the project runtime is available.".to_string()
            }
            ProjectSkillDistillationScheduleStatus::Skipped => schedules
                .iter()
                .find_map(|result| result.message.clone())
                .unwrap_or_else(|| "No eligible evidence was available to queue.".to_string()),
        }),
    }
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

async fn ingest_recent_git_history_outcomes(
    state: &HttpServerState,
    project_id: &ProjectId,
) -> AppResult<GitHistoryIngestSummary> {
    let project = state
        .app_state
        .project_repo
        .get_by_id(project_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("project {} not found", project_id)))?;
    let working_dir =
        validate_absolute_non_root_path(Path::new(&project.working_directory), "project root")?;
    if !working_dir.is_dir() {
        return Ok(GitHistoryIngestSummary::default());
    }

    let commits = read_recent_git_commits(&working_dir, GIT_HISTORY_SCAN_LIMIT).await?;
    let scanned_git_commits = commits.len();
    let mut ingested_outcomes = 0;
    let mut outcome_ids = Vec::new();
    for commit in commits {
        let mut outcome = new_empty_task_outcome(
            project_id.clone(),
            GIT_HISTORY_DISTILL_SOURCE,
            "commit",
            commit.sha.clone(),
        );
        outcome.status = TaskOutcomeStatus::Eligible;
        outcome.outcome_class = Some(TaskOutcomeClass::GitHistoryCommit);
        outcome.evidence_json = serde_json::json!({
            "source": "git_log",
            "commit_sha": commit.sha,
            "authored_at": commit.authored_at,
            "author_name": commit.author_name,
            "subject": commit.subject,
            "scan_limit": GIT_HISTORY_SCAN_LIMIT,
        });
        let saved = state
            .app_state
            .task_outcome_repo
            .upsert(UpsertTaskOutcomeInput { outcome })
            .await?;
        ingested_outcomes += 1;
        if saved.status == TaskOutcomeStatus::Eligible {
            outcome_ids.push(saved.id);
        }
    }

    Ok(GitHistoryIngestSummary {
        ingested_outcomes,
        scanned_git_commits,
        scanned_github_prs: 0,
        outcome_ids,
    })
}

async fn ingest_recent_github_pr_outcomes(
    state: &HttpServerState,
    project_id: &ProjectId,
) -> AppResult<GitHistoryIngestSummary> {
    let project = state
        .app_state
        .project_repo
        .get_by_id(project_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("project {} not found", project_id)))?;
    let working_dir =
        validate_absolute_non_root_path(Path::new(&project.working_directory), "project root")?;
    if !working_dir.is_dir() {
        return Ok(GitHistoryIngestSummary::default());
    }

    let pull_requests =
        read_recent_github_pull_requests(&working_dir, GITHUB_PR_HISTORY_SCAN_LIMIT).await?;
    let scanned_github_prs = pull_requests.len();
    let mut ingested_outcomes = 0;
    let mut outcome_ids = Vec::new();
    for pull_request in pull_requests {
        let mut outcome = new_empty_task_outcome(
            project_id.clone(),
            GITHUB_PR_DISTILL_SOURCE,
            "pull_request",
            pull_request.number.to_string(),
        );
        outcome.status = match pull_request.state.as_deref() {
            Some("MERGED") => TaskOutcomeStatus::Succeeded,
            Some("CLOSED") => TaskOutcomeStatus::Eligible,
            _ => TaskOutcomeStatus::Eligible,
        };
        outcome.outcome_class = Some(TaskOutcomeClass::GithubPrHistory);
        // Redact the free-text PR title before it lands in evidence_json: this
        // path feeds the distiller and can reach a committed SKILL.md, so it must
        // be scrubbed just like the enriched single-PR path.
        outcome.evidence_json = serde_json::json!({
            "source": "gh_pr_list",
            "number": pull_request.number,
            "title": redact_pr_text(&pull_request.title),
            "state": pull_request.state,
            "url": pull_request.url,
            "merged_at": pull_request.merged_at,
            "closed_at": pull_request.closed_at,
            "updated_at": pull_request.updated_at,
            "head_ref_name": pull_request.head_ref_name,
            "base_ref_name": pull_request.base_ref_name,
            "scan_limit": GITHUB_PR_HISTORY_SCAN_LIMIT,
        });
        let saved = state
            .app_state
            .task_outcome_repo
            .upsert(UpsertTaskOutcomeInput { outcome })
            .await?;
        ingested_outcomes += 1;
        if saved.status == TaskOutcomeStatus::Eligible {
            outcome_ids.push(saved.id);
        }
    }

    Ok(GitHistoryIngestSummary {
        ingested_outcomes,
        scanned_git_commits: 0,
        scanned_github_prs,
        outcome_ids,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GitCommitSummary {
    pub(super) sha: String,
    pub(super) authored_at: String,
    pub(super) author_name: String,
    pub(super) subject: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct GithubPrSummary {
    pub(super) number: i64,
    pub(super) title: String,
    pub(super) state: Option<String>,
    pub(super) url: Option<String>,
    pub(super) merged_at: Option<String>,
    pub(super) closed_at: Option<String>,
    pub(super) updated_at: Option<String>,
    pub(super) head_ref_name: Option<String>,
    pub(super) base_ref_name: Option<String>,
}

async fn read_recent_github_pull_requests(
    working_dir: &Path,
    limit: usize,
) -> AppResult<Vec<GithubPrSummary>> {
    let output = timeout(Duration::from_secs(10), async {
        let child = Command::new(resolve_gh_cli_path())
            .arg("pr")
            .arg("list")
            .arg("--state")
            .arg("all")
            .arg("--limit")
            .arg(limit.max(1).to_string())
            .arg("--json")
            .arg("number,title,state,mergedAt,closedAt,updatedAt,headRefName,baseRefName,url")
            .current_dir(working_dir)
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                AppError::Infrastructure(format!("failed to start gh pr list: {error}"))
            })?;
        child.wait_with_output().await.map_err(|error| {
            AppError::Infrastructure(format!("failed to read gh pr list output: {error}"))
        })
    })
    .await
    .map_err(|_| AppError::Infrastructure("timed out reading GitHub PR history".to_string()))??;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    parse_github_pr_summaries(&String::from_utf8_lossy(&output.stdout))
}

pub(super) fn parse_github_pr_summaries(output: &str) -> AppResult<Vec<GithubPrSummary>> {
    let mut pull_requests =
        serde_json::from_str::<Vec<GithubPrSummary>>(output).map_err(|error| {
            AppError::Infrastructure(format!("failed to parse gh PR history: {error}"))
        })?;
    pull_requests
        .retain(|pull_request| pull_request.number > 0 && !pull_request.title.trim().is_empty());
    Ok(pull_requests)
}

/// or rendered into an exported (committed) SKILL.md.
pub(super) fn redact_pr_text(text: &str) -> String {
    use std::sync::OnceLock;
    static TOKEN_PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    let patterns = TOKEN_PATTERNS.get_or_init(|| {
        [
            r"gh[pousr]_[A-Za-z0-9]{20,}",
            r"github_pat_[A-Za-z0-9_]{20,}",
            r"sk-(?:ant-)?[A-Za-z0-9_\-]{16,}",
            r"rxk_(?:live|test)_[A-Za-z0-9]{8,}",
            r"AKIA[0-9A-Z]{16}",
            r"xox[baprs]-[A-Za-z0-9\-]{10,}",
            r"AIza[0-9A-Za-z_\-]{35}",
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
        ]
        .iter()
        .filter_map(|pattern| regex::Regex::new(pattern).ok())
        .collect()
    });
    let mut output = text.to_string();
    for pattern in patterns {
        output = pattern.replace_all(&output, "[REDACTED]").into_owned();
    }
    static KV_PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    let kv = KV_PATTERN.get_or_init(|| {
        // Value capture handles quoted strings ("hunter2 with spaces") fully, and
        // otherwise masks the first unquoted token (so inline prose after a
        // `KEY=value word word` is preserved).
        regex::Regex::new(
            r#"(?i)\b([A-Z0-9_]*(?:secret|token|password|passwd|api[_-]?key|access[_-]?key|private[_-]?key)[A-Z0-9_]*)\s*[:=]\s*(?:"[^"]*"|'[^']*'|\S+)"#,
        )
        .expect("valid secret key=value regex")
    });
    kv.replace_all(&output, "$1=[REDACTED]").into_owned()
}

async fn read_recent_git_commits(
    working_dir: &Path,
    limit: usize,
) -> AppResult<Vec<GitCommitSummary>> {
    let output = timeout(Duration::from_secs(8), async {
        let child = Command::new(resolve_git_cli_path())
            .arg("-C")
            .arg(working_dir)
            .arg("log")
            .arg("--no-merges")
            .arg(format!("--max-count={}", limit.max(1)))
            .arg("--pretty=format:%H%x1f%aI%x1f%an%x1f%s%x1e")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                AppError::Infrastructure(format!("failed to start git log: {error}"))
            })?;
        child.wait_with_output().await.map_err(|error| {
            AppError::Infrastructure(format!("failed to read git log output: {error}"))
        })
    })
    .await
    .map_err(|_| AppError::Infrastructure("timed out reading git history".to_string()))??;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_git_log_summaries(&stdout)
        .into_iter()
        .take(limit.max(1))
        .collect())
}

pub(super) fn parse_git_log_summaries(output: &str) -> Vec<GitCommitSummary> {
    output
        .split('\x1e')
        .filter_map(|record| {
            let record = record.trim();
            if record.is_empty() {
                return None;
            }
            let mut parts = record.splitn(4, '\x1f');
            let sha = parts.next()?.trim();
            let authored_at = parts.next()?.trim();
            let author_name = parts.next()?.trim();
            let subject = parts.next()?.trim();
            if sha.is_empty() || subject.is_empty() {
                return None;
            }
            Some(GitCommitSummary {
                sha: sha.to_string(),
                authored_at: authored_at.to_string(),
                author_name: author_name.to_string(),
                subject: subject.to_string(),
            })
        })
        .collect()
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
        synced_count: 0,
    }))
}

pub async fn apply_project_skill_directory_import(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ProjectSkillDirectoryImportRequest>,
) -> Result<Json<ApplyProjectSkillImportResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    if !req.confirm_import {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(
                "project skill directory import requires confirm_import=true".to_string(),
            ),
        });
    }

    let source_roots = selected_project_skill_source_roots(req.source_roots)?;
    let source_sync_enabled = req.source_sync_enabled.unwrap_or(false);
    let candidates =
        scan_project_native_skills(&state, &project_id, &source_roots, source_sync_enabled)
            .await
            .map_err(|error| {
                error!("failed to scan project .claude/skills: {}", error);
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some("failed to scan project skills".to_string()),
                }
            })?;
    let synced_count = sync_source_tracked_project_skills(&state, &project_id, &candidates)
        .await
        .map_err(|error| {
            error!("failed to sync project source skills: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to sync project source skills".to_string()),
            }
        })?;
    let service =
        ProjectSkillImportPreviewService::new(Arc::clone(&state.app_state.project_skill_repo));
    let result = service
        .apply_import(ProjectSkillImportApplyInput {
            project_id: project_id.clone(),
            candidates: candidates.clone(),
            confirm_import: true,
        })
        .await
        .map_err(|error| match error {
            AppError::Validation(message) => HttpError {
                status: StatusCode::BAD_REQUEST,
                message: Some(message),
            },
            other => {
                error!("failed to import project .claude/skills: {}", other);
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some("failed to import project skills".to_string()),
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
        synced_count,
    }))
}

async fn scan_project_native_skills(
    state: &HttpServerState,
    project_id: &ProjectId,
    source_roots: &[String],
    source_sync_enabled: bool,
) -> AppResult<Vec<ProjectSkillImportCandidate>> {
    let project = state
        .app_state
        .project_repo
        .get_by_id(project_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("project {} not found", project_id)))?;
    let project_root =
        validate_absolute_non_root_path(Path::new(&project.working_directory), "project root")?;
    let mut candidates = Vec::new();
    for source_root in source_roots {
        candidates.extend(
            scan_project_skill_source_root(&project_root, source_root, source_sync_enabled).await?,
        );
    }

    Ok(candidates)
}

pub(super) async fn scan_project_skill_source_root(
    project_root: &Path,
    source_root: &str,
    source_sync_enabled: bool,
) -> AppResult<Vec<ProjectSkillImportCandidate>> {
    let canonical_project_root = tokio::fs::canonicalize(project_root)
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!(
                "failed to canonicalize project root {}: {error}",
                project_root.display()
            ))
        })?;
    let skills_root = canonical_project_root.join(source_root);
    if !skills_root.exists() {
        return Ok(Vec::new());
    }
    let metadata = tokio::fs::symlink_metadata(&skills_root)
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!(
                "failed to inspect project skills directory {}: {error}",
                skills_root.display()
            ))
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(Vec::new());
    }
    let canonical_skills_root = tokio::fs::canonicalize(&skills_root)
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!(
                "failed to canonicalize project skills directory {}: {error}",
                skills_root.display()
            ))
        })?;
    if !canonical_skills_root.starts_with(&canonical_project_root) {
        return Err(AppError::Validation(format!(
            "project skills directory {} escapes project root",
            source_root
        )));
    }

    let mut entries = tokio::fs::read_dir(&canonical_skills_root)
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!(
                "failed to read project skills directory {}: {error}",
                canonical_skills_root.display()
            ))
        })?;
    let mut candidates = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        AppError::Infrastructure(format!("failed to read project skill entry: {error}"))
    })? {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !is_safe_native_skill_dir_name(&file_name) {
            continue;
        }
        let entry_metadata = entry.metadata().await.map_err(|error| {
            AppError::Infrastructure(format!("failed to inspect project skill entry: {error}"))
        })?;
        if !entry_metadata.is_dir() || entry_metadata.file_type().is_symlink() {
            continue;
        }
        let skill_file = contained_native_skill_file(
            &canonical_project_root,
            &canonical_skills_root,
            &file_name,
        )?;
        // The file path is built from a canonicalized project-owned skill root,
        // an allowlisted directory component, and the fixed SKILL.md leaf.
        // codeql[rust/path-injection]
        let file_metadata = match tokio::fs::symlink_metadata(&skill_file).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(AppError::Infrastructure(format!(
                    "failed to inspect project skill file {}: {error}",
                    skill_file.display()
                )));
            }
        };
        if !file_metadata.is_file() || file_metadata.file_type().is_symlink() {
            continue;
        }
        // The same validated path is reused after rejecting symlinks.
        // codeql[rust/path-injection]
        let raw_markdown = tokio::fs::read_to_string(&skill_file)
            .await
            .map_err(|error| {
                AppError::Infrastructure(format!(
                    "failed to read project skill file {}: {error}",
                    skill_file.display()
                ))
            })?;
        // Parse open-standard frontmatter symmetrically with the exporter so
        // `description` and `paths` round-trip and YAML lines are never scraped
        // into the body guidance.
        let (frontmatter, body) = split_skill_frontmatter(&raw_markdown);
        let title = native_skill_title(&body)
            .or_else(|| {
                frontmatter
                    .as_ref()
                    .and_then(|matter| matter.name.as_deref())
                    .map(humanize_skill_dir)
            })
            .unwrap_or_else(|| humanize_skill_dir(&file_name));
        // Strip the H1/Predicted-Effect wrapper so re-export does not duplicate them.
        let (body_markdown, extracted_effect) = split_imported_skill_body(&body);
        let compact_guidance = frontmatter
            .as_ref()
            .and_then(|matter| matter.description.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            // Same cap as the exporter so the description does not drift on round-trip.
            .map(|value| truncate_text(value, MAX_SKILL_DESCRIPTION_CHARS))
            .or_else(|| native_skill_compact_guidance(&body_markdown))
            .unwrap_or_else(|| {
                format!(
                    "Use the `{}` project skill when its procedure applies.",
                    title
                )
            });
        let scope_paths = frontmatter
            .as_ref()
            .map(|matter| {
                matter
                    .paths
                    .iter()
                    .map(|path| path.trim().to_string())
                    .filter(|path| !path.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let predicted_effect = extracted_effect.unwrap_or_else(|| {
            "Makes an existing target-repository skill available for RalphX review, approval, and future injection.".to_string()
        });
        let relative_path = format!("{source_root}/{file_name}/SKILL.md");
        candidates.push(ProjectSkillImportCandidate {
            external_id: Some(relative_path.clone()),
            title,
            bucket: "execution".to_string(),
            stage: "execution".to_string(),
            scope_paths,
            compact_guidance,
            body_markdown,
            predicted_effect,
            provenance_json: serde_json::json!({
                "source": "target_project_skill_folder",
                "relative_path": relative_path,
                "source_root": source_root,
                "source_sync_enabled": source_sync_enabled,
            }),
            source_snapshot_json: serde_json::json!({
                "kind": "target_project_skill_folder",
                "relative_path": relative_path,
                "source_root": source_root,
                "source_sync_enabled": source_sync_enabled,
            }),
        });
    }
    Ok(candidates)
}

pub(super) fn contained_native_skill_file(
    canonical_project_root: &Path,
    canonical_skills_root: &Path,
    file_name: &str,
) -> AppResult<PathBuf> {
    if !is_safe_native_skill_dir_name(file_name) {
        return Err(AppError::Validation(
            "project skill folder name contains unsafe characters".to_string(),
        ));
    }
    if !canonical_skills_root.starts_with(canonical_project_root) {
        return Err(AppError::Validation(
            "project skills directory escapes project root".to_string(),
        ));
    }
    let skill_file = canonical_skills_root.join(file_name).join("SKILL.md");
    let Some(skill_parent) = skill_file.parent() else {
        return Err(AppError::Validation(
            "project skill file has no parent directory".to_string(),
        ));
    };
    if !skill_parent.starts_with(canonical_skills_root) {
        return Err(AppError::Validation(
            "project skill file escapes project skills directory".to_string(),
        ));
    }
    Ok(skill_file)
}

fn is_safe_native_skill_dir_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) fn selected_project_skill_source_roots(
    requested: Vec<String>,
) -> Result<Vec<String>, HttpError> {
    let roots = if requested.is_empty() {
        vec![".claude/skills".to_string()]
    } else {
        requested
    };
    let mut selected = Vec::new();
    for root in roots {
        let root = root.trim().trim_matches('/').to_string();
        if !is_supported_project_skill_source_root(&root) {
            return Err(HttpError {
                status: StatusCode::BAD_REQUEST,
                message: Some(format!("unsupported project skill source folder: {root}")),
            });
        }
        if !selected.contains(&root) {
            selected.push(root);
        }
    }
    Ok(selected)
}

fn is_supported_project_skill_source_root(value: &str) -> bool {
    matches!(
        value,
        ".claude/skills" | ".codex/skills" | ".agents/skills" | ".ralphx/skills"
    )
}

pub(super) async fn sync_source_tracked_project_skills(
    state: &HttpServerState,
    project_id: &ProjectId,
    candidates: &[ProjectSkillImportCandidate],
) -> AppResult<usize> {
    let service = ProjectSkillService::new(Arc::clone(&state.app_state.project_skill_repo));
    let mut synced_count = 0;
    for candidate in candidates {
        let Some(result) = service
            .sync_source_candidate(project_id.clone(), candidate.clone())
            .await?
        else {
            continue;
        };
        if result.outcome != crate::domain::repositories::ProjectSkillResolutionOutcome::Duplicate {
            synced_count += 1;
        }
    }
    Ok(synced_count)
}

#[cfg(test)]
pub(super) fn project_skill_source_sync_enabled(skill: &ProjectSkill) -> bool {
    skill
        .provenance_json
        .get("source_sync_enabled")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            skill
                .provenance_json
                .get("source_snapshot")
                .and_then(|value| value.get("source_sync_enabled"))
                .and_then(serde_json::Value::as_bool)
        })
        .unwrap_or(false)
}

/// Open Agent Skills frontmatter fields RalphX reads back on import. Unknown
/// keys (e.g. `metadata`, Claude-only fields) are ignored so any spec-compliant
/// SKILL.md parses cleanly.
#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct ParsedSkillFrontmatter {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(default)]
    pub(super) paths: Vec<String>,
}

/// Split a leading `---`-fenced YAML frontmatter block from the markdown body.
/// Returns the parsed frontmatter (when present and valid) and the body with the
/// frontmatter removed, so body scraping never reads YAML values.
pub(super) fn split_skill_frontmatter(markdown: &str) -> (Option<ParsedSkillFrontmatter>, String) {
    let normalized = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let after_open = match normalized
        .strip_prefix("---\n")
        .or_else(|| normalized.strip_prefix("---\r\n"))
    {
        Some(rest) => rest,
        None => return (None, markdown.to_string()),
    };

    let mut frontmatter_text = String::new();
    let mut body_offset: Option<usize> = None;
    let mut offset = 0usize;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']) == "---" {
            body_offset = Some(offset + line.len());
            break;
        }
        frontmatter_text.push_str(line);
        offset += line.len();
    }

    let Some(body_offset) = body_offset else {
        // Opening fence with no closing fence: treat as plain markdown.
        return (None, markdown.to_string());
    };
    let body = after_open[body_offset..]
        .trim_start_matches(['\n', '\r'])
        .to_string();
    let parsed = serde_yaml::from_str::<ParsedSkillFrontmatter>(&frontmatter_text).ok();
    (parsed, body)
}

/// Strip a leading `# <title>` H1 and a trailing `## Predicted Effect` section
/// from an imported skill body, returning the procedure body and any extracted
/// predicted effect. This keeps the round-trip idempotent: a RalphX-exported
/// skill re-imports to the same procedure body the exporter will re-wrap, so the
/// H1/Predicted-Effect sections are not duplicated on re-export.
pub(super) fn split_imported_skill_body(body: &str) -> (String, Option<String>) {
    let (main, predicted_effect) = match body.split_once("\n## Predicted Effect") {
        Some((before, after)) => {
            let effect = after.trim_start_matches([':', ' ', '\n', '\r']).trim();
            let effect =
                (!effect.is_empty() && effect != "Not specified.").then(|| effect.to_string());
            (before, effect)
        }
        None => (body, None),
    };
    let trimmed = main.trim_start();
    let procedure = match trimmed.strip_prefix("# ") {
        Some(after_hash) => after_hash
            .split_once('\n')
            .map(|(_, rest)| rest)
            .unwrap_or("")
            .trim()
            .to_string(),
        None => trimmed.trim().to_string(),
    };
    (procedure, predicted_effect)
}

pub(super) fn native_skill_title(markdown: &str) -> Option<String> {
    markdown
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

pub(super) fn native_skill_compact_guidance(markdown: &str) -> Option<String> {
    markdown
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#') && !line.starts_with("---"))
        .find(|line| line.chars().count() >= 24)
        .map(|line| truncate_text(line, 220))
}

pub(super) fn humanize_skill_dir(value: &str) -> String {
    value
        .replace(['-', '_'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

pub(super) async fn update_project_skill_lifecycle(
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
        ProjectSkillLifecycleStatus::Staged
        | ProjectSkillLifecycleStatus::Stale
        | ProjectSkillLifecycleStatus::Retired => {
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
