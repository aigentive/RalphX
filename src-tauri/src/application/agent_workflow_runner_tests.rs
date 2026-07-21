use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use tokio::io::{duplex, DuplexStream};
use tokio::sync::Mutex;

use super::{
    kill_and_reap_after_drive_error, read_frame, require_successful_exit, require_transition,
    write_frame, AgentWorkflowHost, AgentWorkflowRunAuthority, AgentWorkflowRunner,
    WorkflowChildSession, WorkflowChildTermination,
};
use crate::application::agent_capability_gate::{AgentCapabilities, AgentCapabilityGate};
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::agent_workflow_protocol::{
    AgentWorkflowFrame, AgentWorkflowProtocolMessage, AGENT_WORKFLOW_PROTOCOL_VERSION,
};
use crate::domain::entities::{
    AgentWorkflowMeta, AgentWorkflowRun, AgentWorkflowRunId, AgentWorkflowRunStatus,
    AgentWorkflowScript, ChatConversationId, ProjectId,
};
use crate::domain::repositories::AgentWorkflowRepository;
use crate::error::AppError;
use crate::infrastructure::sqlite::SqliteAgentWorkflowRepository;
use crate::testing::SqliteTestDb;

#[derive(Default)]
struct RecordingChildTermination {
    events: Vec<&'static str>,
}

#[async_trait]
impl WorkflowChildTermination for RecordingChildTermination {
    async fn kill_child(&mut self) {
        self.events.push("kill");
    }

    async fn reap_child(&mut self) {
        self.events.push("reap");
    }
}

struct FakeWorkflowChild {
    stdin: Option<DuplexStream>,
    stdout: Option<DuplexStream>,
    killed: Arc<AtomicBool>,
    reaped: Arc<AtomicBool>,
    wait_success: bool,
}

impl FakeWorkflowChild {
    fn new(wait_success: bool) -> (Self, DuplexStream, DuplexStream) {
        let (runner_stdin, sidecar_input) = duplex(64 * 1024);
        let (sidecar_output, runner_stdout) = duplex(64 * 1024);
        (
            Self {
                stdin: Some(runner_stdin),
                stdout: Some(runner_stdout),
                killed: Arc::new(AtomicBool::new(false)),
                reaped: Arc::new(AtomicBool::new(false)),
                wait_success,
            },
            sidecar_input,
            sidecar_output,
        )
    }
}

#[async_trait]
impl WorkflowChildTermination for FakeWorkflowChild {
    async fn kill_child(&mut self) {
        self.killed.store(true, Ordering::Release);
    }

    async fn reap_child(&mut self) {
        self.reaped.store(true, Ordering::Release);
    }
}

#[async_trait]
impl WorkflowChildSession for FakeWorkflowChild {
    type Stdin = DuplexStream;
    type Stdout = DuplexStream;

    fn take_stdin(&mut self) -> Option<Self::Stdin> {
        self.stdin.take()
    }

    fn take_stdout(&mut self) -> Option<Self::Stdout> {
        self.stdout.take()
    }

    async fn wait_success(&mut self) -> crate::error::AppResult<bool> {
        Ok(self.wait_success)
    }
}

enum HostSideEffect {
    Cancel,
    DisableWorkflows,
    ReplaceRunner,
}

struct RecordingHost {
    calls: Mutex<Vec<(String, Value)>>,
    result: Result<Value, String>,
    side_effect: Option<HostSideEffect>,
    repository: Arc<SqliteAgentWorkflowRepository>,
    connection: Arc<Mutex<rusqlite::Connection>>,
    gate: Arc<AgentCapabilityGate>,
    run_id: AgentWorkflowRunId,
}

impl RecordingHost {
    fn success(context: &RunnerTestContext, result: Value) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            result: Ok(result),
            side_effect: None,
            repository: context.repository.clone(),
            connection: context.connection.clone(),
            gate: context.gate.clone(),
            run_id: context.run.id.clone(),
        }
    }

    fn with_side_effect(mut self, side_effect: HostSideEffect) -> Self {
        self.side_effect = Some(side_effect);
        self
    }
}

#[async_trait]
impl AgentWorkflowHost for RecordingHost {
    async fn handle_call(
        &self,
        _authority: &AgentWorkflowRunAuthority,
        operation: &str,
        payload: Value,
    ) -> crate::error::AppResult<Value> {
        self.calls.lock().await.push((operation.into(), payload));
        match self.side_effect {
            Some(HostSideEffect::Cancel) => {
                self.repository.request_cancel(&self.run_id).await?;
            }
            Some(HostSideEffect::DisableWorkflows) => {
                self.gate.replace(AgentCapabilities::default());
            }
            Some(HostSideEffect::ReplaceRunner) => {
                let connection = self.connection.lock().await;
                connection
                    .execute(
                        "UPDATE agent_workflow_runs SET runner_instance_id='replacement' WHERE id=?1",
                        [self.run_id.as_str()],
                    )
                    .unwrap();
            }
            None => {}
        }
        self.result
            .as_ref()
            .cloned()
            .map_err(|message| AppError::ExecutionBlocked(message.clone()))
    }
}

struct RunnerTestContext {
    _db: SqliteTestDb,
    repository: Arc<SqliteAgentWorkflowRepository>,
    connection: Arc<Mutex<rusqlite::Connection>>,
    gate: Arc<AgentCapabilityGate>,
    runner: AgentWorkflowRunner,
    run: AgentWorkflowRun,
    script: AgentWorkflowScript,
}

impl RunnerTestContext {
    async fn new(name: &str) -> Self {
        let db = SqliteTestDb::new(name);
        let conversation = db.seed_ideation_conversation();
        let connection = db.shared_conn();
        let repository = Arc::new(SqliteAgentWorkflowRepository::from_shared(
            connection.clone(),
        ));
        let script = AgentWorkflowScript::new(
            ChatConversationId::from_string(conversation.id.to_string()),
            ProjectId::from_string("workflow-runner-project".into()),
            "return { ok: true };".into(),
            AgentWorkflowMeta {
                name: "Runner test".into(),
                description: String::new(),
                phases: vec!["review".into()],
                max_concurrency: 1,
                max_invocations: 2,
            },
            r#"{"filesystem":"read-only"}"#.into(),
            1,
        )
        .unwrap();
        let script = repository.save_script(script).await.unwrap();
        assert!(repository
            .approve_script(&script.id, &script.script_hash, &script.permission_hash)
            .await
            .unwrap());
        let now = Utc::now();
        let run = repository
            .create_run(AgentWorkflowRun {
                id: AgentWorkflowRunId::new(),
                script_id: script.id.clone(),
                conversation_id: script.conversation_id.clone(),
                project_id: script.project_id.clone(),
                harness: AgentHarnessKind::Codex,
                script_hash: script.script_hash.clone(),
                permission_hash: script.permission_hash.clone(),
                args_json: r#"{"scope":"changed"}"#.into(),
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
            })
            .await
            .unwrap();
        let gate = Arc::new(AgentCapabilityGate::default());
        gate.replace(AgentCapabilities {
            team: false,
            workflows: true,
            autopilot: false,
        });
        let runner = AgentWorkflowRunner::new(
            repository.clone(),
            gate.clone(),
            db.path().with_file_name("missing-workflow-runner"),
            db.path().with_file_name("workflow-runtime"),
        );
        Self {
            _db: db,
            repository,
            connection,
            gate,
            runner,
            run,
            script,
        }
    }

    async fn claim(&self) -> AgentWorkflowRunAuthority {
        assert!(self
            .repository
            .claim_run(
                &self.run.id,
                0,
                "runner-current",
                Utc::now() + Duration::seconds(30),
            )
            .await
            .unwrap());
        AgentWorkflowRunAuthority {
            run_id: self.run.id.to_string(),
            attempt: 1,
            runner_instance_id: "runner-current".into(),
        }
    }
}

fn protocol_frame(
    authority: &AgentWorkflowRunAuthority,
    message: AgentWorkflowProtocolMessage,
) -> AgentWorkflowFrame {
    AgentWorkflowFrame {
        version: AGENT_WORKFLOW_PROTOCOL_VERSION,
        run_id: authority.run_id.clone(),
        attempt: authority.attempt,
        runner_instance_id: authority.runner_instance_id.clone(),
        message,
    }
}

async fn accept_execute_and_send_ready(
    input: &mut DuplexStream,
    output: &mut DuplexStream,
    authority: &AgentWorkflowRunAuthority,
) {
    let execute = read_frame(input).await.unwrap();
    assert_eq!(execute.run_id, authority.run_id);
    assert!(matches!(
        execute.message,
        AgentWorkflowProtocolMessage::Execute { args, .. }
            if args == json!({"scope": "changed"})
    ));
    write_frame(
        output,
        &protocol_frame(authority, AgentWorkflowProtocolMessage::Ready),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn execute_rejects_disabled_mismatched_and_already_claimed_runs() {
    let disabled = RunnerTestContext::new("workflow-runner-disabled").await;
    disabled.gate.replace(AgentCapabilities::default());
    let error = disabled
        .runner
        .execute(
            disabled.run.clone(),
            disabled.script.clone(),
            Arc::new(RecordingHost::success(&disabled, json!(null))),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::FeatureDisabled(_)));
    assert_eq!(
        disabled
            .repository
            .get_run(&disabled.run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkflowRunStatus::Queued
    );

    let mismatched = RunnerTestContext::new("workflow-runner-mismatched").await;
    let mut stale_run = mismatched.run.clone();
    stale_run.script_hash = "0".repeat(64);
    let error = mismatched
        .runner
        .execute(
            stale_run,
            mismatched.script.clone(),
            Arc::new(RecordingHost::success(&mismatched, json!(null))),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Validation(_)));

    let claimed = RunnerTestContext::new("workflow-runner-claimed").await;
    claimed.claim().await;
    let error = claimed
        .runner
        .execute(
            claimed.run.clone(),
            claimed.script.clone(),
            Arc::new(RecordingHost::success(&claimed, json!(null))),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Conflict(message) if message.contains("another runner")));
}

#[tokio::test]
async fn execute_spawn_failure_terminalizes_only_the_claimed_attempt() {
    let context = RunnerTestContext::new("workflow-runner-spawn-failure").await;

    let error = context
        .runner
        .execute(
            context.run.clone(),
            context.script.clone(),
            Arc::new(RecordingHost::success(&context, json!(null))),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Infrastructure(message) if message.contains("Failed to start"))
    );
    let failed = context
        .repository
        .get_run(&context.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.status, AgentWorkflowRunStatus::Failed);
    assert_eq!(failed.attempt, 1);
    assert!(failed.error.unwrap().contains("Failed to start"));
}

#[tokio::test]
async fn host_call_response_heartbeats_and_completion_persists_result() {
    let context = RunnerTestContext::new("workflow-runner-completed").await;
    let authority = context.claim().await;
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
    let sidecar_authority = authority.clone();
    let sidecar = tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::HostCall {
                    call_id: "call-1".into(),
                    operation: "checkpoint".into(),
                    payload: json!({"key": "review"}),
                },
            ),
        )
        .await
        .unwrap();
        let response = read_frame(&mut input).await.unwrap();
        assert!(matches!(
            response.message,
            AgentWorkflowProtocolMessage::HostResponse {
                call_id,
                result: Some(result),
                error: None,
            } if call_id == "call-1" && result == json!({"saved": true})
        ));
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::Completed {
                    result: json!({"summary": "done"}),
                },
            ),
        )
        .await
        .unwrap();
    });
    let host = Arc::new(RecordingHost::success(&context, json!({"saved": true})));

    context
        .runner
        .drive(
            &mut child,
            &context.run,
            &authority,
            &context.script,
            host.clone(),
        )
        .await
        .unwrap();
    sidecar.await.unwrap();

    assert_eq!(host.calls.lock().await.len(), 1);
    let completed = context
        .repository
        .get_run(&context.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.status, AgentWorkflowRunStatus::Completed);
    assert_eq!(
        completed.result_json.as_deref(),
        Some(r#"{"summary":"done"}"#)
    );
    assert!(completed.heartbeat_at.is_some());
}

#[tokio::test]
async fn host_error_is_returned_to_sidecar_before_reported_failure() {
    let context = RunnerTestContext::new("workflow-runner-host-error").await;
    let authority = context.claim().await;
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
    let sidecar_authority = authority.clone();
    let sidecar = tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::HostCall {
                    call_id: "call-error".into(),
                    operation: "agent".into(),
                    payload: json!({"prompt": "review"}),
                },
            ),
        )
        .await
        .unwrap();
        let response = read_frame(&mut input).await.unwrap();
        assert!(matches!(
            response.message,
            AgentWorkflowProtocolMessage::HostResponse {
                result: None,
                error: Some(error),
                ..
            } if error.contains("delegate unavailable")
        ));
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::Failed {
                    error: "script stopped".into(),
                },
            ),
        )
        .await
        .unwrap();
    });
    let host = Arc::new(RecordingHost {
        result: Err("delegate unavailable".into()),
        ..RecordingHost::success(&context, json!(null))
    });

    let error = context
        .runner
        .drive(&mut child, &context.run, &authority, &context.script, host)
        .await
        .unwrap_err();
    sidecar.await.unwrap();

    assert!(matches!(error, AppError::ExecutionBlocked(message) if message == "script stopped"));
    let failed = context
        .repository
        .get_run(&context.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.status, AgentWorkflowRunStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("script stopped"));
}

#[tokio::test]
async fn cancellation_before_host_call_suppresses_host_side_effects() {
    let context = RunnerTestContext::new("workflow-runner-cancel-before-call").await;
    let authority = context.claim().await;
    assert!(context
        .repository
        .request_cancel(&context.run.id)
        .await
        .unwrap());
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
    let sidecar_authority = authority.clone();
    let sidecar = tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::HostCall {
                    call_id: "suppressed".into(),
                    operation: "agent".into(),
                    payload: json!({}),
                },
            ),
        )
        .await
        .unwrap();
    });
    let host = Arc::new(RecordingHost::success(&context, json!({})));

    context
        .runner
        .drive(
            &mut child,
            &context.run,
            &authority,
            &context.script,
            host.clone(),
        )
        .await
        .unwrap();
    sidecar.await.unwrap();

    assert!(host.calls.lock().await.is_empty());
    assert!(child.killed.load(Ordering::Acquire));
    assert_eq!(
        context
            .repository
            .get_run(&context.run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkflowRunStatus::Cancelled
    );
}

#[tokio::test]
async fn disabled_capability_before_host_call_durably_pauses_run() {
    let context = RunnerTestContext::new("workflow-runner-disable-before-call").await;
    let authority = context.claim().await;
    context.gate.replace(AgentCapabilities::default());
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
    let sidecar_authority = authority.clone();
    let sidecar = tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::HostCall {
                    call_id: "suppressed".into(),
                    operation: "checkpoint".into(),
                    payload: json!({}),
                },
            ),
        )
        .await
        .unwrap();
    });
    let host = Arc::new(RecordingHost::success(&context, json!({})));

    context
        .runner
        .drive(
            &mut child,
            &context.run,
            &authority,
            &context.script,
            host.clone(),
        )
        .await
        .unwrap();
    sidecar.await.unwrap();

    assert!(host.calls.lock().await.is_empty());
    assert_eq!(
        context
            .repository
            .get_run(&context.run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkflowRunStatus::Paused
    );
}

#[tokio::test]
async fn cancellation_after_host_call_wins_before_response_is_written() {
    let context = RunnerTestContext::new("workflow-runner-cancel-after-call").await;
    let authority = context.claim().await;
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
    let sidecar_authority = authority.clone();
    let sidecar = tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::HostCall {
                    call_id: "cancel".into(),
                    operation: "agent".into(),
                    payload: json!({}),
                },
            ),
        )
        .await
        .unwrap();
        let response =
            tokio::time::timeout(std::time::Duration::from_millis(50), read_frame(&mut input))
                .await;
        assert!(response.is_err() || response.unwrap().is_err());
    });
    let host = Arc::new(
        RecordingHost::success(&context, json!({})).with_side_effect(HostSideEffect::Cancel),
    );

    context
        .runner
        .drive(&mut child, &context.run, &authority, &context.script, host)
        .await
        .unwrap();
    sidecar.await.unwrap();

    assert_eq!(
        context
            .repository
            .get_run(&context.run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkflowRunStatus::Cancelled
    );
}

#[tokio::test]
async fn capability_disable_after_host_call_pauses_before_response() {
    let context = RunnerTestContext::new("workflow-runner-disable-after-call").await;
    let authority = context.claim().await;
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
    let sidecar_authority = authority.clone();
    let sidecar = tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::HostCall {
                    call_id: "pause".into(),
                    operation: "checkpoint".into(),
                    payload: json!({}),
                },
            ),
        )
        .await
        .unwrap();
    });
    let host = Arc::new(
        RecordingHost::success(&context, json!({}))
            .with_side_effect(HostSideEffect::DisableWorkflows),
    );

    context
        .runner
        .drive(&mut child, &context.run, &authority, &context.script, host)
        .await
        .unwrap();
    sidecar.await.unwrap();

    assert_eq!(
        context
            .repository
            .get_run(&context.run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkflowRunStatus::Paused
    );
}

#[tokio::test]
async fn stale_runner_is_rejected_before_and_after_host_call() {
    let before = RunnerTestContext::new("workflow-runner-stale-before").await;
    let authority = before.claim().await;
    before
        .connection
        .lock()
        .await
        .execute(
            "UPDATE agent_workflow_runs SET runner_instance_id='replacement' WHERE id=?1",
            [before.run.id.as_str()],
        )
        .unwrap();
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
    let sidecar_authority = authority.clone();
    tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::HostCall {
                    call_id: "stale".into(),
                    operation: "agent".into(),
                    payload: json!({}),
                },
            ),
        )
        .await
        .unwrap();
    });
    let host = Arc::new(RecordingHost::success(&before, json!({})));
    let error = before
        .runner
        .drive(
            &mut child,
            &before.run,
            &authority,
            &before.script,
            host.clone(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Conflict(message) if message.contains("lost run authority")));
    assert!(host.calls.lock().await.is_empty());

    let after = RunnerTestContext::new("workflow-runner-stale-after").await;
    let authority = after.claim().await;
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
    let sidecar_authority = authority.clone();
    tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::HostCall {
                    call_id: "stale-heartbeat".into(),
                    operation: "checkpoint".into(),
                    payload: json!({}),
                },
            ),
        )
        .await
        .unwrap();
        let _ = read_frame(&mut input).await;
    });
    let host = Arc::new(
        RecordingHost::success(&after, json!({})).with_side_effect(HostSideEffect::ReplaceRunner),
    );
    let error = after
        .runner
        .drive(&mut child, &after.run, &authority, &after.script, host)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Conflict(message) if message.contains("heartbeat rejected")));
}

#[tokio::test]
async fn terminal_frames_honor_pending_cancel_pause_and_exit_status() {
    for (name, request_cancel, message, expected) in [
        (
            "workflow-runner-complete-cancel",
            true,
            AgentWorkflowProtocolMessage::Completed { result: json!({}) },
            AgentWorkflowRunStatus::Cancelled,
        ),
        (
            "workflow-runner-complete-pause",
            false,
            AgentWorkflowProtocolMessage::Completed { result: json!({}) },
            AgentWorkflowRunStatus::Paused,
        ),
        (
            "workflow-runner-failed-cancel",
            true,
            AgentWorkflowProtocolMessage::Failed {
                error: "ignored after cancel".into(),
            },
            AgentWorkflowRunStatus::Cancelled,
        ),
        (
            "workflow-runner-failed-pause",
            false,
            AgentWorkflowProtocolMessage::Failed {
                error: "ignored after pause".into(),
            },
            AgentWorkflowRunStatus::Paused,
        ),
    ] {
        let context = RunnerTestContext::new(name).await;
        let authority = context.claim().await;
        if request_cancel {
            assert!(context
                .repository
                .request_cancel(&context.run.id)
                .await
                .unwrap());
        } else {
            assert!(context
                .repository
                .request_pause(&context.run.id)
                .await
                .unwrap());
        }
        let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
        let sidecar_authority = authority.clone();
        tokio::spawn(async move {
            accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
            write_frame(&mut output, &protocol_frame(&sidecar_authority, message))
                .await
                .unwrap();
        });

        context
            .runner
            .drive(
                &mut child,
                &context.run,
                &authority,
                &context.script,
                Arc::new(RecordingHost::success(&context, json!({}))),
            )
            .await
            .unwrap();
        assert_eq!(
            context
                .repository
                .get_run(&context.run.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            expected
        );
    }

    let context = RunnerTestContext::new("workflow-runner-bad-exit").await;
    let authority = context.claim().await;
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(false);
    let sidecar_authority = authority.clone();
    tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(
                &sidecar_authority,
                AgentWorkflowProtocolMessage::Completed { result: json!({}) },
            ),
        )
        .await
        .unwrap();
    });
    let error = context
        .runner
        .drive(
            &mut child,
            &context.run,
            &authority,
            &context.script,
            Arc::new(RecordingHost::success(&context, json!({}))),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::Infrastructure(message) if message.contains("exited unsuccessfully"))
    );
    assert_eq!(
        context
            .repository
            .get_run(&context.run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkflowRunStatus::Running
    );
}

#[tokio::test]
async fn invalid_ready_lineage_and_unexpected_frames_fail_closed() {
    for (name, first_message, stale_lineage, expected) in [
        (
            "workflow-runner-not-ready",
            AgentWorkflowProtocolMessage::Completed { result: json!({}) },
            false,
            "did not send ready",
        ),
        (
            "workflow-runner-stale-lineage",
            AgentWorkflowProtocolMessage::Ready,
            true,
            "Stale workflow protocol frame rejected",
        ),
    ] {
        let context = RunnerTestContext::new(name).await;
        let authority = context.claim().await;
        let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
        let sidecar_authority = authority.clone();
        tokio::spawn(async move {
            let _ = read_frame(&mut input).await.unwrap();
            let mut frame = protocol_frame(&sidecar_authority, first_message);
            if stale_lineage {
                frame.runner_instance_id = "stale".into();
            }
            write_frame(&mut output, &frame).await.unwrap();
        });
        let error = context
            .runner
            .drive(
                &mut child,
                &context.run,
                &authority,
                &context.script,
                Arc::new(RecordingHost::success(&context, json!({}))),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains(expected));
    }

    let context = RunnerTestContext::new("workflow-runner-unexpected-frame").await;
    let authority = context.claim().await;
    let (mut child, mut input, mut output) = FakeWorkflowChild::new(true);
    let sidecar_authority = authority.clone();
    tokio::spawn(async move {
        accept_execute_and_send_ready(&mut input, &mut output, &sidecar_authority).await;
        write_frame(
            &mut output,
            &protocol_frame(&sidecar_authority, AgentWorkflowProtocolMessage::Ready),
        )
        .await
        .unwrap();
    });
    let error = context
        .runner
        .drive(
            &mut child,
            &context.run,
            &authority,
            &context.script,
            Arc::new(RecordingHost::success(&context, json!({}))),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::Infrastructure(message) if message == "Unexpected workflow runner frame")
    );
    assert_eq!(
        context
            .repository
            .get_run(&context.run.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentWorkflowRunStatus::Failed
    );
}

#[tokio::test]
async fn drive_rejects_missing_streams_and_invalid_args() {
    let context = RunnerTestContext::new("workflow-runner-missing-streams").await;
    let authority = context.claim().await;
    let (mut child, _, _) = FakeWorkflowChild::new(true);
    child.stdin = None;
    let error = context
        .runner
        .drive(
            &mut child,
            &context.run,
            &authority,
            &context.script,
            Arc::new(RecordingHost::success(&context, json!({}))),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("stdin unavailable"));

    let (mut child, _, _) = FakeWorkflowChild::new(true);
    child.stdout = None;
    let error = context
        .runner
        .drive(
            &mut child,
            &context.run,
            &authority,
            &context.script,
            Arc::new(RecordingHost::success(&context, json!({}))),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("stdout unavailable"));

    let (mut child, _, _) = FakeWorkflowChild::new(true);
    let mut invalid_run = context.run.clone();
    invalid_run.args_json = "{".into();
    let error = context
        .runner
        .drive(
            &mut child,
            &invalid_run,
            &authority,
            &context.script,
            Arc::new(RecordingHost::success(&context, json!({}))),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::Validation(message) if message.contains("Invalid workflow args"))
    );
}

#[tokio::test]
async fn drive_error_cleanup_kills_before_reaping_the_sidecar() {
    let mut child = RecordingChildTermination::default();

    kill_and_reap_after_drive_error(&mut child).await;

    assert_eq!(child.events, vec!["kill", "reap"]);
}

#[test]
fn accepted_lifecycle_transition_allows_runner_to_finish() {
    assert!(require_transition(true, "pause").is_ok());
}

#[test]
fn rejected_lifecycle_transition_prevents_stale_runner_success() {
    let error = require_transition(false, "cancellation").expect_err("stale CAS must fail closed");

    assert!(matches!(
        error,
        AppError::Conflict(message) if message == "Stale workflow cancellation rejected"
    ));
}

#[test]
fn unsuccessful_runner_exit_prevents_completion() {
    let error = require_successful_exit(false).expect_err("non-zero exit must fail closed");

    assert!(matches!(
        error,
        AppError::Infrastructure(message)
            if message == "Workflow runner exited unsuccessfully after completion"
    ));
}

#[test]
fn successful_runner_exit_allows_completion_transition() {
    assert!(require_successful_exit(true).is_ok());
}
