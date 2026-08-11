use std::sync::Arc;

use axum::{http::StatusCode, Json};

use crate::application::agent_lane_resolution::{
    resolve_manual_role_spawn_settings, routing_role_for_delegated_launch,
};
use crate::application::chat_service::{ChatService, SendMessageOptions, SendQueuePolicy};
use crate::application::harness_runtime_registry::resolve_harness_plugin_dir;
use crate::application::AgentTaskService;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentTaskAssignmentView, AgentTaskScope, ChatContextType,
    DelegatedSession, DelegatedSessionId, ProjectId,
};
use crate::http_server::handlers::coordination::{
    build_delegated_prompt, ensure_delegated_conversation, fail_started_delegated_launch,
    mark_delegated_launch_failed, resolve_delegation_policy,
};
use crate::http_server::types::HttpServerState;
use crate::infrastructure::agents::harness_agent_catalog::resolve_project_root_from_plugin_dir;
use tracing::warn;

pub(crate) type NativeDelegationLaunchError = (StatusCode, Json<serde_json::Value>);

fn json_error(status: StatusCode, error: impl Into<String>) -> NativeDelegationLaunchError {
    (
        status,
        Json(serde_json::json!({
            "status": status.as_u16(),
            "error": error.into(),
        })),
    )
}

fn json_error_detail(error: &NativeDelegationLaunchError) -> String {
    error
        .1
         .0
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Delegated launch setup failed")
        .to_string()
}

/// Trusted parent authority resolved by the caller before a delegated run is launched.
#[derive(Debug, Clone)]
pub struct NativeDelegationLaunchParent {
    pub context_type: ChatContextType,
    pub context_id: String,
    pub project_id: String,
    pub working_directory: std::path::PathBuf,
    pub caller_conversation_id: Option<String>,
    /// Conversation whose agent workspace anchored `working_directory`. Differs from
    /// `caller_conversation_id` when a child runtime delegates from a descendant conversation.
    pub workspace_anchor_conversation_id: Option<String>,
    pub parent_conversation_id: Option<String>,
    pub ideation_verification: bool,
}

/// Application-layer launch inputs. Callers provide only trusted, backend-resolved authority.
#[derive(Debug, Clone)]
pub struct NativeDelegationLaunchRequest {
    pub caller_agent_name: String,
    pub caller_agent_profile: Option<String>,
    pub parent: NativeDelegationLaunchParent,
    pub inherit_context: bool,
    /// Allocated by the native delegation entrypoint before any durable session write.
    pub job_id: Option<String>,
    pub caller_agent_run_id: Option<String>,
    pub target_agent_name: String,
    pub reusable_delegated_session: Option<DelegatedSession>,
    pub task_ref: Option<String>,
    pub preallocated_agent_run_id: Option<AgentRunId>,
    pub prompt: String,
    pub title: Option<String>,
    pub parent_turn_id: Option<String>,
    pub parent_message_id: Option<String>,
    pub parent_tool_use_id: Option<String>,
    pub harness: Option<AgentHarnessKind>,
    pub model: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

/// Durable delegated-session/run outcome, deliberately independent of HTTP job snapshots.
#[derive(Debug, Clone)]
pub struct NativeDelegationLaunchResult {
    pub parent: NativeDelegationLaunchParent,
    pub caller_agent_run_id: Option<String>,
    pub delegated_session_id: String,
    pub delegated_conversation_id: String,
    pub delegated_agent_run_id: String,
    pub agent_name: String,
    pub harness: AgentHarnessKind,
    pub assignment: Option<AgentTaskAssignmentView>,
    pub launched_run: Option<AgentRun>,
    pub logical_model: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

pub struct NativeDelegationLauncher<'a> {
    state: &'a HttpServerState,
}

fn resolve_caller_agent_task_scope(
    parent: &NativeDelegationLaunchParent,
    actor_agent: &str,
) -> AgentTaskScope {
    let mut scope = if parent.context_type == ChatContextType::Delegation {
        AgentTaskScope::new("delegation", parent.context_id.clone())
    } else if let Some(caller_conversation_id) = &parent.caller_conversation_id {
        AgentTaskScope::new("conversation", caller_conversation_id.clone())
    } else {
        AgentTaskScope::new(parent.context_type.to_string(), parent.context_id.clone())
    };
    scope.project_id = Some(ProjectId::from_string(parent.project_id.clone()));
    scope.actor_agent = Some(actor_agent.to_string());
    scope
}

impl<'a> NativeDelegationLauncher<'a> {
    pub fn new(state: &'a HttpServerState) -> Self {
        Self { state }
    }

    pub async fn launch(
        &self,
        req: NativeDelegationLaunchRequest,
    ) -> Result<NativeDelegationLaunchResult, NativeDelegationLaunchError> {
        let state = self.state;
        let parent = &req.parent;
        let caller_agent_name = req.caller_agent_name.as_str();
        let parent_agent_run_id = req.caller_agent_run_id.clone();
        let requested_session = req.reusable_delegated_session.clone();
        let requested_harness = requested_session
            .as_ref()
            .map(|session| session.harness)
            .or(req.harness);
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
            &req.target_agent_name,
            parent.context_type,
            parent.ideation_verification,
        );
        let resolved_spawn = resolve_manual_role_spawn_settings(
            &req.target_agent_name,
            Some(parent.project_id.as_str()),
            Some(std::path::Path::new(&project.working_directory)),
            role,
            None,
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
            &req.target_agent_name,
        )?;
        let delegated_session_id = if let Some(session) = requested_session.as_ref() {
            session.id.as_str().to_string()
        } else {
            let mut session = DelegatedSession::new(
                ProjectId::from_string(parent.project_id.clone()),
                parent.context_type.to_string(),
                parent.context_id.clone(),
                req.target_agent_name.clone(),
                harness,
            );
            session.status = "pending".to_string();
            session.delegate_context_authorized = req.inherit_context;
            session.caller_conversation_id = parent.caller_conversation_id.clone();
            session.job_id = req.job_id.clone();
            session.parent_agent_run_id = parent_agent_run_id.clone();
            session.parent_turn_id = req.parent_turn_id.clone();
            session.parent_message_id = req.parent_message_id.clone();
            session.title = req.title.clone();
            state
                .app_state
                .delegated_session_repo
                .create(session)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to create delegated session: {error}"),
                    )
                })?
                .id
                .as_str()
                .to_string()
        };
        let delegated_session_entity =
            DelegatedSessionId::from_string(delegated_session_id.clone());
        if requested_session.is_some() {
            if let Some(job_id) = req.job_id.clone() {
                state
                    .app_state
                    .delegated_session_repo
                    .update_job_identity(
                        &delegated_session_entity,
                        job_id,
                        parent_agent_run_id.clone(),
                    )
                    .await
                    .map_err(|error| {
                        json_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to refresh delegated session job identity: {error}"),
                        )
                    })?;
            }
        }
        let assignment_service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
        let planned_agent_run_id = req
            .preallocated_agent_run_id
            .clone()
            .or_else(|| req.task_ref.as_ref().map(|_| AgentRunId::new()));
        let reserved_assignment = if let Some(task_ref) = req.task_ref.as_deref() {
            let Some(caller_run_id) = parent_agent_run_id.as_deref() else {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "delegate_start task_ref requires a trusted active caller run",
                ));
            };
            let caller_scope = resolve_caller_agent_task_scope(parent, caller_agent_name);
            match assignment_service
                .reserve_assignment(
                    &caller_scope,
                    task_ref,
                    &delegated_session_entity,
                    &AgentRunId::from_string(caller_run_id.to_string()),
                    &definition.name,
                )
                .await
            {
                Ok(Some(reservation)) => Some(reservation.assignment),
                Ok(None) => {
                    let message = format!(
                        "Agent task '{task_ref}' was not found in the caller's current ledger"
                    );
                    mark_delegated_launch_failed(state, &delegated_session_id, &message).await?;
                    return Err(json_error(StatusCode::NOT_FOUND, message));
                }
                Err(error) => {
                    let message = format!("Failed to reserve delegated agent task: {error}");
                    mark_delegated_launch_failed(state, &delegated_session_id, &message).await?;
                    return Err(json_error(StatusCode::CONFLICT, message));
                }
            }
        } else {
            None
        };
        if let Some(reserved) = reserved_assignment.as_ref() {
            let Some(planned_run_id) = planned_agent_run_id.as_ref() else {
                let message =
                    "Assigned delegated launch has no preallocated run identity".to_string();
                mark_delegated_launch_failed(state, &delegated_session_id, &message).await?;
                return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, message));
            };
            match assignment_service
                .plan_assignment_run(
                    &reserved.assignment.id,
                    &delegated_session_entity,
                    planned_run_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let message =
                        "Reserved delegate assignment disappeared before run planning".to_string();
                    mark_delegated_launch_failed(state, &delegated_session_id, &message).await?;
                    return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, message));
                }
                Err(error) => {
                    let message = format!("Failed to plan delegated assignment run: {error}");
                    mark_delegated_launch_failed(state, &delegated_session_id, &message).await?;
                    return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, message));
                }
            }
        }
        let logical_effort = req.logical_effort.or(resolved_spawn.logical_effort);
        let approval_policy = req
            .approval_policy
            .clone()
            .or(resolved_spawn.approval_policy.clone());
        let sandbox_mode = req
            .sandbox_mode
            .clone()
            .or(resolved_spawn.sandbox_mode.clone());
        if let Err(error) = state
            .app_state
            .delegated_session_repo
            .update_status(
                &DelegatedSessionId::from_string(delegated_session_id.clone()),
                "running",
                None,
                None,
            )
            .await
        {
            let message = format!("Failed to update delegated session status: {error}");
            mark_delegated_launch_failed(state, &delegated_session_id, &message).await?;
            return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, message));
        }

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
                    reserved_assignment.as_ref(),
                    &req.prompt,
                ),
                SendMessageOptions {
                    preallocated_agent_run_id: planned_agent_run_id,
                    queue_policy: if reserved_assignment.is_some() {
                        SendQueuePolicy::RequireImmediateStart
                    } else {
                        SendQueuePolicy::AllowQueue
                    },
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
        if let Some(planned_run_id) = planned_agent_run_id {
            if send_result.agent_run_id != planned_run_id.as_str() {
                let error_message =
                    "Delegated run did not use its preallocated assignment identity".to_string();
                fail_started_delegated_launch(
                    state,
                    &chat_service,
                    &delegated_session_id,
                    &send_result.agent_run_id,
                    &error_message,
                )
                .await?;
                return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, error_message));
            }
        }
        let bound_assignment = if let Some(reserved) = reserved_assignment.as_ref() {
            let planned_run_id = planned_agent_run_id.ok_or_else(|| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Assigned delegated launch lost its preallocated run identity",
                )
            })?;
            match assignment_service
                .bind_assignment_run(
                    &reserved.assignment.id,
                    &delegated_session_entity,
                    &planned_run_id,
                )
                .await
            {
                Ok(assignment) => assignment,
                Err(error) => {
                    let error_message = format!(
                        "Delegated run started but task assignment binding failed: {error}"
                    );
                    fail_started_delegated_launch(
                        state,
                        &chat_service,
                        &delegated_session_id,
                        &send_result.agent_run_id,
                        &error_message,
                    )
                    .await?;
                    return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, error_message));
                }
            }
        } else {
            None
        };
        if reserved_assignment.is_some() && bound_assignment.is_none() {
            let error_message =
                "Delegated run started but its reserved task assignment disappeared".to_string();
            fail_started_delegated_launch(
                state,
                &chat_service,
                &delegated_session_id,
                &send_result.agent_run_id,
                &error_message,
            )
            .await?;
            return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, error_message));
        }

        let launched_run = match state
            .app_state
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(send_result.agent_run_id.clone()))
            .await
        {
            Ok(run) => run,
            Err(error) => {
                warn!(
                    agent_run_id = send_result.agent_run_id,
                    %error,
                    "Delegated run started but effective runtime attribution could not be loaded"
                );
                None
            }
        };
        Ok(NativeDelegationLaunchResult {
            parent: parent.clone(),
            caller_agent_run_id: parent_agent_run_id,
            delegated_session_id,
            delegated_conversation_id: delegated_conversation.id.as_str(),
            delegated_agent_run_id: send_result.agent_run_id,
            agent_name: definition.name,
            harness,
            assignment: bound_assignment,
            launched_run,
            logical_model: delegated_model,
            logical_effort,
            approval_policy,
            sandbox_mode,
        })
    }
}
