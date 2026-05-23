use std::sync::Arc;

use crate::application::AgentTaskService;
use serde_json::json;

use crate::domain::entities::{AgentTaskCreate, AgentTaskPatch, AgentTaskScope, AgentTaskState};
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
