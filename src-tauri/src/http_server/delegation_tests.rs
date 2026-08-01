use std::sync::Arc;

use super::delegation::{persist_terminal_projection, DelegationJobSnapshot, DelegationService};
use crate::domain::entities::{ChatConversation, ProjectId};
use crate::domain::repositories::{ChatConversationRepository, ChatTimelineRepository};
use crate::infrastructure::sqlite::{
    SqliteChatConversationRepository, SqliteChatTimelineRepository,
};
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn terminal_projection_is_idempotent_and_fully_hydrated() {
    let db = SqliteTestDb::new("delegation-terminal-projection");
    let conversation_repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let timeline_repo: Arc<dyn ChatTimelineRepository> =
        Arc::new(SqliteChatTimelineRepository::from_shared(db.shared_conn()));
    let conversation_id = conversation_repo
        .create(ChatConversation::new_project(ProjectId::new()))
        .await
        .expect("create parent conversation")
        .id;
    let snapshot = DelegationJobSnapshot {
        job_id: "job-terminal-projection".to_string(),
        parent_context_type: "project".to_string(),
        parent_context_id: "project-1".to_string(),
        parent_turn_id: None,
        parent_message_id: None,
        parent_conversation_id: Some(conversation_id.to_string()),
        parent_agent_run_id: Some("parent-run".to_string()),
        parent_tool_use_id: Some("call-delegate-start".to_string()),
        delegated_session_id: "delegated-session".to_string(),
        delegated_conversation_id: Some("delegated-conversation".to_string()),
        delegated_agent_run_id: Some("delegated-run".to_string()),
        agent_name: "reviewer".to_string(),
        assignment: None,
        harness: "codex".to_string(),
        provider_session_id: Some("thread-1".to_string()),
        upstream_provider: Some("openai".to_string()),
        provider_profile: Some("openai".to_string()),
        logical_model: Some("gpt-5.4".to_string()),
        effective_model_id: Some("gpt-5.4-2026-04-01".to_string()),
        logical_effort: Some("high".to_string()),
        effective_effort: Some("high".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        status: "completed".to_string(),
        content: Some("x".repeat(2_000)),
        error: None,
        started_at: "2026-04-12T10:00:00Z".to_string(),
        completed_at: Some("2026-04-12T10:00:05Z".to_string()),
        history: Vec::new(),
        delegated_status: None,
        timed_out: None,
    };

    persist_terminal_projection(&timeline_repo, &snapshot, None)
        .await
        .expect("persist terminal projection");
    persist_terminal_projection(&timeline_repo, &snapshot, None)
        .await
        .expect("repeat terminal projection");

    assert_eq!(
        timeline_repo
            .count_by_conversation(&conversation_id)
            .await
            .expect("count projection rows"),
        1
    );
    let page = timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load terminal projection");
    let terminal = page.items.first().expect("terminal projection row");
    assert_eq!(terminal.tool_name.as_deref(), Some("delegate_terminal"));
    let result: serde_json::Value = serde_json::from_str(
        terminal
            .result_json
            .as_deref()
            .expect("fully hydrated terminal result"),
    )
    .expect("terminal result json");
    assert_eq!(result["job_id"], "job-terminal-projection");
    assert_eq!(result["status"], "completed");
    assert_eq!(result["content"].as_str().map(str::len), Some(2_000));
}

#[tokio::test]
async fn cancellation_blocks_competing_failure_until_cancelled_projection_is_committed() {
    let service = DelegationService::new();
    service
        .register_running(
            "job-cancel".to_string(),
            "project".to_string(),
            "project-1".to_string(),
            None,
            None,
            Some("parent-conversation".to_string()),
            Some("parent-run".to_string()),
            Some("tool-delegate".to_string()),
            "delegated-session".to_string(),
            Some("delegated-conversation".to_string()),
            Some("delegated-run".to_string()),
            "reviewer".to_string(),
            None,
            "codex",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

    service
        .begin_cancellation("job-cancel")
        .await
        .expect("running job accepts cancellation intent");

    assert!(
        service
            .terminal_candidate(
                "job-cancel",
                "failed",
                None,
                Some("Agent stopped by user".to_string()),
            )
            .await
            .is_none(),
        "the monitor must not publish the stop-induced failed state while cancellation owns settlement"
    );

    let candidate = service
        .terminal_candidate("job-cancel", "cancelled", None, None)
        .await
        .expect("authoritative cancelled run produces a terminal candidate");
    assert_eq!(
        service.snapshot("job-cancel").await.unwrap().status,
        "running",
        "terminal state must not become observable before durable persistence"
    );
    assert!(service.commit_terminal(candidate.clone()).await);
    assert!(!service.commit_terminal(candidate).await);
    assert_eq!(
        service.snapshot("job-cancel").await.unwrap().status,
        "cancelled"
    );
}

#[tokio::test]
async fn terminal_candidate_reserves_settlement_against_competing_cancellation() {
    let service = DelegationService::new();
    service
        .register_running(
            "job-complete".to_string(),
            "project".to_string(),
            "project-1".to_string(),
            None,
            None,
            Some("parent-conversation".to_string()),
            Some("parent-run".to_string()),
            Some("tool-delegate".to_string()),
            "delegated-session".to_string(),
            Some("delegated-conversation".to_string()),
            Some("delegated-run".to_string()),
            "reviewer".to_string(),
            None,
            "codex",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

    let candidate = service
        .terminal_candidate("job-complete", "completed", Some("done".to_string()), None)
        .await
        .expect("completed run reserves terminal settlement");

    assert!(
        service.begin_cancellation("job-complete").await.is_none(),
        "cancellation must not claim a job while terminal persistence is in flight"
    );
    assert!(
        service
            .terminal_candidate(
                "job-complete",
                "failed",
                None,
                Some("late failure".to_string())
            )
            .await
            .is_none(),
        "a different terminal status must not overwrite the reserved settlement"
    );
    assert!(service.commit_terminal(candidate).await);
    assert_eq!(
        service.snapshot("job-complete").await.unwrap().status,
        "completed"
    );
}

/// Registers a running job and returns the service plus its id.
async fn register_running_job(job_id: &str) -> DelegationService {
    let service = DelegationService::new();
    service
        .register_running(
            job_id.to_string(),
            "project".to_string(),
            "project-1".to_string(),
            None,
            None,
            None,
            None,
            None,
            "delegated-session-1".to_string(),
            Some("delegated-conversation-1".to_string()),
            Some("delegated-run-1".to_string()),
            "ralphx-general-explorer".to_string(),
            None,
            "codex",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
    service
}

#[tokio::test]
async fn settlement_signal_fires_once_after_commit_terminal_accepts() {
    let service = register_running_job("job-signal").await;
    let mut receiver = service
        .subscribe_settlement("job-signal")
        .await
        .expect("subscribe to a registered job");
    assert_eq!(*receiver.borrow(), None, "a running job has not settled");

    let candidate = service
        .terminal_candidate("job-signal", "completed", Some("done".to_string()), None)
        .await
        .expect("terminal candidate");
    assert_eq!(
        *receiver.borrow(),
        None,
        "a speculative terminal_candidate must NOT signal settlement before the CAS accepts"
    );

    assert!(service.commit_terminal(candidate).await);
    receiver
        .changed()
        .await
        .expect("settlement signal after commit_terminal");
    assert_eq!(*receiver.borrow_and_update(), Some("completed".to_string()));

    // A second terminal for an already-settled job is rejected, so no further signal is emitted.
    assert!(service
        .terminal_candidate("job-signal", "failed", None, Some("late".to_string()))
        .await
        .is_none());
    assert!(!receiver.has_changed().expect("receiver alive"));
}

#[tokio::test]
async fn rejected_commit_terminal_emits_no_settlement_signal() {
    let service = register_running_job("job-rejected").await;
    let receiver = service
        .subscribe_settlement("job-rejected")
        .await
        .expect("subscribe to a registered job");

    let mut candidate = service
        .terminal_candidate("job-rejected", "completed", Some("done".to_string()), None)
        .await
        .expect("terminal candidate");
    // Stale delegated run identity: commit_terminal must refuse this candidate.
    candidate.delegated_agent_run_id = Some("some-other-run".to_string());

    assert!(
        !service.commit_terminal(candidate).await,
        "a candidate whose delegated run id no longer matches must be rejected"
    );
    assert!(
        !receiver.has_changed().expect("receiver alive"),
        "a rejected commit_terminal must never signal settlement; a blocked delegate_wait would \
         otherwise wake on a terminal that was never durably accepted"
    );
    assert_eq!(
        service
            .snapshot("job-rejected")
            .await
            .expect("job still registered")
            .status,
        "running"
    );
}

#[tokio::test]
async fn subscribe_settlement_returns_none_for_unknown_jobs() {
    let service = DelegationService::new();
    assert!(service.subscribe_settlement("missing").await.is_none());
}
