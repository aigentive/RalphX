use serde_json::json;

use super::sqlite_agent_task_repo::SqliteAgentTaskRepository;
use crate::domain::entities::{AgentTaskCreate, AgentTaskPatch, AgentTaskScope, AgentTaskState};
use crate::domain::repositories::{AgentTaskListOptions, AgentTaskRepository};
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_agent_task_repo_tests")
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
async fn test_create_and_get_task() {
    let db = setup_test_db();
    let repo = SqliteAgentTaskRepository::from_shared(db.shared_conn());

    let created = repo
        .create_task(&scope(), create("Plan work"))
        .await
        .unwrap();
    assert_eq!(created.task.task_number, 1);

    let found = repo.get_task(&scope(), "1").await.unwrap().unwrap();
    assert_eq!(found.title, "Plan work");
    assert_eq!(found.state, AgentTaskState::Open);
}

#[tokio::test]
async fn test_dependency_cycle_rejected_and_unresolved_blockers_filtered() {
    let db = setup_test_db();
    let repo = SqliteAgentTaskRepository::from_shared(db.shared_conn());
    let scope = scope();

    repo.create_task(&scope, create("A")).await.unwrap();
    repo.create_task(&scope, create("B")).await.unwrap();
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

    let err = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                add_blocked_by: vec!["2".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("cycle"));

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
    assert_eq!(listed.len(), 1);
    assert!(listed[0].blocked_by.is_empty());
    let second = repo.get_task(&scope, "2").await.unwrap().unwrap();
    assert_eq!(second.blocked_by, vec!["1"]);
}

#[tokio::test]
async fn test_metadata_merge_removes_null_keys() {
    let db = setup_test_db();
    let repo = SqliteAgentTaskRepository::from_shared(db.shared_conn());
    let scope = scope();
    repo.create_task(
        &scope,
        AgentTaskCreate {
            metadata: Some(json!({"priority": "high", "old": true})),
            ..create("Metadata")
        },
    )
    .await
    .unwrap();

    let updated = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                metadata_patch: Some(json!({"old": null, "lane": "implementation"})),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        updated.task.metadata,
        Some(json!({"priority": "high", "lane": "implementation"}))
    );
}
