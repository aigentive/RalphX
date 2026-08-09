use axum::{extract::State, http::StatusCode, Json};

use crate::application::agent_conversation_workspace::AgentConversationWorkspaceBaseSelection;
use crate::commands::unified_chat_commands::{
    start_agent_conversation_for_state, AgentWorkspaceSourcePullRequestInput,
    StartAgentConversationInput,
};
use crate::domain::entities::{
    canonicalize_agent_conversation_issue, AgentConversationIssueCanonicalInput,
    AgentConversationWorkspace, AgentWorkspaceFollowupProvenance, AgentWorkspaceSourcePullRequest,
    ChatContextType, ChatConversation, ChatConversationId, TaskId,
};
use crate::http_server::helpers::get_task_context_impl;
use crate::http_server::types::{
    CreateFollowupAgentConversationRequest, CreateFollowupAgentConversationResponse,
    HttpServerState,
};

type JsonError = (StatusCode, Json<serde_json::Value>);

fn json_error(status: StatusCode, message: impl Into<String>) -> JsonError {
    (status, Json(serde_json::json!({ "error": message.into() })))
}

fn trim_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn request_origin_conversation_id(req: &CreateFollowupAgentConversationRequest) -> Option<String> {
    trim_optional(req.origin_conversation_id.as_deref()).or_else(|| {
        (req.source_context_type.as_deref() == Some("agent_conversation"))
            .then(|| trim_optional(req.source_context_id.as_deref()))
            .flatten()
    })
}

fn source_pull_request_input(
    source: Option<AgentWorkspaceSourcePullRequest>,
) -> Option<AgentWorkspaceSourcePullRequestInput> {
    source.map(|pull_request| AgentWorkspaceSourcePullRequestInput {
        number: pull_request.number,
        url: pull_request.url,
        title: pull_request.title,
        head_ref_name: pull_request.head_ref_name,
        base_ref_name: pull_request.base_ref_name,
        head_ref_oid: pull_request.head_ref_oid,
    })
}

fn followup_base_selection(
    workspace: Option<&AgentConversationWorkspace>,
) -> AgentConversationWorkspaceBaseSelection {
    workspace
        .map(AgentConversationWorkspaceBaseSelection::for_workspace_reuse)
        .unwrap_or_default()
}

async fn resolve_origin_conversation(
    state: &HttpServerState,
    req: &CreateFollowupAgentConversationRequest,
) -> Result<
    (
        ChatConversation,
        Option<crate::domain::entities::TaskContext>,
    ),
    JsonError,
> {
    let source_task_id = trim_optional(req.source_task_id.as_deref());
    let task_context = if let Some(source_task_id) = source_task_id.as_ref() {
        let task_id = TaskId::from_string(source_task_id.clone());
        Some(
            get_task_context_impl(&state.app_state, &task_id)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to resolve source task context: {}", error),
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
                    format!("Failed to load origin Agent conversation: {}", error),
                )
            })?
            .ok_or_else(|| {
                json_error(
                    StatusCode::NOT_FOUND,
                    format!("Origin Agent conversation not found: {}", origin_id),
                )
            })?
    } else if let Some(task_context) = task_context.as_ref() {
        let Some(session_id) = task_context.task.ideation_session_id.as_ref() else {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "source_task_id is not attached to an ideation session",
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
                    format!("Failed to find Agent workspace linked to source task: {}", error),
                )
            })?
            .ok_or_else(|| {
                json_error(
                    StatusCode::BAD_REQUEST,
                    "source_task_id belongs to an ideation session that is not attached to a visible Agent conversation",
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
                    format!("Failed to load linked Agent conversation: {}", error),
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
            "create_followup_agent_conversation requires origin_conversation_id or source_task_id",
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
                "source_task_id belongs to a different project than the origin Agent conversation",
            ));
        }
    }

    Ok((conversation, task_context))
}

fn resolved_blocker_fingerprint(
    req: &CreateFollowupAgentConversationRequest,
    task_context: Option<&crate::domain::entities::TaskContext>,
) -> Option<String> {
    trim_optional(req.blocker_fingerprint.as_deref()).or_else(|| {
        match req.spawn_reason.as_deref() {
            Some("out_of_scope_failure") => {
                task_context.and_then(|context| context.out_of_scope_blocker_fingerprint.clone())
            }
            Some("execution_blocked") => Some(
                canonicalize_agent_conversation_issue(&AgentConversationIssueCanonicalInput {
                    issue_kind: "execution_blocked",
                    blocking_scope: "project",
                    title: req.title.as_str(),
                    summary: req.description.as_deref().unwrap_or(req.title.as_str()),
                    evidence: req.initial_prompt.as_deref(),
                    recommendation: None,
                    blocker_fingerprint: None,
                    source_task_id: req.source_task_id.as_deref(),
                })
                .fingerprint,
            ),
            _ => None,
        }
    })
}

fn followup_provenance(
    req: &CreateFollowupAgentConversationRequest,
    origin: &ChatConversation,
    blocker_fingerprint: Option<String>,
) -> AgentWorkspaceFollowupProvenance {
    AgentWorkspaceFollowupProvenance {
        origin_conversation_id: origin.id.clone(),
        source_task_id: trim_optional(req.source_task_id.as_deref()),
        source_context_type: trim_optional(req.source_context_type.as_deref()),
        source_context_id: trim_optional(req.source_context_id.as_deref()),
        source_agent_name: trim_optional(req.source_agent_name.as_deref()),
        spawn_reason: trim_optional(req.spawn_reason.as_deref()),
        blocker_fingerprint,
    }
}

async fn existing_followup_response(
    state: &HttpServerState,
    origin: &ChatConversation,
    provenance: &AgentWorkspaceFollowupProvenance,
) -> Result<Option<CreateFollowupAgentConversationResponse>, JsonError> {
    let (Some(source_task_id), Some(blocker_fingerprint)) = (
        provenance.source_task_id.as_deref(),
        provenance.blocker_fingerprint.as_deref(),
    ) else {
        return Ok(None);
    };

    let Some(workspace) = state
        .app_state
        .agent_conversation_workspace_repo
        .find_active_followup_by_blocker(&origin.id, source_task_id, blocker_fingerprint)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to inspect existing follow-up Agent conversations: {error}"),
            )
        })?
    else {
        return Ok(None);
    };

    let Some(conversation) = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&workspace.conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load existing follow-up Agent conversation: {error}"),
            )
        })?
    else {
        return Ok(None);
    };

    Ok(Some(CreateFollowupAgentConversationResponse {
        reused_existing: true,
        origin_conversation_id: origin.id.as_str(),
        source_task_id: provenance.source_task_id.clone(),
        source_context_type: provenance.source_context_type.clone(),
        source_context_id: provenance.source_context_id.clone(),
        source_agent_name: provenance.source_agent_name.clone(),
        spawn_reason: provenance.spawn_reason.clone(),
        blocker_fingerprint: provenance.blocker_fingerprint.clone(),
        conversation: conversation.into(),
        workspace: Some(workspace.into()),
        send_result: None,
    }))
}

fn followup_prompt(
    req: &CreateFollowupAgentConversationRequest,
    origin: &ChatConversation,
    blocker_fingerprint: Option<&str>,
) -> String {
    let requested_work = trim_optional(req.initial_prompt.as_deref())
        .or_else(|| trim_optional(req.description.as_deref()))
        .unwrap_or_else(|| req.title.trim().to_string());
    let mut lines = vec![
        "Create a visible follow-up Agent conversation in Ideation mode.".to_string(),
        "This is a branch from an existing Agent conversation, not a hidden child ideation session.".to_string(),
        format!("Origin Agent conversation: {}", origin.id),
    ];
    if let Some(source_agent_name) = trim_optional(req.source_agent_name.as_deref()) {
        lines.push(format!("Source agent: {}", source_agent_name));
    }
    if let Some(source_task_id) = trim_optional(req.source_task_id.as_deref()) {
        lines.push(format!("Source task: {}", source_task_id));
    }
    if let Some(source_context_type) = trim_optional(req.source_context_type.as_deref()) {
        lines.push(format!("Source context type: {}", source_context_type));
    }
    if let Some(source_context_id) = trim_optional(req.source_context_id.as_deref()) {
        lines.push(format!("Source context ID: {}", source_context_id));
    }
    if let Some(spawn_reason) = trim_optional(req.spawn_reason.as_deref()) {
        lines.push(format!("Reason: {}", spawn_reason));
    }
    if let Some(blocker_fingerprint) = blocker_fingerprint {
        lines.push(format!("Blocker fingerprint: {}", blocker_fingerprint));
    }
    lines.push(String::new());
    lines.push("Follow-up request:".to_string());
    lines.push(requested_work);
    lines.push(String::new());
    lines.push(
        "Use the Agent conversation ideation flow for this branch: establish a plan, proposals, and execution independently of the origin conversation."
            .to_string(),
    );
    lines.join("\n")
}

pub(crate) async fn create_followup_agent_conversation_for_request(
    state: &HttpServerState,
    req: CreateFollowupAgentConversationRequest,
) -> Result<CreateFollowupAgentConversationResponse, JsonError> {
    if req.title.trim().is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "title is required"));
    }

    let (origin, task_context) = resolve_origin_conversation(state, &req).await?;
    let blocker_fingerprint = resolved_blocker_fingerprint(&req, task_context.as_ref());
    let provenance = followup_provenance(&req, &origin, blocker_fingerprint.clone());
    if let Some(response) = existing_followup_response(state, &origin, &provenance).await? {
        return Ok(response);
    }

    let parent_workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&origin.id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load origin Agent workspace: {}", error),
            )
        })?;

    let content = followup_prompt(&req, &origin, blocker_fingerprint.as_deref());
    let base_selection = followup_base_selection(parent_workspace.as_ref());
    let response = start_agent_conversation_for_state(
        StartAgentConversationInput {
            project_id: Some(origin.context_id.clone()),
            content,
            persona_id: None,
            source_persona_id: None,
            conversation_id: None,
            parent_conversation_id: Some(origin.id.as_str()),
            title: Some(req.title.clone()),
            provider_harness: req.provider_harness.clone(),
            model_override: req.model_override.clone(),
            logical_effort: req.logical_effort,
            codex_fast_mode: None,
            mode: Some("ideation".to_string()),
            base_ref_kind: base_selection.kind.map(|kind| kind.to_string()),
            base_branch_mode: base_selection
                .branch_mode
                .map(|branch_mode| branch_mode.to_string()),
            base_ref: base_selection.base_ref,
            base_display_name: base_selection.display_name,
            base_source_pull_request: source_pull_request_input(base_selection.source_pull_request),
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references: Vec::new(),
            composer_selection_snapshot: None,
            team_intent: None,
        },
        &state.app_state,
        &state.execution_state,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;

    let created_conversation_id = ChatConversationId::from_string(response.conversation.id.clone());
    state
        .app_state
        .agent_conversation_workspace_repo
        .save_followup_provenance(&created_conversation_id, provenance.clone())
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to save follow-up Agent conversation provenance: {error}"),
            )
        })?;
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&created_conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to reload follow-up Agent workspace: {error}"),
            )
        })?
        .map(Into::into);

    Ok(CreateFollowupAgentConversationResponse {
        reused_existing: false,
        origin_conversation_id: origin.id.as_str(),
        source_task_id: provenance.source_task_id,
        source_context_type: provenance.source_context_type,
        source_context_id: provenance.source_context_id,
        source_agent_name: provenance.source_agent_name,
        spawn_reason: provenance.spawn_reason,
        blocker_fingerprint: provenance.blocker_fingerprint,
        conversation: response.conversation,
        workspace,
        send_result: Some(response.send_result),
    })
}

pub async fn create_followup_agent_conversation(
    State(state): State<HttpServerState>,
    Json(req): Json<CreateFollowupAgentConversationRequest>,
) -> Result<Json<CreateFollowupAgentConversationResponse>, JsonError> {
    create_followup_agent_conversation_for_request(&state, req)
        .await
        .map(Json)
}

#[cfg(test)]
#[path = "agent_followups_tests.rs"]
mod agent_followups_tests;
