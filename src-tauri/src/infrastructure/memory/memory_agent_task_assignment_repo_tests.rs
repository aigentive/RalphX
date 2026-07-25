use serde_json::json;

use super::MemoryAgentTaskRepository;
use crate::domain::entities::{
    AgentRunId, AgentTaskAssignmentId, AgentTaskAssignmentState, AgentTaskAssignmentTerminalStatus,
    AgentTaskCreate, AgentTaskPatch, AgentTaskScope, AgentTaskState, DelegatedSessionId,
};
use crate::domain::repositories::AgentTaskRepository;

fn scope() -> AgentTaskScope {
    AgentTaskScope::new("conversation", "conversation-1")
}

fn task(title: &str, owner_agent: Option<&str>) -> AgentTaskCreate {
    AgentTaskCreate {
        title: title.to_string(),
        details: format!("Details for {title}"),
        active_label: None,
        owner_agent: owner_agent.map(str::to_string),
        metadata: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
    }
}

async fn seeded_repo() -> MemoryAgentTaskRepository {
    let repo = MemoryAgentTaskRepository::new();
    repo.create_task(&scope(), task("Implement", Some("orchestrator")))
        .await
        .unwrap();
    repo.create_task(&scope(), task("Validate", None))
        .await
        .unwrap();
    repo
}

#[tokio::test]
async fn assignment_reservation_locks_owned_fields_and_completion_is_two_phase() {
    let repo = seeded_repo().await;
    let session = DelegatedSessionId::from_string("session-1");
    let caller_run = AgentRunId::from_string("caller-run-1");
    let delegated_run = AgentRunId::from_string("delegated-run-1");

    let reserved = repo
        .reserve_assignment(
            &scope(),
            "1",
            &session,
            &caller_run,
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reserved.assignment.assignment.state,
        AgentTaskAssignmentState::Reserved
    );
    assert_eq!(reserved.assignment.task.state, AgentTaskState::Active);
    assert_eq!(
        reserved.assignment.task.owner_agent.as_deref(),
        Some("ralphx-general-worker")
    );

    let locked = repo
        .update_task(
            &scope(),
            "1",
            AgentTaskPatch {
                state: Some(AgentTaskState::Done),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(locked.to_string().contains("controlled by an active"));

    let descriptive = repo
        .update_task(
            &scope(),
            "1",
            AgentTaskPatch {
                title: Some("Implement assignment support".to_string()),
                metadata_patch: Some(json!({"note": "still assigned"})),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(descriptive.task.state, AgentTaskState::Active);

    let wrong_assignment_id = AgentTaskAssignmentId::new();
    assert!(repo
        .bind_assignment_run(&wrong_assignment_id, &session, &delegated_run)
        .await
        .unwrap()
        .is_none());
    let planned = repo
        .plan_assignment_run(&reserved.assignment.assignment.id, &session, &delegated_run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(planned.assignment.state, AgentTaskAssignmentState::Reserved);
    assert_eq!(
        planned.assignment.planned_delegated_agent_run_id,
        Some(delegated_run)
    );
    assert_eq!(planned.assignment.delegated_agent_run_id, None);
    assert!(repo
        .bind_assignment_run(
            &reserved.assignment.assignment.id,
            &session,
            &AgentRunId::new(),
        )
        .await
        .is_err());
    let bound = repo
        .bind_assignment_run(&reserved.assignment.assignment.id, &session, &delegated_run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bound.assignment.state, AgentTaskAssignmentState::Active);
    let local_scope = AgentTaskScope::new("delegation", session.as_str());
    repo.create_task(&local_scope, task("Implement locally", None))
        .await
        .unwrap();
    repo.create_task(&local_scope, task("Validate locally", None))
        .await
        .unwrap();
    let unfinished_local = repo
        .request_assignment_completion(&session, &delegated_run, &local_scope, None)
        .await
        .unwrap_err();
    assert!(unfinished_local
        .to_string()
        .contains("delegate-local tasks must be resolved"));
    for task_ref in ["1", "2"] {
        repo.update_task(
            &local_scope,
            task_ref,
            AgentTaskPatch {
                state: Some(AgentTaskState::Done),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let requested = repo
        .request_assignment_completion(
            &session,
            &delegated_run,
            &local_scope,
            Some(json!({"verified": true})),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        requested.assignment.state,
        AgentTaskAssignmentState::CompletionRequested
    );
    assert_eq!(requested.task.state, AgentTaskState::Active);

    let settled = repo
        .settle_assignment_for_run(
            &delegated_run,
            AgentTaskAssignmentTerminalStatus::Completed,
            None,
        )
        .await
        .unwrap()
        .unwrap();
    assert!(settled.task_completed);
    assert!(!settled.task_reopened);
    assert_eq!(
        settled.assignment.assignment.state,
        AgentTaskAssignmentState::Completed
    );
    assert_eq!(settled.assignment.task.state, AgentTaskState::Done);
    assert_eq!(
        settled.assignment.task.metadata,
        Some(json!({"note": "still assigned", "verified": true}))
    );
    assert!(repo
        .settle_assignment_for_run(
            &delegated_run,
            AgentTaskAssignmentTerminalStatus::Cancelled,
            None,
        )
        .await
        .unwrap()
        .is_none());
    let reloaded = repo
        .get_assignment_for_run(&delegated_run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reloaded.assignment.state,
        AgentTaskAssignmentState::Completed
    );
    assert_eq!(reloaded.task.state, AgentTaskState::Done);
}

#[tokio::test]
async fn terminal_without_completion_reopens_and_reused_session_gets_fresh_attempt() {
    let repo = seeded_repo().await;
    let session = DelegatedSessionId::from_string("session-1");
    let first_run = AgentRunId::from_string("delegated-run-1");

    let first_reservation = repo
        .reserve_assignment(
            &scope(),
            "1",
            &session,
            &AgentRunId::from_string("caller-run-1"),
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .unwrap();
    repo.plan_assignment_run(
        &first_reservation.assignment.assignment.id,
        &session,
        &first_run,
    )
    .await
    .unwrap();
    repo.bind_assignment_run(
        &first_reservation.assignment.assignment.id,
        &session,
        &first_run,
    )
    .await
    .unwrap();
    let released = repo
        .settle_assignment_for_run(
            &first_run,
            AgentTaskAssignmentTerminalStatus::Completed,
            None,
        )
        .await
        .unwrap()
        .unwrap();
    assert!(released.task_reopened);
    assert_eq!(
        released.assignment.assignment.state,
        AgentTaskAssignmentState::Released
    );
    assert_eq!(released.assignment.task.state, AgentTaskState::Open);
    assert_eq!(
        released.assignment.task.owner_agent.as_deref(),
        Some("orchestrator")
    );

    let second = repo
        .reserve_assignment(
            &scope(),
            "1",
            &session,
            &AgentRunId::from_string("caller-run-2"),
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.assignment.assignment.attempt_number, 2);
    assert_eq!(
        second.assignment.assignment.state,
        AgentTaskAssignmentState::Reserved
    );
    assert!(second.assignment.assignment.completion_metadata.is_none());

    let failed = repo
        .fail_reserved_assignment(&session, "spawn failed")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        failed.assignment.assignment.state,
        AgentTaskAssignmentState::Failed
    );
    assert_eq!(failed.assignment.task.state, AgentTaskState::Open);
    assert_eq!(
        failed.assignment.task.owner_agent.as_deref(),
        Some("orchestrator")
    );
}

#[tokio::test]
async fn assignment_intent_retries_preserve_first_payload_and_event_count() {
    let repo = seeded_repo().await;
    let session = DelegatedSessionId::from_string("session-1");
    let first_run = AgentRunId::from_string("delegated-run-1");
    let first = repo
        .reserve_assignment(
            &scope(),
            "1",
            &session,
            &AgentRunId::from_string("caller-run-1"),
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .unwrap();
    repo.plan_assignment_run(&first.assignment.assignment.id, &session, &first_run)
        .await
        .unwrap();
    repo.bind_assignment_run(&first.assignment.assignment.id, &session, &first_run)
        .await
        .unwrap();
    let local_scope = AgentTaskScope::new("delegation", session.as_str());
    let requested = repo
        .request_assignment_completion(
            &session,
            &first_run,
            &local_scope,
            Some(json!({"verified": true})),
        )
        .await
        .unwrap()
        .unwrap();
    let event_count = repo.state.read().unwrap().events.len();
    let retried = repo
        .request_assignment_completion(&session, &first_run, &local_scope, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retried.assignment.completion_metadata,
        requested.assignment.completion_metadata
    );
    assert_eq!(repo.state.read().unwrap().events.len(), event_count);
    assert!(repo
        .request_assignment_release(&session, &first_run, "opposite intent")
        .await
        .is_err());

    repo.settle_assignment_for_run(&first_run, AgentTaskAssignmentTerminalStatus::Failed, None)
        .await
        .unwrap();
    let second_run = AgentRunId::from_string("delegated-run-2");
    let second = repo
        .reserve_assignment(
            &scope(),
            "2",
            &session,
            &AgentRunId::from_string("caller-run-2"),
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .unwrap();
    repo.plan_assignment_run(&second.assignment.assignment.id, &session, &second_run)
        .await
        .unwrap();
    repo.bind_assignment_run(&second.assignment.assignment.id, &session, &second_run)
        .await
        .unwrap();
    let requested = repo
        .request_assignment_release(&session, &second_run, "first reason")
        .await
        .unwrap()
        .unwrap();
    let event_count = repo.state.read().unwrap().events.len();
    let retried = repo
        .request_assignment_release(&session, &second_run, "replacement reason")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retried.assignment.settlement_reason,
        requested.assignment.settlement_reason
    );
    assert_eq!(repo.state.read().unwrap().events.len(), event_count);
    assert!(repo
        .request_assignment_completion(&session, &second_run, &local_scope, None)
        .await
        .is_err());
}
