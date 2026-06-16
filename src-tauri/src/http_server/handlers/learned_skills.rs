use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
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
    UpdateProjectSkillContentInput,
};
use crate::error::{AppError, AppResult};
use crate::http_server::handlers::learned_skills_export::assert_project_id_scope;
use crate::http_server::project_scope::{ProjectScope, ProjectScopeGuard};
use crate::http_server::types::HttpError;
use crate::application::project_skill_export_service::MAX_SKILL_DESCRIPTION_CHARS;
use crate::infrastructure::tool_paths::{resolve_gh_cli_path, resolve_git_cli_path};
use crate::utils::path_safety::validate_absolute_non_root_path;

const GIT_HISTORY_SCAN_LIMIT: usize = 50;
const GIT_HISTORY_DISTILL_SOURCE: &str = "git_commit_history";
const GITHUB_PR_HISTORY_SCAN_LIMIT: usize = 25;
const GITHUB_PR_CANDIDATE_LIST_LIMIT: usize = 25;
const GITHUB_PR_CANDIDATE_MAX_LIMIT: usize = 50;
const GITHUB_PR_DISTILL_SOURCE: &str = "github_pr_history";

#[derive(Debug, Clone, Copy, Default)]
struct GitHistoryIngestSummary {
    ingested_outcomes: usize,
    scanned_git_commits: usize,
    scanned_github_prs: usize,
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
    let requested_source = req.source.clone();
    let limit = req.limit.unwrap_or(25);
    let include_git_history = req.include_git_history.unwrap_or_else(|| {
        requested_source
            .as_deref()
            .map_or(true, |source| source == GIT_HISTORY_DISTILL_SOURCE)
    });
    let include_github_pr_history = req.include_github_pr_history.unwrap_or_else(|| {
        requested_source
            .as_deref()
            .map_or(true, |source| source == GITHUB_PR_DISTILL_SOURCE)
    });

    let distiller = ProjectSkillDistillerService::new(
        Arc::clone(&state.app_state.task_outcome_repo),
        Arc::clone(&state.app_state.project_skill_repo),
    );
    let mut result = distiller
        .distill_eligible_outcomes(DistillEligibleOutcomesInput {
            project_id: project_id.clone(),
            source: requested_source.clone(),
            limit,
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
    let mut ingested_outcomes = 0;
    let mut scanned_git_commits = 0;
    let mut scanned_github_prs = 0;

    if result.staged_skills.is_empty() && include_git_history {
        match ingest_recent_git_history_outcomes(&state, &project_id).await {
            Ok(summary) if summary.ingested_outcomes > 0 => {
                ingested_outcomes = summary.ingested_outcomes;
                scanned_git_commits = summary.scanned_git_commits;
                scanned_github_prs = summary.scanned_github_prs;
                result = distiller
                    .distill_eligible_outcomes(DistillEligibleOutcomesInput {
                        project_id: project_id.clone(),
                        source: Some(GIT_HISTORY_DISTILL_SOURCE.to_string()),
                        limit,
                        origin: ProjectSkillDistillationOrigin::ManualCurator,
                    })
                    .await
                    .map_err(|error| {
                        error!("failed to distill git history project skills: {}", error);
                        HttpError {
                            status: StatusCode::INTERNAL_SERVER_ERROR,
                            message: Some("failed to distill project skills".to_string()),
                        }
                    })?;
            }
            Ok(summary) => {
                scanned_git_commits = summary.scanned_git_commits;
                scanned_github_prs = summary.scanned_github_prs;
            }
            Err(error) => {
                error!(
                    "failed to ingest git history for skill candidates: {}",
                    error
                );
            }
        }
    }
    if result.staged_skills.is_empty() && include_github_pr_history {
        match ingest_recent_github_pr_outcomes(&state, &project_id).await {
            Ok(summary) if summary.ingested_outcomes > 0 => {
                ingested_outcomes += summary.ingested_outcomes;
                scanned_git_commits += summary.scanned_git_commits;
                scanned_github_prs += summary.scanned_github_prs;
                result = distiller
                    .distill_eligible_outcomes(DistillEligibleOutcomesInput {
                        project_id: project_id.clone(),
                        source: Some(GITHUB_PR_DISTILL_SOURCE.to_string()),
                        limit,
                        origin: ProjectSkillDistillationOrigin::ManualCurator,
                    })
                    .await
                    .map_err(|error| {
                        error!("failed to distill GitHub PR project skills: {}", error);
                        HttpError {
                            status: StatusCode::INTERNAL_SERVER_ERROR,
                            message: Some("failed to distill project skills".to_string()),
                        }
                    })?;
            }
            Ok(summary) => {
                scanned_git_commits += summary.scanned_git_commits;
                scanned_github_prs += summary.scanned_github_prs;
            }
            Err(error) => {
                error!(
                    "failed to ingest GitHub PR history for skill candidates: {}",
                    error
                );
            }
        }
    }

    Ok(Json(DistillProjectSkillsResponse {
        staged_skills: result
            .staged_skills
            .into_iter()
            .map(ProjectSkillResponse::from)
            .collect(),
        skipped_existing: result.skipped_existing,
        ingested_outcomes,
        scanned_git_commits,
        scanned_github_prs,
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

pub async fn list_project_skill_pull_request_candidates(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ListProjectSkillPullRequestCandidatesRequest>,
) -> Result<Json<ListProjectSkillPullRequestCandidatesResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    let limit = req
        .limit
        .unwrap_or(GITHUB_PR_CANDIDATE_LIST_LIMIT)
        .clamp(1, GITHUB_PR_CANDIDATE_MAX_LIMIT);
    let working_dir = project_working_dir(&state, &project_id).await?;
    if !working_dir.is_dir() {
        return Ok(Json(ListProjectSkillPullRequestCandidatesResponse {
            candidates: Vec::new(),
            count: 0,
            limit,
        }));
    }

    let candidates = read_recent_github_pull_requests(&working_dir, limit)
        .await
        .map_err(|error| {
            error!("failed to list GitHub PR skill candidates: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to list GitHub PR skill candidates".to_string()),
            }
        })?
        .into_iter()
        .map(ProjectSkillPullRequestCandidateResponse::from)
        .collect::<Vec<_>>();
    let count = candidates.len();

    Ok(Json(ListProjectSkillPullRequestCandidatesResponse {
        candidates,
        count,
        limit,
    }))
}

pub async fn stage_project_skill_from_pull_request(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<StageProjectSkillFromPullRequestRequest>,
) -> Result<Json<StageProjectSkillFromPullRequestResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id);
    assert_project_id_scope(&project_id, &scope)?;
    if req.number <= 0 {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("pull request number must be positive".to_string()),
        });
    }

    let working_dir = project_working_dir(&state, &project_id).await?;
    if !working_dir.is_dir() {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("project working directory is unavailable".to_string()),
        });
    }
    let pull_request =
        read_recent_github_pull_requests(&working_dir, GITHUB_PR_CANDIDATE_MAX_LIMIT)
            .await
            .map_err(|error| {
                error!("failed to read GitHub PR skill candidate: {}", error);
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some("failed to read GitHub PR skill candidate".to_string()),
                }
            })?
            .into_iter()
            .find(|candidate| candidate.number == req.number)
            .ok_or_else(|| HttpError {
                status: StatusCode::NOT_FOUND,
                message: Some(
                    "pull request candidate was not found in recent GitHub PRs".to_string(),
                ),
            })?;

    if let Some(existing) =
        existing_project_skill_for_pull_request(&state, &project_id, pull_request.number).await?
    {
        return Ok(Json(StageProjectSkillFromPullRequestResponse {
            skill: Some(ProjectSkillResponse::from(existing)),
            skipped_existing: true,
        }));
    }

    // Enrich from `gh pr view` (files, commits, reviews, labels, body) so the
    // candidate encodes real triggers + scope; fall back to summary-only
    // templating if detail is unavailable (graceful degradation).
    let detail = read_github_pull_request_detail(&working_dir, pull_request.number)
        .await
        .unwrap_or(None);
    let source = match detail {
        Some(detail) => pr_skill_source_from_detail(detail),
        None => pr_skill_source_from_summary(&pull_request),
    };
    let fields = build_pr_skill_fields(&source);

    let now = Utc::now();
    let skill = ProjectSkill {
        id: ProjectSkillId::new(),
        project_id: project_id.clone(),
        title: fields.title,
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        status: ProjectSkillLifecycleStatus::Staged,
        pinned: false,
        archived: false,
        scope_paths: fields.scope_paths,
        compact_guidance: fields.compact_guidance,
        body_markdown: fields.body_markdown,
        predicted_effect: Some(fields.predicted_effect),
        provenance_json: serde_json::json!({
            "source": GITHUB_PR_DISTILL_SOURCE,
            "source_ref_kind": "pull_request",
            "source_ref_id": pull_request.number.to_string(),
            "pull_request_number": pull_request.number,
            "enriched_from_pr_detail": fields.enriched,
            "evidence": fields.evidence,
        }),
        companion_of_skill_id: None,
        created_at: now,
        updated_at: now,
    };

    let service = ProjectSkillService::new(Arc::clone(&state.app_state.project_skill_repo));
    let staged = service
        .stage_skill(skill)
        .await
        .map_err(|error| match error {
            AppError::Validation(message) => HttpError {
                status: StatusCode::BAD_REQUEST,
                message: Some(message),
            },
            other => {
                error!("failed to stage GitHub PR project skill: {}", other);
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some("failed to stage GitHub PR project skill".to_string()),
                }
            }
        })?;

    Ok(Json(StageProjectSkillFromPullRequestResponse {
        skill: Some(ProjectSkillResponse::from(staged)),
        skipped_existing: false,
    }))
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
    for commit in commits {
        let mut outcome = new_empty_task_outcome(
            project_id.clone(),
            GIT_HISTORY_DISTILL_SOURCE,
            "commit",
            commit.sha.clone(),
        );
        outcome.status = TaskOutcomeStatus::Eligible;
        outcome.outcome_class = Some("git_history_commit".to_string());
        outcome.evidence_json = serde_json::json!({
            "source": "git_log",
            "commit_sha": commit.sha,
            "authored_at": commit.authored_at,
            "author_name": commit.author_name,
            "subject": commit.subject,
            "scan_limit": GIT_HISTORY_SCAN_LIMIT,
        });
        state
            .app_state
            .task_outcome_repo
            .upsert(UpsertTaskOutcomeInput { outcome })
            .await?;
        ingested_outcomes += 1;
    }

    Ok(GitHistoryIngestSummary {
        ingested_outcomes,
        scanned_git_commits,
        scanned_github_prs: 0,
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
        outcome.outcome_class = Some("github_pr_history".to_string());
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
        state
            .app_state
            .task_outcome_repo
            .upsert(UpsertTaskOutcomeInput { outcome })
            .await?;
        ingested_outcomes += 1;
    }

    Ok(GitHistoryIngestSummary {
        ingested_outcomes,
        scanned_git_commits: 0,
        scanned_github_prs,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitCommitSummary {
    sha: String,
    authored_at: String,
    author_name: String,
    subject: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct GithubPrSummary {
    number: i64,
    title: String,
    state: Option<String>,
    url: Option<String>,
    merged_at: Option<String>,
    closed_at: Option<String>,
    updated_at: Option<String>,
    head_ref_name: Option<String>,
    base_ref_name: Option<String>,
}

impl From<GithubPrSummary> for ProjectSkillPullRequestCandidateResponse {
    fn from(summary: GithubPrSummary) -> Self {
        Self {
            number: summary.number,
            title: summary.title,
            state: summary.state,
            url: summary.url,
            merged_at: summary.merged_at,
            closed_at: summary.closed_at,
            updated_at: summary.updated_at,
            head_ref_name: summary.head_ref_name,
            base_ref_name: summary.base_ref_name,
        }
    }
}

async fn project_working_dir(
    state: &HttpServerState,
    project_id: &ProjectId,
) -> Result<std::path::PathBuf, HttpError> {
    let project = state
        .app_state
        .project_repo
        .get_by_id(project_id)
        .await
        .map_err(|error| {
            error!(
                "failed to load project for skill candidate discovery: {}",
                error
            );
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to load project".to_string()),
            }
        })?
        .ok_or_else(|| HttpError {
            status: StatusCode::NOT_FOUND,
            message: Some(format!("project {} not found", project_id)),
        })?;
    validate_absolute_non_root_path(Path::new(&project.working_directory), "project root")
        .map(|path| path.to_path_buf())
        .map_err(|error| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(error.to_string()),
        })
}

async fn existing_project_skill_for_pull_request(
    state: &HttpServerState,
    project_id: &ProjectId,
    number: i64,
) -> Result<Option<ProjectSkill>, HttpError> {
    let source_ref_id = number.to_string();
    let skills = state
        .app_state
        .project_skill_repo
        .list_by_project(
            project_id,
            ProjectSkillListOptions {
                include_archived: true,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| {
            error!("failed to list project skills for PR dedupe: {}", error);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("failed to list project skills".to_string()),
            }
        })?;
    Ok(skills.into_iter().find(|skill| {
        let provenance = &skill.provenance_json;
        provenance
            .get("pull_request_number")
            .and_then(serde_json::Value::as_i64)
            == Some(number)
            || (provenance.get("source").and_then(serde_json::Value::as_str)
                == Some(GITHUB_PR_DISTILL_SOURCE)
                && provenance
                    .get("source_ref_kind")
                    .and_then(serde_json::Value::as_str)
                    == Some("pull_request")
                && provenance
                    .get("source_ref_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(source_ref_id.as_str()))
    }))
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

fn parse_github_pr_summaries(output: &str) -> AppResult<Vec<GithubPrSummary>> {
    let mut pull_requests =
        serde_json::from_str::<Vec<GithubPrSummary>>(output).map_err(|error| {
            AppError::Infrastructure(format!("failed to parse gh PR history: {error}"))
        })?;
    pull_requests
        .retain(|pull_request| pull_request.number > 0 && !pull_request.title.trim().is_empty());
    Ok(pull_requests)
}

const MAX_PR_BODY_EXCERPT_CHARS: usize = 600;
const MAX_PR_SCOPE_PATHS: usize = 8;
const MAX_PR_WORKFLOW_COMMITS: usize = 8;
const MAX_PR_SKILL_TITLE_CHARS: usize = 64;

/// Structured `gh pr view --json` fields used to build a useful, reusable skill
/// candidate. The raw diff is intentionally NOT fetched: structured metadata
/// (files, commits, reviews, labels, body) gives enough signal for deterministic
/// templating while avoiding the large-payload / secret-in-diff surface.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GithubPrDetail {
    #[serde(default)]
    number: i64,
    #[serde(default)]
    body: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    additions: i64,
    #[serde(default)]
    deletions: i64,
    #[serde(default)]
    base_ref_name: Option<String>,
    #[serde(default)]
    files: Vec<GithubPrFile>,
    #[serde(default)]
    commits: Vec<GithubPrCommit>,
    #[serde(default)]
    reviews: Vec<GithubPrReview>,
    #[serde(default)]
    labels: Vec<GithubPrLabel>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubPrFile {
    #[serde(default)]
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GithubPrCommit {
    #[serde(default)]
    message_headline: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubPrReview {
    #[serde(default)]
    state: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubPrLabel {
    #[serde(default)]
    name: String,
}

/// Read structured detail for one PR via `gh pr view <n> --json ...`.
/// Returns `Ok(None)` when gh is unavailable/unauthenticated or the PR is not
/// found, so staging degrades gracefully to summary-only templating.
async fn read_github_pull_request_detail(
    working_dir: &Path,
    number: i64,
) -> AppResult<Option<GithubPrDetail>> {
    if number <= 0 {
        return Ok(None);
    }
    let output = timeout(Duration::from_secs(15), async {
        let child = Command::new(resolve_gh_cli_path())
            .arg("pr")
            .arg("view")
            // `number` is a validated positive integer, never a raw string arg.
            .arg(number.to_string())
            .arg("--json")
            .arg("number,body,state,url,additions,deletions,baseRefName,files,commits,reviews,labels")
            .current_dir(working_dir)
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                AppError::Infrastructure(format!("failed to start gh pr view: {error}"))
            })?;
        child.wait_with_output().await.map_err(|error| {
            AppError::Infrastructure(format!("failed to read gh pr view output: {error}"))
        })
    })
    .await
    .map_err(|_| AppError::Infrastructure("timed out reading GitHub PR detail".to_string()))??;

    if !output.status.success() {
        return Ok(None);
    }
    let detail = serde_json::from_slice::<GithubPrDetail>(&output.stdout)
        .map_err(|error| AppError::Infrastructure(format!("failed to parse gh pr view: {error}")))?;
    Ok(Some(detail))
}

/// Mask common secret shapes before PR free-text is persisted to provenance JSON
/// or rendered into an exported (committed) SKILL.md.
fn redact_pr_text(text: &str) -> String {
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

/// Provider-neutral inputs for deterministic PR skill templating.
struct PrSkillSource {
    number: i64,
    url: Option<String>,
    state: Option<String>,
    base_ref: Option<String>,
    changed_paths: Vec<String>,
    commit_headlines: Vec<String>,
    review_states: Vec<String>,
    labels: Vec<String>,
    body_excerpt: Option<String>,
    additions: i64,
    deletions: i64,
    enriched: bool,
}

struct PrSkillFields {
    title: String,
    compact_guidance: String,
    body_markdown: String,
    predicted_effect: String,
    scope_paths: Vec<String>,
    evidence: serde_json::Value,
    enriched: bool,
}

fn pr_skill_source_from_detail(detail: GithubPrDetail) -> PrSkillSource {
    let changed_paths = detail
        .files
        .into_iter()
        .map(|file| file.path.trim().to_string())
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    let commit_headlines = detail
        .commits
        .into_iter()
        .map(|commit| redact_pr_text(commit.message_headline.trim()))
        .filter(|headline| !headline.is_empty())
        .collect::<Vec<_>>();
    let review_states = detail
        .reviews
        .into_iter()
        .map(|review| review.state.trim().to_string())
        .filter(|state| !state.is_empty())
        .collect::<Vec<_>>();
    let labels = detail
        .labels
        .into_iter()
        // Label names are arbitrary user-controlled free text → redact like other PR text.
        .map(|label| redact_pr_text(label.name.trim()))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    let body_excerpt = {
        let redacted = redact_pr_text(detail.body.trim());
        if redacted.is_empty() {
            None
        } else {
            Some(truncate_text(&redacted, MAX_PR_BODY_EXCERPT_CHARS))
        }
    };
    PrSkillSource {
        number: detail.number,
        url: detail.url,
        state: detail.state,
        base_ref: detail.base_ref_name,
        changed_paths,
        commit_headlines,
        review_states,
        labels,
        body_excerpt,
        additions: detail.additions,
        deletions: detail.deletions,
        enriched: true,
    }
}

fn pr_skill_source_from_summary(summary: &GithubPrSummary) -> PrSkillSource {
    PrSkillSource {
        number: summary.number,
        url: summary.url.clone(),
        state: summary.state.clone(),
        base_ref: summary.base_ref_name.clone(),
        changed_paths: Vec::new(),
        commit_headlines: Vec::new(),
        review_states: Vec::new(),
        labels: Vec::new(),
        body_excerpt: None,
        additions: 0,
        deletions: 0,
        enriched: false,
    }
}

/// Reduce a changed file path to a stable directory scope prefix (up to 3 dir
/// segments), dropping the file name. Top-level files yield no scope.
fn pr_scope_path(file: &str) -> Option<String> {
    let parts: Vec<&str> = file
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if parts.len() <= 1 {
        return None;
    }
    let take = (parts.len() - 1).min(3);
    Some(parts[..take].join("/"))
}

fn pr_scope_paths(changed_paths: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut paths = Vec::new();
    for file in changed_paths {
        if let Some(scope) = pr_scope_path(file) {
            if seen.insert(scope.clone()) {
                paths.push(scope);
                if paths.len() >= MAX_PR_SCOPE_PATHS {
                    break;
                }
            }
        }
    }
    paths
}

fn pr_file_extensions(changed_paths: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut extensions = Vec::new();
    for file in changed_paths {
        if let Some(extension) = Path::new(file).extension().and_then(|value| value.to_str()) {
            let dotted = format!(".{extension}");
            if seen.insert(dotted.clone()) {
                extensions.push(dotted);
                if extensions.len() >= 4 {
                    break;
                }
            }
        }
    }
    extensions
}

fn pr_primary_area(scope_paths: &[String]) -> String {
    scope_paths
        .first()
        .cloned()
        .unwrap_or_else(|| "the repository".to_string())
}

fn pr_review_summary(review_states: &[String]) -> String {
    if review_states.is_empty() {
        return "no recorded reviews".to_string();
    }
    let approved = review_states
        .iter()
        .filter(|state| state.eq_ignore_ascii_case("APPROVED"))
        .count();
    let changes = review_states
        .iter()
        .filter(|state| state.eq_ignore_ascii_case("CHANGES_REQUESTED"))
        .count();
    let mut parts = Vec::new();
    if approved > 0 {
        parts.push(format!("{approved} approving"));
    }
    if changes > 0 {
        parts.push(format!("{changes} requesting changes"));
    }
    if parts.is_empty() {
        format!("{} review(s)", review_states.len())
    } else {
        parts.join(", ")
    }
}

/// Build deterministic, open-standard-aligned skill fields from PR signal:
/// a gerund title, a third-person what+when description carrying concrete
/// triggers, derived scope paths, and a progressive-disclosure body.
fn build_pr_skill_fields(source: &PrSkillSource) -> PrSkillFields {
    let scope_paths = pr_scope_paths(&source.changed_paths);
    let extensions = pr_file_extensions(&source.changed_paths);
    let area = pr_primary_area(&scope_paths);
    let review_summary = pr_review_summary(&source.review_states);

    let title = truncate_text(&format!("Reviewing {area} changes"), MAX_PR_SKILL_TITLE_CHARS);

    let mut compact_guidance = String::from("Reuse the reviewed approach from this change set.");
    if scope_paths.is_empty() {
        compact_guidance.push_str(" Use when reviewing similar changes");
    } else {
        compact_guidance.push_str(&format!(" Use when working on {}", scope_paths.join(", ")));
    }
    if !extensions.is_empty() {
        compact_guidance.push_str(&format!(" or editing {} files", extensions.join("/")));
    }
    if !source.labels.is_empty() {
        compact_guidance.push_str(&format!(" ({} work)", source.labels.join(", ")));
    }
    compact_guidance.push('.');

    let when = if scope_paths.is_empty() {
        "similar changes".to_string()
    } else {
        scope_paths.join(", ")
    };
    let extension_clause = if extensions.is_empty() {
        String::new()
    } else {
        format!(" ({})", extensions.join(", "))
    };
    let workflow = if source.commit_headlines.is_empty() {
        "1. Review the change set and mirror its approach for the same kind of work.".to_string()
    } else {
        source
            .commit_headlines
            .iter()
            .take(MAX_PR_WORKFLOW_COMMITS)
            .enumerate()
            .map(|(index, headline)| format!("{}. {headline}", index + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let url_clause = source
        .url
        .as_deref()
        .map(|url| format!(" ({url})"))
        .unwrap_or_default();
    let labels_line = if source.labels.is_empty() {
        String::new()
    } else {
        format!("\nLabels: {}.", source.labels.join(", "))
    };
    let body_excerpt_block = source
        .body_excerpt
        .as_deref()
        .filter(|excerpt| !excerpt.is_empty())
        .map(|excerpt| format!("\n\nPR summary:\n\n{excerpt}"))
        .unwrap_or_default();
    let body_markdown = format!(
        "## When to use\n\nWhen working on {when}{extension_clause}.\n\n## Workflow\n\n{workflow}\n\n## Verification\n\n- Re-run the project's lint and tests before approving.\n- Confirm the result matches the original review outcome ({review_summary}).\n\n<details>\n<summary>Provenance</summary>\n\nDistilled from GitHub PR #{number}{url_clause}. Changed {file_count} files (+{additions}/-{deletions}).{labels_line}{body_excerpt_block}\n\n</details>",
        number = source.number,
        file_count = source.changed_paths.len(),
        additions = source.additions,
        deletions = source.deletions,
    );

    let predicted_effect =
        format!("Fewer repeated mistakes when changing {area} ({review_summary}).");

    let evidence = serde_json::json!({
        "source": "gh_pr_view",
        "number": source.number,
        "url": source.url,
        "state": source.state,
        "base_ref_name": source.base_ref,
        "changed_paths": source.changed_paths.iter().take(50).collect::<Vec<_>>(),
        "changed_file_count": source.changed_paths.len(),
        "additions": source.additions,
        "deletions": source.deletions,
        "commit_headlines": source
            .commit_headlines
            .iter()
            .take(MAX_PR_WORKFLOW_COMMITS)
            .collect::<Vec<_>>(),
        "review_states": source.review_states,
        "labels": source.labels,
        "body_excerpt": source.body_excerpt,
        "full_diff_read": false,
        "redaction_applied": true,
        "enriched_from_pr_detail": source.enriched,
    });

    PrSkillFields {
        title,
        compact_guidance,
        body_markdown,
        predicted_effect,
        scope_paths,
        evidence,
        enriched: source.enriched,
    }
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

fn parse_git_log_summaries(output: &str) -> Vec<GitCommitSummary> {
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

async fn scan_project_skill_source_root(
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

fn contained_native_skill_file(
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

fn selected_project_skill_source_roots(requested: Vec<String>) -> Result<Vec<String>, HttpError> {
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

async fn sync_source_tracked_project_skills(
    state: &HttpServerState,
    project_id: &ProjectId,
    candidates: &[ProjectSkillImportCandidate],
) -> AppResult<usize> {
    let existing_skills = state
        .app_state
        .project_skill_repo
        .list_by_project(
            project_id,
            ProjectSkillListOptions {
                include_archived: true,
                ..Default::default()
            },
        )
        .await?;
    let service = ProjectSkillService::new(Arc::clone(&state.app_state.project_skill_repo));
    let mut synced_count = 0;
    for candidate in candidates {
        let Some(external_id) = candidate.external_id.as_deref() else {
            continue;
        };
        let Some(existing) = existing_skills
            .iter()
            .find(|skill| project_skill_external_id(skill).as_deref() == Some(external_id))
        else {
            continue;
        };
        if !project_skill_source_sync_enabled(existing) {
            continue;
        }
        if existing.archived
            || matches!(
                existing.status,
                ProjectSkillLifecycleStatus::Archived | ProjectSkillLifecycleStatus::Retired
            )
        {
            continue;
        }
        if service
            .update_skill_content(UpdateProjectSkillContentInput {
                project_skill_id: existing.id.clone(),
                title: candidate.title.clone(),
                bucket: candidate.bucket.clone(),
                stage: candidate.stage.clone(),
                scope_paths: candidate.scope_paths.clone(),
                compact_guidance: candidate.compact_guidance.clone(),
                body_markdown: candidate.body_markdown.clone(),
                predicted_effect: candidate.predicted_effect.clone(),
                source_sync_enabled: Some(true),
            })
            .await?
            .is_some()
        {
            synced_count += 1;
        }
    }
    Ok(synced_count)
}

fn project_skill_external_id(skill: &ProjectSkill) -> Option<String> {
    skill
        .provenance_json
        .get("external_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn project_skill_source_sync_enabled(skill: &ProjectSkill) -> bool {
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
struct ParsedSkillFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
}

/// Split a leading `---`-fenced YAML frontmatter block from the markdown body.
/// Returns the parsed frontmatter (when present and valid) and the body with the
/// frontmatter removed, so body scraping never reads YAML values.
fn split_skill_frontmatter(markdown: &str) -> (Option<ParsedSkillFrontmatter>, String) {
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
fn split_imported_skill_body(body: &str) -> (String, Option<String>) {
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

fn native_skill_title(markdown: &str) -> Option<String> {
    markdown
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn native_skill_compact_guidance(markdown: &str) -> Option<String> {
    markdown
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#') && !line.starts_with("---"))
        .find(|line| line.chars().count() >= 24)
        .map(|line| truncate_text(line, 220))
}

fn humanize_skill_dir(value: &str) -> String {
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

    fn source_import_candidate() -> ProjectSkillImportCandidate {
        ProjectSkillImportCandidate {
            external_id: Some(".claude/skills/review/SKILL.md".to_string()),
            title: "Updated source skill".to_string(),
            bucket: "execution".to_string(),
            stage: "execution".to_string(),
            scope_paths: Vec::new(),
            compact_guidance: "Use the updated source procedure.".to_string(),
            body_markdown: "## Updated\n\nFollow the updated source procedure.".to_string(),
            predicted_effect: "Keeps RalphX skill guidance aligned with source files.".to_string(),
            provenance_json: json!({
                "source": "target_project_skill_folder",
                "relative_path": ".claude/skills/review/SKILL.md",
                "source_sync_enabled": true
            }),
            source_snapshot_json: json!({
                "kind": "target_project_skill_folder",
                "relative_path": ".claude/skills/review/SKILL.md",
                "source_root": ".claude/skills",
                "source_sync_enabled": true
            }),
        }
    }

    #[test]
    fn split_skill_frontmatter_parses_fields_and_strips_body() {
        let markdown = "---\nname: foo-bar\ndescription: \"Does X. Use when Y.\"\npaths:\n  - \"src/a\"\n  - \"src/b\"\nmetadata:\n  generator: ralphx-learned-skill\n---\n\n# Title\n\nThis is a sufficiently long body line.\n";
        let (frontmatter, body) = split_skill_frontmatter(markdown);
        let frontmatter = frontmatter.expect("frontmatter parsed");
        assert_eq!(frontmatter.name.as_deref(), Some("foo-bar"));
        assert_eq!(frontmatter.description.as_deref(), Some("Does X. Use when Y."));
        assert_eq!(frontmatter.paths, vec!["src/a".to_string(), "src/b".to_string()]);
        assert!(body.starts_with("# Title"));
        // Frontmatter values must NOT leak into the scraped body: the scraped
        // guidance is the body line, never the frontmatter `description`.
        assert!(!body.contains("description:"));
        assert_eq!(
            native_skill_compact_guidance(&body).as_deref(),
            Some("This is a sufficiently long body line.")
        );
    }

    #[test]
    fn split_skill_frontmatter_returns_none_for_plain_markdown() {
        let markdown = "# Title\n\nNo frontmatter here.\n";
        let (frontmatter, body) = split_skill_frontmatter(markdown);
        assert!(frontmatter.is_none());
        assert_eq!(body, markdown);
    }

    #[tokio::test]
    async fn scan_project_skill_source_root_round_trips_description_and_paths() {
        let project_dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd"))
            .expect("temp project dir");
        let skill_dir = project_dir.path().join(".claude/skills/reviewing-merge");
        std::fs::create_dir_all(&skill_dir).unwrap();
        // Exactly the open-standard shape the exporter writes.
        let content = "---\nname: reviewing-merge-abc123\ndescription: \"Review merge-validation output before approving. Use when working on src-tauri/src.\"\npaths:\n  - \"src-tauri/src\"\n  - \"frontend/src\"\nmetadata:\n  generator: ralphx-learned-skill\n---\n\n# Reviewing Merge Validation Changes\n\n## When to use\n\nWhen touching merge validation code.\n\n## Predicted Effect\n\nFewer repeated validation failures.\n";
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();

        let candidates =
            scan_project_skill_source_root(project_dir.path(), ".claude/skills", false)
                .await
                .expect("scan succeeds");

        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        // Title from the body H1, guidance from the frontmatter description.
        assert_eq!(candidate.title, "Reviewing Merge Validation Changes");
        assert!(candidate
            .compact_guidance
            .starts_with("Review merge-validation output before approving"));
        // paths -> scope_paths round-trips.
        assert_eq!(
            candidate.scope_paths,
            vec!["src-tauri/src".to_string(), "frontend/src".to_string()]
        );
        // Frontmatter is stripped from the stored body.
        assert!(!candidate.body_markdown.contains("description:"));
        assert!(candidate.body_markdown.contains("## When to use"));
        // H1 + Predicted Effect are stripped (idempotent re-export); effect captured.
        assert!(!candidate
            .body_markdown
            .contains("# Reviewing Merge Validation Changes"));
        assert!(!candidate.body_markdown.contains("## Predicted Effect"));
        assert_eq!(
            candidate.predicted_effect,
            "Fewer repeated validation failures."
        );
    }

    #[test]
    fn redact_pr_text_masks_secrets() {
        let input = "Token ghp_abcdefghijklmnopqrstuvwxyz0123 and API_KEY=supersecretvalue123 plus normal text.";
        let output = redact_pr_text(input);
        assert!(!output.contains("ghp_abcdefghijklmnopqrstuvwxyz0123"));
        assert!(!output.contains("supersecretvalue123"));
        assert!(output.contains("[REDACTED]"));
        assert!(output.contains("normal text"));

        // Quoted multi-word secret values are masked in full (not just the first token).
        let quoted = redact_pr_text("password = \"hunter2 with spaces\" trailing");
        assert!(!quoted.contains("hunter2 with spaces"));
        assert!(quoted.contains("[REDACTED]"));
        assert!(quoted.contains("trailing"));
    }

    #[test]
    fn split_imported_skill_body_strips_h1_and_predicted_effect() {
        let body = "# Reviewing Merge Changes\n\n## When to use\n\nWhen touching merge code.\n\n## Predicted Effect\n\nFewer repeats.";
        let (procedure, effect) = split_imported_skill_body(body);
        assert!(procedure.starts_with("## When to use"));
        assert!(!procedure.contains("# Reviewing Merge Changes"));
        assert!(!procedure.contains("## Predicted Effect"));
        assert_eq!(effect.as_deref(), Some("Fewer repeats."));
    }

    #[test]
    fn build_pr_skill_fields_encodes_triggers_scope_and_workflow() {
        let source = PrSkillSource {
            number: 42,
            url: Some("https://github.com/x/y/pull/42".to_string()),
            state: Some("MERGED".to_string()),
            base_ref: Some("main".to_string()),
            changed_paths: vec![
                "src-tauri/src/domain/services/merge.rs".to_string(),
                "src-tauri/src/domain/services/validation.rs".to_string(),
                "frontend/src/components/Merge.tsx".to_string(),
            ],
            commit_headlines: vec![
                "Add merge guard".to_string(),
                "Cover guard with tests".to_string(),
            ],
            review_states: vec!["APPROVED".to_string(), "CHANGES_REQUESTED".to_string()],
            labels: vec!["backend".to_string()],
            body_excerpt: Some("Adds a guard.".to_string()),
            additions: 120,
            deletions: 8,
            enriched: true,
        };

        let fields = build_pr_skill_fields(&source);

        // Gerund title within the open-standard length budget.
        assert!(fields.title.chars().count() <= 64);
        assert!(fields.title.starts_with("Reviewing "));
        // Third-person what+when description carrying concrete triggers.
        assert!(fields.compact_guidance.contains("Use when working on"));
        assert!(fields.compact_guidance.contains("src-tauri/src/domain"));
        assert!(
            fields.compact_guidance.contains(".rs") || fields.compact_guidance.contains(".tsx")
        );
        // scope_paths derived from changed files (dir prefixes, deduped).
        assert!(fields
            .scope_paths
            .contains(&"src-tauri/src/domain".to_string()));
        assert!(fields
            .scope_paths
            .contains(&"frontend/src/components".to_string()));
        // Progressive-disclosure body with workflow from commit headlines.
        assert!(fields.body_markdown.contains("## When to use"));
        assert!(fields.body_markdown.contains("## Workflow"));
        assert!(fields.body_markdown.contains("## Verification"));
        assert!(fields.body_markdown.contains("1. Add merge guard"));
        assert!(fields.body_markdown.contains("PR #42"));
        assert!(fields.enriched);
    }

    #[test]
    fn build_pr_skill_fields_degrades_without_detail() {
        let summary = GithubPrSummary {
            number: 7,
            title: "Fix bug".to_string(),
            state: Some("MERGED".to_string()),
            url: None,
            merged_at: None,
            closed_at: None,
            updated_at: None,
            head_ref_name: None,
            base_ref_name: Some("main".to_string()),
        };
        let source = pr_skill_source_from_summary(&summary);
        let fields = build_pr_skill_fields(&source);

        assert!(!fields.enriched);
        assert!(fields.scope_paths.is_empty());
        assert!(fields
            .compact_guidance
            .contains("Use when reviewing similar changes"));
        assert!(fields.body_markdown.contains("Review the change set"));
    }

    #[test]
    fn pr_skill_source_from_detail_redacts_body_and_commits() {
        let detail = GithubPrDetail {
            number: 9,
            body: "Set API_KEY=supersecretvalue123 in env.".to_string(),
            state: Some("OPEN".to_string()),
            url: None,
            additions: 1,
            deletions: 0,
            base_ref_name: None,
            files: vec![GithubPrFile {
                path: "src/a.rs".to_string(),
            }],
            commits: vec![GithubPrCommit {
                message_headline: "Use ghp_abcdefghijklmnopqrstuvwxyz0123".to_string(),
            }],
            reviews: vec![GithubPrReview {
                state: "APPROVED".to_string(),
            }],
            labels: vec![GithubPrLabel {
                name: "feature".to_string(),
            }],
        };

        let source = pr_skill_source_from_detail(detail);

        let excerpt = source.body_excerpt.as_deref().expect("body excerpt");
        assert!(excerpt.contains("[REDACTED]"));
        assert!(!excerpt.contains("supersecretvalue123"));
        assert!(source.commit_headlines[0].contains("[REDACTED]"));
        assert!(!source.commit_headlines[0].contains("ghp_abcdefghijklmnopqrstuvwxyz0123"));
    }

    #[test]
    fn contained_native_skill_file_accepts_safe_folder_name() {
        let project_root = Path::new("/workspace/project");
        let skills_root = Path::new("/workspace/project/.claude/skills");

        let skill_file =
            contained_native_skill_file(project_root, skills_root, "review-flow").unwrap();

        assert_eq!(
            skill_file,
            PathBuf::from("/workspace/project/.claude/skills/review-flow/SKILL.md")
        );
    }

    #[test]
    fn contained_native_skill_file_rejects_unsafe_folder_name() {
        let project_root = Path::new("/workspace/project");
        let skills_root = Path::new("/workspace/project/.claude/skills");

        let error =
            contained_native_skill_file(project_root, skills_root, "../review-flow").unwrap_err();

        assert!(error
            .to_string()
            .contains("project skill folder name contains unsafe characters"));
    }

    #[test]
    fn contained_native_skill_file_rejects_source_root_escape() {
        let project_root = Path::new("/workspace/project");
        let skills_root = Path::new("/workspace/other/.claude/skills");

        let error =
            contained_native_skill_file(project_root, skills_root, "review-flow").unwrap_err();

        assert!(error
            .to_string()
            .contains("project skills directory escapes project root"));
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
    async fn update_project_skill_handler_updates_reviewable_fields() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-update".to_string());
        let mut skill = staged_skill(project_id.clone());
        skill.id = ProjectSkillId::from_string("skill-update".to_string());
        let skill_id = skill.id.clone();
        app_state.project_skill_repo.create(skill).await.unwrap();

        let response = update_project_skill(
            State(test_state(app_state)),
            ProjectScope(Some(vec![project_id])),
            Json(UpdateProjectSkillRequest {
                project_skill_id: skill_id.as_str().to_string(),
                title: "Check branch before skill export".to_string(),
                bucket: "execution".to_string(),
                stage: "execution".to_string(),
                scope_paths: vec!["src-tauri".to_string()],
                compact_guidance: "Check the current branch before exporting skills.".to_string(),
                body_markdown: "Detailed updated procedure.".to_string(),
                predicted_effect: "Prevents exporting from protected branches.".to_string(),
                source_sync_enabled: Some(false),
            }),
        )
        .await
        .unwrap()
        .0
        .skill
        .expect("updated skill");

        assert_eq!(response.title, "Check branch before skill export");
        assert_eq!(response.bucket, "execution");
        assert_eq!(response.scope_paths, vec!["src-tauri".to_string()]);
        assert_eq!(
            response.predicted_effect.as_deref(),
            Some("Prevents exporting from protected branches.")
        );
    }

    #[tokio::test]
    async fn source_tracked_project_skill_sync_updates_internal_copy() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-source-sync".to_string());
        let mut skill = staged_skill(project_id.clone());
        skill.id = ProjectSkillId::from_string("skill-source-sync".to_string());
        skill.title = "Old source title".to_string();
        skill.provenance_json = json!({
            "source": "project_skill_import",
            "external_id": ".claude/skills/review/SKILL.md",
            "source_sync_enabled": true,
            "source_snapshot": {
                "relative_path": ".claude/skills/review/SKILL.md",
                "source_sync_enabled": true
            }
        });
        let skill_id = skill.id.clone();
        app_state.project_skill_repo.create(skill).await.unwrap();
        let state = test_state(app_state.clone());

        let synced =
            sync_source_tracked_project_skills(&state, &project_id, &[source_import_candidate()])
                .await
                .unwrap();

        let updated = app_state
            .project_skill_repo
            .get_by_id(&skill_id)
            .await
            .unwrap()
            .expect("updated skill");
        assert_eq!(synced, 1);
        assert_eq!(updated.title, "Updated source skill");
        assert_eq!(
            updated.body_markdown,
            "## Updated\n\nFollow the updated source procedure."
        );
        assert!(project_skill_source_sync_enabled(&updated));
    }

    #[tokio::test]
    async fn snapshot_project_skill_sync_does_not_update_internal_copy() {
        let app_state = Arc::new(AppState::new_test());
        let project_id = ProjectId::from_string("project-source-snapshot".to_string());
        let mut skill = staged_skill(project_id.clone());
        skill.id = ProjectSkillId::from_string("skill-source-snapshot".to_string());
        skill.title = "Old source title".to_string();
        skill.provenance_json = json!({
            "source": "project_skill_import",
            "external_id": ".claude/skills/review/SKILL.md",
            "source_sync_enabled": false,
            "source_snapshot": {
                "relative_path": ".claude/skills/review/SKILL.md",
                "source_sync_enabled": false
            }
        });
        let skill_id = skill.id.clone();
        app_state.project_skill_repo.create(skill).await.unwrap();
        let state = test_state(app_state.clone());

        let synced =
            sync_source_tracked_project_skills(&state, &project_id, &[source_import_candidate()])
                .await
                .unwrap();

        let unchanged = app_state
            .project_skill_repo
            .get_by_id(&skill_id)
            .await
            .unwrap()
            .expect("unchanged skill");
        assert_eq!(synced, 0);
        assert_eq!(unchanged.title, "Old source title");
        assert!(!project_skill_source_sync_enabled(&unchanged));
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
                include_git_history: Some(false),
                include_github_pr_history: Some(false),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.0.staged_skills.len(), 1);
        assert_eq!(response.0.staged_skills[0].status, "staged");
        assert_eq!(response.0.staged_skills[0].bucket, "review");
        assert_eq!(response.0.skipped_existing, 0);
        assert_eq!(response.0.ingested_outcomes, 0);
        assert_eq!(response.0.scanned_git_commits, 0);
        assert_eq!(response.0.scanned_github_prs, 0);
    }

    #[test]
    fn parse_git_log_summaries_parses_metadata_records() {
        let parsed = parse_git_log_summaries(
            "abc123\x1f2026-06-15T10:00:00Z\x1fAda\x1fAdd skill candidate fallback\x1edef456\x1f2026-06-14T10:00:00Z\x1fAda\x1fFix export branch gate\x1e",
        );

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].sha, "abc123");
        assert_eq!(parsed[0].subject, "Add skill candidate fallback");
        assert_eq!(parsed[1].author_name, "Ada");
    }

    #[test]
    fn parse_github_pr_summaries_parses_cli_json() {
        let parsed = parse_github_pr_summaries(
            r#"[{"number":42,"title":"Fix learned skill export","state":"MERGED","url":"https://github.com/aigentive/ralphx.app/pull/42","mergedAt":"2026-06-15T10:00:00Z","closedAt":null,"updatedAt":"2026-06-15T10:30:00Z","headRefName":"feature/skills","baseRefName":"main"},{"number":0,"title":"Ignored","state":"OPEN"}]"#,
        )
        .unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].number, 42);
        assert_eq!(parsed[0].state.as_deref(), Some("MERGED"));
        assert_eq!(parsed[0].head_ref_name.as_deref(), Some("feature/skills"));
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
                include_git_history: None,
                include_github_pr_history: None,
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
