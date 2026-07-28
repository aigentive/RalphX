//! Caller-bound coordinator member tools for RX-native Teams.
//!
//! Team/member/run identifiers are never accepted in model-facing payloads.
//! The transport-owned conversation and run headers resolve a coordinator's
//! durable Team binding before a normalized member name is used for lookup.

use std::path::PathBuf;
use std::str::FromStr;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::Utc;

use crate::application::chat_service::ChatService;
use crate::application::managed_team::{
    ManagedTeamAssignmentRequest, ManagedTeamMemberSpec, ManagedTeamWorkspaceRequest,
};
use crate::application::native_delegation_launcher::{
    NativeDelegationLaunchParent, NativeDelegationLaunchRequest, NativeDelegationLauncher,
};
use crate::application::AgentTaskService;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{
    AgentRunId, AgentTaskAssignmentTerminalStatus, AgentTaskScope, ChatContextType,
    ChatConversationId, DelegatedSession, ProjectId, TeamRunBindingStatus, TeamWorkClassification,
};
use crate::http_server::handlers::coordination::{
    ensure_delegated_conversation, fail_started_delegated_launch,
};
use crate::http_server::types::{
    AddManagedTeamMemberRequest, AssignManagedTeamMemberRequest, HttpServerState,
    ManagedTeamAssignmentResponse, ManagedTeamMemberSummary, StopManagedTeamMemberRequest,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

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

fn enum_wire_value<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn member_summary(member: &crate::domain::entities::TeamMember) -> ManagedTeamMemberSummary {
    ManagedTeamMemberSummary {
        id: member.id.as_str().to_string(),
        team_id: member.team_id.as_str().to_string(),
        name: member.name.clone(),
        normalized_name: member.normalized_name.clone(),
        canonical_agent_name: member.canonical_agent_name.clone(),
        role_summary: member.role_summary.clone(),
        status: enum_wire_value(&member.status),
        generation: member.generation,
    }
}

struct CoordinatorAuthority {
    team_id: crate::domain::entities::TeamSessionId,
    project_id: ProjectId,
    conversation_id: ChatConversationId,
    run_id: AgentRunId,
    caller_agent_name: String,
    working_directory: PathBuf,
}

/// Resolves model authority only from the transport identity and a member-null
/// coordinator binding. Read failures are terminal: there is no fallback to a
/// request body team/member/run id.
async fn resolve_coordinator_authority(
    state: &HttpServerState,
    headers: &HeaderMap,
) -> Result<CoordinatorAuthority, JsonError> {
    let enabled = state
        .app_state
        .managed_team
        .team_capability_enabled()
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read Team capability: {error}"),
            )
        })?;
    if !enabled {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "Team capability is disabled",
        ));
    }
    let conversation_id = headers
        .get("x-ralphx-conversation-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "Team tools require trusted caller conversation context",
            )
        })?;
    let run_id = headers
        .get("x-ralphx-agent-run-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "Team tools require trusted caller run context",
            )
        })?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let run_id = AgentRunId::from_string(run_id);
    let active_run = state
        .app_state
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load active coordinator run: {error}"),
            )
        })?
        .ok_or_else(|| {
            json_error(
                StatusCode::CONFLICT,
                "Trusted coordinator run is not active",
            )
        })?;
    if active_run.id != run_id {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Trusted coordinator run is not active",
        ));
    }
    let binding = state
        .app_state
        .managed_team
        .run_binding_repo()
        .get_by_agent_run_id(&run_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to resolve Team run binding: {error}"),
            )
        })?
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                "Trusted coordinator run has no Team binding",
            )
        })?;
    if binding.team_member_id.is_some()
        || binding.conversation_id != conversation_id
        || !matches!(
            binding.status,
            TeamRunBindingStatus::Planned
                | TeamRunBindingStatus::Launching
                | TeamRunBindingStatus::Running
        )
    {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "Trusted run is not a current Team coordinator binding",
        ));
    }
    let session = state
        .app_state
        .managed_team
        .team_repo()
        .get_session(&binding.team_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load Team session: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Team session was not found"))?;
    if session.coordinator_conversation_id != conversation_id || session.status.is_closed() {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "Trusted run does not own an open Team",
        ));
    }
    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load coordinator conversation: {error}"),
            )
        })?
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                "Coordinator conversation was not found",
            )
        })?;
    let caller_agent_name = conversation.bound_agent_name.ok_or_else(|| {
        json_error(
            StatusCode::FORBIDDEN,
            "Coordinator conversation has no canonical agent identity",
        )
    })?;
    let project = state
        .app_state
        .project_repo
        .get_by_id(&session.project_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load Team project: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Team project was not found"))?;
    let working_directory = validate_absolute_non_root_path(
        &PathBuf::from(&project.working_directory),
        "Team project working directory",
    )
    .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(CoordinatorAuthority {
        team_id: session.id,
        project_id: session.project_id,
        conversation_id,
        run_id,
        caller_agent_name,
        working_directory,
    })
}

pub async fn add_managed_team_member(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<AddManagedTeamMemberRequest>,
) -> Result<Json<ManagedTeamMemberSummary>, JsonError> {
    let authority = resolve_coordinator_authority(&state, &headers).await?;
    let harness = request
        .harness
        .as_deref()
        .map(AgentHarnessKind::from_str)
        .transpose()
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error))?;
    let logical_effort = request
        .logical_effort
        .as_deref()
        .map(LogicalEffort::from_str)
        .transpose()
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error))?;
    let member = state
        .app_state
        .managed_team
        .add_member(
            &authority.team_id,
            ManagedTeamMemberSpec {
                name: request.name,
                canonical_agent_name: request.canonical_agent_name,
                role_summary: request.role_summary,
                harness,
                logical_model: request.logical_model,
                logical_effort,
            },
        )
        .await
        .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(member_summary(&member)))
}

pub async fn list_idle_managed_team_members(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ManagedTeamMemberSummary>>, JsonError> {
    let authority = resolve_coordinator_authority(&state, &headers).await?;
    let members = state
        .app_state
        .managed_team
        .idle_members(&authority.team_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(members.iter().map(member_summary).collect()))
}

pub async fn assign_managed_team_member(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<AssignManagedTeamMemberRequest>,
) -> Result<Json<ManagedTeamAssignmentResponse>, JsonError> {
    let authority = resolve_coordinator_authority(&state, &headers).await?;
    let work_classification = serde_json::from_value::<TeamWorkClassification>(
        serde_json::Value::String(request.work_classification),
    )
    .map_err(|_| json_error(StatusCode::BAD_REQUEST, "Invalid Team work classification"))?;
    if work_classification == TeamWorkClassification::CoordinationOnly {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Member assignments cannot be coordination_only",
        ));
    }
    let member = state
        .app_state
        .managed_team
        .member_by_normalized_name(&authority.team_id, &request.member_name)
        .await
        .map_err(|error| json_error(StatusCode::NOT_FOUND, error.to_string()))?;
    let harness = member.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let mut delegated = DelegatedSession::new(
        authority.project_id.clone(),
        ChatContextType::Project.to_string(),
        authority.project_id.as_str(),
        member.canonical_agent_name.clone(),
        harness,
    );
    delegated.status = "pending".to_string();
    delegated.parent_message_id = None;
    delegated.title = Some(format!("Team assignment: {}", member.name));
    let delegated = state
        .app_state
        .delegated_session_repo
        .create(delegated)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create Team delegated session: {error}"),
            )
        })?;
    let delegated_conversation = ensure_delegated_conversation(
        &state,
        delegated.id.as_str(),
        Some(&authority.conversation_id.as_str()),
        delegated.title.as_deref(),
    )
    .await?;
    let task_service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    let mut scope = AgentTaskScope::new("conversation", authority.conversation_id.as_str());
    scope.project_id = Some(authority.project_id.clone());
    scope.actor_agent = Some(authority.caller_agent_name.clone());
    let workspace = (!request.writable_paths.is_empty()
        || !request.generated_outputs.is_empty()
        || !request.resource_locks.is_empty())
    .then_some(ManagedTeamWorkspaceRequest {
        writable_paths: request.writable_paths,
        generated_outputs: request.generated_outputs,
        resource_locks: request.resource_locks,
    });
    let plan = match state
        .app_state
        .managed_team
        .plan_member_assignment(
            &task_service,
            ManagedTeamAssignmentRequest {
                team_id: authority.team_id.clone(),
                member_name: request.member_name,
                expected_member_generation: member.generation,
                caller_scope: scope,
                caller_agent_run_id: authority.run_id,
                task_ref: request.task_ref,
                delegated_session_id: delegated.id.clone(),
                delegated_conversation_id: delegated_conversation.id,
                planned_agent_run_id: AgentRunId::new(),
                work_classification,
                workspace,
            },
        )
        .await
    {
        Ok(plan) => plan,
        Err(error) => {
            let _ = state
                .app_state
                .delegated_session_repo
                .update_status(
                    &delegated.id,
                    "failed",
                    Some("team_assignment_planning_failed".to_string()),
                    Some(Utc::now()),
                )
                .await;
            return Err(json_error(StatusCode::CONFLICT, error.to_string()));
        }
    };
    let launching = match state
        .app_state
        .managed_team
        .mark_member_assignment_launching(&plan)
        .await
    {
        Ok(binding) => binding,
        Err(error) => {
            state
                .app_state
                .managed_team
                .fail_member_assignment_launch(&task_service, &plan, "stale_before_launch")
                .await;
            return Err(json_error(StatusCode::CONFLICT, error.to_string()));
        }
    };
    let launch = NativeDelegationLauncher::new(&state)
        .launch(NativeDelegationLaunchRequest {
            caller_agent_name: authority.caller_agent_name,
            caller_agent_profile: Some("team_coordinator".to_string()),
            parent: NativeDelegationLaunchParent {
                context_type: ChatContextType::Project,
                context_id: authority.project_id.as_str().to_string(),
                project_id: authority.project_id.as_str().to_string(),
                working_directory: authority.working_directory,
                caller_conversation_id: Some(authority.conversation_id.as_str()),
                parent_conversation_id: Some(authority.conversation_id.as_str()),
                ideation_verification: false,
            },
            caller_agent_run_id: Some(authority.run_id.as_str()),
            target_agent_name: plan.member.canonical_agent_name.clone(),
            reusable_delegated_session: Some(delegated.clone()),
            task_ref: None,
            preallocated_agent_run_id: Some(launching.agent_run_id),
            prompt: plan.assignment.task.details.clone(),
            title: delegated.title.clone(),
            parent_turn_id: None,
            parent_message_id: None,
            parent_tool_use_id: None,
            harness: plan.member.harness,
            model: plan.member.logical_model.clone(),
            logical_effort: plan.member.logical_effort,
            approval_policy: None,
            sandbox_mode: None,
        })
        .await;
    let launch = match launch {
        Ok(value) => value,
        Err((status, body)) => {
            state
                .app_state
                .managed_team
                .fail_member_assignment_launch(&task_service, &plan, "native_launch_failed")
                .await;
            let _ = state
                .app_state
                .delegated_session_repo
                .update_status(
                    &delegated.id,
                    "failed",
                    Some("native_launch_failed".to_string()),
                    Some(Utc::now()),
                )
                .await;
            return Err((status, body));
        }
    };
    if launch.delegated_agent_run_id != launching.agent_run_id.as_str() {
        let error = "Team member launch did not use its planned run identity";
        let chat_service = state
            .app_state
            .build_chat_service_with_execution_state(state.execution_state.clone());
        let _ = fail_started_delegated_launch(
            &state,
            &chat_service,
            delegated.id.as_str(),
            &launch.delegated_agent_run_id,
            error,
        )
        .await;
        state
            .app_state
            .managed_team
            .fail_member_assignment_launch(&task_service, &plan, error)
            .await;
        return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, error));
    }
    let assignment = match state
        .app_state
        .managed_team
        .complete_member_assignment_launch(&task_service, &plan)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            let chat_service = state
                .app_state
                .build_chat_service_with_execution_state(state.execution_state.clone());
            let message = format!("Team member assignment binding failed: {error}");
            let _ = fail_started_delegated_launch(
                &state,
                &chat_service,
                delegated.id.as_str(),
                &launch.delegated_agent_run_id,
                &message,
            )
            .await;
            state
                .app_state
                .managed_team
                .fail_member_assignment_launch(&task_service, &plan, &message)
                .await;
            return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, message));
        }
    };
    Ok(Json(ManagedTeamAssignmentResponse {
        assignment_id: assignment.assignment.id.as_str().to_string(),
        agent_run_id: launch.delegated_agent_run_id,
        member: member_summary(&plan.member),
    }))
}

pub async fn stop_managed_team_member(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<StopManagedTeamMemberRequest>,
) -> Result<Json<ManagedTeamMemberSummary>, JsonError> {
    let authority = resolve_coordinator_authority(&state, &headers).await?;
    let member = state
        .app_state
        .managed_team
        .member_by_normalized_name(&authority.team_id, &request.member_name)
        .await
        .map_err(|error| json_error(StatusCode::NOT_FOUND, error.to_string()))?;
    if let (Some(session_id), Some(run_id)) = (
        member.delegated_session_id.as_ref(),
        member.current_run_id.as_ref(),
    ) {
        let chat_service = state
            .app_state
            .build_chat_service_with_execution_state(state.execution_state.clone());
        chat_service
            .stop_agent(ChatContextType::Delegation, session_id.as_str())
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to stop Team member process: {error}"),
                )
            })?;
        let task_service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
        if let Some(assignment) = task_service
            .get_assignment_for_run(run_id)
            .await
            .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        {
            task_service
                .settle_assignment_for_run(
                    run_id,
                    AgentTaskAssignmentTerminalStatus::Cancelled,
                    Some("team_member_stopped"),
                )
                .await
                .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string()))?;
            state
                .app_state
                .managed_team
                .recover_orphaned_member_assignment(&assignment, "team_member_stopped")
                .await
                .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string()))?;
        }
    }
    let stopped = state
        .app_state
        .managed_team
        .stop_member(&authority.team_id, &request.member_name, member.generation)
        .await
        .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(member_summary(&stopped)))
}
