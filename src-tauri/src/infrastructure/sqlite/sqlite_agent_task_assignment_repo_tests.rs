use serde_json::json;

use super::{
    SqliteAgentRunRepository, SqliteAgentTaskRepository, SqliteDelegatedSessionRepository,
};
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentRun, AgentTaskAssignmentId, AgentTaskAssignmentState, AgentTaskAssignmentTerminalStatus,
    AgentTaskCreate, AgentTaskPatch, AgentTaskScope, AgentTaskState, ChatConversation,
    DelegatedSession,
};
use crate::domain::repositories::{
    AgentRunRepository, AgentTaskRepository, DelegatedSessionRepository,
};
use crate::testing::SqliteTestDb;

fn scope() -> AgentTaskScope {
    AgentTaskScope::new("conversation", "conversation-ledger")
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

#[tokio::test]
async fn sqlite_assignment_lifecycle_is_atomic_locked_and_attempt_scoped() {
    let db = SqliteTestDb::new("sqlite_agent_task_assignment_repo_tests");
    let project = db.seed_project("Assignment project");
    let conversation = db.insert_conversation(ChatConversation::new_project(project.id.clone()));
    let run_repo = SqliteAgentRunRepository::from_shared(db.shared_conn());
    let delegated_repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let task_repo = SqliteAgentTaskRepository::from_shared(db.shared_conn());
    let caller_run = run_repo
        .create(AgentRun::new(conversation.id))
        .await
        .unwrap();
    let delegated_session = delegated_repo
        .create(DelegatedSession::new(
            project.id,
            "project".to_string(),
            "project-context".to_string(),
            "ralphx-general-worker".to_string(),
            AgentHarnessKind::Codex,
        ))
        .await
        .unwrap();
    let delegated_conversation = db.insert_conversation(ChatConversation::new_delegation(
        delegated_session.id.clone(),
    ));
    let delegated_run = run_repo
        .create(AgentRun::new(delegated_conversation.id))
        .await
        .unwrap();

    task_repo
        .create_task(&scope(), task("Implement", Some("orchestrator")))
        .await
        .unwrap();
    task_repo
        .create_task(&scope(), task("Validate", None))
        .await
        .unwrap();

    let reserved = task_repo
        .reserve_assignment(
            &scope(),
            "1",
            &delegated_session.id,
            &caller_run.id,
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

    let locked = task_repo
        .update_task(
            &scope(),
            "1",
            AgentTaskPatch {
                owner_agent: Some(Some("other".to_string())),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(locked.to_string().contains("controlled by an active"));
    task_repo
        .update_task(
            &scope(),
            "1",
            AgentTaskPatch {
                title: Some("Implement durable assignments".to_string()),
                metadata_patch: Some(json!({"note": "assigned"})),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let wrong_assignment_id = AgentTaskAssignmentId::new();
    assert!(task_repo
        .bind_assignment_run(
            &wrong_assignment_id,
            &delegated_session.id,
            &delegated_run.id,
        )
        .await
        .unwrap()
        .is_none());
    let planned = task_repo
        .plan_assignment_run(
            &reserved.assignment.assignment.id,
            &delegated_session.id,
            &delegated_run.id,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(planned.assignment.state, AgentTaskAssignmentState::Reserved);
    assert_eq!(
        planned.assignment.delegated_agent_run_id,
        Some(delegated_run.id)
    );
    assert!(task_repo
        .bind_assignment_run(
            &reserved.assignment.assignment.id,
            &delegated_session.id,
            &crate::domain::entities::AgentRunId::new(),
        )
        .await
        .is_err());
    task_repo
        .bind_assignment_run(
            &reserved.assignment.assignment.id,
            &delegated_session.id,
            &delegated_run.id,
        )
        .await
        .unwrap();
    let requested = task_repo
        .request_assignment_completion(
            &delegated_session.id,
            &delegated_run.id,
            &AgentTaskScope::new("delegation", delegated_session.id.as_str()),
            Some(json!({"verified": true})),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requested.task.state, AgentTaskState::Active);
    let completed = task_repo
        .settle_assignment_for_run(
            &delegated_run.id,
            AgentTaskAssignmentTerminalStatus::Completed,
            None,
        )
        .await
        .unwrap()
        .unwrap();
    assert!(completed.task_completed);
    assert_eq!(completed.assignment.task.state, AgentTaskState::Done);
    assert_eq!(
        completed.assignment.task.metadata,
        Some(json!({"note": "assigned", "verified": true}))
    );
    assert!(task_repo
        .settle_assignment_for_run(
            &delegated_run.id,
            AgentTaskAssignmentTerminalStatus::Cancelled,
            None,
        )
        .await
        .unwrap()
        .is_none());
    let reloaded = task_repo
        .get_assignment_for_run(&delegated_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reloaded.assignment.state,
        AgentTaskAssignmentState::Completed
    );
    assert_eq!(reloaded.task.state, AgentTaskState::Done);

    let second = task_repo
        .reserve_assignment(
            &scope(),
            "2",
            &delegated_session.id,
            &caller_run.id,
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.assignment.assignment.attempt_number, 2);
    let failed = task_repo
        .fail_reserved_assignment(&delegated_session.id, "spawn failed")
        .await
        .unwrap()
        .unwrap();
    assert!(failed.task_reopened);
    assert_eq!(failed.assignment.task.state, AgentTaskState::Open);

    let event_types = db.with_connection(|conn| {
        let mut statement = conn
            .prepare(
                "SELECT event_type
                 FROM agent_task_events
                 WHERE event_type LIKE 'agent_task.assignment_%'
                 ORDER BY seq",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    });
    assert!(event_types.contains(&"agent_task.assignment_reserved".to_string()));
    assert!(event_types.contains(&"agent_task.assignment_completed".to_string()));
    assert!(event_types.contains(&"agent_task.assignment_reopened".to_string()));
}

#[tokio::test]
async fn sqlite_assignment_intent_retries_preserve_first_payload_and_event_count() {
    let db = SqliteTestDb::new("sqlite_assignment_intent_retries");
    let project = db.seed_project("Assignment retry project");
    let caller_conversation =
        db.insert_conversation(ChatConversation::new_project(project.id.clone()));
    let run_repo = SqliteAgentRunRepository::from_shared(db.shared_conn());
    let delegated_repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let task_repo = SqliteAgentTaskRepository::from_shared(db.shared_conn());
    let caller_run = run_repo
        .create(AgentRun::new(caller_conversation.id))
        .await
        .unwrap();
    let delegated_session = delegated_repo
        .create(DelegatedSession::new(
            project.id,
            "project".to_string(),
            "project-context".to_string(),
            "ralphx-general-worker".to_string(),
            AgentHarnessKind::Codex,
        ))
        .await
        .unwrap();
    let delegated_conversation = db.insert_conversation(ChatConversation::new_delegation(
        delegated_session.id.clone(),
    ));
    let first_run = run_repo
        .create(AgentRun::new(delegated_conversation.id))
        .await
        .unwrap();
    for title in ["Implement", "Validate"] {
        task_repo
            .create_task(&scope(), task(title, Some("orchestrator")))
            .await
            .unwrap();
    }

    let first = task_repo
        .reserve_assignment(
            &scope(),
            "1",
            &delegated_session.id,
            &caller_run.id,
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .unwrap();
    task_repo
        .plan_assignment_run(
            &first.assignment.assignment.id,
            &delegated_session.id,
            &first_run.id,
        )
        .await
        .unwrap();
    task_repo
        .bind_assignment_run(
            &first.assignment.assignment.id,
            &delegated_session.id,
            &first_run.id,
        )
        .await
        .unwrap();
    let local_scope = AgentTaskScope::new("delegation", delegated_session.id.as_str());
    let requested = task_repo
        .request_assignment_completion(
            &delegated_session.id,
            &first_run.id,
            &local_scope,
            Some(json!({"verified": true})),
        )
        .await
        .unwrap()
        .unwrap();
    let completion_event_count =
        assignment_event_count(&db, "agent_task.assignment_completion_requested");
    let retried = task_repo
        .request_assignment_completion(&delegated_session.id, &first_run.id, &local_scope, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retried.assignment.completion_metadata,
        requested.assignment.completion_metadata
    );
    assert_eq!(
        assignment_event_count(&db, "agent_task.assignment_completion_requested"),
        completion_event_count
    );
    assert!(task_repo
        .request_assignment_release(&delegated_session.id, &first_run.id, "opposite intent")
        .await
        .is_err());

    task_repo
        .settle_assignment_for_run(
            &first_run.id,
            AgentTaskAssignmentTerminalStatus::Failed,
            None,
        )
        .await
        .unwrap();
    let second_run = run_repo
        .create(AgentRun::new(delegated_conversation.id))
        .await
        .unwrap();
    let second = task_repo
        .reserve_assignment(
            &scope(),
            "2",
            &delegated_session.id,
            &caller_run.id,
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .unwrap();
    task_repo
        .plan_assignment_run(
            &second.assignment.assignment.id,
            &delegated_session.id,
            &second_run.id,
        )
        .await
        .unwrap();
    task_repo
        .bind_assignment_run(
            &second.assignment.assignment.id,
            &delegated_session.id,
            &second_run.id,
        )
        .await
        .unwrap();
    let requested = task_repo
        .request_assignment_release(&delegated_session.id, &second_run.id, "first reason")
        .await
        .unwrap()
        .unwrap();
    let release_event_count =
        assignment_event_count(&db, "agent_task.assignment_release_requested");
    let retried = task_repo
        .request_assignment_release(&delegated_session.id, &second_run.id, "replacement reason")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retried.assignment.settlement_reason,
        requested.assignment.settlement_reason
    );
    assert_eq!(
        assignment_event_count(&db, "agent_task.assignment_release_requested"),
        release_event_count
    );
    assert!(task_repo
        .request_assignment_completion(&delegated_session.id, &second_run.id, &local_scope, None,)
        .await
        .is_err());
}

fn assignment_event_count(db: &SqliteTestDb, event_type: &str) -> i64 {
    db.with_connection(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM agent_task_events WHERE event_type = ?1",
            [event_type],
            |row| row.get(0),
        )
        .unwrap()
    })
}
