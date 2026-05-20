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

#[tokio::test]
async fn test_update_tracks_fields_events_and_include_done() {
    let db = setup_test_db();
    let repo = SqliteAgentTaskRepository::from_shared(db.shared_conn());
    let scope = scope();
    let first = repo.create_task(&scope, create("Plan")).await.unwrap();
    repo.create_task(&scope, create("Build")).await.unwrap();

    let updated = repo
        .update_task(
            &scope,
            first.task.task_id.as_str(),
            AgentTaskPatch {
                title: Some("Plan refined".to_string()),
                details: Some("Updated planning details".to_string()),
                active_label: Some(Some("Planning".to_string())),
                owner_agent: Some(Some("planner".to_string())),
                state: Some(AgentTaskState::Done),
                metadata_patch: Some(json!({"priority": "high"})),
                add_blocks: vec!["2".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        updated.changed_fields,
        vec![
            "active_label",
            "dependencies",
            "details",
            "metadata",
            "owner_agent",
            "state",
            "title"
        ]
    );
    assert_eq!(updated.task.title, "Plan refined");
    assert_eq!(updated.task.active_label.as_deref(), Some("Planning"));
    assert_eq!(updated.task.owner_agent.as_deref(), Some("planner"));
    assert_eq!(updated.task.blocks, vec!["2"]);
    assert!(updated.task.completed_at.is_some());
    assert_eq!(updated.state_change.unwrap().to, AgentTaskState::Done);

    let active_only = repo
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap();
    assert_eq!(active_only.len(), 1);
    assert_eq!(active_only[0].task_number, 2);
    assert_eq!(active_only[0].blocked_by, Vec::<String>::new());

    let include_done = repo
        .list_tasks(&scope, AgentTaskListOptions { include_done: true })
        .await
        .unwrap();
    assert_eq!(include_done.len(), 2);

    let conn = db.shared_conn();
    let conn = conn.lock().await;
    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_task_events WHERE event_type IN (
                'agent_task.updated',
                'agent_task.state_changed',
                'agent_task.owner_changed',
                'agent_task.metadata_changed',
                'agent_task.dependencies_changed'
             )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_count, 5);
}

#[tokio::test]
async fn test_remove_dependencies_noop_updates_and_validation_errors() {
    let db = setup_test_db();
    let repo = SqliteAgentTaskRepository::from_shared(db.shared_conn());
    let scope = scope();

    assert!(repo.get_task(&scope, "1").await.unwrap().is_none());
    assert!(repo
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap()
        .is_empty());
    assert!(repo
        .update_task(&scope, "1", AgentTaskPatch::default())
        .await
        .unwrap()
        .is_none());

    let blank_title = repo.create_task(&scope, create("")).await.unwrap_err();
    assert!(blank_title.to_string().contains("title"));

    repo.create_task(&scope, create("A")).await.unwrap();
    repo.create_task(
        &scope,
        AgentTaskCreate {
            blocked_by: vec!["1".to_string()],
            ..create("B")
        },
    )
    .await
    .unwrap();

    let duplicate = repo
        .update_task(
            &scope,
            "2",
            AgentTaskPatch {
                add_blocked_by: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(duplicate.changed_fields.is_empty());

    let removed = repo
        .update_task(
            &scope,
            "2",
            AgentTaskPatch {
                remove_blocked_by: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(removed.changed_fields, vec!["dependencies"]);
    assert!(removed.task.blocked_by.is_empty());

    let noop_remove = repo
        .update_task(
            &scope,
            "2",
            AgentTaskPatch {
                remove_blocked_by: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(noop_remove.changed_fields.is_empty());

    let blank_details = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                details: Some(" ".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(blank_details.to_string().contains("details"));

    let missing_dependency = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                add_blocks: vec!["missing".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(missing_dependency
        .to_string()
        .contains("dependency not found"));

    let self_dependency = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                add_blocks: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(self_dependency.to_string().contains("reference itself"));
}
