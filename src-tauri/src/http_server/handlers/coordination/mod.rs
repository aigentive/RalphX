use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::Utc;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path_for_send;
use crate::application::agent_workspace_pr_description::escape_xml_text;
use crate::application::chat_service::{
    chat_service_context, events, resolve_working_directory, AgentTaskCompletedPayload,
    AgentTaskStartedPayload, CachedStreamingTask, ChatService, StreamingStateCache,
};
use crate::application::ideation_workspace::resolve_ideation_workspace_path;
use crate::application::native_delegation_launcher::{
    NativeDelegationLaunchParent, NativeDelegationLaunchRequest, NativeDelegationLauncher,
};
use crate::application::AgentTaskService;
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentTaskAssignmentTerminalStatus, AgentTaskAssignmentView,
    ChatContextType, ChatConversation, ChatConversationId, ChatMessage, DelegatedSession,
    DelegatedSessionId, IdeationSessionId, Project, ProjectId, SessionPurpose, TaskId,
};
use crate::http_server::delegation::{
    persist_terminal_projection, DelegationAssignmentSummary, DelegationJobSnapshot,
};
use crate::http_server::types::{
    AgentStateInfo, ChatMessageSummary, DelegateCancelRequest, DelegateStartRequest,
    DelegateWaitRequest, DelegatedRunSummary, DelegatedSessionStatusResponse,
    DelegatedSessionSummary, HttpServerState,
};
use crate::infrastructure::agents::harness_agent_catalog::{
    load_canonical_agent_definition, load_canonical_agent_definition_for_profile,
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

pub(crate) fn resolve_delegation_policy(
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
    caller_conversation_id: Option<String>,
    parent_conversation_id: Option<String>,
    ideation_verification: bool,
}

async fn preflight_requested_delegated_session(
    state: &HttpServerState,
    req: &DelegateStartRequest,
    parent: &ResolvedDelegateParent,
) -> Result<Option<DelegatedSession>, JsonError> {
    let requested_id = req
        .delegated_session_id
        .as_ref()
        .or(req.child_session_id.as_ref());

    let Some(delegated_session_id) = requested_id else {
        return Ok(None);
    };

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
    if delegated.agent_name != req.agent_name {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "Delegated session agent '{}' does not match requested agent '{}'",
                delegated.agent_name, req.agent_name
            ),
        ));
    }
    if let Some(requested_harness) = req.harness {
        if requested_harness != delegated.harness {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                format!(
                    "Delegated session harness '{}' does not match requested harness '{}'",
                    delegated.harness, requested_harness
                ),
            ));
        }
    }
    let delegated_conversation = state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Delegation, delegated.id.as_str())
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated session conversation: {error}"),
            )
        })?;
    let stored_parent_conversation_id = delegated_conversation
        .as_ref()
        .and_then(|conversation| conversation.parent_conversation_id.as_deref());
    let lineage_matches = match parent.parent_conversation_id.as_deref() {
        Some(expected) => stored_parent_conversation_id == Some(expected),
        None => stored_parent_conversation_id.is_none(),
    };
    if !lineage_matches {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Delegated session conversation lineage does not match the trusted caller lineage",
        ));
    }
    Ok(Some(delegated))
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
        let caller_conversation_id = state
            .app_state
            .chat_conversation_repo
            .get_active_for_context(ChatContextType::Ideation, caller_context_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load caller ideation conversation: {error}"),
                )
            })?
            .map(|conversation| conversation.id.as_str())
            .or_else(|| parent_conversation_id.clone());
        let (project_id, working_directory) =
            load_ideation_parent_project_working_directory(state, &derived_parent_session_id)
                .await?;

        return Ok(ResolvedDelegateParent {
            context_type: ChatContextType::Ideation,
            context_id: derived_parent_session_id,
            project_id: project_id.as_str().to_string(),
            working_directory,
            caller_conversation_id,
            parent_conversation_id,
            ideation_verification: caller_session.session_purpose == SessionPurpose::Verification,
        });
    }

    let parent_session_id = req.parent_session_id.as_deref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "delegate_start requires parent_session_id",
        )
    })?;
    let parent_conversation_id =
        resolve_parent_conversation_id(state, req, parent_session_id).await?;
    let caller_conversation_id = state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Ideation, parent_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load caller ideation conversation: {error}"),
            )
        })?
        .map(|conversation| conversation.id.as_str())
        .or_else(|| parent_conversation_id.clone());
    let (project_id, working_directory) =
        load_ideation_parent_project_working_directory(state, parent_session_id).await?;
    Ok(ResolvedDelegateParent {
        context_type: ChatContextType::Ideation,
        context_id: parent_session_id.to_string(),
        project_id: project_id.as_str().to_string(),
        working_directory,
        caller_conversation_id,
        parent_conversation_id,
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
        .get_by_id(&ChatConversationId::from_string(
            conversation_id.to_string(),
        ))
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
    let conversation = if let Some(parent_conversation_id) = req.parent_conversation_id.as_deref() {
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
    Ok(ResolvedDelegateParent {
        context_type: ChatContextType::Project,
        context_id: project_id,
        project_id: project.id.as_str().to_string(),
        working_directory,
        caller_conversation_id: parent_conversation_id.clone(),
        parent_conversation_id,
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
        Some(state.app_state.app_paths.app_data_dir()),
    )
    .await
    .map_err(|error| json_error(StatusCode::CONFLICT, error))?;
    let caller_conversation_id = state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(context_type, caller_context_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load caller task conversation: {error}"),
            )
        })?
        .map(|conversation| conversation.id.as_str())
        .or_else(|| parent_conversation_id.clone());

    Ok(ResolvedDelegateParent {
        context_type,
        context_id: caller_context_id.to_string(),
        project_id: project.id.as_str().to_string(),
        working_directory,
        caller_conversation_id,
        parent_conversation_id,
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
        .get_by_id(&DelegatedSessionId::from_string(
            caller_context_id.to_string(),
        ))
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
        })?
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                "Active caller delegated conversation not found",
            )
        })?;
    let stored_parent_conversation_id = delegated_conversation.parent_conversation_id.clone();
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
        resolve_project_parent_working_directory(state, &project, Some(&parent_conversation))
            .await?
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
            Some(state.app_state.app_paths.app_data_dir()),
        )
        .await
        .map_err(|error| json_error(StatusCode::CONFLICT, error))?
    };

    Ok(ResolvedDelegateParent {
        context_type: ChatContextType::Delegation,
        context_id: session.id.as_str().to_string(),
        project_id: project.id.as_str().to_string(),
        working_directory,
        caller_conversation_id: Some(delegated_conversation.id.as_str()),
        parent_conversation_id: Some(parent_conversation_id),
        ideation_verification: false,
    })
}

pub(crate) async fn mark_delegated_launch_failed(
    state: &HttpServerState,
    delegated_session_id: &str,
    error_message: &str,
) -> Result<(), JsonError> {
    let assignment_error = AgentTaskService::new(state.app_state.agent_task_repo.clone())
        .fail_reserved_assignment(
            &DelegatedSessionId::from_string(delegated_session_id.to_string()),
            error_message,
        )
        .await
        .err();
    let session_result = state
        .app_state
        .delegated_session_repo
        .update_status(
            &DelegatedSessionId::from_string(delegated_session_id.to_string()),
            "failed",
            Some(error_message.to_string()),
            Some(Utc::now()),
        )
        .await;
    match (assignment_error, session_result) {
        (None, Ok(())) => Ok(()),
        (Some(assignment_error), Ok(())) => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "{error_message}; additionally failed to release the reserved task assignment: {assignment_error}"
            ),
        )),
        (None, Err(session_error)) => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "{error_message}; additionally failed to terminalize delegated session: {session_error}"
            ),
        )),
        (Some(assignment_error), Err(session_error)) => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "{error_message}; additionally failed to release the reserved task assignment: {assignment_error}; and failed to terminalize delegated session: {session_error}"
            ),
        )),
    }
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
        ChatContextType::Standalone => Err(json_error(
            StatusCode::BAD_REQUEST,
            "delegate_start is not supported for standalone conversations",
        )),
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

pub(crate) fn build_delegated_prompt(
    agent_name: &str,
    parent_context_type: ChatContextType,
    parent_context_id: &str,
    parent_turn_id: Option<&str>,
    parent_message_id: Option<&str>,
    parent_conversation_id: Option<&str>,
    parent_tool_use_id: Option<&str>,
    delegated_session_id: &str,
    assignment: Option<&AgentTaskAssignmentView>,
    prompt: &str,
) -> String {
    let parent_line = if parent_context_type == ChatContextType::Ideation {
        format!("Parent ideation session: `{parent_context_id}`")
    } else {
        format!(
            "Parent {} context: `{parent_context_id}`",
            parent_context_type
        )
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
    let assignment_block = assignment.map_or_else(String::new, |assignment| {
        format!(
            "\n\nImmutable assigned work:\n- Parent task: #{} — {}\n- Requirements: {}\n- Assignment state: {}\n\nDo not recreate or mirror this assigned task in your local ledger. Use your delegate-local ledger only to decompose the work. When all local work is resolved, call `complete_delegate_assignment` (or `release_delegate_assignment` if you cannot finish), then stop work and return your final handoff. Backend terminal settlement finalizes the exact assignment attempt.",
            assignment.task.task_number,
            assignment.task.title,
            assignment.task.details,
            assignment.assignment.state,
        )
    });

    format!(
        "You are running as delegated RalphX specialist `{agent_name}`.\n{}\nOperate through the RalphX MCP tools available to your role and treat the delegated session as your working context.{assignment_block}\n\n<delegated_task>\n{}\n</delegated_task>",
        metadata_lines.join("\n"),
        escape_xml_text(prompt),
    )
}

mod native_delegation;

use native_delegation::resolve_parent_conversation_id;
pub(crate) use native_delegation::{
    build_delegated_session_status_response, cancel_delegate_impl, ensure_delegated_conversation,
    fail_started_delegated_launch, start_delegate_impl_with_parent_run,
};
pub use native_delegation::{
    build_delegated_task_completed_payload, build_delegated_task_started_payload, cancel_delegate,
    get_delegated_session_status, start_delegate, start_delegate_with_runtime_context,
    wait_delegate, DELEGATION_INVALID_RUN_IDENTITY_ERROR, DELEGATION_MISSING_RUN_IDENTITY_ERROR,
};
