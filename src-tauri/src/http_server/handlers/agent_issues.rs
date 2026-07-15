use axum::{extract::State, http::StatusCode, Json};
use sha2::{Digest, Sha256};

use crate::domain::entities::{
    canonicalize_agent_conversation_issue, AgentConversationIssue,
    AgentConversationIssueCanonicalIdentity, AgentConversationIssueCanonicalInput,
    AgentConversationIssueOccurrence, ChatContextType, ChatConversation, ChatConversationId,
    ProjectId, TaskId, AGENT_CONVERSATION_ISSUE_DEDUPE_CANDIDATE_ATTACHED,
    AGENT_CONVERSATION_ISSUE_DEDUPE_CONFIRMED_NEW, AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED,
    AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED, AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED,
    AGENT_CONVERSATION_ISSUE_STATUS_OPEN, AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED,
};
use crate::http_server::handlers::agent_followups::create_followup_agent_conversation_for_request;
use crate::http_server::helpers::get_task_context_impl;
use crate::http_server::types::{
    AgentConversationIssueResponse, ConvertAgentConversationIssueFollowupRequest,
    ConvertAgentConversationIssueFollowupResponse, CreateFollowupAgentConversationRequest,
    CreateFollowupAgentConversationResponse, HttpServerState, ListAgentConversationIssuesRequest,
    ListAgentConversationIssuesResponse, RegisterAgentConversationIssueRequest,
    RegisterAgentConversationIssueResponse, UpdateAgentConversationIssueStatusRequest,
    UpdateAgentConversationIssueStatusResponse,
};

type JsonError = (StatusCode, Json<serde_json::Value>);

fn json_error(status: StatusCode, message: impl Into<String>) -> JsonError {
    (status, Json(serde_json::json!({ "error": message.into() })))
}

fn json_error_body(status: StatusCode, body: serde_json::Value) -> JsonError {
    (status, Json(body))
}

fn trim_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_token(value: Option<&str>, default_value: &str) -> String {
    let Some(value) = trim_optional(value) else {
        return default_value.to_string();
    };
    let normalized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn request_origin_conversation_id(req: &RegisterAgentConversationIssueRequest) -> Option<String> {
    trim_optional(req.origin_conversation_id.as_deref()).or_else(|| {
        (req.source_context_type.as_deref() == Some("agent_conversation"))
            .then(|| trim_optional(req.source_context_id.as_deref()))
            .flatten()
    })
}

fn request_source_task_id(req: &RegisterAgentConversationIssueRequest) -> Option<String> {
    trim_optional(req.source_task_id.as_deref()).or_else(|| {
        matches!(
            req.source_context_type.as_deref(),
            Some("task_execution" | "review" | "merge" | "task")
        )
        .then(|| trim_optional(req.source_context_id.as_deref()))
        .flatten()
    })
}

fn canonical_identity_for_issue(
    issue: &AgentConversationIssue,
) -> AgentConversationIssueCanonicalIdentity {
    canonicalize_agent_conversation_issue(&AgentConversationIssueCanonicalInput {
        issue_kind: issue.issue_kind.as_str(),
        blocking_scope: issue.blocking_scope.as_str(),
        title: issue.title.as_str(),
        summary: issue.summary.as_str(),
        evidence: issue.evidence.as_deref(),
        recommendation: issue.recommendation.as_deref(),
        blocker_fingerprint: issue.blocker_fingerprint.as_deref(),
        source_task_id: issue.source_task_id.as_deref(),
    })
}

fn issue_check_token(
    conversation_id: &ChatConversationId,
    issues: &[AgentConversationIssue],
) -> String {
    let mut parts = issues
        .iter()
        .map(|issue| {
            format!(
                "{}:{}:{}",
                issue.id,
                issue.updated_at.to_rfc3339(),
                issue.status
            )
        })
        .collect::<Vec<_>>();
    parts.sort();
    let mut hasher = Sha256::new();
    hasher.update(conversation_id.as_str().as_bytes());
    hasher.update(b"|");
    hasher.update(parts.join("|").as_bytes());
    let digest = hasher.finalize();
    let suffix = digest
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("v1:{}:{suffix}", conversation_id.as_str())
}

async fn issue_response_with_occurrences(
    state: &HttpServerState,
    issue: AgentConversationIssue,
) -> Result<AgentConversationIssueResponse, JsonError> {
    let occurrences = state
        .app_state
        .agent_conversation_issue_repo
        .list_occurrences_by_issue(&issue.id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load Agent conversation issue occurrences: {error}"),
            )
        })?;
    Ok(AgentConversationIssueResponse::from(issue).with_occurrences(occurrences))
}

async fn issue_responses_with_occurrences(
    state: &HttpServerState,
    issues: Vec<AgentConversationIssue>,
) -> Result<Vec<AgentConversationIssueResponse>, JsonError> {
    let mut responses = Vec::with_capacity(issues.len());
    for issue in issues {
        responses.push(issue_response_with_occurrences(state, issue).await?);
    }
    Ok(responses)
}

async fn append_issue_occurrence(
    state: &HttpServerState,
    issue: &AgentConversationIssue,
    dedupe_decision: &str,
) -> Result<AgentConversationIssueOccurrence, JsonError> {
    let occurrence = AgentConversationIssueOccurrence::from_issue(issue, dedupe_decision);
    state
        .app_state
        .agent_conversation_issue_repo
        .append_occurrence(&occurrence)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to save Agent conversation issue occurrence: {error}"),
            )
        })
}

async fn resolve_origin_conversation(
    state: &HttpServerState,
    req: &RegisterAgentConversationIssueRequest,
) -> Result<ChatConversation, JsonError> {
    let source_task_id = request_source_task_id(req);
    let task_context = if let Some(source_task_id) = source_task_id.as_ref() {
        let task_id = TaskId::from_string(source_task_id.clone());
        Some(
            get_task_context_impl(&state.app_state, &task_id)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to resolve source task context: {error}"),
                    )
                })?,
        )
    } else {
        None
    };

    let conversation = if let Some(origin_id) = request_origin_conversation_id(req) {
        let origin_id = ChatConversationId::from_string(origin_id);
        state
            .app_state
            .chat_conversation_repo
            .get_by_id(&origin_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load origin Agent conversation: {error}"),
                )
            })?
            .ok_or_else(|| {
                json_error(
                    StatusCode::NOT_FOUND,
                    format!("Origin Agent conversation not found: {origin_id}"),
                )
            })?
    } else if let Some(task_context) = task_context.as_ref() {
        let Some(session_id) = task_context.task.ideation_session_id.as_ref() else {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "source task is not attached to an ideation session",
            ));
        };
        let workspace = state
            .app_state
            .agent_conversation_workspace_repo
            .get_by_linked_ideation_session_id(session_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to find Agent workspace linked to source task: {error}"),
                )
            })?
            .ok_or_else(|| {
                json_error(
                    StatusCode::BAD_REQUEST,
                    "source task belongs to an ideation session that is not attached to a visible Agent conversation",
                )
            })?;
        state
            .app_state
            .chat_conversation_repo
            .get_by_id(&workspace.conversation_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load linked Agent conversation: {error}"),
                )
            })?
            .ok_or_else(|| {
                json_error(
                    StatusCode::NOT_FOUND,
                    "Linked Agent conversation for source task was not found",
                )
            })?
    } else {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "register_agent_issue requires origin_conversation_id, agent_conversation source_context, or source_task_id",
        ));
    };

    if conversation.context_type != ChatContextType::Project {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Origin conversation must be a project Agent conversation",
        ));
    }
    if let Some(task_context) = task_context.as_ref() {
        if task_context.task.project_id.as_str() != conversation.context_id {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "source task belongs to a different project than the origin Agent conversation",
            ));
        }
    }
    Ok(conversation)
}

fn followup_request_from_issue(
    issue: &AgentConversationIssue,
    provider_harness: Option<String>,
    model_override: Option<String>,
    logical_effort: Option<crate::domain::agents::LogicalEffort>,
    override_title: Option<String>,
    override_prompt: Option<String>,
) -> CreateFollowupAgentConversationRequest {
    let title = override_title
        .or_else(|| issue.followup_title.clone())
        .unwrap_or_else(|| issue.title.clone());
    let issue_prompt = issue
        .followup_prompt
        .clone()
        .or_else(|| issue.recommendation.clone())
        .unwrap_or_else(|| issue.summary.clone());
    let initial_prompt = override_prompt.unwrap_or_else(|| {
        [
            issue.summary.clone(),
            issue
                .evidence
                .as_ref()
                .map(|value| format!("\nEvidence:\n{value}"))
                .unwrap_or_default(),
            format!("\nRecommended follow-up:\n{issue_prompt}"),
        ]
        .join("")
    });
    CreateFollowupAgentConversationRequest {
        origin_conversation_id: Some(issue.conversation_id.as_str()),
        source_task_id: issue.source_task_id.clone(),
        source_context_type: issue.source_context_type.clone(),
        source_context_id: issue.source_context_id.clone(),
        source_agent_name: issue.source_agent_name.clone(),
        title,
        description: issue
            .recommendation
            .clone()
            .or_else(|| Some(issue.summary.clone())),
        initial_prompt: Some(initial_prompt),
        spawn_reason: Some(issue.issue_kind.clone()),
        blocker_fingerprint: issue.blocker_fingerprint.clone(),
        provider_harness,
        model_override,
        logical_effort,
    }
}

async fn maybe_create_auto_followup(
    state: &HttpServerState,
    issue: &AgentConversationIssue,
    req: &RegisterAgentConversationIssueRequest,
) -> Result<Option<CreateFollowupAgentConversationResponse>, JsonError> {
    if !issue.auto_followup_eligible || issue.linked_followup_conversation_id.is_some() {
        return Ok(None);
    }
    let settings = state
        .app_state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load autonomy policy settings: {error}"),
            )
        })?;
    if !settings.auto_create_followup_agent_conversation {
        return Ok(None);
    }

    let followup_req = followup_request_from_issue(
        issue,
        req.provider_harness.clone(),
        req.model_override.clone(),
        req.logical_effort,
        None,
        None,
    );
    create_followup_agent_conversation_for_request(state, followup_req)
        .await
        .map(Some)
}

async fn save_issue_linked_followup(
    state: &HttpServerState,
    issue_id: &str,
    followup: &CreateFollowupAgentConversationResponse,
) -> Result<AgentConversationIssue, JsonError> {
    let followup_conversation_id =
        ChatConversationId::from_string(followup.conversation.id.clone());
    state
        .app_state
        .agent_conversation_issue_repo
        .link_followup_conversation(issue_id, &followup_conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to link issue to follow-up Agent conversation: {error}"),
            )
        })?
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                "Agent conversation issue disappeared before follow-up link could be saved",
            )
        })
}

pub async fn register_agent_issue(
    State(state): State<HttpServerState>,
    Json(req): Json<RegisterAgentConversationIssueRequest>,
) -> Result<Json<RegisterAgentConversationIssueResponse>, JsonError> {
    if req.title.trim().is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "title is required"));
    }
    if req.summary.trim().is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "summary is required"));
    }
    if req.issue_kind.trim().is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "issue_kind is required",
        ));
    }

    let origin = resolve_origin_conversation(&state, &req).await?;
    let source_task_id = request_source_task_id(&req);
    let issue_kind = normalize_token(Some(req.issue_kind.as_str()), "plan_drift");
    let severity = normalize_token(req.severity.as_deref(), "medium");
    let blocking_scope = normalize_token(req.blocking_scope.as_deref(), "none");

    let mut issue = AgentConversationIssue::new(
        ProjectId::from_string(origin.context_id.clone()),
        origin.id.clone(),
        source_task_id.clone(),
        trim_optional(req.source_context_type.as_deref()),
        trim_optional(req.source_context_id.as_deref()),
        trim_optional(req.source_agent_name.as_deref()),
        issue_kind.clone(),
        severity,
        blocking_scope,
        req.title.trim().to_string(),
        req.summary.trim().to_string(),
        trim_optional(req.evidence.as_deref()),
        trim_optional(req.recommendation.as_deref()),
        trim_optional(req.blocker_fingerprint.as_deref()),
        trim_optional(req.followup_title.as_deref()),
        trim_optional(req.followup_prompt.as_deref()),
        req.auto_followup_eligible,
    );
    let canonical_identity = canonical_identity_for_issue(&issue);
    issue.apply_canonical_identity(&canonical_identity);

    let mut dedupe_result = AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED.to_string();
    let mut candidate_issues = Vec::new();
    let mut issue_check_token_response = None;

    if let Some(attach_to_issue_id) = trim_optional(req.attach_to_issue_id.as_deref()) {
        let mut existing = state
            .app_state
            .agent_conversation_issue_repo
            .get_by_id(&attach_to_issue_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load existing Agent conversation issue: {error}"),
                )
            })?
            .ok_or_else(|| {
                json_error(
                    StatusCode::NOT_FOUND,
                    "attach_to_issue_id did not match an existing Agent conversation issue",
                )
            })?;
        if existing.conversation_id != origin.id
            || existing.project_id.as_str() != origin.context_id
            || existing.status != AGENT_CONVERSATION_ISSUE_STATUS_OPEN
        {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "attach_to_issue_id must be an open issue on the same origin Agent conversation",
            ));
        }
        existing.refresh_from(issue);
        issue = existing;
        dedupe_result = AGENT_CONVERSATION_ISSUE_DEDUPE_CANDIDATE_ATTACHED.to_string();
    } else if let Some(mut existing) = state
        .app_state
        .agent_conversation_issue_repo
        .find_open_by_canonical_fingerprint(&origin.id, &canonical_identity.fingerprint)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to inspect existing Agent conversation issues: {error}"),
            )
        })?
    {
        existing.refresh_from(issue);
        issue = existing;
        dedupe_result = AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED.to_string();
    } else if let Some(blocker_fingerprint) = issue.blocker_fingerprint.as_deref() {
        if let Some(mut existing) = state
            .app_state
            .agent_conversation_issue_repo
            .find_open_by_fingerprint(
                &origin.id,
                source_task_id.as_deref(),
                &issue_kind,
                blocker_fingerprint,
            )
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to inspect existing Agent conversation issues: {error}"),
                )
            })?
        {
            existing.apply_canonical_identity(&canonical_identity);
            existing.refresh_from(issue);
            issue = existing;
            dedupe_result = AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED.to_string();
        } else if canonical_identity.candidate_match_eligible {
            candidate_issues = state
                .app_state
                .agent_conversation_issue_repo
                .list_open_candidates_by_identity(
                    &origin.id,
                    &canonical_identity.scope_kind,
                    &canonical_identity.scope_subject,
                    &canonical_identity.family,
                    &canonical_identity.fingerprint,
                    5,
                )
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to inspect candidate Agent conversation issues: {error}"),
                    )
                })?;
        }
    } else if canonical_identity.candidate_match_eligible {
        candidate_issues = state
            .app_state
            .agent_conversation_issue_repo
            .list_open_candidates_by_identity(
                &origin.id,
                &canonical_identity.scope_kind,
                &canonical_identity.scope_subject,
                &canonical_identity.family,
                &canonical_identity.fingerprint,
                5,
            )
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to inspect candidate Agent conversation issues: {error}"),
                )
            })?;
    }

    if dedupe_result == AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED && !candidate_issues.is_empty() {
        let expected_token = issue_check_token(&origin.id, &candidate_issues);
        if !req.confirm_new {
            let candidate_responses =
                issue_responses_with_occurrences(&state, candidate_issues).await?;
            return Err(json_error_body(
                StatusCode::CONFLICT,
                serde_json::json!({
                    "error": "needs_issue_disambiguation",
                    "message": "A similar open Agent conversation issue already exists. Retry with attach_to_issue_id, or confirm_new plus new_issue_reason and issue_check_token.",
                    "dedupe_result": "candidate_required",
                    "canonical_fingerprint": canonical_identity.fingerprint,
                    "candidate_issues": candidate_responses,
                    "issue_check_token": expected_token,
                }),
            ));
        }
        let Some(new_issue_reason) = trim_optional(req.new_issue_reason.as_deref()) else {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "new_issue_reason is required when confirm_new is true and candidates exist",
            ));
        };
        if trim_optional(req.issue_check_token.as_deref()).as_deref()
            != Some(expected_token.as_str())
        {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "issue_check_token must match the current candidate issue set when confirming a new issue",
            ));
        }
        issue.recommendation = issue
            .recommendation
            .take()
            .map(|value| format!("{value}\n\nConfirmed separate issue: {new_issue_reason}"))
            .or_else(|| Some(format!("Confirmed separate issue: {new_issue_reason}")));
        dedupe_result = AGENT_CONVERSATION_ISSUE_DEDUPE_CONFIRMED_NEW.to_string();
        issue_check_token_response = Some(expected_token);
    } else if req.confirm_new {
        let Some(new_issue_reason) = trim_optional(req.new_issue_reason.as_deref()) else {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "new_issue_reason is required when confirm_new is true",
            ));
        };
        issue.recommendation = issue
            .recommendation
            .take()
            .map(|value| format!("{value}\n\nConfirmed separate issue: {new_issue_reason}"))
            .or_else(|| Some(format!("Confirmed separate issue: {new_issue_reason}")));
        dedupe_result = AGENT_CONVERSATION_ISSUE_DEDUPE_CONFIRMED_NEW.to_string();
    }

    let mut saved_issue = state
        .app_state
        .agent_conversation_issue_repo
        .save(&issue)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to save Agent conversation issue: {error}"),
            )
        })?;
    let occurrence = append_issue_occurrence(&state, &saved_issue, &dedupe_result).await?;
    let followup = maybe_create_auto_followup(&state, &saved_issue, &req).await?;
    if let Some(followup) = followup.as_ref() {
        saved_issue = save_issue_linked_followup(&state, &saved_issue.id, followup).await?;
    }
    let auto_followup_created = followup
        .as_ref()
        .map(|response| !response.reused_existing)
        .unwrap_or(false);
    let issue_response = issue_response_with_occurrences(&state, saved_issue).await?;
    let occurrence_count = issue_response.occurrence_count;

    Ok(Json(RegisterAgentConversationIssueResponse {
        issue: issue_response,
        auto_followup_created,
        followup,
        dedupe_result,
        canonical_fingerprint: Some(canonical_identity.fingerprint),
        occurrence_id: Some(occurrence.id),
        occurrence_count,
        candidate_issues: Vec::new(),
        issue_check_token: issue_check_token_response,
    }))
}

pub async fn list_agent_conversation_issues(
    State(state): State<HttpServerState>,
    Json(req): Json<ListAgentConversationIssuesRequest>,
) -> Result<Json<ListAgentConversationIssuesResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(req.conversation_id);
    let raw_issues = state
        .app_state
        .agent_conversation_issue_repo
        .list_by_conversation(&conversation_id, req.include_resolved)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to list Agent conversation issues: {error}"),
            )
        })?;
    let issue_check_token = issue_check_token(&conversation_id, &raw_issues);
    let issues = issue_responses_with_occurrences(&state, raw_issues).await?;
    Ok(Json(ListAgentConversationIssuesResponse {
        issues,
        issue_check_token,
    }))
}

fn validate_status(status: &str) -> Result<&'static str, JsonError> {
    match status {
        AGENT_CONVERSATION_ISSUE_STATUS_OPEN => Ok(AGENT_CONVERSATION_ISSUE_STATUS_OPEN),
        AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED => Ok(AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED),
        AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED => Ok(AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED),
        _ => Err(json_error(
            StatusCode::BAD_REQUEST,
            "status must be one of open, resolved, or dismissed",
        )),
    }
}

pub async fn update_agent_conversation_issue_status(
    State(state): State<HttpServerState>,
    Json(req): Json<UpdateAgentConversationIssueStatusRequest>,
) -> Result<Json<UpdateAgentConversationIssueStatusResponse>, JsonError> {
    let status = validate_status(req.status.trim())?;
    let issue = state
        .app_state
        .agent_conversation_issue_repo
        .update_status(&req.issue_id, status)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update Agent conversation issue status: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent conversation issue not found"))?;

    Ok(Json(UpdateAgentConversationIssueStatusResponse {
        issue: issue.into(),
    }))
}

pub async fn convert_agent_conversation_issue_followup(
    State(state): State<HttpServerState>,
    Json(req): Json<ConvertAgentConversationIssueFollowupRequest>,
) -> Result<Json<ConvertAgentConversationIssueFollowupResponse>, JsonError> {
    let issue = state
        .app_state
        .agent_conversation_issue_repo
        .get_by_id(&req.issue_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load Agent conversation issue: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Agent conversation issue not found"))?;
    let followup_req = followup_request_from_issue(
        &issue,
        req.provider_harness,
        req.model_override,
        req.logical_effort,
        trim_optional(req.title.as_deref()),
        trim_optional(req.initial_prompt.as_deref()),
    );
    let followup = create_followup_agent_conversation_for_request(&state, followup_req).await?;
    let linked_issue = save_issue_linked_followup(&state, &issue.id, &followup).await?;

    Ok(Json(ConvertAgentConversationIssueFollowupResponse {
        issue: linked_issue.into(),
        followup,
    }))
}

#[cfg(test)]
#[path = "agent_issues_tests.rs"]
mod agent_issues_tests;
