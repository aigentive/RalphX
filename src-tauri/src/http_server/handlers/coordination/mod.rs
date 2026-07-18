use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path_for_send;
use crate::application::agent_lane_resolution::{
    resolve_manual_role_spawn_settings, routing_role_for_delegated_launch,
};
use crate::application::chat_service::{
    chat_service_context, events, resolve_working_directory, AgentTaskCompletedPayload,
    AgentTaskStartedPayload, CachedStreamingTask, ChatService, SendMessageOptions,
};
use crate::application::harness_runtime_registry::resolve_harness_plugin_dir;
use crate::application::ideation_workspace::resolve_ideation_workspace_path;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentRun, ChatContextType, ChatConversation, ChatConversationId, ChatMessage,
    DelegatedSession, DelegatedSessionId, IdeationSessionId, Project, ProjectId, SessionPurpose,
    TaskId,
};
use crate::http_server::delegation::DelegationJobSnapshot;
use crate::http_server::types::{
    AgentStateInfo, ChatMessageSummary, DelegateCancelRequest, DelegateStartRequest,
    DelegateWaitRequest, DelegatedRunSummary, DelegatedSessionStatusResponse,
    DelegatedSessionSummary, HttpServerState,
};
use crate::infrastructure::agents::harness_agent_catalog::{
    load_canonical_agent_definition, load_canonical_agent_definition_for_profile,
    resolve_project_root_from_plugin_dir,
};
use crate::utils::path_safety::validate_absolute_non_root_path;
use tracing::warn;

type JsonError = (StatusCode, Json<serde_json::Value>);

fn json_error(status: StatusCode, error: impl Into<String>) -> JsonError {
    (
        status,
        Json(serde_json::json!({
            "status": status.as_u16(),
            "error": error.into(),
        })),
    )
}

fn resolve_delegation_policy(
    project_root: &std::path::Path,
    caller_agent_name: &str,
    caller_agent_profile: Option<&str>,
    target_agent_name: &str,
) -> Result<
    (
        crate::infrastructure::agents::harness_agent_catalog::CanonicalAgentDefinition,
        crate::infrastructure::agents::harness_agent_catalog::CanonicalAgentDefinition,
    ),
    JsonError,
> {
    let caller = load_canonical_agent_definition_for_profile(
        project_root,
        caller_agent_name,
        caller_agent_profile,
    )
    .ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "Unknown canonical caller agent '{}'{}",
                caller_agent_name,
                caller_agent_profile
                    .map(|profile| format!(" profile '{profile}'"))
                    .unwrap_or_default()
            ),
        )
    })?;
    let target =
        load_canonical_agent_definition(project_root, target_agent_name).ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                format!("Unknown canonical agent '{}'", target_agent_name),
            )
        })?;

    if !caller.delegation.is_enabled() {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            format!("Agent '{}' is not allowed to delegate", caller.name),
        ));
    }

    if !caller
        .delegation
        .allowed_targets
        .iter()
        .any(|candidate| candidate == &target.name)
    {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            format!(
                "Agent '{}' may not delegate to '{}'",
                caller.name, target.name
            ),
        ));
    }

    Ok((caller, target))
}

struct ResolvedDelegateParent {
    context_type: ChatContextType,
    context_id: String,
    project_id: String,
    working_directory: PathBuf,
    parent_conversation_id: Option<String>,
    inherited_harness: Option<AgentHarnessKind>,
    ideation_verification: bool,
}

async fn resolve_delegated_session_id(
    state: &HttpServerState,
    req: &DelegateStartRequest,
    parent: &ResolvedDelegateParent,
    harness: AgentHarnessKind,
) -> Result<String, JsonError> {
    let requested_id = req
        .delegated_session_id
        .as_ref()
        .or(req.child_session_id.as_ref());

    if let Some(delegated_session_id) = requested_id {
        let delegated_id = DelegatedSessionId::from_string(delegated_session_id.clone());
        let delegated = state
            .app_state
            .delegated_session_repo
            .get_by_id(&delegated_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load delegated session: {error}"),
                )
            })?
            .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegated session not found"))?;
        if delegated.parent_context_type != parent.context_type.to_string()
            || delegated.parent_context_id != parent.context_id
        {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "Delegated session does not belong to the provided parent context",
            ));
        }
        return Ok(delegated_session_id.clone());
    }

    let mut session = DelegatedSession::new(
        ProjectId::from_string(parent.project_id.clone()),
        parent.context_type.to_string(),
        parent.context_id.clone(),
        req.agent_name.clone(),
        harness,
    );
    session.status = "pending".to_string();
    session.parent_turn_id = req.parent_turn_id.clone();
    session.parent_message_id = req.parent_message_id.clone();
    session.title = req.title.clone();
    let created = state
        .app_state
        .delegated_session_repo
        .create(session)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create delegated session: {error}"),
            )
        })?;
    Ok(created.id.as_str().to_string())
}

async fn load_project_by_id(
    state: &HttpServerState,
    project_id: &ProjectId,
) -> Result<Project, JsonError> {
    state
        .app_state
        .project_repo
        .get_by_id(project_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load parent project: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Parent project not found"))
}

fn validate_project_working_directory(project: &Project) -> Result<PathBuf, JsonError> {
    validate_absolute_non_root_path(
        &PathBuf::from(&project.working_directory),
        "project working directory",
    )
    .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string()))
}

async fn load_ideation_parent_project_working_directory(
    state: &HttpServerState,
    parent_session_id: &str,
) -> Result<(ProjectId, PathBuf), JsonError> {
    let parent_id = IdeationSessionId::from_string(parent_session_id.to_string());
    let parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load parent session: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Parent session not found"))?;

    let project = load_project_by_id(state, &parent.project_id).await?;

    let working_directory = resolve_ideation_workspace_path(&parent, &project)
        .map_err(|error| json_error(StatusCode::CONFLICT, error))?;

    Ok((parent.project_id, working_directory))
}

async fn resolve_ideation_delegate_parent(
    state: &HttpServerState,
    req: &DelegateStartRequest,
) -> Result<ResolvedDelegateParent, JsonError> {
    if req.caller_context_type.as_deref() == Some("ideation") {
        let caller_context_id = req.caller_context_id.as_ref().ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "delegate_start ideation callers require caller_context_id from the MCP transport",
            )
        })?;
        let caller_session_id = IdeationSessionId::from_string(caller_context_id.clone());
        let caller_session = state
            .app_state
            .ideation_session_repo
            .get_by_id(&caller_session_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load caller ideation session: {error}"),
                )
            })?
            .ok_or_else(|| {
                json_error(
                    StatusCode::NOT_FOUND,
                    "Caller ideation session not found for delegate_start",
                )
            })?;

        let derived_parent_session_id =
            if caller_session.session_purpose == SessionPurpose::Verification {
                caller_session
                    .parent_session_id
                    .as_ref()
                    .map(|id| id.as_str().to_string())
                    .ok_or_else(|| {
                        json_error(
                        StatusCode::BAD_REQUEST,
                        "Verification child session has no parent_session_id for delegate_start",
                    )
                    })?
            } else {
                caller_session.id.as_str().to_string()
            };

        if let Some(explicit_parent_session_id) = req.parent_session_id.as_ref() {
            if explicit_parent_session_id != &derived_parent_session_id {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    format!(
                        "delegate_start parent_session_id '{}' does not match caller context parent '{}'",
                        explicit_parent_session_id, derived_parent_session_id
                    ),
                ));
            }
        }

        let parent_conversation_id =
            resolve_parent_conversation_id(state, req, &derived_parent_session_id).await?;
        let (project_id, working_directory) =
            load_ideation_parent_project_working_directory(state, &derived_parent_session_id)
                .await?;

        return Ok(ResolvedDelegateParent {
            context_type: ChatContextType::Ideation,
            context_id: derived_parent_session_id,
            project_id: project_id.as_str().to_string(),
            working_directory,
            parent_conversation_id,
            inherited_harness: None,
            ideation_verification: caller_session.session_purpose == SessionPurpose::Verification,
        });
    }

    let parent_session_id = req.parent_session_id.as_deref().ok_or_else(|| {
        json_error(StatusCode::BAD_REQUEST, "delegate_start requires parent_session_id")
    })?;
    let parent_conversation_id =
        resolve_parent_conversation_id(state, req, parent_session_id).await?;
    let (project_id, working_directory) =
        load_ideation_parent_project_working_directory(state, parent_session_id).await?;
    Ok(ResolvedDelegateParent {
        context_type: ChatContextType::Ideation,
        context_id: parent_session_id.to_string(),
        project_id: project_id.as_str().to_string(),
        working_directory,
        parent_conversation_id,
        inherited_harness: None,
        ideation_verification: false,
    })
}

async fn load_parent_conversation(
    state: &HttpServerState,
    conversation_id: &str,
) -> Result<ChatConversation, JsonError> {
    state
        .app_state
        .chat_conversation_repo
        .get_by_id(&ChatConversationId::from_string(conversation_id.to_string()))
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load parent conversation: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Parent conversation not found"))
}

async fn load_project_parent_conversation(
    state: &HttpServerState,
    req: &DelegateStartRequest,
    caller_context_id: &str,
) -> Result<Option<ChatConversation>, JsonError> {
    let conversation = if let Some(parent_conversation_id) = req.parent_conversation_id.as_deref()
    {
        Some(load_parent_conversation(state, parent_conversation_id).await?)
    } else {
        let candidate_id = ChatConversationId::from_string(caller_context_id.to_string());
        state
            .app_state
            .chat_conversation_repo
            .get_by_id(&candidate_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load caller conversation: {error}"),
                )
            })?
            .filter(|conversation| conversation.context_type == ChatContextType::Project)
    };

    let Some(conversation) = conversation else {
        return Ok(None);
    };

    if conversation.context_type != ChatContextType::Project {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "delegate_start parent_conversation_id does not reference a project conversation",
        ));
    }

    if caller_context_id != conversation.context_id && caller_context_id != conversation.id.as_str()
    {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "delegate_start project caller context '{}' does not match parent conversation '{}'",
                caller_context_id,
                conversation.id.as_str()
            ),
        ));
    }

    Ok(Some(conversation))
}

async fn resolve_project_parent_working_directory(
    state: &HttpServerState,
    project: &Project,
    parent_conversation: Option<&ChatConversation>,
) -> Result<PathBuf, JsonError> {
    let Some(parent_conversation) = parent_conversation else {
        return validate_project_working_directory(project);
    };
    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&parent_conversation.id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load parent agent workspace: {error}"),
            )
        })?;

    match workspace {
        Some(workspace) => resolve_agent_conversation_workspace_path_for_send(project, &workspace)
            .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string())),
        None => validate_project_working_directory(project),
    }
}

async fn resolve_project_delegate_parent(
    state: &HttpServerState,
    req: &DelegateStartRequest,
    caller_context_id: &str,
) -> Result<ResolvedDelegateParent, JsonError> {
    let parent_conversation =
        load_project_parent_conversation(state, req, caller_context_id).await?;
    let project_id = parent_conversation
        .as_ref()
        .map(|conversation| conversation.context_id.clone())
        .unwrap_or_else(|| caller_context_id.to_string());
    let project = load_project_by_id(state, &ProjectId::from_string(project_id.clone())).await?;
    let working_directory =
        resolve_project_parent_working_directory(state, &project, parent_conversation.as_ref())
            .await?;
    let parent_conversation_id = parent_conversation
        .as_ref()
        .map(|conversation| conversation.id.as_str());
    let inherited_harness = parent_conversation
        .as_ref()
        .and_then(|conversation| conversation.provider_harness);

    Ok(ResolvedDelegateParent {
        context_type: ChatContextType::Project,
        context_id: project_id,
        project_id: project.id.as_str().to_string(),
        working_directory,
        parent_conversation_id,
        inherited_harness,
        ideation_verification: false,
    })
}

async fn resolve_task_like_delegate_parent(
    state: &HttpServerState,
    context_type: ChatContextType,
    caller_context_id: &str,
    parent_conversation_id: Option<String>,
) -> Result<ResolvedDelegateParent, JsonError> {
    let task = state
        .app_state
        .task_repo
        .get_by_id(&TaskId::from_string(caller_context_id.to_string()))
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load caller task context: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Caller task context not found"))?;
    let project = load_project_by_id(state, &task.project_id).await?;
    let default_working_directory = validate_project_working_directory(&project)?;
    let working_directory = resolve_working_directory(
        context_type,
        caller_context_id,
        Arc::clone(&state.app_state.project_repo),
        Arc::clone(&state.app_state.task_repo),
        Arc::clone(&state.app_state.ideation_session_repo),
        Arc::clone(&state.app_state.delegated_session_repo),
        &default_working_directory,
    )
    .await
    .map_err(|error| json_error(StatusCode::CONFLICT, error))?;

    Ok(ResolvedDelegateParent {
        context_type,
        context_id: caller_context_id.to_string(),
        project_id: project.id.as_str().to_string(),
        working_directory,
        parent_conversation_id,
        inherited_harness: None,
        ideation_verification: false,
    })
}

async fn resolve_nested_delegation_parent(
    state: &HttpServerState,
    caller_context_id: &str,
    parent_conversation_id: Option<String>,
) -> Result<ResolvedDelegateParent, JsonError> {
    let session = state
        .app_state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(caller_context_id.to_string()))
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load caller delegated session: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Caller delegated session not found"))?;
    let project = load_project_by_id(state, &session.project_id).await?;
    let delegated_conversation = state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Delegation, caller_context_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load caller delegated conversation: {error}"),
            )
        })?;
    let stored_parent_conversation_id = delegated_conversation
        .as_ref()
        .and_then(|conversation| conversation.parent_conversation_id.clone());
    if let (Some(supplied), Some(stored)) = (
        parent_conversation_id.as_deref(),
        stored_parent_conversation_id.as_deref(),
    ) {
        if supplied != stored {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "delegate_start parent_conversation_id does not match delegated session lineage",
            ));
        }
    }
    let parent_conversation_id = parent_conversation_id
        .or(stored_parent_conversation_id)
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "Nested delegate_start requires original parent conversation lineage",
            )
        })?;
    let parent_conversation = load_parent_conversation(state, &parent_conversation_id).await?;
    if parent_conversation.context_type == ChatContextType::Delegation {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Nested delegate_start parent_conversation_id must reference the original non-delegated parent conversation",
        ));
    }
    let lineage_project_id = chat_service_context::resolve_project_id(
        parent_conversation.context_type,
        &parent_conversation.context_id,
        Arc::clone(&state.app_state.task_repo),
        Arc::clone(&state.app_state.ideation_session_repo),
        Arc::clone(&state.app_state.delegated_session_repo),
    )
    .await
    .ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "Nested delegate parent conversation project could not be resolved",
        )
    })?;
    if lineage_project_id != session.project_id.as_str() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Nested delegate parent conversation belongs to a different project",
        ));
    }
    let working_directory = if parent_conversation.context_type == ChatContextType::Project {
        resolve_project_parent_working_directory(state, &project, Some(&parent_conversation)).await?
    } else {
        let default_working_directory = validate_project_working_directory(&project)?;
        resolve_working_directory(
            parent_conversation.context_type,
            &parent_conversation.context_id,
            Arc::clone(&state.app_state.project_repo),
            Arc::clone(&state.app_state.task_repo),
            Arc::clone(&state.app_state.ideation_session_repo),
            Arc::clone(&state.app_state.delegated_session_repo),
            &default_working_directory,
        )
        .await
        .map_err(|error| json_error(StatusCode::CONFLICT, error))?
    };

    Ok(ResolvedDelegateParent {
        context_type: ChatContextType::Delegation,
        context_id: session.id.as_str().to_string(),
        project_id: project.id.as_str().to_string(),
        working_directory,
        parent_conversation_id: Some(parent_conversation_id),
        inherited_harness: Some(session.harness),
        ideation_verification: false,
    })
}

async fn mark_delegated_launch_failed(
    state: &HttpServerState,
    delegated_session_id: &str,
    error_message: &str,
) -> Result<(), JsonError> {
    state
        .app_state
        .delegated_session_repo
        .update_status(
            &DelegatedSessionId::from_string(delegated_session_id.to_string()),
            "failed",
            Some(error_message.to_string()),
            Some(Utc::now()),
        )
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "{error_message}; additionally failed to terminalize delegated session: {error}"
                ),
            )
        })
}

fn json_error_detail(error: &JsonError) -> String {
    error
        .1
        .0
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Delegated launch setup failed")
        .to_string()
}

async fn resolve_delegate_parent(
    state: &HttpServerState,
    req: &DelegateStartRequest,
) -> Result<ResolvedDelegateParent, JsonError> {
    if req.caller_context_type.as_deref() == Some("ideation")
        || (req.caller_context_type.is_none() && req.parent_session_id.is_some())
    {
        return resolve_ideation_delegate_parent(state, req).await;
    }

    let caller_context_type = req.caller_context_type.as_deref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "delegate_start requires caller_context_type from the MCP transport",
        )
    })?;
    let caller_context_type = ChatContextType::from_str(caller_context_type)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error))?;
    let caller_context_id = req.caller_context_id.as_deref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "delegate_start requires caller_context_id from the MCP transport",
        )
    })?;
    match caller_context_type {
        ChatContextType::Ideation => resolve_ideation_delegate_parent(state, req).await,
        ChatContextType::Project => {
            resolve_project_delegate_parent(state, req, caller_context_id).await
        }
        ChatContextType::Task
        | ChatContextType::TaskExecution
        | ChatContextType::Review
        | ChatContextType::Merge
        | ChatContextType::BranchUpdate => {
            resolve_task_like_delegate_parent(
                state,
                caller_context_type,
                caller_context_id,
                req.parent_conversation_id.clone(),
            )
            .await
        }
        ChatContextType::Delegation => {
            resolve_nested_delegation_parent(
                state,
                caller_context_id,
                req.parent_conversation_id.clone(),
            )
            .await
        }
    }
}

fn build_delegated_prompt(
    agent_name: &str,
    parent_context_type: ChatContextType,
    parent_context_id: &str,
    parent_turn_id: Option<&str>,
    parent_message_id: Option<&str>,
    parent_conversation_id: Option<&str>,
    parent_tool_use_id: Option<&str>,
    delegated_session_id: &str,
    prompt: &str,
) -> String {
    let parent_line = if parent_context_type == ChatContextType::Ideation {
        format!("Parent ideation session: `{parent_context_id}`")
    } else {
        format!("Parent {} context: `{parent_context_id}`", parent_context_type)
    };
    let mut metadata_lines = vec![
        parent_line,
        format!("Delegated session: `{delegated_session_id}`"),
    ];
    if let Some(turn_id) = parent_turn_id {
        metadata_lines.push(format!("Parent turn id: `{turn_id}`"));
    }
    if let Some(message_id) = parent_message_id {
        metadata_lines.push(format!("Parent message id: `{message_id}`"));
    }
    if let Some(conversation_id) = parent_conversation_id {
        metadata_lines.push(format!("Parent conversation id: `{conversation_id}`"));
    }
    if let Some(tool_use_id) = parent_tool_use_id {
        metadata_lines.push(format!("Parent tool use id: `{tool_use_id}`"));
    }

    format!(
        "You are running as delegated RalphX specialist `{agent_name}`.\n{}\nOperate through the RalphX MCP tools available to your role and treat the delegated session as your working context.\n\nDelegated task:\n{prompt}",
        metadata_lines.join("\n"),
    )
}

fn delegated_event_seq() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or_default()
}

fn delegated_total_tokens(latest_run: &DelegatedRunSummary) -> Option<u64> {
    let total = latest_run.input_tokens.unwrap_or(0)
        + latest_run.output_tokens.unwrap_or(0)
        + latest_run.cache_creation_tokens.unwrap_or(0)
        + latest_run.cache_read_tokens.unwrap_or(0);
    if total == 0
        && latest_run.input_tokens.is_none()
        && latest_run.output_tokens.is_none()
        && latest_run.cache_creation_tokens.is_none()
        && latest_run.cache_read_tokens.is_none()
    {
        None
    } else {
        Some(total)
    }
}

fn delegated_duration_ms(latest_run: &DelegatedRunSummary) -> Option<u64> {
    let completed_at = latest_run.completed_at.as_ref()?;
    let started = chrono::DateTime::parse_from_rfc3339(&latest_run.started_at).ok()?;
    let completed = chrono::DateTime::parse_from_rfc3339(completed_at).ok()?;
    let duration = completed.signed_duration_since(started).num_milliseconds();
    if duration < 0 {
        None
    } else {
        u64::try_from(duration).ok()
    }
}

fn cached_streaming_task_from_started_payload(
    payload: &AgentTaskStartedPayload,
) -> CachedStreamingTask {
    CachedStreamingTask {
        tool_use_id: payload.tool_use_id.clone(),
        description: payload.description.clone(),
        subagent_type: payload.subagent_type.clone(),
        model: payload
            .model
            .clone()
            .or_else(|| payload.effective_model_id.clone())
            .or_else(|| payload.logical_model.clone()),
        status: "running".to_string(),
        agent_id: payload.delegated_agent_run_id.clone(),
        teammate_name: payload.teammate_name.clone(),
        delegated_job_id: payload.delegated_job_id.clone(),
        delegated_session_id: payload.delegated_session_id.clone(),
        delegated_conversation_id: payload.delegated_conversation_id.clone(),
        delegated_agent_run_id: payload.delegated_agent_run_id.clone(),
        provider_harness: payload.provider_harness.clone(),
        provider_session_id: payload.provider_session_id.clone(),
        upstream_provider: payload.upstream_provider.clone(),
        provider_profile: payload.provider_profile.clone(),
        logical_model: payload.logical_model.clone(),
        effective_model_id: payload.effective_model_id.clone(),
        logical_effort: payload.logical_effort.clone(),
        effective_effort: payload.effective_effort.clone(),
        approval_policy: payload.approval_policy.clone(),
        sandbox_mode: payload.sandbox_mode.clone(),
        total_tokens: None,
        total_tool_uses: None,
        duration_ms: None,
        input_tokens: None,
        output_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        estimated_usd: None,
        text_output: None,
    }
}

fn cached_streaming_task_from_completed_payload(
    payload: &AgentTaskCompletedPayload,
) -> CachedStreamingTask {
    CachedStreamingTask {
        tool_use_id: payload.tool_use_id.clone(),
        description: None,
        subagent_type: Some("delegated".to_string()),
        model: payload
            .effective_model_id
            .clone()
            .or_else(|| payload.logical_model.clone()),
        status: payload
            .status
            .clone()
            .unwrap_or_else(|| "completed".to_string()),
        agent_id: payload
            .agent_id
            .clone()
            .or_else(|| payload.delegated_agent_run_id.clone()),
        teammate_name: payload.teammate_name.clone(),
        delegated_job_id: payload.delegated_job_id.clone(),
        delegated_session_id: payload.delegated_session_id.clone(),
        delegated_conversation_id: payload.delegated_conversation_id.clone(),
        delegated_agent_run_id: payload.delegated_agent_run_id.clone(),
        provider_harness: payload.provider_harness.clone(),
        provider_session_id: payload.provider_session_id.clone(),
        upstream_provider: payload.upstream_provider.clone(),
        provider_profile: payload.provider_profile.clone(),
        logical_model: payload.logical_model.clone(),
        effective_model_id: payload.effective_model_id.clone(),
        logical_effort: payload.logical_effort.clone(),
        effective_effort: payload.effective_effort.clone(),
        approval_policy: payload.approval_policy.clone(),
        sandbox_mode: payload.sandbox_mode.clone(),
        total_tokens: payload.total_tokens,
        total_tool_uses: payload.total_tool_use_count,
        duration_ms: payload.total_duration_ms,
        input_tokens: payload.input_tokens,
        output_tokens: payload.output_tokens,
        cache_creation_tokens: payload.cache_creation_tokens,
        cache_read_tokens: payload.cache_read_tokens,
        estimated_usd: payload.estimated_usd,
        text_output: payload.text_output.clone(),
    }
}

#[doc(hidden)]
pub fn build_delegated_task_started_payload(
    snapshot: &DelegationJobSnapshot,
    logical_model: Option<&str>,
    logical_effort: Option<&str>,
    approval_policy: Option<&str>,
    sandbox_mode: Option<&str>,
    seq: u64,
) -> Option<AgentTaskStartedPayload> {
    let parent_tool_use_id = snapshot.parent_tool_use_id.as_ref()?;
    let parent_conversation_id = snapshot.parent_conversation_id.as_ref()?;
    Some(AgentTaskStartedPayload {
        tool_use_id: parent_tool_use_id.clone(),
        tool_name: "delegate_start".to_string(),
        description: Some(snapshot.agent_name.clone()),
        subagent_type: Some("delegated".to_string()),
        model: logical_model.map(str::to_string),
        teammate_name: None,
        delegated_job_id: Some(snapshot.job_id.clone()),
        delegated_session_id: Some(snapshot.delegated_session_id.clone()),
        delegated_conversation_id: snapshot.delegated_conversation_id.clone(),
        delegated_agent_run_id: snapshot.delegated_agent_run_id.clone(),
        provider_harness: Some(snapshot.harness.clone()),
        provider_session_id: None,
        upstream_provider: None,
        provider_profile: None,
        logical_model: logical_model.map(str::to_string),
        effective_model_id: None,
        logical_effort: logical_effort.map(str::to_string),
        effective_effort: None,
        approval_policy: approval_policy.map(str::to_string),
        sandbox_mode: sandbox_mode.map(str::to_string),
        conversation_id: parent_conversation_id.clone(),
        context_type: snapshot.parent_context_type.clone(),
        context_id: snapshot.parent_context_id.clone(),
        seq,
    })
}

#[doc(hidden)]
pub fn build_delegated_task_completed_payload(
    snapshot: &DelegationJobSnapshot,
    latest_run: Option<&DelegatedRunSummary>,
    status: &str,
    text_output: Option<&str>,
    error: Option<&str>,
    seq: u64,
) -> Option<AgentTaskCompletedPayload> {
    let parent_tool_use_id = snapshot.parent_tool_use_id.as_ref()?;
    let parent_conversation_id = snapshot.parent_conversation_id.as_ref()?;
    let latest_run_id = latest_run.map(|run| run.agent_run_id.clone());
    Some(AgentTaskCompletedPayload {
        tool_use_id: parent_tool_use_id.clone(),
        agent_id: latest_run_id.or_else(|| snapshot.delegated_agent_run_id.clone()),
        status: Some(status.to_string()),
        total_duration_ms: latest_run.and_then(delegated_duration_ms),
        total_tokens: latest_run.and_then(delegated_total_tokens),
        total_tool_use_count: None,
        teammate_name: None,
        delegated_job_id: Some(snapshot.job_id.clone()),
        delegated_session_id: Some(snapshot.delegated_session_id.clone()),
        delegated_conversation_id: snapshot.delegated_conversation_id.clone(),
        delegated_agent_run_id: latest_run
            .map(|run| run.agent_run_id.clone())
            .or_else(|| snapshot.delegated_agent_run_id.clone()),
        provider_harness: latest_run
            .and_then(|run| run.harness.clone())
            .or_else(|| Some(snapshot.harness.clone())),
        provider_session_id: latest_run.and_then(|run| run.provider_session_id.clone()),
        upstream_provider: latest_run.and_then(|run| run.upstream_provider.clone()),
        provider_profile: latest_run.and_then(|run| run.provider_profile.clone()),
        logical_model: latest_run.and_then(|run| run.logical_model.clone()),
        effective_model_id: latest_run.and_then(|run| run.effective_model_id.clone()),
        logical_effort: latest_run.and_then(|run| run.logical_effort.clone()),
        effective_effort: latest_run.and_then(|run| run.effective_effort.clone()),
        approval_policy: latest_run.and_then(|run| run.approval_policy.clone()),
        sandbox_mode: latest_run.and_then(|run| run.sandbox_mode.clone()),
        input_tokens: latest_run.and_then(|run| run.input_tokens),
        output_tokens: latest_run.and_then(|run| run.output_tokens),
        cache_creation_tokens: latest_run.and_then(|run| run.cache_creation_tokens),
        cache_read_tokens: latest_run.and_then(|run| run.cache_read_tokens),
        estimated_usd: latest_run.and_then(|run| run.estimated_usd),
        text_output: text_output.map(str::to_string),
        error: error.map(str::to_string),
        conversation_id: parent_conversation_id.clone(),
        context_type: snapshot.parent_context_type.clone(),
        context_id: snapshot.parent_context_id.clone(),
        seq,
    })
}

fn delegated_run_summary(run: AgentRun) -> DelegatedRunSummary {
    DelegatedRunSummary {
        agent_run_id: run.id.as_str(),
        status: run.status.to_string(),
        started_at: run.started_at.to_rfc3339(),
        completed_at: run.completed_at.map(|timestamp| timestamp.to_rfc3339()),
        error_message: run.error_message,
        harness: run.harness.map(|harness| harness.to_string()),
        provider_session_id: run.provider_session_id,
        upstream_provider: run.upstream_provider,
        provider_profile: run.provider_profile,
        logical_model: run.logical_model,
        effective_model_id: run.effective_model_id,
        logical_effort: run.logical_effort.map(|effort| effort.to_string()),
        effective_effort: run.effective_effort,
        approval_policy: run.approval_policy,
        sandbox_mode: run.sandbox_mode,
        input_tokens: run.input_tokens,
        output_tokens: run.output_tokens,
        cache_creation_tokens: run.cache_creation_tokens,
        cache_read_tokens: run.cache_read_tokens,
        estimated_usd: run.estimated_usd,
    }
}

fn latest_delegated_handoff_message(messages: Vec<ChatMessage>) -> Option<ChatMessage> {
    messages.into_iter().rev().find(|message| {
        !matches!(
            message.role,
            crate::domain::entities::MessageRole::User
                | crate::domain::entities::MessageRole::System
        ) && !message.content.trim().is_empty()
    })
}

fn delegated_handoff_message_summary(message: ChatMessage) -> ChatMessageSummary {
    ChatMessageSummary {
        role: message.role.to_string(),
        content: message.content.chars().take(500).collect(),
        created_at: message.created_at.to_rfc3339(),
    }
}

async fn resolve_parent_conversation_id(
    state: &HttpServerState,
    req: &DelegateStartRequest,
    parent_session_id: &str,
) -> Result<Option<String>, JsonError> {
    if let Some(parent_conversation_id) = req.parent_conversation_id.as_ref() {
        return Ok(Some(parent_conversation_id.clone()));
    }

    if req.caller_context_type.as_deref() == Some("ideation") {
        if let Some(caller_context_id) = req.caller_context_id.as_deref() {
            return Ok(state
                .app_state
                .chat_conversation_repo
                .get_active_for_context(ChatContextType::Ideation, caller_context_id)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to load caller conversation: {error}"),
                    )
                })?
                .map(|conversation| conversation.id.as_str()));
        }
    }

    Ok(state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Ideation, parent_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load parent conversation: {error}"),
            )
        })?
        .map(|conversation| conversation.id.as_str()))
}

async fn ensure_delegated_conversation(
    state: &HttpServerState,
    delegated_session_id: &str,
    parent_conversation_id: Option<&str>,
    title: Option<&str>,
) -> Result<ChatConversation, JsonError> {
    if let Some(conversation) = state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Delegation, delegated_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated conversation: {error}"),
            )
        })?
    {
        return Ok(conversation);
    }

    let mut conversation = ChatConversation::new_delegation(DelegatedSessionId::from_string(
        delegated_session_id.to_string(),
    ));
    conversation.parent_conversation_id = parent_conversation_id.map(str::to_string);
    conversation.title = title.map(str::to_string);
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create delegated conversation: {error}"),
            )
        })
}

pub(crate) async fn build_delegated_session_status_response(
    state: &HttpServerState,
    delegated_session_id: &str,
    include_messages: bool,
    message_limit: Option<u32>,
) -> Result<DelegatedSessionStatusResponse, JsonError> {
    let session_id = DelegatedSessionId::from_string(delegated_session_id.to_string());
    let session = state
        .app_state
        .delegated_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated session: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegated session not found"))?;

    let estimated_status = match session.status.as_str() {
        "running" => "running",
        "completed" => "completed",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => "idle",
    };
    let agent_state = AgentStateInfo {
        is_running: session.status == "running",
        started_at: Some(session.created_at.to_rfc3339()),
        last_active_at: Some(session.updated_at.to_rfc3339()),
        pid: None,
        estimated_status: estimated_status.to_string(),
    };

    let recent_messages = if include_messages {
        let _ = message_limit;
        if let Some(conversation) = state
            .app_state
            .chat_conversation_repo
            .get_active_for_context(ChatContextType::Delegation, delegated_session_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load delegated conversation: {error}"),
                )
            })?
        {
            let messages = state
                .app_state
                .chat_message_repo
                .get_by_conversation(&conversation.id)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to load delegated messages: {error}"),
                    )
                })?;
            Some(
                latest_delegated_handoff_message(messages)
                    .map(delegated_handoff_message_summary)
                    .into_iter()
                    .collect(),
            )
        } else {
            Some(Vec::new())
        }
    } else {
        None
    };

    let (conversation_id, latest_run) = if let Some(conversation) = state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Delegation, delegated_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated conversation: {error}"),
            )
        })? {
        let latest_run = state
            .app_state
            .agent_run_repo
            .get_latest_for_conversation(&conversation.id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load delegated run: {error}"),
                )
            })?
            .map(delegated_run_summary);
        (Some(conversation.id.as_str()), latest_run)
    } else {
        (None, None)
    };

    Ok(DelegatedSessionStatusResponse {
        session: DelegatedSessionSummary {
            id: session.id.as_str().to_string(),
            title: session.title,
            status: session.status,
            parent_context_type: session.parent_context_type,
            parent_context_id: session.parent_context_id,
            agent_name: session.agent_name,
            harness: session.harness.to_string(),
            provider_session_id: session.provider_session_id,
            created_at: session.created_at.to_rfc3339(),
            updated_at: session.updated_at.to_rfc3339(),
            completed_at: session.completed_at.map(|timestamp| timestamp.to_rfc3339()),
        },
        agent_state,
        conversation_id,
        latest_run,
        recent_messages,
    })
}

pub async fn get_delegated_session_status(
    State(state): State<HttpServerState>,
    axum::extract::Path(delegated_session_id): axum::extract::Path<String>,
) -> Result<Json<DelegatedSessionStatusResponse>, JsonError> {
    let status =
        build_delegated_session_status_response(&state, &delegated_session_id, false, None).await?;
    Ok(Json(status))
}

pub(crate) async fn start_delegate_impl(
    state: &HttpServerState,
    req: DelegateStartRequest,
) -> Result<DelegationJobSnapshot, JsonError> {
    let caller_agent_name = req.caller_agent_name.as_deref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "delegate_start requires caller_agent_name from the MCP transport",
        )
    })?;
    let parent = resolve_delegate_parent(state, &req).await?;
    let requested_harness = req.harness.or(parent.inherited_harness);
    let project = state
        .app_state
        .project_repo
        .get_by_id(&ProjectId::from_string(parent.project_id.clone()))
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated project: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegated project not found"))?;
    let role = routing_role_for_delegated_launch(
        &req.agent_name,
        parent.context_type,
        parent.ideation_verification,
    );
    let resolved_spawn = resolve_manual_role_spawn_settings(
        &req.agent_name,
        Some(parent.project_id.as_str()),
        Some(std::path::Path::new(&project.working_directory)),
        role,
        requested_harness,
        req.model.as_deref(),
        &state.app_state.manual_role_default_service(),
    )
    .await
    .map_err(|error| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to resolve delegated agent defaults: {error}"),
        )
    })?;
    let harness = resolved_spawn.effective_harness;
    let delegated_model = req.model.clone();
    let plugin_dir = resolve_harness_plugin_dir(harness, &parent.working_directory);
    let project_root = resolve_project_root_from_plugin_dir(&plugin_dir);
    let (_caller_definition, definition) = resolve_delegation_policy(
        &project_root,
        caller_agent_name,
        req.caller_agent_profile.as_deref(),
        &req.agent_name,
    )?;
    let delegated_session_id = resolve_delegated_session_id(state, &req, &parent, harness).await?;
    let logical_effort = req.logical_effort.clone();
    let approval_policy = req.approval_policy.clone();
    let sandbox_mode = req.sandbox_mode.clone();
    state
        .app_state
        .delegated_session_repo
        .update_status(
            &DelegatedSessionId::from_string(delegated_session_id.clone()),
            "running",
            None,
            None,
        )
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update delegated session status: {error}"),
            )
        })?;

    let delegated_conversation = match ensure_delegated_conversation(
        state,
        &delegated_session_id,
        parent.parent_conversation_id.as_deref(),
        req.title.as_deref(),
    )
    .await
    {
        Ok(conversation) => conversation,
        Err(error) => {
            mark_delegated_launch_failed(
                state,
                &delegated_session_id,
                &json_error_detail(&error),
            )
            .await?;
            return Err(error);
        }
    };

    let chat_service = state
        .app_state
        .build_chat_service_with_execution_state(Arc::clone(&state.execution_state));
    let send_result = chat_service
        .send_message(
            ChatContextType::Delegation,
            &delegated_session_id,
            &build_delegated_prompt(
                &definition.name,
                parent.context_type,
                &parent.context_id,
                req.parent_turn_id.as_deref(),
                req.parent_message_id.as_deref(),
                parent.parent_conversation_id.as_deref(),
                req.parent_tool_use_id.as_deref(),
                &delegated_session_id,
                &req.prompt,
            ),
            SendMessageOptions {
                routing_role_override: Some(role),
                harness_override: Some(harness),
                agent_name_override: Some(definition.name.clone()),
                model_override: delegated_model.clone(),
                working_directory_override: Some(parent.working_directory.clone()),
                logical_effort_override: logical_effort.clone(),
                approval_policy_override: approval_policy.clone(),
                sandbox_mode_override: sandbox_mode.clone(),
                is_external_mcp: true,
                ..Default::default()
            },
        )
        .await;
    let send_result = match send_result {
        Ok(result) => result,
        Err(error) => {
            let error_message = format!("Failed to start delegated chat run: {error}");
            mark_delegated_launch_failed(state, &delegated_session_id, &error_message).await?;
            return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, error_message));
        }
    };

    let job_id = uuid::Uuid::new_v4().to_string();
    let snapshot = state
        .delegation_service
        .register_running(
            job_id.clone(),
            parent.context_type.to_string(),
            parent.context_id.clone(),
            req.parent_turn_id.clone(),
            req.parent_message_id.clone(),
            parent.parent_conversation_id.clone(),
            req.parent_tool_use_id.clone(),
            delegated_session_id.clone(),
            Some(delegated_conversation.id.as_str()),
            Some(send_result.agent_run_id.clone()),
            definition.name.clone(),
            harness.to_string(),
        )
        .await;

    let logical_effort_label = logical_effort.as_ref().map(|value| value.to_string());
    if let Some(payload) = build_delegated_task_started_payload(
        &snapshot,
        delegated_model.as_deref(),
        logical_effort_label.as_deref(),
        approval_policy.as_deref(),
        sandbox_mode.as_deref(),
        delegated_event_seq(),
    ) {
        state
            .app_state
            .streaming_state_cache
            .add_task(
                &payload.conversation_id,
                cached_streaming_task_from_started_payload(&payload),
            )
            .await;
        crate::http_server::emit_serialized_http_event(state, events::AGENT_TASK_STARTED, &payload);
    }

    let delegation_service = state.delegation_service.clone();
    let delegated_session_repo = state.app_state.delegated_session_repo.clone();
    let chat_message_repo = state.app_state.chat_message_repo.clone();
    let agent_run_repo = state.app_state.agent_run_repo.clone();
    let app_events = Arc::clone(&state.app_state.events);
    let streaming_state_cache = state.app_state.streaming_state_cache.clone();
    let snapshot_for_events = snapshot.clone();
    let agent_run_id = send_result.agent_run_id.clone();
    let conversation_id = delegated_conversation.id;
    let delegated_session_id_for_task = delegated_session_id.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let run = match agent_run_repo
                .get_by_id(&crate::domain::entities::AgentRunId::from_string(
                    agent_run_id.clone(),
                ))
                .await
            {
                Ok(Some(run)) => run,
                Ok(None) => continue,
                Err(error) => {
                    if let Some(payload) = build_delegated_task_completed_payload(
                        &snapshot_for_events,
                        None,
                        "failed",
                        None,
                        Some(&error.to_string()),
                        delegated_event_seq(),
                    ) {
                        streaming_state_cache
                            .add_task(
                                &payload.conversation_id,
                                cached_streaming_task_from_completed_payload(&payload),
                            )
                            .await;
                        if let Err(error) = ralphx_events::emit_serialized(
                            app_events.as_ref(),
                            events::AGENT_TASK_COMPLETED,
                            &payload,
                        ) {
                            tracing::warn!(
                                event = events::AGENT_TASK_COMPLETED,
                                %error,
                                "Failed to serialize delegated task completion event payload"
                            );
                        }
                    }
                    delegation_service
                        .mark_failed(&job_id, error.to_string())
                        .await;
                    let _ = delegated_session_repo
                        .update_status(
                            &DelegatedSessionId::from_string(delegated_session_id_for_task.clone()),
                            "failed",
                            Some(error.to_string()),
                            Some(Utc::now()),
                        )
                        .await;
                    break;
                }
            };

            if run.status == crate::domain::entities::AgentRunStatus::Running {
                continue;
            }

            let latest_run = delegated_run_summary(run.clone());

            match run.status {
                crate::domain::entities::AgentRunStatus::Completed => {
                    let mut content = String::new();
                    for _ in 0..10 {
                        content = chat_message_repo
                            .get_by_conversation(&conversation_id)
                            .await
                            .ok()
                            .and_then(latest_delegated_handoff_message)
                            .map(|message| message.content)
                            .unwrap_or_default();
                        if !content.is_empty() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                    if let Some(payload) = build_delegated_task_completed_payload(
                        &snapshot_for_events,
                        Some(&latest_run),
                        "completed",
                        Some(&content),
                        None,
                        delegated_event_seq(),
                    ) {
                        streaming_state_cache
                            .add_task(
                                &payload.conversation_id,
                                cached_streaming_task_from_completed_payload(&payload),
                            )
                            .await;
                        if let Err(error) = ralphx_events::emit_serialized(
                            app_events.as_ref(),
                            events::AGENT_TASK_COMPLETED,
                            &payload,
                        ) {
                            tracing::warn!(
                                event = events::AGENT_TASK_COMPLETED,
                                %error,
                                "Failed to serialize delegated task completion event payload"
                            );
                        }
                    }
                    delegation_service.mark_completed(&job_id, content).await;
                    let _ = delegated_session_repo
                        .update_status(
                            &DelegatedSessionId::from_string(delegated_session_id_for_task.clone()),
                            "completed",
                            None,
                            Some(Utc::now()),
                        )
                        .await;
                }
                crate::domain::entities::AgentRunStatus::Failed => {
                    let detail = run
                        .error_message
                        .unwrap_or_else(|| "Delegated run failed".to_string());
                    if let Some(payload) = build_delegated_task_completed_payload(
                        &snapshot_for_events,
                        Some(&latest_run),
                        "failed",
                        None,
                        Some(&detail),
                        delegated_event_seq(),
                    ) {
                        streaming_state_cache
                            .add_task(
                                &payload.conversation_id,
                                cached_streaming_task_from_completed_payload(&payload),
                            )
                            .await;
                        if let Err(error) = ralphx_events::emit_serialized(
                            app_events.as_ref(),
                            events::AGENT_TASK_COMPLETED,
                            &payload,
                        ) {
                            tracing::warn!(
                                event = events::AGENT_TASK_COMPLETED,
                                %error,
                                "Failed to serialize delegated task completion event payload"
                            );
                        }
                    }
                    delegation_service
                        .mark_failed(&job_id, detail.clone())
                        .await;
                    let _ = delegated_session_repo
                        .update_status(
                            &DelegatedSessionId::from_string(delegated_session_id_for_task.clone()),
                            "failed",
                            Some(detail),
                            Some(Utc::now()),
                        )
                        .await;
                }
                crate::domain::entities::AgentRunStatus::Cancelled => {
                    if let Some(payload) = build_delegated_task_completed_payload(
                        &snapshot_for_events,
                        Some(&latest_run),
                        "cancelled",
                        None,
                        None,
                        delegated_event_seq(),
                    ) {
                        streaming_state_cache
                            .add_task(
                                &payload.conversation_id,
                                cached_streaming_task_from_completed_payload(&payload),
                            )
                            .await;
                        if let Err(error) = ralphx_events::emit_serialized(
                            app_events.as_ref(),
                            events::AGENT_TASK_COMPLETED,
                            &payload,
                        ) {
                            tracing::warn!(
                                event = events::AGENT_TASK_COMPLETED,
                                %error,
                                "Failed to serialize delegated task completion event payload"
                            );
                        }
                    }
                    let _ = delegated_session_repo
                        .update_status(
                            &DelegatedSessionId::from_string(delegated_session_id_for_task.clone()),
                            "cancelled",
                            None,
                            Some(Utc::now()),
                        )
                        .await;
                }
                crate::domain::entities::AgentRunStatus::Running => {}
            }
            break;
        }
    });

    Ok(snapshot)
}

pub async fn start_delegate(
    State(state): State<HttpServerState>,
    Json(req): Json<DelegateStartRequest>,
) -> Result<Json<DelegationJobSnapshot>, JsonError> {
    Ok(Json(start_delegate_impl(&state, req).await?))
}

pub async fn wait_delegate(
    State(state): State<HttpServerState>,
    Json(req): Json<DelegateWaitRequest>,
) -> Result<Json<DelegationJobSnapshot>, JsonError> {
    let mut snapshot = state
        .delegation_service
        .snapshot(&req.job_id)
        .await
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegation job not found"))?;
    if req
        .include_delegated_status
        .or(req.include_child_status)
        .unwrap_or(true)
    {
        match build_delegated_session_status_response(
            &state,
            &snapshot.delegated_session_id,
            req.include_messages.unwrap_or(false),
            req.message_limit,
        )
        .await
        {
            Ok(delegated_status) => {
                snapshot.delegated_status = Some(delegated_status);
            }
            Err((status, error)) => {
                warn!(
                    job_id = snapshot.job_id,
                    delegated_session_id = snapshot.delegated_session_id,
                    status = status.as_u16(),
                    error = %error.0["error"].as_str().unwrap_or("unknown error"),
                    "Failed to hydrate delegated session status"
                );
            }
        }
    }
    Ok(Json(snapshot))
}

pub async fn cancel_delegate(
    State(state): State<HttpServerState>,
    Json(req): Json<DelegateCancelRequest>,
) -> Result<Json<DelegationJobSnapshot>, JsonError> {
    cancel_delegate_impl(&state, &req.job_id).await.map(Json)
}

pub(crate) async fn cancel_delegate_impl(
    state: &HttpServerState,
    job_id: &str,
) -> Result<DelegationJobSnapshot, JsonError> {
    let snapshot = state
        .delegation_service
        .cancel(job_id)
        .await
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                "Delegation job not found or no longer cancellable",
            )
        })?;
    let chat_service = state
        .app_state
        .build_chat_service_with_execution_state(Arc::clone(&state.execution_state));
    let stopped = chat_service
        .stop_agent(ChatContextType::Delegation, &snapshot.delegated_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to stop delegated agent: {error}"),
            )
        })?;
    if !stopped {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "Delegation job is no longer running",
        ));
    }
    state
        .app_state
        .delegated_session_repo
        .update_status(
            &DelegatedSessionId::from_string(snapshot.delegated_session_id.clone()),
            "cancelled",
            None,
            Some(Utc::now()),
        )
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update delegated session cancellation: {error}"),
            )
        })?;
    Ok(snapshot)
}
