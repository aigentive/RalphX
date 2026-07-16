use chrono::{Duration, Utc};
use rusqlite::Connection;

use super::migrations::migrate_scripted_agent_workflows_for_test;
use super::SqliteAgentWorkflowRepository;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    sha256_hex, AgentWorkflowInvocation, AgentWorkflowInvocationId, AgentWorkflowMeta,
    AgentWorkflowPhase, AgentWorkflowPhaseId, AgentWorkflowRun, AgentWorkflowRunId,
    AgentWorkflowRunStatus, AgentWorkflowScript, AgentWorkflowStepStatus, ChatConversationId,
    ProjectId,
};
use crate::domain::repositories::AgentWorkflowRepository;

const CONVERSATION_ID: &str = "00000000-0000-0000-0000-000000000001";

fn repository() -> SqliteAgentWorkflowRepository {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE chat_conversations (id TEXT PRIMARY KEY);
             CREATE TABLE delegated_sessions (id TEXT PRIMARY KEY);
             INSERT INTO chat_conversations VALUES ('00000000-0000-0000-0000-000000000001');",
        )
        .unwrap();
    migrate_scripted_agent_workflows_for_test(&connection).unwrap();
    SqliteAgentWorkflowRepository::new(connection)
}

fn script() -> AgentWorkflowScript {
    AgentWorkflowScript::new(
        ChatConversationId::from_string(CONVERSATION_ID),
        ProjectId::from_string("project-1".to_string()),
        "return await agent('review');".into(),
        AgentWorkflowMeta {
            name: "Review".into(),
            description: String::new(),
            phases: vec!["review".into()],
            max_concurrency: 2,
            max_invocations: 4,
        },
        r#"{"filesystem":"read-only"}"#.into(),
        1,
    )
    .unwrap()
}

fn run(script: &AgentWorkflowScript) -> AgentWorkflowRun {
    let now = Utc::now();
    AgentWorkflowRun {
        id: AgentWorkflowRunId::new(),
        script_id: script.id.clone(),
        conversation_id: script.conversation_id.clone(),
        project_id: script.project_id.clone(),
        harness: AgentHarnessKind::Codex,
        script_hash: script.script_hash.clone(),
        permission_hash: script.permission_hash.clone(),
        args_json: "{}".into(),
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
    }
}

#[tokio::test]
async fn launch_requires_exact_current_approval_hashes() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    assert!(repository.create_run(run(&script)).await.is_err());
    assert!(repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap());
    repository.create_run(run(&script)).await.unwrap();
}

#[tokio::test]
async fn run_once_approval_is_consumed_but_same_launch_id_is_idempotent() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    assert!(repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap());
    let launch = run(&script);
    let created = repository.create_run(launch.clone()).await.unwrap();

    assert!(repository.create_run(run(&script)).await.is_err());

    assert!(repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap());
    let retried = repository.create_run(launch).await.unwrap();
    assert_eq!(retried.id, created.id);
    assert_eq!(
        repository
            .get_latest_run_for_script(&script.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        created.id
    );
    assert!(repository.create_run(run(&script)).await.is_err());
}

#[tokio::test]
async fn stale_runner_attempt_cannot_heartbeat_or_complete() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap();
    let run = repository.create_run(run(&script)).await.unwrap();
    assert!(repository
        .claim_run(
            &run.id,
            0,
            "runner-current",
            Utc::now() + Duration::seconds(30)
        )
        .await
        .unwrap());
    assert!(!repository
        .heartbeat(
            &run.id,
            0,
            "runner-stale",
            Utc::now() + Duration::seconds(30)
        )
        .await
        .unwrap());
    assert!(repository
        .append_log(&run.id, 0, "runner-stale", "info", "must not persist")
        .await
        .unwrap()
        .is_none());
    assert!(!repository
        .upsert_phase(
            AgentWorkflowPhase {
                id: AgentWorkflowPhaseId::new(),
                run_id: run.id.clone(),
                key: "stale".into(),
                name: "Stale".into(),
                ordinal: 0,
                status: AgentWorkflowStepStatus::Completed,
                started_at: None,
                completed_at: Some(Utc::now()),
                error: None,
            },
            0,
            "runner-stale",
        )
        .await
        .unwrap());
    assert!(!repository
        .transition_run(
            &run.id,
            0,
            "runner-stale",
            AgentWorkflowRunStatus::Running,
            AgentWorkflowRunStatus::Completed,
            Some("{}".into()),
            None,
        )
        .await
        .unwrap());
    assert_eq!(
        repository.get_run(&run.id).await.unwrap().unwrap().status,
        AgentWorkflowRunStatus::Running
    );
    let progress = repository.get_progress(&run.id).await.unwrap();
    assert!(progress.logs.is_empty());
    assert!(progress.phases.is_empty());
}

#[tokio::test]
async fn completed_logical_invocation_is_reused_without_duplicate_row() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap();
    let run = repository.create_run(run(&script)).await.unwrap();
    let now = Utc::now();
    let invocation = AgentWorkflowInvocation {
        id: AgentWorkflowInvocationId::new(),
        run_id: run.id.clone(),
        phase_id: None,
        logical_key: "critic:0".into(),
        agent_name: "ralphx-general-explorer".into(),
        prompt_hash: sha256_hex(b"review"),
        schema_hash: None,
        status: AgentWorkflowStepStatus::Completed,
        delegated_session_id: None,
        child_conversation_id: None,
        result_json: Some(r#"{"ok":true}"#.into()),
        error: None,
        created_at: now,
        updated_at: now,
        completed_at: Some(now),
    };
    let first = repository
        .begin_invocation(invocation.clone())
        .await
        .unwrap();
    let mut replay = invocation;
    replay.id = AgentWorkflowInvocationId::new();
    let second = repository.begin_invocation(replay).await.unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(
        repository
            .get_progress(&run.id)
            .await
            .unwrap()
            .invocations
            .len(),
        1
    );
}

#[tokio::test]
async fn paused_run_resumes_with_cleared_runner_authority() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap();
    let run = repository.create_run(run(&script)).await.unwrap();
    assert!(repository.request_pause(&run.id).await.unwrap());
    assert_eq!(
        repository.get_run(&run.id).await.unwrap().unwrap().status,
        AgentWorkflowRunStatus::Paused
    );
    assert!(repository.resume_run(&run.id).await.unwrap());
    let resumed = repository.get_run(&run.id).await.unwrap().unwrap();
    assert_eq!(resumed.status, AgentWorkflowRunStatus::Queued);
    assert!(!resumed.pause_requested);
    assert!(resumed.runner_instance_id.is_none());
}

#[tokio::test]
async fn active_runner_can_heartbeat_while_pause_request_waits_for_child_settlement() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap();
    let run = repository.create_run(run(&script)).await.unwrap();
    assert!(repository
        .claim_run(&run.id, 0, "runner", Utc::now() + Duration::seconds(30))
        .await
        .unwrap());
    assert!(repository.request_pause(&run.id).await.unwrap());

    assert!(repository
        .heartbeat(&run.id, 1, "runner", Utc::now() + Duration::seconds(30),)
        .await
        .unwrap());
    assert_eq!(
        repository.get_run(&run.id).await.unwrap().unwrap().status,
        AgentWorkflowRunStatus::PauseRequested
    );
}

#[tokio::test]
async fn expired_runner_is_taken_over_but_live_runner_is_untouched() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap();
    let run = repository.create_run(run(&script)).await.unwrap();
    assert!(repository
        .claim_run(&run.id, 0, "runner-old", Utc::now() - Duration::seconds(1))
        .await
        .unwrap());
    assert!(!repository
        .prepare_recovery(&run.id, 1, Utc::now() - Duration::seconds(10))
        .await
        .unwrap());
    assert!(repository
        .prepare_recovery(&run.id, 1, Utc::now())
        .await
        .unwrap());
    let recovering = repository.get_run(&run.id).await.unwrap().unwrap();
    assert_eq!(recovering.status, AgentWorkflowRunStatus::Recovering);
    assert!(recovering.runner_instance_id.is_none());
    assert!(repository
        .claim_run(&run.id, 1, "runner-new", Utc::now() + Duration::seconds(30))
        .await
        .unwrap());
}

#[tokio::test]
async fn recovery_failure_terminalizes_only_the_expected_unclaimed_state() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap();
    let run = repository.create_run(run(&script)).await.unwrap();

    assert!(!repository
        .fail_unclaimed_run(
            &run.id,
            AgentWorkflowRunStatus::Recovering,
            "missing script",
        )
        .await
        .unwrap());
    assert_eq!(
        repository.get_run(&run.id).await.unwrap().unwrap().status,
        AgentWorkflowRunStatus::Queued
    );

    assert!(repository
        .fail_unclaimed_run(&run.id, AgentWorkflowRunStatus::Queued, "missing script")
        .await
        .unwrap());
    let failed = repository.get_run(&run.id).await.unwrap().unwrap();
    assert_eq!(failed.status, AgentWorkflowRunStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("missing script"));
    assert!(failed.completed_at.is_some());
}

#[tokio::test]
async fn current_attempt_persists_phase_invocation_and_ordered_logs() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap();
    let run = repository.create_run(run(&script)).await.unwrap();
    assert!(repository
        .claim_run(&run.id, 0, "runner", Utc::now() + Duration::seconds(30))
        .await
        .unwrap());

    let phase_id = AgentWorkflowPhaseId::new();
    assert!(repository
        .upsert_phase(
            AgentWorkflowPhase {
                id: phase_id.clone(),
                run_id: run.id.clone(),
                key: "review".into(),
                name: "Review".into(),
                ordinal: 0,
                status: AgentWorkflowStepStatus::Running,
                started_at: Some(Utc::now()),
                completed_at: None,
                error: None,
            },
            1,
            "runner",
        )
        .await
        .unwrap());

    let now = Utc::now();
    let invocation = repository
        .begin_invocation(AgentWorkflowInvocation {
            id: AgentWorkflowInvocationId::new(),
            run_id: run.id.clone(),
            phase_id: Some(phase_id),
            logical_key: "review:0".into(),
            agent_name: "ralphx-general-explorer".into(),
            prompt_hash: sha256_hex(b"review changed files"),
            schema_hash: Some(sha256_hex(b"result-schema")),
            status: AgentWorkflowStepStatus::Running,
            delegated_session_id: None,
            child_conversation_id: None,
            result_json: None,
            error: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
        .await
        .unwrap();

    assert!(!repository
        .settle_invocation(
            invocation.id.as_str(),
            1,
            "stale-runner",
            AgentWorkflowStepStatus::Completed,
            None,
            None,
            Some(r#"{"accepted":false}"#.into()),
            None,
        )
        .await
        .unwrap());
    assert!(repository
        .settle_invocation(
            invocation.id.as_str(),
            1,
            "runner",
            AgentWorkflowStepStatus::Completed,
            None,
            None,
            Some(r#"{"accepted":true}"#.into()),
            None,
        )
        .await
        .unwrap());

    assert!(repository
        .append_log(&run.id, 1, "stale-runner", "info", "must not persist")
        .await
        .unwrap()
        .is_none());
    let first = repository
        .append_log(&run.id, 1, "runner", "info", "started")
        .await
        .unwrap()
        .unwrap();
    let second = repository
        .append_log(&run.id, 1, "runner", "warn", "needs attention")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((first.sequence, second.sequence), (0, 1));
    assert!(repository
        .append_log(&run.id, 1, "runner", "fatal", "invalid")
        .await
        .is_err());

    let progress = repository.get_progress(&run.id).await.unwrap();
    assert_eq!(progress.phases.len(), 1);
    assert_eq!(progress.phases[0].key, "review");
    assert_eq!(progress.phases[0].status, AgentWorkflowStepStatus::Running);
    assert_eq!(progress.invocations.len(), 1);
    assert_eq!(
        progress.invocations[0].status,
        AgentWorkflowStepStatus::Completed
    );
    assert_eq!(
        progress.invocations[0].result_json.as_deref(),
        Some(r#"{"accepted":true}"#)
    );
    assert_eq!(
        progress
            .logs
            .iter()
            .map(|entry| (entry.sequence, entry.level.as_str(), entry.message.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "info", "started"), (1, "warn", "needs attention")]
    );
}

#[tokio::test]
async fn launch_id_reuse_with_changed_inputs_fails_without_consuming_new_approval() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap();
    let created = repository.create_run(run(&script)).await.unwrap();
    let mut conflicting = created.clone();
    conflicting.args_json = r#"{"different":true}"#.into();

    let error = repository.create_run(conflicting).await.unwrap_err();

    assert!(
        matches!(error, crate::error::AppError::Conflict(message) if message.contains("different inputs"))
    );
    assert_eq!(
        repository
            .get_run(&created.id)
            .await
            .unwrap()
            .unwrap()
            .args_json,
        "{}"
    );
}

#[tokio::test]
async fn recoverable_listing_excludes_live_lease_and_includes_expired_lease() {
    let repository = repository();
    let script = repository.save_script(script()).await.unwrap();
    repository
        .approve_script(&script.id, &script.script_hash, &script.permission_hash)
        .await
        .unwrap();
    let run = repository.create_run(run(&script)).await.unwrap();
    assert!(repository
        .claim_run(&run.id, 0, "runner", Utc::now() + Duration::seconds(30))
        .await
        .unwrap());

    assert!(repository
        .list_recoverable(Utc::now())
        .await
        .unwrap()
        .iter()
        .all(|candidate| candidate.id != run.id));
    assert!(repository
        .heartbeat(&run.id, 1, "runner", Utc::now() - Duration::seconds(1),)
        .await
        .unwrap());
    assert!(repository
        .list_recoverable(Utc::now())
        .await
        .unwrap()
        .iter()
        .any(|candidate| candidate.id == run.id));
}
