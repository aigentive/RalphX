use axum::{extract::State, http::HeaderMap, Json};

use crate::application::AgentTaskService;
use crate::domain::entities::{
    AgentRunId, AgentTaskAssignmentView, AgentTaskCreate, AgentTaskDetail, AgentTaskListId,
    AgentTaskListSummary, AgentTaskMutationResult, AgentTaskPatch, AgentTaskScope,
    AgentTaskSummary, ChatContextType, ChatConversationId, DelegatedSessionId, ProjectId,
};
use crate::domain::repositories::AgentTaskListOptions;
use crate::http_server::{
    AgentTaskContextFields, AgentTaskDto, AgentTaskGetResponse, AgentTaskListResponse,
    AgentTaskListSummaryDto, AgentTaskListsResponse, AgentTaskMutationResponse,
    AgentTaskStateChangeDto, AgentTaskSummaryDto, ClaimAgentTaskRequest, CompleteAgentTaskRequest,
    CompleteDelegateAssignmentRequest, CreateAgentTaskRequest, DelegateAssignmentDto,
    DelegateAssignmentResponse, GetAgentTaskRequest, HttpServerState, ListAgentTaskListsRequest,
    ListAgentTasksForListRequest, ListAgentTasksRequest, ReleaseDelegateAssignmentRequest,
    UpdateAgentTaskRequest,
};

pub async fn create_agent_task(
    State(state): State<HttpServerState>,
    Json(request): Json<CreateAgentTaskRequest>,
) -> Json<AgentTaskMutationResponse> {
    let scope = match resolve_scope(&request.context) {
        Ok(scope) => scope,
        Err(error) => return Json(mutation_error(error)),
    };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    let input = AgentTaskCreate {
        title: request.title,
        details: request.details,
        active_label: request.active_label,
        owner_agent: request.owner_agent,
        metadata: request.metadata,
        blocked_by: request.blocked_by,
        blocks: request.blocks,
    };
    Json(match service.create_task(&scope, input).await {
        Ok(result) => mutation_success(result),
        Err(error) => mutation_error(error.to_string()),
    })
}

pub async fn get_agent_task(
    State(state): State<HttpServerState>,
    Json(request): Json<GetAgentTaskRequest>,
) -> Json<AgentTaskGetResponse> {
    let scope = match resolve_scope(&request.context) {
        Ok(scope) => scope,
        Err(error) => return Json(get_error(error)),
    };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    Json(match service.get_task(&scope, &request.task_ref).await {
        Ok(task) => AgentTaskGetResponse {
            success: task.is_some(),
            task: task.map(task_dto),
            error: None,
        },
        Err(error) => get_error(error.to_string()),
    })
}

pub async fn list_agent_tasks(
    State(state): State<HttpServerState>,
    Json(request): Json<ListAgentTasksRequest>,
) -> Json<AgentTaskListResponse> {
    let scope = match resolve_scope(&request.context) {
        Ok(scope) => scope,
        Err(error) => return Json(list_error(error)),
    };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    Json(
        match service
            .list_tasks(
                &scope,
                AgentTaskListOptions {
                    include_done: request.include_done,
                },
            )
            .await
        {
            Ok(tasks) => AgentTaskListResponse {
                success: true,
                tasks: tasks.into_iter().map(summary_dto).collect(),
                error: None,
            },
            Err(error) => list_error(error.to_string()),
        },
    )
}

pub async fn list_agent_task_lists(
    State(state): State<HttpServerState>,
    Json(request): Json<ListAgentTaskListsRequest>,
) -> Json<AgentTaskListsResponse> {
    let scope = match resolve_scope(&request.context) {
        Ok(scope) => scope,
        Err(error) => return Json(lists_error(error)),
    };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    Json(match service.list_task_lists(&scope).await {
        Ok(lists) => AgentTaskListsResponse {
            success: true,
            lists: lists.into_iter().map(list_summary_dto).collect(),
            error: None,
        },
        Err(error) => lists_error(error.to_string()),
    })
}

pub async fn list_agent_tasks_for_list(
    State(state): State<HttpServerState>,
    Json(request): Json<ListAgentTasksForListRequest>,
) -> Json<AgentTaskListResponse> {
    let scope = match resolve_scope(&request.context) {
        Ok(scope) => scope,
        Err(error) => return Json(list_error(error)),
    };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    let list_id = AgentTaskListId::from_string(request.list_id);
    Json(
        match service
            .list_tasks_for_list(
                &scope,
                &list_id,
                AgentTaskListOptions {
                    include_done: request.include_done,
                },
            )
            .await
        {
            Ok(tasks) => AgentTaskListResponse {
                success: true,
                tasks: tasks.into_iter().map(summary_dto).collect(),
                error: None,
            },
            Err(error) => list_error(error.to_string()),
        },
    )
}

pub async fn update_agent_task(
    State(state): State<HttpServerState>,
    Json(request): Json<UpdateAgentTaskRequest>,
) -> Json<AgentTaskMutationResponse> {
    let scope = match resolve_scope(&request.context) {
        Ok(scope) => scope,
        Err(error) => return Json(mutation_error(error)),
    };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    let patch = AgentTaskPatch {
        title: request.title,
        details: request.details,
        active_label: request.active_label,
        owner_agent: request.owner_agent,
        state: request.state,
        metadata_patch: request.metadata,
        add_blocked_by: request.add_blocked_by,
        add_blocks: request.add_blocks,
        remove_blocked_by: request.remove_blocked_by,
        remove_blocks: request.remove_blocks,
    };
    Json(
        match service.update_task(&scope, &request.task_ref, patch).await {
            Ok(Some(result)) => mutation_success(result),
            Ok(None) => mutation_error("Agent task not found".to_string()),
            Err(error) => mutation_error(error.to_string()),
        },
    )
}

pub async fn claim_agent_task(
    State(state): State<HttpServerState>,
    Json(request): Json<ClaimAgentTaskRequest>,
) -> Json<AgentTaskMutationResponse> {
    let scope = match resolve_scope(&request.context) {
        Ok(scope) => scope,
        Err(error) => return Json(mutation_error(error)),
    };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    Json(
        match service
            .claim_task(&scope, &request.task_ref, request.owner_agent)
            .await
        {
            Ok(Some(result)) => mutation_success(result),
            Ok(None) => mutation_error("Agent task not found".to_string()),
            Err(error) => mutation_error(error.to_string()),
        },
    )
}

pub async fn complete_agent_task(
    State(state): State<HttpServerState>,
    Json(request): Json<CompleteAgentTaskRequest>,
) -> Json<AgentTaskMutationResponse> {
    let scope = match resolve_scope(&request.context) {
        Ok(scope) => scope,
        Err(error) => return Json(mutation_error(error)),
    };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    Json(
        match service
            .complete_task(&scope, &request.task_ref, request.metadata)
            .await
        {
            Ok(Some(result)) => mutation_success(result),
            Ok(None) => mutation_error("Agent task not found".to_string()),
            Err(error) => mutation_error(error.to_string()),
        },
    )
}

pub async fn get_delegate_assignment(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Json<DelegateAssignmentResponse> {
    let (delegated_session_id, delegated_agent_run_id, _delegated_conversation_id) =
        match trusted_delegate_identity(&state, &headers, "get_delegate_assignment").await {
            Ok(identity) => identity,
            Err(error) => return Json(assignment_error(error)),
        };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    Json(
        match service
            .get_assignment_for_run(&delegated_agent_run_id)
            .await
        {
            Ok(Some(assignment))
                if assignment.assignment.delegated_session_id == delegated_session_id =>
            {
                assignment_success(Some(assignment))
            }
            Ok(Some(_)) => assignment_error(
                "Bound delegate assignment does not belong to the current session".to_string(),
            ),
            Ok(None) => assignment_success(None),
            Err(error) => assignment_error(error.to_string()),
        },
    )
}

pub async fn complete_delegate_assignment(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<CompleteDelegateAssignmentRequest>,
) -> Json<DelegateAssignmentResponse> {
    let (delegated_session_id, delegated_agent_run_id, _delegated_conversation_id) =
        match trusted_delegate_identity(&state, &headers, "complete_delegate_assignment").await {
            Ok(identity) => identity,
            Err(error) => return Json(assignment_error(error)),
        };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    Json(
        match service
            .request_assignment_completion(
                &delegated_session_id,
                &delegated_agent_run_id,
                request.metadata,
            )
            .await
        {
            Ok(Some(assignment)) => assignment_success(Some(assignment)),
            Ok(None) => assignment_error("No bound delegate assignment".to_string()),
            Err(error) => assignment_error(error.to_string()),
        },
    )
}

pub async fn release_delegate_assignment(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<ReleaseDelegateAssignmentRequest>,
) -> Json<DelegateAssignmentResponse> {
    if request.reason.trim().is_empty() {
        return Json(assignment_error(
            "release_delegate_assignment requires a non-empty reason".to_string(),
        ));
    }
    let (delegated_session_id, delegated_agent_run_id, _delegated_conversation_id) =
        match trusted_delegate_identity(&state, &headers, "release_delegate_assignment").await {
            Ok(identity) => identity,
            Err(error) => return Json(assignment_error(error)),
        };
    let service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    Json(
        match service
            .request_assignment_release(
                &delegated_session_id,
                &delegated_agent_run_id,
                request.reason.trim(),
            )
            .await
        {
            Ok(Some(assignment)) => assignment_success(Some(assignment)),
            Ok(None) => assignment_error("No bound delegate assignment".to_string()),
            Err(error) => assignment_error(error.to_string()),
        },
    )
}

pub(crate) async fn trusted_delegate_identity(
    state: &HttpServerState,
    headers: &HeaderMap,
    operation: &str,
) -> Result<(DelegatedSessionId, AgentRunId, ChatConversationId), String> {
    let conversation_id = header_value(headers, "x-ralphx-conversation-id", operation)?;
    let run_id = header_value(headers, "x-ralphx-agent-run-id", operation)?;
    let conversation_id = conversation_id
        .parse::<ChatConversationId>()
        .map_err(|_| format!("{operation} received an invalid trusted conversation id"))?;
    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| {
            format!("{operation} could not validate the trusted conversation: {error}")
        })?
        .ok_or_else(|| format!("{operation} trusted conversation was not found"))?;
    if conversation.context_type != ChatContextType::Delegation {
        return Err(format!(
            "{operation} is available only from a delegated conversation"
        ));
    }
    let delegated_session_id = DelegatedSessionId::from_string(conversation.context_id);
    let run_id = AgentRunId::from_string(run_id);
    let run = state
        .app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .map_err(|error| format!("{operation} could not validate the trusted run: {error}"))?
        .ok_or_else(|| format!("{operation} trusted run was not found"))?;
    if run.conversation_id != conversation_id {
        return Err(format!(
            "{operation} trusted run does not belong to the delegated conversation"
        ));
    }
    let active_run = state
        .app_state
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await
        .map_err(|error| format!("{operation} could not resolve the active run: {error}"))?;
    if active_run.as_ref().map(|active| &active.id) != Some(&run_id) {
        return Err(format!(
            "{operation} trusted run is stale or no longer active"
        ));
    }
    let session = state
        .app_state
        .delegated_session_repo
        .get_by_id(&delegated_session_id)
        .await
        .map_err(|error| format!("{operation} could not validate the delegated session: {error}"))?
        .ok_or_else(|| format!("{operation} delegated session was not found"))?;
    if session.status != "running" {
        return Err(format!(
            "{operation} delegated session is not currently running"
        ));
    }
    Ok((delegated_session_id, run_id, conversation_id))
}

pub(crate) fn header_value(
    headers: &HeaderMap,
    header_name: &str,
    operation: &str,
) -> Result<String, String> {
    headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{operation} requires trusted runtime identity"))
}

#[doc(hidden)]
pub fn resolve_scope(context: &AgentTaskContextFields) -> Result<AgentTaskScope, String> {
    let project_id = context.project_id.clone().map(ProjectId::from_string);
    if let (Some(scope_type), Some(scope_id)) = (&context.context_type, &context.context_id) {
        if scope_type.trim().is_empty() || scope_id.trim().is_empty() {
            return Err("Agent task context_type and context_id must be non-empty".to_string());
        }
        return Ok(AgentTaskScope {
            project_id,
            scope_type: scope_type.clone(),
            scope_id: scope_id.clone(),
            actor_agent: context.actor_agent.clone(),
        });
    }
    Err(
        "Agent task context_type and context_id are required; explicit project scope must use context_type=project"
            .to_string(),
    )
}

fn mutation_success(result: AgentTaskMutationResult) -> AgentTaskMutationResponse {
    AgentTaskMutationResponse {
        success: true,
        task: Some(task_dto(result.task)),
        changed_fields: result.changed_fields,
        state_change: result.state_change.map(|change| AgentTaskStateChangeDto {
            from: change.from.as_str().to_string(),
            to: change.to.as_str().to_string(),
        }),
        error: None,
    }
}

fn mutation_error(error: String) -> AgentTaskMutationResponse {
    AgentTaskMutationResponse {
        success: false,
        task: None,
        changed_fields: Vec::new(),
        state_change: None,
        error: Some(error),
    }
}

fn get_error(error: String) -> AgentTaskGetResponse {
    AgentTaskGetResponse {
        success: false,
        task: None,
        error: Some(error),
    }
}

fn list_error(error: String) -> AgentTaskListResponse {
    AgentTaskListResponse {
        success: false,
        tasks: Vec::new(),
        error: Some(error),
    }
}

fn lists_error(error: String) -> AgentTaskListsResponse {
    AgentTaskListsResponse {
        success: false,
        lists: Vec::new(),
        error: Some(error),
    }
}

fn assignment_success(assignment: Option<AgentTaskAssignmentView>) -> DelegateAssignmentResponse {
    DelegateAssignmentResponse {
        success: true,
        assignment: assignment.map(assignment_dto),
        error: None,
    }
}

fn assignment_error(error: String) -> DelegateAssignmentResponse {
    DelegateAssignmentResponse {
        success: false,
        assignment: None,
        error: Some(error),
    }
}

fn assignment_dto(assignment: AgentTaskAssignmentView) -> DelegateAssignmentDto {
    DelegateAssignmentDto {
        task_number: assignment.task.task_number,
        title: assignment.task.title,
        details: assignment.task.details,
        task_state: assignment.task.state.as_str().to_string(),
        assignment_state: assignment.assignment.state.as_str().to_string(),
        delegate_agent_name: assignment.assignment.delegate_agent_name,
        caller_scope_type: assignment.caller_scope_type,
    }
}

fn task_dto(task: AgentTaskDetail) -> AgentTaskDto {
    let availability = task.availability().to_string();
    AgentTaskDto {
        task_id: task.task_id.to_string(),
        task_number: task.task_number,
        title: task.title,
        details: task.details,
        active_label: task.active_label,
        owner_agent: task.owner_agent,
        state: task.state.as_str().to_string(),
        metadata: task.metadata,
        blocked_by: task.blocked_by,
        unresolved_blocked_by: task.unresolved_blocked_by,
        blocks: task.blocks,
        availability,
        version: task.version,
        created_at: task.created_at.to_rfc3339(),
        updated_at: task.updated_at.to_rfc3339(),
        completed_at: task.completed_at.map(|dt| dt.to_rfc3339()),
    }
}

fn summary_dto(task: AgentTaskSummary) -> AgentTaskSummaryDto {
    AgentTaskSummaryDto {
        task_id: task.task_id.to_string(),
        task_number: task.task_number,
        title: task.title,
        state: task.state.as_str().to_string(),
        owner_agent: task.owner_agent,
        blocked_by: task.blocked_by,
        blocks: task.blocks,
        availability: task.availability,
        updated_at: task.updated_at.to_rfc3339(),
    }
}

fn list_summary_dto(list: AgentTaskListSummary) -> AgentTaskListSummaryDto {
    AgentTaskListSummaryDto {
        list_id: list.list_id.to_string(),
        list_sequence: list.list_sequence,
        task_count: list.task_count,
        open_count: list.open_count,
        active_count: list.active_count,
        done_count: list.done_count,
        dropped_count: list.dropped_count,
        created_at: list.created_at.to_rfc3339(),
        updated_at: list.updated_at.to_rfc3339(),
    }
}
