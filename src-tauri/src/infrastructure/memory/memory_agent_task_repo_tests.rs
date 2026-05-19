use serde_json::json;

use super::MemoryAgentTaskRepository;
use crate::domain::entities::{AgentTaskCreate, AgentTaskPatch, AgentTaskScope, AgentTaskState};
use crate::domain::repositories::{AgentTaskListOptions, AgentTaskRepository};

fn scope() -> AgentTaskScope {
    AgentTaskScope::new("conversation", "conv-1")
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
async fn create_assigns_monotonic_task_numbers() {
    let repo = MemoryAgentTaskRepository::new();

    let first = repo.create_task(&scope(), create("First")).await.unwrap();
    let second = repo.create_task(&scope(), create("Second")).await.unwrap();

    assert_eq!(first.task.task_number, 1);
    assert_eq!(second.task.task_number, 2);
}

#[tokio::test]
async fn list_hides_resolved_blockers_but_get_keeps_full_dependencies() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();
    repo.create_task(&scope, create("Design")).await.unwrap();
    repo.create_task(&scope, create("Build")).await.unwrap();

    repo.update_task(
        &scope,
        "2",
        AgentTaskPatch {
            add_blocked_by: vec!["1".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let listed = repo
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap();
    assert_eq!(listed[1].blocked_by, vec!["1"]);

    repo.update_task(
        &scope,
        "1",
        AgentTaskPatch {
            state: Some(AgentTaskState::Done),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let listed = repo
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap();
    assert!(listed[0].blocked_by.is_empty());
    let build = repo.get_task(&scope, "2").await.unwrap().unwrap();
    assert_eq!(build.blocked_by, vec!["1"]);
    assert!(build.unresolved_blocked_by.is_empty());
}

#[tokio::test]
async fn metadata_patch_merges_and_removes_null_keys() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();
    repo.create_task(
        &scope,
        AgentTaskCreate {
            metadata: Some(json!({"priority": "high", "stale": true})),
            ..create("Track metadata")
        },
    )
    .await
    .unwrap();

    let updated = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                metadata_patch: Some(json!({"owner": "planner", "stale": null})),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        updated.task.metadata,
        Some(json!({"priority": "high", "owner": "planner"}))
    );
}

#[tokio::test]
async fn dependency_cycle_is_rejected() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();
    repo.create_task(&scope, create("A")).await.unwrap();
    repo.create_task(&scope, create("B")).await.unwrap();

    repo.update_task(
        &scope,
        "1",
        AgentTaskPatch {
            add_blocks: vec!["2".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let err = repo
        .update_task(
            &scope,
            "2",
            AgentTaskPatch {
                add_blocks: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("cycle"));
}
