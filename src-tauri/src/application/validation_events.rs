use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ralphx_events::{emit_serialized, EventSink};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::application::validation_service::ValidationCommandRequest;
use crate::application::AppState;
use crate::domain::entities::{
    ValidationCacheDecision, ValidationCommandCategory, ValidationCommandResult,
    ValidationCommandSource, ValidationRun,
};

const OUTPUT_EVENT_CHUNK_BYTES: usize = 8 * 1024;
pub const TASK_VALIDATION_EVENT: &str = "task_validation:event";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskValidationEventType {
    RunStarted,
    CommandStarted,
    CommandOutput,
    CommandCompleted,
    RunCompleted,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskValidationEventPayload {
    #[serde(rename = "type")]
    pub event_type: TaskValidationEventType,
    pub task_id: String,
    pub project_id: String,
    pub run_id: String,
    pub status: String,
    pub purpose: String,
    pub context_type: String,
    pub mode: String,
    pub policy_enabled: bool,
    pub run_started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_by_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_short_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_completed_at: Option<String>,
    pub emitted_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ValidationCommandEventContext {
    task_id: String,
    project_id: String,
    run_id: String,
    purpose: String,
    context_type: String,
    mode: String,
    policy_enabled: bool,
    run_started_at: String,
    requested_by_agent: Option<String>,
    head_sha: Option<String>,
    head_short_sha: Option<String>,
    base_ref: Option<String>,
    command_id: String,
    command_source: String,
    command_ref: Option<String>,
    command: String,
    cwd: String,
    label: Option<String>,
    category: String,
    reason: Option<String>,
    cache_decision: String,
    command_started_at: String,
}

impl TaskValidationEventPayload {
    pub(crate) fn run_started(run: &ValidationRun) -> Self {
        Self::from_run(run, TaskValidationEventType::RunStarted)
    }

    pub(crate) fn run_completed(run: &ValidationRun) -> Self {
        Self::from_run(run, TaskValidationEventType::RunCompleted)
    }

    pub(crate) fn command_started(context: &ValidationCommandEventContext) -> Self {
        Self::from_command_context(
            context,
            TaskValidationEventType::CommandStarted,
            "running".to_string(),
        )
    }

    pub(crate) fn command_output(
        context: &ValidationCommandEventContext,
        stream: &str,
        delta: String,
    ) -> Self {
        let mut payload = Self::from_command_context(
            context,
            TaskValidationEventType::CommandOutput,
            "running".to_string(),
        );
        payload.stream = Some(stream.to_string());
        if stream == "stderr" {
            payload.stderr_delta = Some(delta);
        } else {
            payload.stdout_delta = Some(delta);
        }
        payload
    }

    pub(crate) fn command_completed(run: &ValidationRun, result: &ValidationCommandResult) -> Self {
        Self {
            event_type: TaskValidationEventType::CommandCompleted,
            task_id: result.task_id.as_str().to_string(),
            project_id: result.project_id.as_str().to_string(),
            run_id: result.validation_run_id.clone(),
            status: result.status.as_str().to_string(),
            purpose: run.purpose.as_str().to_string(),
            context_type: run.context_type.as_str().to_string(),
            mode: run.mode.as_str().to_string(),
            policy_enabled: run.policy_enabled,
            run_started_at: run.started_at.to_rfc3339(),
            run_completed_at: run.completed_at.map(|dt| dt.to_rfc3339()),
            requested_by_agent: run.requested_by_agent.clone(),
            head_sha: run.head_sha.clone(),
            head_short_sha: run
                .head_sha
                .as_ref()
                .map(|sha| sha.chars().take(8).collect::<String>()),
            base_ref: run.base_ref.clone(),
            command_id: Some(result.id.clone()),
            command_source: Some(result.command_source.as_str().to_string()),
            command_ref: result.command_ref.clone(),
            command: Some(result.command.clone()),
            cwd: Some(result.cwd.clone()),
            label: result.label.clone(),
            category: Some(result.category.as_str().to_string()),
            reason: result.reason.clone(),
            cache_decision: Some(result.cache_decision.as_str().to_string()),
            exit_code: result.exit_code,
            duration_ms: result.duration_ms,
            stream: None,
            stdout_delta: None,
            stderr_delta: None,
            stdout_snippet: result.stdout_snippet.clone(),
            stderr_snippet: result.stderr_snippet.clone(),
            stdout_log_path: result.stdout_log_path.clone(),
            stderr_log_path: result.stderr_log_path.clone(),
            command_started_at: None,
            command_completed_at: Some(result.created_at.to_rfc3339()),
            emitted_at: Utc::now().to_rfc3339(),
        }
    }

    fn from_run(run: &ValidationRun, event_type: TaskValidationEventType) -> Self {
        Self {
            event_type,
            task_id: run.task_id.as_str().to_string(),
            project_id: run.project_id.as_str().to_string(),
            run_id: run.id.clone(),
            status: run.status.as_str().to_string(),
            purpose: run.purpose.as_str().to_string(),
            context_type: run.context_type.as_str().to_string(),
            mode: run.mode.as_str().to_string(),
            policy_enabled: run.policy_enabled,
            run_started_at: run.started_at.to_rfc3339(),
            run_completed_at: run.completed_at.map(|dt| dt.to_rfc3339()),
            requested_by_agent: run.requested_by_agent.clone(),
            head_sha: run.head_sha.clone(),
            head_short_sha: run
                .head_sha
                .as_ref()
                .map(|sha| sha.chars().take(8).collect::<String>()),
            base_ref: run.base_ref.clone(),
            command_id: None,
            command_source: None,
            command_ref: None,
            command: None,
            cwd: None,
            label: None,
            category: None,
            reason: None,
            cache_decision: None,
            exit_code: None,
            duration_ms: None,
            stream: None,
            stdout_delta: None,
            stderr_delta: None,
            stdout_snippet: None,
            stderr_snippet: None,
            stdout_log_path: None,
            stderr_log_path: None,
            command_started_at: None,
            command_completed_at: None,
            emitted_at: Utc::now().to_rfc3339(),
        }
    }

    fn from_command_context(
        context: &ValidationCommandEventContext,
        event_type: TaskValidationEventType,
        status: String,
    ) -> Self {
        Self {
            event_type,
            task_id: context.task_id.clone(),
            project_id: context.project_id.clone(),
            run_id: context.run_id.clone(),
            status,
            purpose: context.purpose.clone(),
            context_type: context.context_type.clone(),
            mode: context.mode.clone(),
            policy_enabled: context.policy_enabled,
            run_started_at: context.run_started_at.clone(),
            run_completed_at: None,
            requested_by_agent: context.requested_by_agent.clone(),
            head_sha: context.head_sha.clone(),
            head_short_sha: context.head_short_sha.clone(),
            base_ref: context.base_ref.clone(),
            command_id: Some(context.command_id.clone()),
            command_source: Some(context.command_source.clone()),
            command_ref: context.command_ref.clone(),
            command: Some(context.command.clone()),
            cwd: Some(context.cwd.clone()),
            label: context.label.clone(),
            category: Some(context.category.clone()),
            reason: context.reason.clone(),
            cache_decision: Some(context.cache_decision.clone()),
            exit_code: None,
            duration_ms: None,
            stream: None,
            stdout_delta: None,
            stderr_delta: None,
            stdout_snippet: None,
            stderr_snippet: None,
            stdout_log_path: None,
            stderr_log_path: None,
            command_started_at: Some(context.command_started_at.clone()),
            command_completed_at: None,
            emitted_at: Utc::now().to_rfc3339(),
        }
    }
}

impl ValidationCommandEventContext {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_request(
        run: &ValidationRun,
        command_id: &str,
        command_source: ValidationCommandSource,
        request: &ValidationCommandRequest,
        command: &str,
        cwd: &Path,
        category: ValidationCommandCategory,
        cache_decision: ValidationCacheDecision,
        command_started_at: DateTime<Utc>,
    ) -> Self {
        Self {
            task_id: run.task_id.as_str().to_string(),
            project_id: run.project_id.as_str().to_string(),
            run_id: run.id.clone(),
            purpose: run.purpose.as_str().to_string(),
            context_type: run.context_type.as_str().to_string(),
            mode: run.mode.as_str().to_string(),
            policy_enabled: run.policy_enabled,
            run_started_at: run.started_at.to_rfc3339(),
            requested_by_agent: run.requested_by_agent.clone(),
            head_sha: run.head_sha.clone(),
            head_short_sha: run
                .head_sha
                .as_ref()
                .map(|sha| sha.chars().take(8).collect::<String>()),
            base_ref: run.base_ref.clone(),
            command_id: command_id.to_string(),
            command_source: command_source.as_str().to_string(),
            command_ref: request.command_ref.clone(),
            command: command.to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            label: request.label.clone(),
            category: category.as_str().to_string(),
            reason: request.reason.clone(),
            cache_decision: cache_decision.as_str().to_string(),
            command_started_at: command_started_at.to_rfc3339(),
        }
    }
}

pub(crate) fn emit_task_validation_event(state: &AppState, payload: &TaskValidationEventPayload) {
    emit_task_validation_event_to_sink(state.events.as_ref(), payload);
}

pub(crate) fn emit_task_validation_event_to_sink(
    events: &dyn EventSink,
    payload: &TaskValidationEventPayload,
) {
    let _ = emit_serialized(events, TASK_VALIDATION_EVENT, payload);
}

pub(crate) async fn read_stream_with_events<R>(
    stream: Option<R>,
    events: Arc<dyn EventSink>,
    event_context: Option<ValidationCommandEventContext>,
    stream_name: &'static str,
) -> Vec<u8>
where
    R: AsyncRead + Unpin,
{
    let mut collected = Vec::new();
    let Some(mut reader) = stream else {
        return collected;
    };
    let mut chunk = vec![0; OUTPUT_EVENT_CHUNK_BYTES];

    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(read) => {
                collected.extend_from_slice(&chunk[..read]);
                if let Some(context) = event_context.as_ref() {
                    let delta = String::from_utf8_lossy(&chunk[..read]).to_string();
                    emit_task_validation_event_to_sink(
                        events.as_ref(),
                        &TaskValidationEventPayload::command_output(context, stream_name, delta),
                    );
                }
            }
            Err(error) => {
                tracing::warn!(%error, stream = stream_name, "Failed to read validation command output");
                break;
            }
        }
    }

    collected
}
