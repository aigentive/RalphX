use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use ralphx_events::emit_serialized;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::coordination::{
    build_delegated_session_status_response, cancel_delegate_impl,
    start_delegate_impl_with_parent_run,
};
use crate::application::agent_workflow_runner::{AgentWorkflowHost, AgentWorkflowRunAuthority};
use crate::application::chat_service::ChatService;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    sha256_hex, AgentWorkflowInvocation, AgentWorkflowInvocationId, AgentWorkflowMeta,
    AgentWorkflowPhase, AgentWorkflowPhaseId, AgentWorkflowProgress, AgentWorkflowRun,
    AgentWorkflowRunId, AgentWorkflowRunStatus, AgentWorkflowScript, AgentWorkflowScriptId,
    AgentWorkflowStepStatus, ChatContextType, ChatConversation, ChatConversationId,
    CoordinationMode, ProjectId,
};
use crate::error::{AppError, AppResult};
use crate::http_server::types::{DelegateStartRequest, HttpServerState};

type JsonError = (StatusCode, Json<Value>);

const AGENT_WORKFLOW_PROGRESS_EVENT: &str = "agent:workflow_progress";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentWorkflowProgressEvent {
    run_id: String,
    emitted_at: chrono::DateTime<Utc>,
}

fn emit_workflow_progress(state: &HttpServerState, run_id: &AgentWorkflowRunId) {
    if let Err(error) = emit_serialized(
        state.app_state.events.as_ref(),
        AGENT_WORKFLOW_PROGRESS_EVENT,
        &AgentWorkflowProgressEvent {
            run_id: run_id.to_string(),
            emitted_at: Utc::now(),
        },
    ) {
        tracing::warn!(%run_id, %error, "Failed to emit Workflow progress invalidation");
    }
}

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

async fn require_live_workflow_conversation(
    state: &HttpServerState,
    script: &AgentWorkflowScript,
) -> Result<ChatConversation, JsonError> {
    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&script.conversation_id)
        .await
        .map_err(app_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Workflow conversation not found"))?;
    if !is_live_workflow_conversation(&conversation, script) {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Workflow approval and execution require the owning project conversation to remain in Workflow mode",
        ));
    }
    Ok(conversation)
}

fn is_live_workflow_conversation(
    conversation: &ChatConversation,
    script: &AgentWorkflowScript,
) -> bool {
    conversation.context_type == ChatContextType::Project
        && conversation.context_id == script.project_id.to_string()
        && conversation.coordination_mode == CoordinationMode::RxNativeWorkflow
}

pub(super) fn validate_workflow_agent_output(
    content: &str,
    schema: Option<&Value>,
) -> AppResult<Value> {
    let Some(schema) = schema else {
        return Ok(Value::String(content.to_string()));
    };
    let value: Value = serde_json::from_str(content).map_err(|error| {
        AppError::Validation(format!(
            "Workflow agent output must be JSON when a schema is declared: {error}"
        ))
    })?;
    validate_json_schema_value(&value, schema, "$")?;
    Ok(value)
}

fn validate_json_schema_value(value: &Value, schema: &Value, path: &str) -> AppResult<()> {
    if let Some(allowed) = schema.as_bool() {
        return if allowed {
            Ok(())
        } else {
            Err(AppError::Validation(format!(
                "Workflow agent output is forbidden by schema at {path}"
            )))
        };
    }
    let schema = schema.as_object().ok_or_else(|| {
        AppError::Validation("Workflow output schema must be an object or boolean".into())
    })?;
    if let Some(variants) = schema.get("enum").and_then(Value::as_array) {
        if !variants.contains(value) {
            return Err(AppError::Validation(format!(
                "Workflow agent output at {path} is not an allowed enum value"
            )));
        }
    }
    if let Some(expected) = schema.get("type") {
        let matches = match expected {
            Value::String(expected) => json_value_matches_type(value, expected),
            Value::Array(expected) => expected.iter().any(|expected| {
                expected
                    .as_str()
                    .is_some_and(|expected| json_value_matches_type(value, expected))
            }),
            _ => false,
        };
        if !matches {
            return Err(AppError::Validation(format!(
                "Workflow agent output at {path} does not match the declared type"
            )));
        }
    }
    if let Some(object) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for property in required {
                let property = property.as_str().ok_or_else(|| {
                    AppError::Validation(
                        "Workflow output schema required entries must be strings".into(),
                    )
                })?;
                let required_value = object.get(property).ok_or_else(|| {
                    AppError::Validation(format!(
                        "Workflow agent output is missing required value {path}.{property}"
                    ))
                })?;
                if required_value.is_null() {
                    return Err(AppError::Validation(format!(
                        "Workflow agent output required value {path}.{property} cannot be null"
                    )));
                }
            }
        }
        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            for property in object.keys() {
                if !properties.is_some_and(|properties| properties.contains_key(property)) {
                    return Err(AppError::Validation(format!(
                        "Workflow agent output contains undeclared value {path}.{property}"
                    )));
                }
            }
        }
        if let Some(properties) = properties {
            for (property, property_schema) in properties {
                if let Some(property_value) = object.get(property) {
                    validate_json_schema_value(
                        property_value,
                        property_schema,
                        &format!("{path}.{property}"),
                    )?;
                }
            }
        }
    }
    if let Some(array) = value.as_array() {
        if let Some(items) = schema.get("items") {
            for (index, item) in array.iter().enumerate() {
                validate_json_schema_value(item, items, &format!("{path}[{index}]"))?;
            }
        }
    }
    Ok(())
}

fn json_value_matches_type(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn workflow_delegate_prompt(prompt: &str, schema: Option<&Value>) -> AppResult<String> {
    let Some(schema) = schema else {
        return Ok(prompt.to_string());
    };
    if !schema.is_object() && !schema.is_boolean() {
        return Err(AppError::Validation(
            "Workflow output schema must be an object or boolean".into(),
        ));
    }
    Ok(format!(
        "{prompt}\n\nReturn only JSON matching this JSON Schema. Do not wrap it in Markdown fences:\n{schema}"
    ))
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
    pub launch_id: Option<String>,
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
pub struct WorkflowScriptRequest {
    pub script_id: String,
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
    let permission_summary = json!({
        "enforcement": "inherits_parent_agent_workspace",
        "directScriptOsAccess": false,
        "delegation": "canonical caller allowlist and global admission",
        "projectId": request.project_id,
    });
    let permission_summary_json = serde_json::to_string(&permission_summary)
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error))?;
    let invocation_ceiling = request.meta.max_invocations;
    let script = AgentWorkflowScript::new(
        conversation_id,
        ProjectId::from_string(request.project_id),
        request.script,
        request.meta,
        permission_summary_json,
        invocation_ceiling,
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
    let script_id = AgentWorkflowScriptId::from_string(request.script_id);
    let script = state
        .app_state
        .agent_workflow_repo
        .get_script(&script_id)
        .await
        .map_err(app_error)?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Workflow script not found"))?;
    require_live_workflow_conversation(&state, &script).await?;
    let approved = state
        .app_state
        .agent_workflow_repo
        .approve_script(&script_id, &request.script_hash, &request.permission_hash)
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
    let conversation = require_live_workflow_conversation(&state, &script).await?;
    let harness = request
        .harness
        .or(conversation.provider_harness)
        .ok_or_else(|| {
            json_error(
                StatusCode::CONFLICT,
                "Select a provider runtime before launching this Workflow",
            )
        })?;
    let run_id = request
        .launch_id
        .map(|launch_id| {
            uuid::Uuid::parse_str(&launch_id)
                .map(|id| AgentWorkflowRunId::from_string(id.to_string()))
                .map_err(|_| json_error(StatusCode::BAD_REQUEST, "Invalid Workflow launch id"))
        })
        .transpose()?
        .unwrap_or_else(AgentWorkflowRunId::new);
    let run = AgentWorkflowRun {
        id: run_id,
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
    emit_workflow_progress(&state, &run.id);
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

pub async fn get_latest_agent_workflow_run_for_script(
    State(state): State<HttpServerState>,
    Json(request): Json<WorkflowScriptRequest>,
) -> Result<Json<Option<AgentWorkflowRun>>, JsonError> {
    state
        .app_state
        .agent_workflow_repo
        .get_latest_run_for_script(&AgentWorkflowScriptId::from_string(request.script_id))
        .await
        .map(Json)
        .map_err(app_error)
}

pub async fn pause_agent_workflow_run(
    State(state): State<HttpServerState>,
    Json(request): Json<WorkflowRunRequest>,
) -> Result<Json<Value>, JsonError> {
    let run_id = AgentWorkflowRunId::from_string(request.run_id);
    let changed = state
        .app_state
        .agent_workflow_repo
        .request_pause(&run_id)
        .await
        .map_err(app_error)?;
    if changed {
        emit_workflow_progress(&state, &run_id);
    }
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
    require_live_workflow_conversation(&state, &script).await?;
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
    emit_workflow_progress(&state, &run_id);
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
    let run_id = AgentWorkflowRunId::from_string(request.run_id);
    let changed = state
        .app_state
        .agent_workflow_repo
        .request_cancel(&run_id)
        .await
        .map_err(app_error)?;
    if changed {
        emit_workflow_progress(&state, &run_id);
    }
    Ok(Json(json!({ "changed": changed })))
}

struct HttpWorkflowHost {
    state: HttpServerState,
    harness: AgentHarnessKind,
    caller_agent_name: String,
    caller_agent_profile: Option<String>,
    conversation_id: String,
    max_concurrency: u32,
    max_invocations: u32,
}

fn validate_run_script(run: &AgentWorkflowRun, script: &AgentWorkflowScript) -> AppResult<()> {
    if run.script_hash != script.script_hash || run.permission_hash != script.permission_hash {
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
    let repository = Arc::clone(&state.app_state.agent_workflow_repo);
    let event_state = state.clone();
    let host = Arc::new(HttpWorkflowHost {
        state: state.clone(),
        harness: run.harness,
        caller_agent_name,
        caller_agent_profile,
        conversation_id: script.conversation_id.to_string(),
        max_concurrency: u32::from(script.meta.max_concurrency.min(16)),
        max_invocations: script.meta.max_invocations.min(1_000),
    });
    tokio::spawn(async move {
        if let Err(error) = runner.execute(run.clone(), script, host).await {
            let _ = repository
                .fail_unclaimed_run(&run.id, run.status, &error.to_string())
                .await;
            tracing::error!(run_id = %run.id, %error, "Scripted Agent workflow failed");
        }
        emit_workflow_progress(&event_state, &run.id);
    });
    Ok(())
}

pub async fn recover_agent_workflow_runs(state: &HttpServerState) -> AppResult<usize> {
    let workflows_enabled = state.app_state.agent_capability_gate.workflows_enabled();
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
            emit_workflow_progress(state, &run.id);
        }
        if !workflows_enabled {
            if run.status != AgentWorkflowRunStatus::Paused
                && !state
                    .app_state
                    .agent_workflow_repo
                    .request_pause(&run.id)
                    .await?
            {
                tracing::warn!(run_id = %run.id, status = %run.status, "Disabled Workflow recovery could not settle run as paused");
            }
            emit_workflow_progress(state, &run.id);
            continue;
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
            emit_workflow_progress(state, &run.id);
            continue;
        };
        let conversation = state
            .app_state
            .chat_conversation_repo
            .get_by_id(&script.conversation_id)
            .await?;
        if !conversation
            .as_ref()
            .is_some_and(|conversation| is_live_workflow_conversation(conversation, &script))
        {
            let message = "Cannot recover Workflow run because its owning conversation is no longer in Workflow mode";
            if !state
                .app_state
                .agent_workflow_repo
                .fail_unclaimed_run(&run.id, run.status, message)
                .await?
            {
                tracing::warn!(run_id = %run.id, "Workflow recovery mode validation lost its state guard");
            }
            emit_workflow_progress(state, &run.id);
            continue;
        }
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
            emit_workflow_progress(state, &run.id);
            continue;
        }
        emit_workflow_progress(state, &run.id);
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
                self.emit_progress(authority);
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
    fn emit_progress(&self, authority: &AgentWorkflowRunAuthority) {
        emit_workflow_progress(
            &self.state,
            &AgentWorkflowRunId::from_string(authority.run_id.clone()),
        );
    }

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
        self.emit_progress(authority);
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
        let phase_key = payload
            .get("phaseKey")
            .and_then(Value::as_str)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "Workflow agent calls require an active phase for durable lineage".into(),
                )
            })?;
        let progress = self
            .state
            .app_state
            .agent_workflow_repo
            .get_progress(&AgentWorkflowRunId::from_string(authority.run_id.clone()))
            .await?;
        let phase_id = progress
            .phases
            .iter()
            .find(|phase| {
                phase.key == phase_key && phase.status == AgentWorkflowStepStatus::Running
            })
            .map(|phase| phase.id.clone())
            .ok_or_else(|| {
                AppError::Conflict(format!("Workflow agent phase is not active: {phase_key}"))
            })?;
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
        let schema = payload.get("schema").cloned();
        let schema_hash = schema
            .as_ref()
            .map(|schema| sha256_hex(schema.to_string().as_bytes()));
        let now = Utc::now();
        let invocation = AgentWorkflowInvocation {
            id: AgentWorkflowInvocationId::new(),
            run_id: AgentWorkflowRunId::from_string(authority.run_id.clone()),
            phase_id: Some(phase_id.clone()),
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
        self.emit_progress(authority);
        if stored.prompt_hash != sha256_hex(prompt.as_bytes())
            || stored.schema_hash != schema_hash
            || stored.agent_name != agent_name
            || stored.phase_id.as_ref() != Some(&phase_id)
        {
            return Err(AppError::Conflict(
                "Workflow replay key was reused with changed phase, agent, prompt, or schema"
                    .into(),
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
            return self
                .reconcile_active_invocation(authority, &stored, schema.as_ref())
                .await;
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
        let delegated_prompt = workflow_delegate_prompt(prompt, schema.as_ref())?;
        let snapshot = start_delegate_impl_with_parent_run(
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
                task_ref: None,
                agent_name: agent_name.into(),
                prompt: delegated_prompt,
                title: payload
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                inherit_context: true,
                harness: Some(self.harness),
                model: None,
                logical_effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
            Some(&self.conversation_id),
            Some(&authority.run_id),
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
            let _ = cancel_delegate_impl(&self.state, &snapshot.job_id).await;
            return Err(AppError::Conflict(
                "Stale Workflow invocation start rejected".into(),
            ));
        }
        self.emit_progress(authority);
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
                self.emit_progress(authority);
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
                    return self
                        .complete_invocation(
                            authority,
                            &stored,
                            current.content.unwrap_or_default(),
                            schema.as_ref(),
                            current.delegated_session_id,
                            current.delegated_conversation_id,
                        )
                        .await;
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
                    self.emit_progress(authority);
                    return Err(AppError::Agent(error));
                }
                _ => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }

    async fn complete_invocation(
        &self,
        authority: &AgentWorkflowRunAuthority,
        invocation: &AgentWorkflowInvocation,
        content: String,
        schema: Option<&Value>,
        delegated_session_id: String,
        child_conversation_id: Option<String>,
    ) -> AppResult<Value> {
        let content = match validate_workflow_agent_output(&content, schema) {
            Ok(content) => content,
            Err(error) => {
                if !self
                    .state
                    .app_state
                    .agent_workflow_repo
                    .settle_invocation(
                        invocation.id.as_str(),
                        authority.attempt,
                        &authority.runner_instance_id,
                        AgentWorkflowStepStatus::Failed,
                        Some(delegated_session_id.clone()),
                        child_conversation_id.clone(),
                        None,
                        Some(error.to_string()),
                    )
                    .await?
                {
                    return Err(AppError::Conflict(
                        "Stale Workflow schema failure rejected".into(),
                    ));
                }
                self.emit_progress(authority);
                return Err(error);
            }
        };
        let result = json!({
            "content": content,
            "delegatedSessionId": delegated_session_id.clone(),
            "conversationId": child_conversation_id.clone(),
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
                Some(delegated_session_id),
                child_conversation_id,
                Some(result.to_string()),
                None,
            )
            .await?
        {
            return Err(AppError::Conflict(
                "Stale Workflow completion rejected".into(),
            ));
        }
        self.emit_progress(authority);
        Ok(result)
    }

    async fn reconcile_active_invocation(
        &self,
        authority: &AgentWorkflowRunAuthority,
        invocation: &AgentWorkflowInvocation,
        schema: Option<&Value>,
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
                self.emit_progress(authority);
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
                None,
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
                    return self
                        .complete_invocation(
                            authority,
                            invocation,
                            content,
                            schema,
                            delegated_session_id.to_string(),
                            status.conversation_id,
                        )
                        .await;
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
                    self.emit_progress(authority);
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
