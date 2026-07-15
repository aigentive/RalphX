use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::coordination::{
    build_delegated_session_status_response, cancel_delegate_impl, start_delegate_impl,
};
use crate::application::agent_workflow_runner::{AgentWorkflowHost, AgentWorkflowRunAuthority};
use crate::application::chat_service::ChatService;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    sha256_hex, AgentWorkflowInvocation, AgentWorkflowInvocationId, AgentWorkflowMeta,
    AgentWorkflowPhase, AgentWorkflowPhaseId, AgentWorkflowProgress, AgentWorkflowRun,
    AgentWorkflowRunId, AgentWorkflowRunStatus, AgentWorkflowScript, AgentWorkflowScriptId,
    AgentWorkflowStepStatus, ChatContextType, ChatConversationId, CoordinationMode, ProjectId,
};
use crate::error::{AppError, AppResult};
use crate::http_server::types::{DelegateStartRequest, HttpServerState};

type JsonError = (StatusCode, Json<Value>);

fn json_error(status: StatusCode, error: impl ToString) -> JsonError {
    (
        status,
        Json(json!({ "status": status.as_u16(), "error": error.to_string() })),
    )
}

fn app_error(error: AppError) -> JsonError {
    let status = match error {
        AppError::NotFound(_) => StatusCode::NOT_FOUND,
        AppError::FeatureDisabled(_) => StatusCode::FORBIDDEN,
        AppError::Validation(_) | AppError::Conflict(_) => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    json_error(status, error)
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkflowScriptRequest {
    pub conversation_id: String,
    pub project_id: String,
    pub script: String,
    pub meta: AgentWorkflowMeta,
    pub permission_summary: Value,
    pub estimated_fanout: u32,
}

#[derive(Debug, Deserialize)]
pub struct ApproveWorkflowScriptRequest {
    pub script_id: String,
    pub script_hash: String,
    pub permission_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct StartWorkflowRunRequest {
    pub script_id: String,
    pub script_hash: String,
    pub permission_hash: String,
    #[serde(default)]
    pub args: Value,
    pub harness: Option<AgentHarnessKind>,
    pub caller_agent_name: Option<String>,
    pub caller_agent_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunRequest {
    pub run_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ResumeWorkflowRunRequest {
    pub run_id: String,
    pub caller_agent_name: Option<String>,
    pub caller_agent_profile: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct AgentWorkflowUsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub estimated_usd: f64,
}

#[derive(Debug, Serialize)]
pub struct AgentWorkflowProgressResponse {
    #[serde(flatten)]
    pub progress: AgentWorkflowProgress,
    pub usage: AgentWorkflowUsageSummary,
}

pub async fn create_agent_workflow_script(
    State(state): State<HttpServerState>,
    Json(request): Json<CreateWorkflowScriptRequest>,
) -> Result<Json<AgentWorkflowScript>, JsonError> {
    if !state.app_state.agent_capability_gate.workflows_enabled() {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "Workflows are disabled. Enable them in Settings > Capabilities.",
        ));
    }
    let conversation_id = ChatConversationId::from_string(request.conversation_id);
    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(app_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Conversation not found"))?;
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != request.project_id
        || conversation.coordination_mode != CoordinationMode::RxNativeWorkflow
    {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Workflow scripts require a Workflow-capability conversation in the same project",
        ));
    }
    let permission_summary_json = serde_json::to_string(&request.permission_summary)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error))?;
    let script = AgentWorkflowScript::new(
        conversation_id,
        ProjectId::from_string(request.project_id),
        request.script,
        request.meta,
        permission_summary_json,
        request.estimated_fanout,
    )
    .map_err(|error| json_error(StatusCode::BAD_REQUEST, error))?;
    state
        .app_state
        .agent_workflow_repo
        .save_script(script)
        .await
        .map(Json)
        .map_err(app_error)
}

pub async fn approve_agent_workflow_script(
    State(state): State<HttpServerState>,
    Json(request): Json<ApproveWorkflowScriptRequest>,
) -> Result<Json<Value>, JsonError> {
    if !state.app_state.agent_capability_gate.workflows_enabled() {
        return Err(json_error(StatusCode::FORBIDDEN, "Workflows are disabled"));
    }
    let approved = state
        .app_state
        .agent_workflow_repo
        .approve_script(
            &AgentWorkflowScriptId::from_string(request.script_id),
            &request.script_hash,
            &request.permission_hash,
        )
        .await
        .map_err(app_error)?;
    if !approved {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Workflow changed before approval; review the current script again",
        ));
    }
    Ok(Json(json!({ "approved": true })))
}

pub async fn start_agent_workflow_run(
    State(state): State<HttpServerState>,
    Json(request): Json<StartWorkflowRunRequest>,
) -> Result<Json<AgentWorkflowRun>, JsonError> {
    if !state.app_state.agent_capability_gate.workflows_enabled() {
        return Err(json_error(StatusCode::FORBIDDEN, "Workflows are disabled"));
    }
    let script_id = AgentWorkflowScriptId::from_string(request.script_id);
    let script = state
        .app_state
        .agent_workflow_repo
        .get_script(&script_id)
        .await
        .map_err(app_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Workflow script not found"))?;
    if script.script_hash != request.script_hash
        || script.permission_hash != request.permission_hash
        || !script.is_approved_for_current_content()
    {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Workflow launch requires approval for the exact current hashes",
        ));
    }
    let now = Utc::now();
    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&script.conversation_id)
        .await
        .map_err(app_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Workflow conversation not found"))?;
    let harness = request
        .harness
        .or(conversation.provider_harness)
        .ok_or_else(|| {
            json_error(
                StatusCode::CONFLICT,
                "Select a provider runtime before launching this Workflow",
            )
        })?;
    let run = AgentWorkflowRun {
        id: AgentWorkflowRunId::new(),
        script_id,
        conversation_id: script.conversation_id.clone(),
        project_id: script.project_id.clone(),
        harness,
        script_hash: script.script_hash.clone(),
        permission_hash: script.permission_hash.clone(),
        args_json: serde_json::to_string(&request.args)
            .map_err(|error| json_error(StatusCode::BAD_REQUEST, error))?,
        status: AgentWorkflowRunStatus::Queued,
        attempt: 0,
        runner_instance_id: None,
        lease_expires_at: None,
        heartbeat_at: None,
        pause_requested: false,
        cancel_requested: false,
        result_json: None,
        error: None,
        created_at: now,
        updated_at: now,
        completed_at: None,
    };
    let run = state
        .app_state
        .agent_workflow_repo
        .create_run(run)
        .await
        .map_err(app_error)?;
    spawn_workflow_run(
        &state,
        run.clone(),
        script,
        request
            .caller_agent_name
            .unwrap_or_else(|| "ralphx-general-worker".into()),
        request.caller_agent_profile,
    )
    .map_err(app_error)?;
    Ok(Json(run))
}

pub async fn get_agent_workflow_run(
    State(state): State<HttpServerState>,
    Json(request): Json<WorkflowRunRequest>,
) -> Result<Json<AgentWorkflowProgressResponse>, JsonError> {
    let progress = state
        .app_state
        .agent_workflow_repo
        .get_progress(&AgentWorkflowRunId::from_string(request.run_id))
        .await
        .map_err(app_error)?;
    let mut usage = AgentWorkflowUsageSummary::default();
    let mut seen_conversations = HashSet::new();
    for invocation in &progress.invocations {
        let Some(conversation_id) = invocation.child_conversation_id.as_ref() else {
            continue;
        };
        if !seen_conversations.insert(conversation_id.to_string()) {
            continue;
        }
        if let Some(run) = state
            .app_state
            .agent_run_repo
            .get_latest_for_conversation(conversation_id)
            .await
            .map_err(app_error)?
        {
            usage.input_tokens += run.input_tokens.unwrap_or(0);
            usage.output_tokens += run.output_tokens.unwrap_or(0);
            usage.cache_creation_tokens += run.cache_creation_tokens.unwrap_or(0);
            usage.cache_read_tokens += run.cache_read_tokens.unwrap_or(0);
            usage.estimated_usd += run.estimated_usd.unwrap_or(0.0);
        }
    }
    Ok(Json(AgentWorkflowProgressResponse { progress, usage }))
}

pub async fn pause_agent_workflow_run(
    State(state): State<HttpServerState>,
    Json(request): Json<WorkflowRunRequest>,
) -> Result<Json<Value>, JsonError> {
    let changed = state
        .app_state
        .agent_workflow_repo
        .request_pause(&AgentWorkflowRunId::from_string(request.run_id))
        .await
        .map_err(app_error)?;
    Ok(Json(json!({ "changed": changed })))
}

pub async fn resume_agent_workflow_run(
    State(state): State<HttpServerState>,
    Json(request): Json<ResumeWorkflowRunRequest>,
) -> Result<Json<AgentWorkflowRun>, JsonError> {
    if !state.app_state.agent_capability_gate.workflows_enabled() {
        return Err(json_error(StatusCode::FORBIDDEN, "Workflows are disabled"));
    }
    let run_id = AgentWorkflowRunId::from_string(request.run_id);
    let current = state
        .app_state
        .agent_workflow_repo
        .get_run(&run_id)
        .await
        .map_err(app_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Workflow run not found"))?;
    if current.status != AgentWorkflowRunStatus::Paused {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Only a paused Workflow run can be resumed",
        ));
    }
    let script = state
        .app_state
        .agent_workflow_repo
        .get_script(&current.script_id)
        .await
        .map_err(app_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Workflow script not found"))?;
    validate_run_script(&current, &script).map_err(app_error)?;
    if !state
        .app_state
        .agent_workflow_repo
        .resume_run(&run_id)
        .await
        .map_err(app_error)?
    {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Workflow run changed before it could be resumed",
        ));
    }
    let resumed = state
        .app_state
        .agent_workflow_repo
        .get_run(&run_id)
        .await
        .map_err(app_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Workflow run not found"))?;
    spawn_workflow_run(
        &state,
        resumed.clone(),
        script,
        request
            .caller_agent_name
            .unwrap_or_else(|| "ralphx-general-worker".into()),
        request.caller_agent_profile,
    )
    .map_err(app_error)?;
    Ok(Json(resumed))
}

pub async fn cancel_agent_workflow_run(
    State(state): State<HttpServerState>,
    Json(request): Json<WorkflowRunRequest>,
) -> Result<Json<Value>, JsonError> {
    let changed = state
        .app_state
        .agent_workflow_repo
        .request_cancel(&AgentWorkflowRunId::from_string(request.run_id))
        .await
        .map_err(app_error)?;
    Ok(Json(json!({ "changed": changed })))
}

struct HttpWorkflowHost {
    state: HttpServerState,
    caller_agent_name: String,
    caller_agent_profile: Option<String>,
    conversation_id: String,
    max_concurrency: u32,
    max_invocations: u32,
}

fn validate_run_script(run: &AgentWorkflowRun, script: &AgentWorkflowScript) -> AppResult<()> {
    if run.script_hash != script.script_hash
        || run.permission_hash != script.permission_hash
        || !script.is_approved_for_current_content()
    {
        return Err(AppError::Conflict(
            "Workflow run no longer matches an approved script".into(),
        ));
    }
    Ok(())
}

fn spawn_workflow_run(
    state: &HttpServerState,
    run: AgentWorkflowRun,
    script: AgentWorkflowScript,
    caller_agent_name: String,
    caller_agent_profile: Option<String>,
) -> AppResult<()> {
    validate_run_script(&run, &script)?;
    let runner = state.app_state.agent_workflow_runner()?;
    let host = Arc::new(HttpWorkflowHost {
        state: state.clone(),
        caller_agent_name,
        caller_agent_profile,
        conversation_id: script.conversation_id.to_string(),
        max_concurrency: u32::from(script.meta.max_concurrency.min(16)),
        max_invocations: script.meta.max_invocations.min(1_000),
    });
    tokio::spawn(async move {
        if let Err(error) = runner.execute(run.clone(), script, host).await {
            tracing::error!(run_id = %run.id, %error, "Scripted Agent workflow failed");
        }
    });
    Ok(())
}

pub async fn recover_agent_workflow_runs(state: &HttpServerState) -> AppResult<usize> {
    if !state.app_state.agent_capability_gate.workflows_enabled() {
        return Ok(0);
    }
    let mut launched = 0;
    for candidate in state
        .app_state
        .agent_workflow_repo
        .list_recoverable(Utc::now())
        .await?
    {
        let mut run = candidate;
        if matches!(
            run.status,
            AgentWorkflowRunStatus::Running | AgentWorkflowRunStatus::PauseRequested
        ) {
            if !state
                .app_state
                .agent_workflow_repo
                .prepare_recovery(&run.id, run.attempt, Utc::now())
                .await?
            {
                continue;
            }
            run = state
                .app_state
                .agent_workflow_repo
                .get_run(&run.id)
                .await?
                .ok_or_else(|| AppError::NotFound("Workflow run disappeared".into()))?;
        }
        if run.status == AgentWorkflowRunStatus::Paused {
            continue;
        }
        let Some(script) = state
            .app_state
            .agent_workflow_repo
            .get_script(&run.script_id)
            .await?
        else {
            let message = "Cannot recover Workflow run without its authoritative script";
            if !state
                .app_state
                .agent_workflow_repo
                .fail_unclaimed_run(&run.id, run.status, message)
                .await?
            {
                tracing::warn!(run_id = %run.id, "Workflow recovery failure lost its state guard");
            }
            continue;
        };
        if let Err(error) = spawn_workflow_run(
            state,
            run.clone(),
            script,
            "ralphx-general-worker".into(),
            None,
        ) {
            let message = format!("Failed to recover Workflow run: {error}");
            if !state
                .app_state
                .agent_workflow_repo
                .fail_unclaimed_run(&run.id, run.status, &message)
                .await?
            {
                tracing::warn!(run_id = %run.id, "Workflow recovery launch failure lost its state guard");
            }
            continue;
        }
        launched += 1;
    }
    Ok(launched)
}

#[async_trait]
impl AgentWorkflowHost for HttpWorkflowHost {
    async fn handle_call(
        &self,
        authority: &AgentWorkflowRunAuthority,
        operation: &str,
        payload: Value,
    ) -> AppResult<Value> {
        match operation {
            "log" => {
                let level = payload
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("info");
                let message = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AppError::Validation("Workflow log message is required".into())
                    })?;
                let entry = self
                    .state
                    .app_state
                    .agent_workflow_repo
                    .append_log(
                        &AgentWorkflowRunId::from_string(authority.run_id.clone()),
                        authority.attempt,
                        &authority.runner_instance_id,
                        level,
                        message,
                    )
                    .await?;
                if entry.is_none() {
                    return Err(AppError::Conflict("Stale workflow log rejected".into()));
                }
                Ok(Value::Null)
            }
            "phase" => self.handle_phase(authority, payload).await,
            "checkpoint" => Ok(payload.get("value").cloned().unwrap_or(Value::Null)),
            "agent" => self.handle_agent(authority, payload).await,
            "parallel" => self.handle_parallel(authority, payload).await,
            _ => Err(AppError::Validation(format!(
                "Unknown workflow host operation: {operation}"
            ))),
        }
    }
}

impl HttpWorkflowHost {
    async fn handle_parallel(
        &self,
        authority: &AgentWorkflowRunAuthority,
        payload: Value,
    ) -> AppResult<Value> {
        let items = payload
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AppError::Validation("Workflow parallel items must be an array".into())
            })?;
        if items.is_empty() {
            return Ok(Value::Array(Vec::new()));
        }
        if items.len() > self.max_concurrency as usize {
            return Err(AppError::ExecutionBlocked(format!(
                "Workflow parallel batch exceeds concurrency limit ({})",
                self.max_concurrency
            )));
        }
        let progress = self
            .state
            .app_state
            .agent_workflow_repo
            .get_progress(&AgentWorkflowRunId::from_string(authority.run_id.clone()))
            .await?;
        let active = progress
            .invocations
            .iter()
            .filter(|invocation| {
                matches!(
                    invocation.status,
                    AgentWorkflowStepStatus::Pending | AgentWorkflowStepStatus::Running
                )
            })
            .count();
        let existing_keys = progress
            .invocations
            .iter()
            .map(|invocation| invocation.logical_key.as_str())
            .collect::<HashSet<_>>();
        let mut batch_keys = HashSet::new();
        for item in items {
            let logical_key = item
                .get("logicalKey")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::Validation(
                        "Every parallel Workflow agent requires a logicalKey".into(),
                    )
                })?;
            if !batch_keys.insert(logical_key) {
                return Err(AppError::Validation(format!(
                    "Parallel Workflow logicalKey is duplicated: {logical_key}"
                )));
            }
        }
        let new_count = batch_keys
            .iter()
            .filter(|key| !existing_keys.contains(**key))
            .count();
        if progress.invocations.len() + new_count > self.max_invocations as usize {
            return Err(AppError::ExecutionBlocked(format!(
                "Workflow parallel batch would exceed invocation limit ({})",
                self.max_invocations
            )));
        }
        if active + new_count > self.max_concurrency as usize {
            return Err(AppError::ExecutionBlocked(format!(
                "Workflow parallel batch would exceed concurrency limit ({})",
                self.max_concurrency
            )));
        }
        let results = futures::future::join_all(
            items
                .iter()
                .cloned()
                .map(|item| self.handle_agent(authority, item)),
        )
        .await;
        results
            .into_iter()
            .collect::<AppResult<Vec<_>>>()
            .map(Value::Array)
    }

    async fn handle_phase(
        &self,
        authority: &AgentWorkflowRunAuthority,
        payload: Value,
    ) -> AppResult<Value> {
        let key = payload
            .get("key")
            .or_else(|| payload.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::Validation("Workflow phase key is required".into()))?;
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running");
        let status = AgentWorkflowStepStatus::from_str(status).map_err(AppError::Validation)?;
        let now = Utc::now();
        let phase = AgentWorkflowPhase {
            id: AgentWorkflowPhaseId::new(),
            run_id: AgentWorkflowRunId::from_string(authority.run_id.clone()),
            key: key.into(),
            name: payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(key)
                .into(),
            ordinal: payload.get("ordinal").and_then(Value::as_u64).unwrap_or(0) as u32,
            status,
            started_at: matches!(status, AgentWorkflowStepStatus::Running).then_some(now),
            completed_at: matches!(
                status,
                AgentWorkflowStepStatus::Completed
                    | AgentWorkflowStepStatus::Failed
                    | AgentWorkflowStepStatus::Cancelled
                    | AgentWorkflowStepStatus::Skipped
            )
            .then_some(now),
            error: payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        if !self
            .state
            .app_state
            .agent_workflow_repo
            .upsert_phase(phase, authority.attempt, &authority.runner_instance_id)
            .await?
        {
            return Err(AppError::Conflict("Stale workflow phase rejected".into()));
        }
        Ok(json!({ "key": key, "status": status.to_string() }))
    }

    async fn handle_agent(
        &self,
        authority: &AgentWorkflowRunAuthority,
        payload: Value,
    ) -> AppResult<Value> {
        let prompt = payload
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::Validation("Workflow agent prompt is required".into()))?;
        if prompt.len() > 100_000 {
            return Err(AppError::Validation(
                "Workflow agent prompt exceeds 100000 bytes".into(),
            ));
        }
        let agent_name = payload
            .get("agentName")
            .and_then(Value::as_str)
            .unwrap_or("ralphx-general-explorer");
        let logical_key = payload
            .get("logicalKey")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::Validation(
                    "Workflow agent logicalKey is required for replay safety".into(),
                )
            })?;
        let progress = self
            .state
            .app_state
            .agent_workflow_repo
            .get_progress(&AgentWorkflowRunId::from_string(authority.run_id.clone()))
            .await?;
        let replay = progress
            .invocations
            .iter()
            .any(|invocation| invocation.logical_key == logical_key);
        if !replay && progress.invocations.len() >= self.max_invocations as usize {
            return Err(AppError::ExecutionBlocked(format!(
                "Workflow invocation limit ({}) reached",
                self.max_invocations
            )));
        }
        let active_invocations = progress
            .invocations
            .iter()
            .filter(|invocation| {
                matches!(
                    invocation.status,
                    AgentWorkflowStepStatus::Pending | AgentWorkflowStepStatus::Running
                )
            })
            .count();
        if !replay && active_invocations >= self.max_concurrency as usize {
            return Err(AppError::ExecutionBlocked(format!(
                "Workflow concurrency limit ({}) reached",
                self.max_concurrency
            )));
        }
        let schema_hash = payload
            .get("schema")
            .map(|schema| sha256_hex(schema.to_string().as_bytes()));
        let now = Utc::now();
        let invocation = AgentWorkflowInvocation {
            id: AgentWorkflowInvocationId::new(),
            run_id: AgentWorkflowRunId::from_string(authority.run_id.clone()),
            phase_id: None,
            logical_key: logical_key.into(),
            agent_name: agent_name.into(),
            prompt_hash: sha256_hex(prompt.as_bytes()),
            schema_hash: schema_hash.clone(),
            status: AgentWorkflowStepStatus::Pending,
            delegated_session_id: None,
            child_conversation_id: None,
            result_json: None,
            error: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };
        let proposed_invocation_id = invocation.id.clone();
        let stored = self
            .state
            .app_state
            .agent_workflow_repo
            .begin_invocation(invocation)
            .await?;
        if stored.prompt_hash != sha256_hex(prompt.as_bytes()) || stored.schema_hash != schema_hash
        {
            return Err(AppError::Conflict(
                "Workflow replay key was reused with changed prompt or schema".into(),
            ));
        }
        if stored.status == AgentWorkflowStepStatus::Completed {
            let result = stored.result_json.ok_or_else(|| {
                AppError::Conflict("Completed workflow invocation has no result".into())
            })?;
            return serde_json::from_str(&result)
                .map_err(|error| AppError::Validation(error.to_string()));
        }
        if stored.status == AgentWorkflowStepStatus::Running {
            return self.reconcile_active_invocation(authority, &stored).await;
        }
        if stored.status == AgentWorkflowStepStatus::Pending && stored.id != proposed_invocation_id
        {
            return Err(AppError::Conflict(
                "Recovered Workflow invocation stopped before durable delegation lineage was recorded; refusing to start a duplicate agent"
                    .into(),
            ));
        }
        if stored.status != AgentWorkflowStepStatus::Pending {
            return Err(AppError::Conflict(
                "Workflow invocation is already active and could not be reconciled".into(),
            ));
        }
        let snapshot = start_delegate_impl(
            &self.state,
            DelegateStartRequest {
                caller_agent_name: Some(self.caller_agent_name.clone()),
                caller_agent_profile: self.caller_agent_profile.clone(),
                caller_context_type: Some("conversation".into()),
                caller_context_id: Some(self.conversation_id.clone()),
                parent_session_id: None,
                parent_turn_id: Some(authority.run_id.clone()),
                parent_message_id: None,
                parent_conversation_id: Some(self.conversation_id.clone()),
                parent_tool_use_id: Some(stored.id.to_string()),
                delegated_session_id: None,
                child_session_id: None,
                agent_name: agent_name.into(),
                prompt: prompt.into(),
                title: payload
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                inherit_context: true,
                harness: None,
                model: None,
                logical_effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .map_err(|error| {
            AppError::Agent(
                error.1["error"]
                    .as_str()
                    .unwrap_or("Failed to start workflow delegate")
                    .to_string(),
            )
        })?;
        if !self
            .state
            .app_state
            .agent_workflow_repo
            .settle_invocation(
                stored.id.as_str(),
                authority.attempt,
                &authority.runner_instance_id,
                AgentWorkflowStepStatus::Running,
                Some(snapshot.delegated_session_id.clone()),
                snapshot.delegated_conversation_id.clone(),
                None,
                None,
            )
            .await?
        {
            return Err(AppError::Conflict(
                "Stale Workflow invocation start rejected".into(),
            ));
        }
        let mut last_heartbeat = tokio::time::Instant::now();
        loop {
            let workflow_run = self
                .state
                .app_state
                .agent_workflow_repo
                .get_run(&AgentWorkflowRunId::from_string(authority.run_id.clone()))
                .await?
                .ok_or_else(|| AppError::NotFound("Workflow run disappeared".into()))?;
            if workflow_run.cancel_requested {
                let _ = cancel_delegate_impl(&self.state, &snapshot.job_id).await;
                if !self
                    .state
                    .app_state
                    .agent_workflow_repo
                    .settle_invocation(
                        stored.id.as_str(),
                        authority.attempt,
                        &authority.runner_instance_id,
                        AgentWorkflowStepStatus::Cancelled,
                        Some(snapshot.delegated_session_id.clone()),
                        snapshot.delegated_conversation_id.clone(),
                        None,
                        Some("Cancelled by user".into()),
                    )
                    .await?
                {
                    return Err(AppError::Conflict(
                        "Stale Workflow cancellation rejected".into(),
                    ));
                }
                return Err(AppError::ExecutionBlocked(
                    "Workflow cancelled by user".into(),
                ));
            }
            if last_heartbeat.elapsed() >= Duration::from_secs(10) {
                if !self
                    .state
                    .app_state
                    .agent_workflow_repo
                    .heartbeat(
                        &AgentWorkflowRunId::from_string(authority.run_id.clone()),
                        authority.attempt,
                        &authority.runner_instance_id,
                        Utc::now() + chrono::Duration::seconds(30),
                    )
                    .await?
                {
                    return Err(AppError::Conflict(
                        "Workflow host call lost runner authority".into(),
                    ));
                }
                last_heartbeat = tokio::time::Instant::now();
            }
            let current = self
                .state
                .delegation_service
                .snapshot(&snapshot.job_id)
                .await
                .ok_or_else(|| AppError::Conflict("Workflow delegated job disappeared".into()))?;
            match current.status.as_str() {
                "completed" => {
                    let result = json!({ "content": current.content.unwrap_or_default(),
                        "delegatedSessionId": current.delegated_session_id,
                        "conversationId": current.delegated_conversation_id });
                    if !self
                        .state
                        .app_state
                        .agent_workflow_repo
                        .settle_invocation(
                            stored.id.as_str(),
                            authority.attempt,
                            &authority.runner_instance_id,
                            AgentWorkflowStepStatus::Completed,
                            Some(current.delegated_session_id),
                            current.delegated_conversation_id,
                            Some(result.to_string()),
                            None,
                        )
                        .await?
                    {
                        return Err(AppError::Conflict(
                            "Stale Workflow completion rejected".into(),
                        ));
                    }
                    return Ok(result);
                }
                "failed" | "cancelled" => {
                    let error = current
                        .error
                        .unwrap_or_else(|| format!("Delegated workflow agent {}", current.status));
                    if !self
                        .state
                        .app_state
                        .agent_workflow_repo
                        .settle_invocation(
                            stored.id.as_str(),
                            authority.attempt,
                            &authority.runner_instance_id,
                            if current.status == "cancelled" {
                                AgentWorkflowStepStatus::Cancelled
                            } else {
                                AgentWorkflowStepStatus::Failed
                            },
                            Some(current.delegated_session_id),
                            current.delegated_conversation_id,
                            None,
                            Some(error.clone()),
                        )
                        .await?
                    {
                        return Err(AppError::Conflict("Stale Workflow failure rejected".into()));
                    }
                    return Err(AppError::Agent(error));
                }
                _ => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }

    async fn reconcile_active_invocation(
        &self,
        authority: &AgentWorkflowRunAuthority,
        invocation: &AgentWorkflowInvocation,
    ) -> AppResult<Value> {
        let delegated_session_id = invocation.delegated_session_id.as_ref().ok_or_else(|| {
            AppError::Conflict(
                "Active Workflow invocation is missing delegated-session lineage".into(),
            )
        })?;
        let mut last_heartbeat = tokio::time::Instant::now();
        loop {
            let workflow_run = self
                .state
                .app_state
                .agent_workflow_repo
                .get_run(&AgentWorkflowRunId::from_string(authority.run_id.clone()))
                .await?
                .ok_or_else(|| AppError::NotFound("Workflow run disappeared".into()))?;
            if workflow_run.cancel_requested {
                let chat_service = self
                    .state
                    .app_state
                    .build_chat_service_with_execution_state(Arc::clone(
                        &self.state.execution_state,
                    ));
                let _ = chat_service
                    .stop_agent(ChatContextType::Delegation, delegated_session_id.as_str())
                    .await;
                self.state
                    .app_state
                    .delegated_session_repo
                    .update_status(delegated_session_id, "cancelled", None, Some(Utc::now()))
                    .await?;
                if !self
                    .state
                    .app_state
                    .agent_workflow_repo
                    .settle_invocation(
                        invocation.id.as_str(),
                        authority.attempt,
                        &authority.runner_instance_id,
                        AgentWorkflowStepStatus::Cancelled,
                        Some(delegated_session_id.to_string()),
                        invocation
                            .child_conversation_id
                            .as_ref()
                            .map(ToString::to_string),
                        None,
                        Some("Cancelled by user".into()),
                    )
                    .await?
                {
                    return Err(AppError::Conflict(
                        "Stale recovered Workflow cancellation rejected".into(),
                    ));
                }
                return Err(AppError::ExecutionBlocked(
                    "Workflow cancelled by user".into(),
                ));
            }
            if last_heartbeat.elapsed() >= Duration::from_secs(10) {
                if !self
                    .state
                    .app_state
                    .agent_workflow_repo
                    .heartbeat(
                        &AgentWorkflowRunId::from_string(authority.run_id.clone()),
                        authority.attempt,
                        &authority.runner_instance_id,
                        Utc::now() + chrono::Duration::seconds(30),
                    )
                    .await?
                {
                    return Err(AppError::Conflict(
                        "Recovered Workflow host call lost runner authority".into(),
                    ));
                }
                last_heartbeat = tokio::time::Instant::now();
            }
            let status = build_delegated_session_status_response(
                &self.state,
                delegated_session_id.as_str(),
                true,
                Some(1),
            )
            .await
            .map_err(|error| {
                AppError::Conflict(
                    error.1["error"]
                        .as_str()
                        .unwrap_or("Failed to reconcile delegated Workflow session")
                        .to_string(),
                )
            })?;
            match status.session.status.as_str() {
                "completed" => {
                    let content = status
                        .recent_messages
                        .unwrap_or_default()
                        .into_iter()
                        .rev()
                        .find(|message| message.role == "assistant")
                        .map(|message| message.content)
                        .ok_or_else(|| {
                            AppError::Conflict(
                                "Completed delegated Workflow session has no verified result"
                                    .into(),
                            )
                        })?;
                    let result = json!({
                        "content": content,
                        "delegatedSessionId": delegated_session_id,
                        "conversationId": status.conversation_id,
                    });
                    if !self
                        .state
                        .app_state
                        .agent_workflow_repo
                        .settle_invocation(
                            invocation.id.as_str(),
                            authority.attempt,
                            &authority.runner_instance_id,
                            AgentWorkflowStepStatus::Completed,
                            Some(delegated_session_id.to_string()),
                            status.conversation_id,
                            Some(result.to_string()),
                            None,
                        )
                        .await?
                    {
                        return Err(AppError::Conflict(
                            "Stale recovered Workflow completion rejected".into(),
                        ));
                    }
                    return Ok(result);
                }
                "failed" | "cancelled" => {
                    let error = status
                        .latest_run
                        .and_then(|run| run.error_message)
                        .unwrap_or_else(|| {
                            format!("Delegated Workflow agent {}", status.session.status)
                        });
                    if !self
                        .state
                        .app_state
                        .agent_workflow_repo
                        .settle_invocation(
                            invocation.id.as_str(),
                            authority.attempt,
                            &authority.runner_instance_id,
                            if status.session.status == "cancelled" {
                                AgentWorkflowStepStatus::Cancelled
                            } else {
                                AgentWorkflowStepStatus::Failed
                            },
                            Some(delegated_session_id.to_string()),
                            status.conversation_id,
                            None,
                            Some(error.clone()),
                        )
                        .await?
                    {
                        return Err(AppError::Conflict(
                            "Stale recovered Workflow failure rejected".into(),
                        ));
                    }
                    return Err(AppError::Agent(error));
                }
                "running" => tokio::time::sleep(Duration::from_millis(200)).await,
                other => {
                    return Err(AppError::Conflict(format!(
                        "Delegated Workflow session is not reconcilable from state {other}"
                    )));
                }
            }
        }
    }
}
