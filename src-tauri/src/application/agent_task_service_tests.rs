use std::sync::Arc;

use crate::application::AgentTaskService;
use serde_json::json;

use crate::domain::entities::{
    AgentRunId, AgentTaskAssignmentState, AgentTaskAssignmentTerminalStatus, AgentTaskCreate,
    AgentTaskPatch, AgentTaskScope, AgentTaskState, DelegatedSessionId,
};
use crate::domain::repositories::{AgentTaskListOptions, AgentTaskRepository};
use crate::infrastructure::memory::MemoryAgentTaskRepository;

fn service() -> AgentTaskService {
    let repo: Arc<dyn AgentTaskRepository> = Arc::new(MemoryAgentTaskRepository::new());
    AgentTaskService::new(repo)
}

fn scope() -> AgentTaskScope {
    AgentTaskScope {
        project_id: None,
        scope_type: "conversation".to_string(),
        scope_id: "conv-1".to_string(),
        actor_agent: Some("ralphx-general-worker".to_string()),
    }
}

fn create(title: &str) -> AgentTaskCreate {
    AgentTaskCreate {
        title: title.to_string(),
        details: format!("Details for {title}"),
        active_label: None,
        owner_agent: None,
        metadata: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
    }
}

#[tokio::test]
async fn claim_task_rejects_unresolved_blockers() {
    let service = service();
    let scope = scope();
    service.create_task(&scope, create("A")).await.unwrap();
    service.create_task(&scope, create("B")).await.unwrap();
    service
        .update_task(
            &scope,
            "2",
            AgentTaskPatch {
                add_blocked_by: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let err = service.claim_task(&scope, "2", None).await.unwrap_err();
    assert!(err.to_string().contains("blocked"));
}

#[tokio::test]
async fn claim_task_rejects_single_task_ledger_until_it_is_cleaned_up() {
    let service = service();
    let scope = scope();
    service
        .create_task(&scope, create("Umbrella task"))
        .await
        .unwrap();

    let err = service.claim_task(&scope, "1", None).await.unwrap_err();
    assert!(
        err.to_string().contains("single-task agent task ledger"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string()
            .contains("decompose it into multiple concrete tasks"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("mark it dropped"),
        "unexpected error: {err}"
    );

    let default_list = service
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap();
    assert_eq!(default_list.len(), 1);
    assert_eq!(default_list[0].state, AgentTaskState::Open);

    let dropped = service
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                state: Some(AgentTaskState::Dropped),
                metadata_patch: Some(json!({"reason": "single_step_no_ledger_needed"})),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dropped.task.state, AgentTaskState::Dropped);

    let unresolved = service
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap();
    assert!(
        unresolved.is_empty(),
        "dropped cleanup should hide the accidental single-task ledger"
    );
}

#[tokio::test]
async fn update_task_rejects_single_task_activation_but_allows_decomposed_claims() {
    let service = service();
    let scope = scope();
    service
        .create_task(&scope, create("Only task"))
        .await
        .unwrap();

    let err = service
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                state: Some(AgentTaskState::Active),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("single-task agent task ledger"),
        "unexpected error: {err}"
    );

    service
        .create_task(&scope, create("Validation task"))
        .await
        .unwrap();
    let claimed = service
        .claim_task(&scope, "1", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.task.state, AgentTaskState::Active);
}

#[tokio::test]
async fn complete_task_rejects_single_task_ledger() {
    let service = service();
    let scope = scope();
    service
        .create_task(&scope, create("Only task"))
        .await
        .unwrap();

    let err = service
        .complete_task(&scope, "1", Some(json!({"verified": true})))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("single-task agent task ledger"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn service_forwards_crud_operations() {
    let service = service();
    let scope = scope();

    let created = service.create_task(&scope, create("Plan")).await.unwrap();
    assert_eq!(created.task.task_number, 1);

    let listed = service
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);

    let task_lists = service.list_task_lists(&scope).await.unwrap();
    assert_eq!(task_lists.len(), 1);
    let list_tasks = service
        .list_tasks_for_list(
            &scope,
            &task_lists[0].list_id,
            AgentTaskListOptions { include_done: true },
        )
        .await
        .unwrap();
    assert_eq!(list_tasks.len(), 1);

    let updated = service
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                title: Some("Plan updated".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.task.title, "Plan updated");

    let fetched = service.get_task(&scope, "1").await.unwrap().unwrap();
    assert_eq!(fetched.title, "Plan updated");
}

#[tokio::test]
async fn claim_task_uses_scope_actor_or_explicit_owner() {
    let service = service();
    let scope = scope();
    service
        .create_task(&scope, create("Claim me"))
        .await
        .unwrap();
    service
        .create_task(&scope, create("Validate the claim"))
        .await
        .unwrap();

    let claimed = service
        .claim_task(&scope, "1", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        claimed.task.owner_agent.as_deref(),
        Some("ralphx-general-worker")
    );
    assert_eq!(claimed.task.state, AgentTaskState::Active);

    let reassigned = service
        .claim_task(&scope, "1", Some("verifier".to_string()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reassigned.task.owner_agent.as_deref(), Some("verifier"));
}

#[tokio::test]
async fn claim_and_complete_missing_tasks_return_none() {
    let service = service();
    let scope = scope();

    assert!(service
        .claim_task(&scope, "missing", None)
        .await
        .unwrap()
        .is_none());
    assert!(service
        .complete_task(&scope, "missing", None)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn complete_task_marks_done_and_merges_metadata() {
    let service = service();
    let scope = scope();
    service
        .create_task(
            &scope,
            AgentTaskCreate {
                metadata: Some(json!({"priority": "high", "stale": true})),
                ..create("Complete me")
            },
        )
        .await
        .unwrap();
    service
        .create_task(&scope, create("Follow-up validation"))
        .await
        .unwrap();

    let completed = service
        .complete_task(&scope, "1", Some(json!({"stale": null, "verified": true})))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(completed.task.state, AgentTaskState::Done);
    assert_eq!(
        completed.task.metadata,
        Some(json!({"priority": "high", "verified": true}))
    );
    assert_eq!(completed.state_change.unwrap().to, AgentTaskState::Done);
}

#[tokio::test]
async fn service_drives_assignment_completion_and_release_lifecycles() {
    let service = service();
    let scope = scope();
    let session = DelegatedSessionId::from_string("delegate-session-1");
    let caller_run = AgentRunId::from_string("caller-run-1");
    assert!(service
        .reserve_assignment(
            &AgentTaskScope::new("conversation", "missing"),
            "1",
            &session,
            &caller_run,
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .is_none());
    for title in ["Implement", "Validate"] {
        service.create_task(&scope, create(title)).await.unwrap();
    }
    assert!(service
        .reserve_assignment(
            &scope,
            "missing",
            &session,
            &caller_run,
            "ralphx-general-worker",
        )
        .await
        .unwrap()
        .is_none());

    let delegated_run = AgentRunId::from_string("delegate-run-1");
    let reserved = service
        .reserve_assignment(&scope, "1", &session, &caller_run, "ralphx-general-worker")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reserved.assignment.assignment.state,
        AgentTaskAssignmentState::Reserved
    );
    assert!(service
        .reserve_assignment(&scope, "1", &session, &caller_run, "ralphx-general-worker",)
        .await
        .is_err());
    assert!(service
        .plan_assignment_run(
            &reserved.assignment.assignment.id,
            &DelegatedSessionId::from_string("wrong-session"),
            &delegated_run,
        )
        .await
        .is_err());

    let planned = service
        .plan_assignment_run(&reserved.assignment.assignment.id, &session, &delegated_run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        planned.assignment.planned_delegated_agent_run_id,
        Some(delegated_run)
    );
    assert_eq!(
        service
            .get_unresolved_assignment(&session)
            .await
            .unwrap()
            .unwrap()
            .assignment
            .id,
        reserved.assignment.assignment.id
    );

    service
        .bind_assignment_run(&reserved.assignment.assignment.id, &session, &delegated_run)
        .await
        .unwrap()
        .unwrap();
    let requested = service
        .request_assignment_completion(&session, &delegated_run, Some(json!({"verified": true})))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        requested.assignment.state,
        AgentTaskAssignmentState::CompletionRequested
    );
    assert!(
        service
            .settle_assignment_for_run(
                &delegated_run,
                AgentTaskAssignmentTerminalStatus::Completed,
                None,
            )
            .await
            .unwrap()
            .unwrap()
            .task_completed
    );

    let second_run = AgentRunId::from_string("delegate-run-2");
    let reserved = service
        .reserve_assignment(&scope, "2", &session, &caller_run, "ralphx-general-worker")
        .await
        .unwrap()
        .unwrap();
    service
        .plan_assignment_run(&reserved.assignment.assignment.id, &session, &second_run)
        .await
        .unwrap()
        .unwrap();
    service
        .bind_assignment_run(&reserved.assignment.assignment.id, &session, &second_run)
        .await
        .unwrap()
        .unwrap();
    let requested = service
        .request_assignment_release(&session, &second_run, "delegate requested release")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        requested.assignment.state,
        AgentTaskAssignmentState::ReleaseRequested
    );
    assert!(service
        .fail_reserved_assignment(&session, "too late to fail before launch")
        .await
        .is_err());
}
