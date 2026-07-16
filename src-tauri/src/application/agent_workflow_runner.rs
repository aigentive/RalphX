use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{ChildStdin, ChildStdout, Command};

use crate::application::agent_capability_gate::AgentCapabilityGate;
use crate::domain::entities::agent_workflow_protocol::{
    AgentWorkflowFrame, AgentWorkflowProtocolMessage, AGENT_WORKFLOW_MAX_FRAME_BYTES,
    AGENT_WORKFLOW_PROTOCOL_VERSION,
};
use crate::domain::entities::{AgentWorkflowRun, AgentWorkflowRunStatus, AgentWorkflowScript};
use crate::domain::repositories::AgentWorkflowRepository;
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

const RUNNER_LEASE: chrono::Duration = chrono::Duration::seconds(30);
const RUNNER_FRAME_TIMEOUT: Duration = Duration::from_secs(300);

#[async_trait]
pub trait AgentWorkflowHost: Send + Sync {
    async fn handle_call(
        &self,
        authority: &AgentWorkflowRunAuthority,
        operation: &str,
        payload: Value,
    ) -> AppResult<Value>;
}

#[async_trait]
pub(super) trait WorkflowChildTermination {
    async fn kill_child(&mut self);
    async fn reap_child(&mut self);
}

#[async_trait]
pub(super) trait WorkflowChildSession: WorkflowChildTermination {
    type Stdin: AsyncWrite + Unpin + Send;
    type Stdout: AsyncRead + Unpin + Send;

    fn take_stdin(&mut self) -> Option<Self::Stdin>;
    fn take_stdout(&mut self) -> Option<Self::Stdout>;
    async fn wait_success(&mut self) -> AppResult<bool>;
}

#[async_trait]
impl WorkflowChildTermination for tokio::process::Child {
    async fn kill_child(&mut self) {
        let _ = self.kill().await;
    }

    async fn reap_child(&mut self) {
        let _ = self.wait().await;
    }
}

#[async_trait]
impl WorkflowChildSession for tokio::process::Child {
    type Stdin = ChildStdin;
    type Stdout = ChildStdout;

    fn take_stdin(&mut self) -> Option<Self::Stdin> {
        self.stdin.take()
    }

    fn take_stdout(&mut self) -> Option<Self::Stdout> {
        self.stdout.take()
    }

    async fn wait_success(&mut self) -> AppResult<bool> {
        self.wait()
            .await
            .map(|status| status.success())
            .map_err(|error| AppError::Infrastructure(error.to_string()))
    }
}

pub(super) async fn kill_and_reap_after_drive_error(child: &mut impl WorkflowChildTermination) {
    child.kill_child().await;
    child.reap_child().await;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentWorkflowRunAuthority {
    pub run_id: String,
    pub attempt: u32,
    pub runner_instance_id: String,
}

pub struct AgentWorkflowRunner {
    repository: Arc<dyn AgentWorkflowRepository>,
    capability_gate: Arc<AgentCapabilityGate>,
    runner_path: PathBuf,
    runtime_dir: PathBuf,
}

impl AgentWorkflowRunner {
    pub fn new(
        repository: Arc<dyn AgentWorkflowRepository>,
        capability_gate: Arc<AgentCapabilityGate>,
        runner_path: PathBuf,
        runtime_dir: PathBuf,
    ) -> Self {
        Self {
            repository,
            capability_gate,
            runner_path,
            runtime_dir,
        }
    }

    pub async fn execute(
        &self,
        run: AgentWorkflowRun,
        script: AgentWorkflowScript,
        host: Arc<dyn AgentWorkflowHost>,
    ) -> AppResult<()> {
        if !self.capability_gate.workflows_enabled() {
            return Err(AppError::FeatureDisabled(
                "Workflows are disabled. Enable them in Settings > Capabilities.".into(),
            ));
        }
        if run.script_hash != script.script_hash || run.permission_hash != script.permission_hash {
            return Err(AppError::Validation(
                "Workflow run hashes no longer match the authoritative script".into(),
            ));
        }
        validate_absolute_non_root_path(&self.runner_path, "workflow runner executable")
            .map_err(|error| AppError::Infrastructure(error.to_string()))?;
        validate_absolute_non_root_path(&self.runtime_dir, "workflow runtime directory")
            .map_err(|error| AppError::Infrastructure(error.to_string()))?;
        std::fs::create_dir_all(&self.runtime_dir).map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to create workflow runtime directory: {error}"
            ))
        })?;

        let runner_instance_id = uuid::Uuid::new_v4().to_string();
        if !self
            .repository
            .claim_run(
                &run.id,
                run.attempt,
                &runner_instance_id,
                chrono::Utc::now() + RUNNER_LEASE,
            )
            .await?
        {
            return Err(AppError::Conflict(
                "Workflow run was claimed by another runner".into(),
            ));
        }
        let attempt = run.attempt + 1;
        let authority = AgentWorkflowRunAuthority {
            run_id: run.id.to_string(),
            attempt,
            runner_instance_id: runner_instance_id.clone(),
        };
        let mut command = Command::new(&self.runner_path);
        command
            .env_clear()
            .current_dir(&self.runtime_dir)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let message = format!("Failed to start workflow runner: {error}");
                let _ = self
                    .fail_current_attempt(&run, attempt, &runner_instance_id, &message)
                    .await;
                return Err(AppError::Infrastructure(message));
            }
        };

        match self
            .drive(&mut child, &run, &authority, &script, host)
            .await
        {
            Ok(()) => Ok(()),
            Err(error) => {
                kill_and_reap_after_drive_error(&mut child).await;
                let _ = self
                    .fail_current_attempt(&run, attempt, &runner_instance_id, &error.to_string())
                    .await;
                Err(error)
            }
        }
    }

    async fn drive(
        &self,
        child: &mut impl WorkflowChildSession,
        run: &AgentWorkflowRun,
        authority: &AgentWorkflowRunAuthority,
        script: &AgentWorkflowScript,
        host: Arc<dyn AgentWorkflowHost>,
    ) -> AppResult<()> {
        let mut stdin = child
            .take_stdin()
            .ok_or_else(|| AppError::Infrastructure("Workflow runner stdin unavailable".into()))?;
        let mut stdout = child
            .take_stdout()
            .ok_or_else(|| AppError::Infrastructure("Workflow runner stdout unavailable".into()))?;
        let lineage = AgentWorkflowFrame {
            version: AGENT_WORKFLOW_PROTOCOL_VERSION,
            run_id: run.id.to_string(),
            attempt: authority.attempt,
            runner_instance_id: authority.runner_instance_id.clone(),
            message: AgentWorkflowProtocolMessage::Execute {
                script: script.source.clone(),
                args: serde_json::from_str(&run.args_json).map_err(|error| {
                    AppError::Validation(format!("Invalid workflow args: {error}"))
                })?,
            },
        };
        write_frame(&mut stdin, &lineage).await?;
        let ready = read_frame_with_timeout(&mut stdout).await?;
        validate_lineage(&ready, &lineage)?;
        if !matches!(ready.message, AgentWorkflowProtocolMessage::Ready) {
            return self
                .fail_protocol(
                    run,
                    authority.attempt,
                    &authority.runner_instance_id,
                    "Workflow runner did not send ready",
                )
                .await;
        }

        loop {
            let frame = read_frame_with_timeout(&mut stdout).await?;
            validate_lineage(&frame, &lineage)?;
            match frame.message {
                AgentWorkflowProtocolMessage::HostCall {
                    call_id,
                    operation,
                    payload,
                } => {
                    let current =
                        self.repository.get_run(&run.id).await?.ok_or_else(|| {
                            AppError::NotFound(format!("Workflow run {}", run.id))
                        })?;
                    if current.attempt != authority.attempt
                        || current.runner_instance_id.as_deref()
                            != Some(authority.runner_instance_id.as_str())
                    {
                        return Err(AppError::Conflict(
                            "Stale workflow runner lost run authority".into(),
                        ));
                    }
                    if current.cancel_requested {
                        child.kill_child().await;
                        require_transition(
                            self.repository
                                .transition_run(
                                    &run.id,
                                    authority.attempt,
                                    &authority.runner_instance_id,
                                    current.status,
                                    AgentWorkflowRunStatus::Cancelled,
                                    None,
                                    Some("Cancelled by user".into()),
                                )
                                .await?,
                            "cancellation",
                        )?;
                        return Ok(());
                    }
                    if current.pause_requested || !self.capability_gate.workflows_enabled() {
                        if !current.pause_requested {
                            self.repository.request_pause(&run.id).await?;
                        }
                        child.kill_child().await;
                        require_transition(
                            self.repository
                                .transition_run(
                                    &run.id,
                                    authority.attempt,
                                    &authority.runner_instance_id,
                                    AgentWorkflowRunStatus::PauseRequested,
                                    AgentWorkflowRunStatus::Paused,
                                    None,
                                    None,
                                )
                                .await?,
                            "pause",
                        )?;
                        return Ok(());
                    }
                    let response = match host.handle_call(authority, &operation, payload).await {
                        Ok(result) => AgentWorkflowProtocolMessage::HostResponse {
                            call_id,
                            result: Some(result),
                            error: None,
                        },
                        Err(error) => AgentWorkflowProtocolMessage::HostResponse {
                            call_id,
                            result: None,
                            error: Some(error.to_string()),
                        },
                    };
                    let after_call =
                        self.repository.get_run(&run.id).await?.ok_or_else(|| {
                            AppError::NotFound(format!("Workflow run {}", run.id))
                        })?;
                    if after_call.cancel_requested {
                        child.kill_child().await;
                        if !self
                            .repository
                            .transition_run(
                                &run.id,
                                authority.attempt,
                                &authority.runner_instance_id,
                                after_call.status,
                                AgentWorkflowRunStatus::Cancelled,
                                None,
                                Some("Cancelled by user".into()),
                            )
                            .await?
                        {
                            return Err(AppError::Conflict(
                                "Stale Workflow cancellation rejected".into(),
                            ));
                        }
                        return Ok(());
                    }
                    if after_call.pause_requested || !self.capability_gate.workflows_enabled() {
                        if !after_call.pause_requested {
                            self.repository.request_pause(&run.id).await?;
                        }
                        child.kill_child().await;
                        if !self
                            .repository
                            .transition_run(
                                &run.id,
                                authority.attempt,
                                &authority.runner_instance_id,
                                AgentWorkflowRunStatus::PauseRequested,
                                AgentWorkflowRunStatus::Paused,
                                None,
                                None,
                            )
                            .await?
                        {
                            return Err(AppError::Conflict("Stale Workflow pause rejected".into()));
                        }
                        return Ok(());
                    }
                    write_frame(
                        &mut stdin,
                        &AgentWorkflowFrame {
                            message: response,
                            ..lineage.clone()
                        },
                    )
                    .await?;
                    if !self
                        .repository
                        .heartbeat(
                            &run.id,
                            authority.attempt,
                            &authority.runner_instance_id,
                            chrono::Utc::now() + RUNNER_LEASE,
                        )
                        .await?
                    {
                        return Err(AppError::Conflict(
                            "Workflow heartbeat rejected for stale runner".into(),
                        ));
                    }
                }
                AgentWorkflowProtocolMessage::Completed { result } => {
                    let current =
                        self.repository.get_run(&run.id).await?.ok_or_else(|| {
                            AppError::NotFound(format!("Workflow run {}", run.id))
                        })?;
                    require_successful_exit(child.wait_success().await?)?;
                    if current.cancel_requested {
                        require_transition(
                            self.repository
                                .transition_run(
                                    &run.id,
                                    authority.attempt,
                                    &authority.runner_instance_id,
                                    current.status,
                                    AgentWorkflowRunStatus::Cancelled,
                                    None,
                                    Some("Cancelled by user".into()),
                                )
                                .await?,
                            "cancellation",
                        )?;
                        return Ok(());
                    }
                    if current.pause_requested {
                        require_transition(
                            self.repository
                                .transition_run(
                                    &run.id,
                                    authority.attempt,
                                    &authority.runner_instance_id,
                                    current.status,
                                    AgentWorkflowRunStatus::Paused,
                                    None,
                                    None,
                                )
                                .await?,
                            "pause",
                        )?;
                        return Ok(());
                    }
                    let result_json = serde_json::to_string(&result)
                        .map_err(|error| AppError::Validation(error.to_string()))?;
                    if !self
                        .repository
                        .transition_run(
                            &run.id,
                            authority.attempt,
                            &authority.runner_instance_id,
                            AgentWorkflowRunStatus::Running,
                            AgentWorkflowRunStatus::Completed,
                            Some(result_json),
                            None,
                        )
                        .await?
                    {
                        return Err(AppError::Conflict(
                            "Stale workflow completion rejected".into(),
                        ));
                    }
                    return Ok(());
                }
                AgentWorkflowProtocolMessage::Failed { error } => {
                    let current =
                        self.repository.get_run(&run.id).await?.ok_or_else(|| {
                            AppError::NotFound(format!("Workflow run {}", run.id))
                        })?;
                    if current.cancel_requested || current.pause_requested {
                        let target = if current.cancel_requested {
                            AgentWorkflowRunStatus::Cancelled
                        } else {
                            AgentWorkflowRunStatus::Paused
                        };
                        require_transition(
                            self.repository
                                .transition_run(
                                    &run.id,
                                    authority.attempt,
                                    &authority.runner_instance_id,
                                    current.status,
                                    target,
                                    None,
                                    current
                                        .cancel_requested
                                        .then(|| "Cancelled by user".to_string()),
                                )
                                .await?,
                            if current.cancel_requested {
                                "cancellation"
                            } else {
                                "pause"
                            },
                        )?;
                        return Ok(());
                    }
                    if !self
                        .repository
                        .transition_run(
                            &run.id,
                            authority.attempt,
                            &authority.runner_instance_id,
                            AgentWorkflowRunStatus::Running,
                            AgentWorkflowRunStatus::Failed,
                            None,
                            Some(error.clone()),
                        )
                        .await?
                    {
                        return Err(AppError::Conflict("Stale workflow failure rejected".into()));
                    }
                    return Err(AppError::ExecutionBlocked(error));
                }
                _ => {
                    return self
                        .fail_protocol(
                            run,
                            authority.attempt,
                            &authority.runner_instance_id,
                            "Unexpected workflow runner frame",
                        )
                        .await
                }
            }
        }
    }

    async fn fail_protocol(
        &self,
        run: &AgentWorkflowRun,
        attempt: u32,
        runner: &str,
        message: &str,
    ) -> AppResult<()> {
        let _ = self
            .repository
            .transition_run(
                &run.id,
                attempt,
                runner,
                AgentWorkflowRunStatus::Running,
                AgentWorkflowRunStatus::Failed,
                None,
                Some(message.into()),
            )
            .await?;
        Err(AppError::Infrastructure(message.into()))
    }

    async fn fail_current_attempt(
        &self,
        run: &AgentWorkflowRun,
        attempt: u32,
        runner: &str,
        message: &str,
    ) -> AppResult<()> {
        let Some(current) = self.repository.get_run(&run.id).await? else {
            return Err(AppError::NotFound(format!("Workflow run {}", run.id)));
        };
        if current.attempt != attempt
            || current.runner_instance_id.as_deref() != Some(runner)
            || current.status.is_terminal()
            || current.status == AgentWorkflowRunStatus::Paused
        {
            return Ok(());
        }
        self.repository
            .transition_run(
                &run.id,
                attempt,
                runner,
                current.status,
                AgentWorkflowRunStatus::Failed,
                None,
                Some(message.into()),
            )
            .await?;
        Ok(())
    }
}

fn require_transition(applied: bool, action: &str) -> AppResult<()> {
    if applied {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "Stale workflow {action} rejected"
        )))
    }
}

fn require_successful_exit(success: bool) -> AppResult<()> {
    if success {
        Ok(())
    } else {
        Err(AppError::Infrastructure(
            "Workflow runner exited unsuccessfully after completion".into(),
        ))
    }
}

fn validate_lineage(frame: &AgentWorkflowFrame, expected: &AgentWorkflowFrame) -> AppResult<()> {
    frame.validate().map_err(AppError::Validation)?;
    if frame.run_id != expected.run_id
        || frame.attempt != expected.attempt
        || frame.runner_instance_id != expected.runner_instance_id
    {
        return Err(AppError::Conflict(
            "Stale workflow protocol frame rejected".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "agent_workflow_runner_tests.rs"]
mod tests;

async fn write_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    frame: &AgentWorkflowFrame,
) -> AppResult<()> {
    frame.validate().map_err(AppError::Validation)?;
    let payload =
        serde_json::to_vec(frame).map_err(|error| AppError::Validation(error.to_string()))?;
    if payload.len() > AGENT_WORKFLOW_MAX_FRAME_BYTES {
        return Err(AppError::Validation(
            "Workflow frame exceeds size limit".into(),
        ));
    }
    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await
        .map_err(|error| AppError::Infrastructure(error.to_string()))?;
    writer
        .write_all(&payload)
        .await
        .map_err(|error| AppError::Infrastructure(error.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|error| AppError::Infrastructure(error.to_string()))
}

async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> AppResult<AgentWorkflowFrame> {
    let length = reader
        .read_u32()
        .await
        .map_err(|error| AppError::Infrastructure(error.to_string()))? as usize;
    if length == 0 || length > AGENT_WORKFLOW_MAX_FRAME_BYTES {
        return Err(AppError::Validation("Invalid workflow frame length".into()));
    }
    let mut payload = vec![0; length];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|error| AppError::Infrastructure(error.to_string()))?;
    serde_json::from_slice(&payload).map_err(|error| AppError::Validation(error.to_string()))
}

async fn read_frame_with_timeout(
    reader: &mut (impl AsyncRead + Unpin),
) -> AppResult<AgentWorkflowFrame> {
    tokio::time::timeout(RUNNER_FRAME_TIMEOUT, read_frame(reader))
        .await
        .map_err(|_| AppError::ExecutionBlocked("Workflow runner frame timed out".into()))?
}
